//! EPUB format importer - handles all IO.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use zip::ZipArchive;

use crate::dom::Stylesheet;
use crate::epub::{parse_container_xml, parse_nav_landmarks, parse_nav_toc, parse_ncx, parse_opf};
use crate::import::{ChapterId, Importer, SpineEntry, resolve_path_based_href};
use crate::io::{ByteSource, ByteSourceCursor, FileSource};
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, Landmark, Metadata, TocEntry};

impl From<zip::result::ZipError> for crate::Error {
    fn from(e: zip::result::ZipError) -> Self {
        // A genuine I/O failure while reading the archive is not a malformed
        // book — preserve it (and its ErrorKind) as Error::Io. Only structural
        // ZIP problems become Malformed.
        match e {
            zip::result::ZipError::Io(io) => crate::Error::Io(io),
            other => crate::Error::Malformed {
                format: crate::Format::Epub,
                context: other.to_string(),
            },
        }
    }
}

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

    /// All asset paths in the ZIP (archive entry names, forward slashes).
    assets: Vec<String>,

    /// Cached parsed stylesheets. Behind a lock so parallel chapter
    /// compilation ([`Importer::load_chapters`]) can share it through `&self`.
    css_cache: RwLock<HashMap<String, Arc<Stylesheet>>>,

    // --- Link resolution ---
    /// Maps path (without fragment) -> ChapterId
    path_to_chapter: HashMap<String, ChapterId>,

    /// Maps "path#id" -> GlobalNodeId for fragment resolution. Behind a lock
    /// so `index_anchors` runs through `&self` like every other access.
    anchor_map: RwLock<HashMap<String, GlobalNodeId>>,
}

#[derive(Clone, Copy)]
struct ZipEntryLoc {
    data_offset: u64,
    compressed_size: u64,
    uncompressed_size: u64,
    compression: u16, // 0 = Store, 8 = Deflate
}

impl Importer for EpubImporter {
    fn open(path: &Path) -> crate::Result<Self> {
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

    fn load_raw(&self, id: ChapterId) -> crate::Result<Vec<u8>> {
        let path = self
            .spine_paths
            .get(id.0 as usize)
            .ok_or_else(|| crate::Error::NotFound {
                what: format!("chapter {}", id.0),
            })?;
        self.read_entry(path)
    }

    fn list_assets(&self) -> &[String] {
        &self.assets
    }

    fn load_asset(&self, path: &str) -> crate::Result<Vec<u8>> {
        self.read_entry(path)
    }

    fn load_stylesheet(&self, path: &str) -> Option<Arc<Stylesheet>> {
        if let Ok(cache) = self.css_cache.read()
            && let Some(sheet) = cache.get(path)
        {
            return Some(Arc::clone(sheet));
        }
        let css_bytes = self.read_entry(path).ok()?;
        let css_str = String::from_utf8_lossy(&css_bytes);
        let sheet = Arc::new(Stylesheet::parse(&css_str));
        // Two threads may race to parse the same sheet; the first insert wins
        // so every chapter ends up sharing one Arc.
        match self.css_cache.write() {
            Ok(mut cache) => Some(Arc::clone(cache.entry(path.to_string()).or_insert(sheet))),
            Err(_) => Some(sheet),
        }
    }

    fn index_anchors(&self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        let mut anchor_map = HashMap::new();

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
                    anchor_map.insert(key, GlobalNodeId::new(*chapter_id, node_id));
                }
            }
        }

        if let Ok(mut map) = self.anchor_map.write() {
            *map = anchor_map;
        }
    }

    fn resolve_href(&self, from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        let from_path = self.source_id(from_chapter)?;
        resolve_path_based_href(
            from_path,
            href,
            |p| self.path_to_chapter.get(p).copied(),
            |k| self.anchor_map.read().ok().and_then(|m| m.get(k).copied()),
        )
    }
}

impl EpubImporter {
    /// Create an importer from a ByteSource.
    pub fn from_source(source: Arc<dyn ByteSource>) -> crate::Result<Self> {
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
                    data_offset: file.data_start().unwrap(),
                    compressed_size: file.compressed_size(),
                    uncompressed_size: file.size(),
                    compression: compression_to_u16(file.compression()),
                },
            );
            // Directory entries are ZIP bookkeeping, not assets; surfacing
            // them made re-exports reference "files" like `OEBPS/images/`.
            if !name.ends_with('/') {
                assets.push(name);
            }
        }

        // 2. Find OPF path from container.xml
        let container_bytes = read_entry(&source, &zip_index, "META-INF/container.xml")?;
        let opf_path = parse_container_xml(&container_bytes)?;
        // Directory of the OPF (including trailing slash), or "" for root.
        let opf_base = match opf_path.rfind('/') {
            Some(idx) => opf_path[..=idx].to_string(),
            None => String::new(),
        };

        // 3. Parse OPF
        let opf_bytes = read_entry(&source, &zip_index, &opf_path)?;
        let hint_encoding = crate::util::extract_xml_encoding(&opf_bytes);
        let opf_str = crate::util::decode_text(&opf_bytes, hint_encoding);
        let opf = parse_opf(&opf_str)?;

        // 4. Build spine. Manifest hrefs are URLs (may be percent-encoded);
        // archive entry names are literal, so decode at this join point.
        let mut spine = Vec::new();
        let mut spine_paths = Vec::new();

        for spine_id in &opf.spine_ids {
            if let Some((href, _media_type)) = opf.manifest.get(spine_id) {
                let full_path = format!("{}{}", opf_base, crate::util::percent_decode_href(href));
                let size_estimate = zip_index
                    .get(&full_path)
                    .map(|loc| loc.compressed_size as usize)
                    .unwrap_or(0);

                spine.push(SpineEntry {
                    // Id by position in spine_paths, not the itemref index: a
                    // dangling idref (no manifest entry) is skipped, and using
                    // the raw index would desync every later ChapterId from
                    // its path in spine_paths.
                    id: ChapterId(spine_paths.len() as u32),
                    size_estimate,
                });
                spine_paths.push(full_path);
            }
        }

        // Load the EPUB 3 nav document once, if declared: it serves both the
        // TOC fallback (step 5) and landmarks (step 6).
        let nav_str: Option<String> = opf.nav_href.as_ref().and_then(|nav_href| {
            let nav_path = format!("{}{}", opf_base, crate::util::percent_decode_href(nav_href));
            read_entry(&source, &zip_index, &nav_path)
                .ok()
                .map(|nav_bytes| {
                    let hint_encoding = crate::util::extract_xml_encoding(&nav_bytes);
                    crate::util::decode_text(&nav_bytes, hint_encoding).into_owned()
                })
        });

        // 5. Parse TOC. The NCX is used when it yields entries (existing
        // behavior, kept for dual-TOC books to avoid churn); EPUB 3 makes the
        // nav document canonical and the NCX optional, so books without a
        // usable NCX fall back to `<nav epub:type="toc">`.
        let mut toc = if let Some(ncx_href) = &opf.ncx_href {
            let ncx_path = format!("{}{}", opf_base, crate::util::percent_decode_href(ncx_href));
            if let Ok(ncx_bytes) = read_entry(&source, &zip_index, &ncx_path) {
                let hint_encoding = crate::util::extract_xml_encoding(&ncx_bytes);
                let ncx_str = crate::util::decode_text(&ncx_bytes, hint_encoding);
                // Navigation is auxiliary: a malformed NCX degrades to an
                // empty TOC (like a missing one) instead of failing the open.
                let toc_entries = parse_ncx(&ncx_str).unwrap_or_default();
                // Prepend base path to hrefs (NCX uses relative paths)
                prepend_base_to_toc(&toc_entries, &opf_base)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        if toc.is_empty()
            && let Some(nav_str) = &nav_str
        {
            // Same leniency as the NCX: a malformed nav document must not
            // fail the whole book.
            let toc_entries = parse_nav_toc(nav_str).unwrap_or_default();
            toc = prepend_base_to_toc(&toc_entries, &opf_base);
        }

        // 6. Parse landmarks from EPUB 3 nav document
        let landmarks = if let Some(nav_str) = &nav_str {
            let mut parsed = parse_nav_landmarks(nav_str).unwrap_or_default();
            // Prepend base path to hrefs (nav uses relative, URL-encoded paths)
            for landmark in &mut parsed {
                if !landmark.href.starts_with('#') && !landmark.href.is_empty() {
                    landmark.href = format!(
                        "{}{}",
                        opf_base,
                        crate::util::percent_decode_href(&landmark.href)
                    );
                }
            }
            parsed
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

        // Resolve cover_image to an absolute (zip-relative) path so it matches
        // asset keys downstream. The OPF parser leaves it as a manifest href
        // relative to opf_base; like all manifest hrefs it may be
        // percent-encoded while asset keys are literal.
        let mut metadata = opf.metadata;
        if let Some(ref href) = metadata.cover_image
            && !href.is_empty()
        {
            metadata.cover_image = Some(format!(
                "{}{}",
                opf_base,
                crate::util::percent_decode_href(href)
            ));
        }

        Ok(Self {
            source,
            zip_index,
            metadata,
            toc,
            landmarks,
            spine,
            spine_paths,
            assets,
            path_to_chapter,
            anchor_map: RwLock::new(HashMap::new()),
            css_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Read and decompress a ZIP entry by path.
    fn read_entry(&self, path: &str) -> crate::Result<Vec<u8>> {
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
) -> crate::Result<Vec<u8>> {
    let loc = index.get(path).ok_or_else(|| crate::Error::NotFound {
        what: format!("{} (in EPUB archive)", path),
    })?;

    // Read compressed data via random access
    let compressed = source.read_at(loc.data_offset, loc.compressed_size as usize)?;

    // Decompress
    match loc.compression {
        0 => Ok(compressed), // Stored
        8 => {
            // Deflate. The uncompressed size is an untrusted central-directory
            // field, so cap the output to stop decompression bombs rather than
            // trusting it (see `bounded_inflate`).
            let out = crate::util::bounded_inflate(
                &compressed,
                loc.uncompressed_size,
                crate::util::MAX_DECOMPRESSED_ENTRY,
            )?;
            Ok(out)
        }
        method => Err(crate::Error::Malformed {
            format: crate::Format::Epub,
            context: format!("unsupported compression method: {}", method),
        }),
    }
}

fn compression_to_u16(method: zip::CompressionMethod) -> u16 {
    match method {
        zip::CompressionMethod::Stored => 0,
        zip::CompressionMethod::Deflated => 8,
        _ => 255,
    }
}

/// Prepend base path to TOC entry hrefs (NCX/nav use relative paths).
///
/// TOC hrefs are URLs: percent-escapes are decoded here (path and fragment
/// separately) so the stored hrefs match literal archive entry names.
fn prepend_base_to_toc(entries: &[TocEntry], base: &str) -> Vec<TocEntry> {
    entries
        .iter()
        .map(|entry| {
            let href = if entry.href.is_empty() {
                entry.href.clone()
            } else if entry.href.starts_with('#') {
                crate::util::percent_decode_href(&entry.href).into_owned()
            } else {
                format!("{}{}", base, crate::util::percent_decode_href(&entry.href))
            };
            TocEntry {
                title: entry.title.clone(),
                href,
                children: prepend_base_to_toc(&entry.children, base),
                play_order: entry.play_order,
                target: None,
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
