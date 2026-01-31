//! EPUB format importer - handles all IO.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zip::ZipArchive;

use crate::epub::{parse_container_xml, parse_nav_landmarks, parse_ncx, parse_opf};
use crate::import::{ChapterId, Importer, SpineEntry, resolve_path_based_href};
use crate::io::{ByteSource, ByteSourceCursor, FileSource};
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, Landmark, Metadata, TocEntry};

/// EPUB format importer with random-access ZIP reading.
pub struct EpubImporter {
    /// Random-access byte source for the ZIP file.
    source: Arc<dyn ByteSource>,

    /// Cached ZIP entry locations: path -> ZipEntryLoc.
    zip_index: HashMap<String, ZipEntryLoc>,

    /// Book metadata.
    metadata: Metadata,

    /// Table of contents.
    toc: Vec<TocEntry>,

    /// Landmarks (structural navigation points).
    landmarks: Vec<Landmark>,

    /// Reading order (spine).
    spine: Vec<SpineEntry>,

    /// Maps ChapterId -> ZIP path (e.g., "OEBPS/text/ch01.xhtml").
    spine_paths: Vec<String>,

    /// All asset paths in the ZIP.
    assets: Vec<PathBuf>,

    // --- Link resolution ---
    /// Maps path (without fragment) -> ChapterId
    path_to_chapter: HashMap<String, ChapterId>,

    /// Maps "path#id" -> GlobalNodeId for fragment resolution
    anchor_map: HashMap<String, GlobalNodeId>,
}

#[derive(Clone, Copy)]
struct ZipEntryLoc {
    data_offset: u64,
    compressed_size: u64,
    compression: u16, // 0 = Store, 8 = Deflate
}

impl Importer for EpubImporter {
    fn open(path: &Path) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let source = Arc::new(FileSource::new(file)?);
        Self::from_source(source)
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    fn spine(&self) -> &[SpineEntry] {
        &self.spine
    }

    fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.spine_paths.get(id.0 as usize).map(|s| s.as_str())
    }

    fn load_raw(&mut self, id: ChapterId) -> io::Result<Vec<u8>> {
        let path = self.spine_paths.get(id.0 as usize).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Chapter ID {} not found", id.0),
            )
        })?;
        self.read_entry(path)
    }

    fn list_assets(&self) -> Vec<PathBuf> {
        self.assets.clone()
    }

    fn load_asset(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        let key = path.to_string_lossy().replace('\\', "/");
        self.read_entry(&key)
    }

    fn index_anchors(&mut self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        self.anchor_map.clear();

        for (chapter_id, chapter) in chapters {
            // Get the chapter's source path
            let chapter_path = match self.spine_paths.get(chapter_id.0 as usize) {
                Some(p) => p.split('#').next().unwrap_or(p),
                None => continue,
            };

            // Walk the chapter and record all nodes with IDs
            for node_id in chapter.iter_dfs() {
                if let Some(id) = chapter.semantics.id(node_id) {
                    let key = format!("{}#{}", chapter_path, id);
                    self.anchor_map
                        .insert(key, GlobalNodeId::new(*chapter_id, node_id));
                }
            }
        }
    }

    fn resolve_href(&self, from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        let from_path = self.source_id(from_chapter)?;
        resolve_path_based_href(
            from_path,
            href,
            |p| self.path_to_chapter.get(p).copied(),
            |k| self.anchor_map.get(k).copied(),
        )
    }
}

impl EpubImporter {
    /// Create an importer from a ByteSource.
    pub fn from_source(source: Arc<dyn ByteSource>) -> io::Result<Self> {
        // 1. Scan ZIP central directory and cache entry locations
        let cursor = ByteSourceCursor::new(source.clone());
        let mut archive = ZipArchive::new(cursor)?;

        let mut zip_index = HashMap::new();
        let mut assets = Vec::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let name = file.name().to_string();

            zip_index.insert(
                name.clone(),
                ZipEntryLoc {
                    data_offset: file.data_start(),
                    compressed_size: file.compressed_size(),
                    compression: compression_to_u16(file.compression()),
                },
            );
            assets.push(PathBuf::from(name));
        }

        // 2. Find OPF path from container.xml
        let container_bytes = read_entry(&source, &zip_index, "META-INF/container.xml")?;
        let opf_path = parse_container_xml(&container_bytes)?;
        let opf_base = Path::new(&opf_path)
            .parent()
            .map(|p| {
                let s = p.to_string_lossy();
                if s.is_empty() {
                    String::new()
                } else {
                    format!("{}/", s)
                }
            })
            .unwrap_or_default();

        // 3. Parse OPF
        let opf_bytes = read_entry(&source, &zip_index, &opf_path)?;
        let hint_encoding = crate::util::extract_xml_encoding(&opf_bytes);
        let opf_str = crate::util::decode_text(&opf_bytes, hint_encoding);
        let opf = parse_opf(&opf_str)?;

        // 4. Build spine
        let mut spine = Vec::new();
        let mut spine_paths = Vec::new();

        for (i, spine_id) in opf.spine_ids.iter().enumerate() {
            if let Some((href, _media_type)) = opf.manifest.get(spine_id) {
                let full_path = format!("{}{}", opf_base, href);
                let size_estimate = zip_index
                    .get(&full_path)
                    .map(|loc| loc.compressed_size as usize)
                    .unwrap_or(0);

                spine.push(SpineEntry {
                    id: ChapterId(i as u32),
                    size_estimate,
                });
                spine_paths.push(full_path);
            }
        }

        // 5. Parse TOC (NCX)
        let toc = if let Some(ncx_href) = &opf.ncx_href {
            let ncx_path = format!("{}{}", opf_base, ncx_href);
            if let Ok(ncx_bytes) = read_entry(&source, &zip_index, &ncx_path) {
                let hint_encoding = crate::util::extract_xml_encoding(&ncx_bytes);
                let ncx_str = crate::util::decode_text(&ncx_bytes, hint_encoding);
                let toc_entries = parse_ncx(&ncx_str)?;
                // Prepend base path to hrefs (NCX uses relative paths)
                prepend_base_to_toc(&toc_entries, &opf_base)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // 6. Parse landmarks from EPUB 3 nav document
        let landmarks = if let Some(nav_href) = &opf.nav_href {
            let nav_path = format!("{}{}", opf_base, nav_href);
            if let Ok(nav_bytes) = read_entry(&source, &zip_index, &nav_path) {
                let hint_encoding = crate::util::extract_xml_encoding(&nav_bytes);
                let nav_str = crate::util::decode_text(&nav_bytes, hint_encoding);
                let mut parsed = parse_nav_landmarks(&nav_str)?;
                // Prepend base path to hrefs (nav uses relative paths)
                for landmark in &mut parsed {
                    if !landmark.href.starts_with('#') && !landmark.href.is_empty() {
                        landmark.href = format!("{}{}", opf_base, landmark.href);
                    }
                }
                parsed
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Build path -> ChapterId map
        let mut path_to_chapter = HashMap::new();
        for (i, path) in spine_paths.iter().enumerate() {
            // Store path without fragment
            let base_path = path.split('#').next().unwrap_or(path);
            path_to_chapter.insert(base_path.to_string(), ChapterId(i as u32));
        }

        Ok(Self {
            source,
            zip_index,
            metadata: opf.metadata,
            toc,
            landmarks,
            spine,
            spine_paths,
            assets,
            path_to_chapter,
            anchor_map: HashMap::new(),
        })
    }

    /// Read and decompress a ZIP entry by path.
    fn read_entry(&self, path: &str) -> io::Result<Vec<u8>> {
        read_entry(&self.source, &self.zip_index, path)
    }
}

// ----------------------------------------------------------------------------
// ZIP IO Helpers
// ----------------------------------------------------------------------------

fn read_entry(
    source: &Arc<dyn ByteSource>,
    index: &HashMap<String, ZipEntryLoc>,
    path: &str,
) -> io::Result<Vec<u8>> {
    let loc = index.get(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("File not found in ZIP: {}", path),
        )
    })?;

    // Read compressed data via random access
    let compressed = source.read_at(loc.data_offset, loc.compressed_size as usize)?;

    // Decompress
    match loc.compression {
        0 => Ok(compressed), // Stored
        8 => {
            // Deflate
            let mut decoder = flate2::read::DeflateDecoder::new(&compressed[..]);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out)?;
            Ok(out)
        }
        method => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Unsupported compression method: {}", method),
        )),
    }
}

fn compression_to_u16(method: zip::CompressionMethod) -> u16 {
    match method {
        zip::CompressionMethod::Stored => 0,
        zip::CompressionMethod::Deflated => 8,
        _ => 255,
    }
}

/// Prepend base path to TOC entry hrefs (NCX uses relative paths).
fn prepend_base_to_toc(entries: &[TocEntry], base: &str) -> Vec<TocEntry> {
    entries
        .iter()
        .map(|entry| {
            let href = if entry.href.starts_with('#') || entry.href.is_empty() {
                entry.href.clone()
            } else {
                format!("{}{}", base, entry.href)
            };
            TocEntry {
                title: entry.title.clone(),
                href,
                children: prepend_base_to_toc(&entry.children, base),
                play_order: entry.play_order,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepend_base_to_toc_simple() {
        let entries = vec![
            TocEntry::new("Chapter 1", "text/ch1.xhtml"),
            TocEntry::new("Chapter 2", "text/ch2.xhtml"),
        ];

        let result = prepend_base_to_toc(&entries, "OEBPS/");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].href, "OEBPS/text/ch1.xhtml");
        assert_eq!(result[1].href, "OEBPS/text/ch2.xhtml");
    }

    #[test]
    fn test_prepend_base_to_toc_with_fragments() {
        let entries = vec![
            TocEntry::new("Section 1", "text/ch1.xhtml#section1"),
            TocEntry::new("Section 2", "text/ch1.xhtml#section2"),
        ];

        let result = prepend_base_to_toc(&entries, "epub/");

        assert_eq!(result[0].href, "epub/text/ch1.xhtml#section1");
        assert_eq!(result[1].href, "epub/text/ch1.xhtml#section2");
    }

    #[test]
    fn test_prepend_base_to_toc_preserves_anchor_only() {
        let entries = vec![
            TocEntry::new("Internal Link", "#footnote1"),
            TocEntry::new("Empty", ""),
        ];

        let result = prepend_base_to_toc(&entries, "OEBPS/");

        // Anchor-only hrefs should not be modified
        assert_eq!(result[0].href, "#footnote1");
        // Empty hrefs should not be modified
        assert_eq!(result[1].href, "");
    }

    #[test]
    fn test_prepend_base_to_toc_nested() {
        let mut parent = TocEntry::new("Part I", "text/part1.xhtml");
        parent.children = vec![
            TocEntry::new("Chapter 1", "text/ch1.xhtml"),
            TocEntry::new("Chapter 2", "text/ch2.xhtml"),
        ];
        let entries = vec![parent];

        let result = prepend_base_to_toc(&entries, "epub/");

        assert_eq!(result[0].href, "epub/text/part1.xhtml");
        assert_eq!(result[0].children.len(), 2);
        assert_eq!(result[0].children[0].href, "epub/text/ch1.xhtml");
        assert_eq!(result[0].children[1].href, "epub/text/ch2.xhtml");
    }

    #[test]
    fn test_prepend_base_to_toc_deeply_nested() {
        let grandchild = TocEntry::new("Section", "text/ch1.xhtml#sec1");
        let mut child = TocEntry::new("Chapter 1", "text/ch1.xhtml");
        child.children = vec![grandchild];
        let mut parent = TocEntry::new("Part I", "text/part1.xhtml");
        parent.children = vec![child];
        let entries = vec![parent];

        let result = prepend_base_to_toc(&entries, "content/");

        assert_eq!(result[0].href, "content/text/part1.xhtml");
        assert_eq!(result[0].children[0].href, "content/text/ch1.xhtml");
        assert_eq!(
            result[0].children[0].children[0].href,
            "content/text/ch1.xhtml#sec1"
        );
    }
}
