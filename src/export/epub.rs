//! EPUB exporter.
//!
//! Creates EPUB 2/3 files from Book structures using passthrough for content.

use std::collections::HashMap;
use std::io::{self, Seek, Write};

use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::model::{Book, TocEntry};
use crate::util::guess_media_type;

use super::html_synth::escape_xml;

use super::Exporter;

/// Configuration for EPUB export.
#[derive(Debug, Clone, Default)]
pub struct EpubConfig {
    /// Compression level for deflate (0-9, default 6).
    pub compression_level: Option<u32>,
    /// If true, normalize content through IR pipeline for clean, consistent output.
    /// Default is false (passthrough mode preserves original HTML/CSS).
    pub normalize: bool,
}

/// EPUB format exporter.
///
/// Creates standard EPUB files compatible with most e-readers.
///
/// # Example
///
/// ```no_run
/// use boko::Book;
/// use boko::export::{EpubExporter, Exporter};
/// use std::fs::File;
///
/// let mut book = Book::open("input.azw3")?;
/// let mut file = File::create("output.epub")?;
/// EpubExporter::new().export(&mut book, &mut file)?;
/// # Ok::<(), boko::Error>(())
/// ```
pub struct EpubExporter {
    config: EpubConfig,
}

impl EpubExporter {
    /// Create a new exporter with default configuration.
    pub fn new() -> Self {
        Self {
            config: EpubConfig::default(),
        }
    }

    /// Configure the exporter with custom settings.
    pub fn with_config(mut self, config: EpubConfig) -> Self {
        self.config = config;
        self
    }
}

impl Default for EpubExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter for EpubExporter {
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> crate::Result<()> {
        // Use normalized mode if explicitly requested OR if the source format requires it
        // (e.g., KFX raw content is binary Ion, not HTML)
        if self.config.normalize || book.requires_normalized_export() {
            Ok(self.export_normalized(book, writer)?)
        } else {
            Ok(self.export_raw(book, writer)?)
        }
    }
}

impl EpubExporter {
    /// Export with passthrough mode (preserves original HTML/CSS).
    fn export_raw<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> crate::Result<()> {
        // Resolve TOC fragments before we generate the NCX. AZW3 and MOBI
        // importers leave TOC entries with bare chapter hrefs until
        // `resolve_toc()` populates the `#fileposN` / `#id` suffix from the
        // book's NCX / position map. Without this call the exported NCX has
        // unresolvable anchors and readers land on chapter starts instead of
        // the intended in-chapter targets.
        book.resolve_toc();

        let mut zip = ZipWriter::new(writer);

        let compression_level = self.config.compression_level.unwrap_or(6);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(compression_level as i64));

        // 1. Write mimetype (must be first, uncompressed)
        zip.start_file("mimetype", stored).map_err(io_error)?;
        zip.write_all(b"application/epub+zip")?;

        // 2. Write container.xml
        zip.start_file("META-INF/container.xml", deflated)
            .map_err(io_error)?;
        zip.write_all(CONTAINER_XML)?;

        // 3. Collect content info for manifest
        let spine: Vec<_> = book.spine().to_vec();
        let mut manifest_items: Vec<ManifestItem> = Vec::new();
        let mut spine_refs: Vec<String> = Vec::new();

        // The importer surfaces every ZIP entry as an asset, including the spine
        // XHTML documents. Those are written as chapters below, so track their
        // output paths and skip them when emitting assets — otherwise each spine
        // file is written (and added to the manifest) twice, which fails with a
        // duplicate-filename error on strict ZIP writers.
        let chapter_paths: std::collections::HashSet<String> = spine
            .iter()
            .map(|entry| {
                format!(
                    "OEBPS/{}",
                    sanitize_path(book.source_id(entry.id).unwrap_or("unknown.xhtml"))
                )
            })
            .collect();

        // Add chapters to manifest
        for (i, entry) in spine.iter().enumerate() {
            let source_path = book.source_id(entry.id).unwrap_or("unknown.xhtml");
            let filename = format!("chapter_{}.xhtml", i);
            let id = format!("chapter_{}", i);

            manifest_items.push(ManifestItem {
                id: id.clone(),
                href: filename,
                media_type: "application/xhtml+xml",
                properties: None,
            });
            spine_refs.push(id);

            // Store original path for content writing
            manifest_items.last_mut().unwrap().href =
                format!("OEBPS/{}", sanitize_path(source_path));
        }

        // Add assets to manifest
        let assets: Vec<_> = book.list_assets().to_vec();
        let mut asset_map: HashMap<String, String> = HashMap::new();

        for (i, asset_path) in assets.iter().enumerate() {
            let href = format!("OEBPS/{}", sanitize_path(asset_path));
            // Skip spine documents already emitted as chapters (see above).
            if chapter_paths.contains(&href) {
                continue;
            }
            let media_type = guess_media_type(asset_path);
            let id = format!("asset_{}", i);

            manifest_items.push(ManifestItem {
                id: id.clone(),
                href: href.clone(),
                media_type,
                properties: None,
            });
            asset_map.insert(asset_path.clone(), href);
        }

        // 4. Write content.opf
        let opf = generate_opf(book.metadata(), &manifest_items, &spine_refs);
        zip.start_file("OEBPS/content.opf", deflated)
            .map_err(io_error)?;
        zip.write_all(opf.as_bytes())?;

        // 5. Write toc.ncx
        let ncx = generate_ncx(book.metadata(), book.toc());
        zip.start_file("OEBPS/toc.ncx", deflated)
            .map_err(io_error)?;
        zip.write_all(ncx.as_bytes())?;

        // 6. Write chapters
        for entry in &spine {
            let source_path = book
                .source_id(entry.id)
                .unwrap_or("unknown.xhtml")
                .to_string();
            let content = book.load_raw(entry.id)?;
            let zip_path = format!("OEBPS/{}", sanitize_path(&source_path));

            zip.start_file(&zip_path, deflated).map_err(io_error)?;
            zip.write_all(&content)?;
        }

        // 7. Write assets (skipping spine documents already written as chapters).
        for asset_path in &assets {
            let zip_path = format!("OEBPS/{}", sanitize_path(asset_path));
            if chapter_paths.contains(&zip_path) {
                continue;
            }
            let content = book.load_asset(asset_path)?;

            let opts = asset_options(&zip_path, &content, stored, deflated);
            zip.start_file(&zip_path, opts).map_err(io_error)?;
            zip.write_all(&content)?;
        }

        zip.finish().map_err(io_error)?;
        Ok(())
    }

    /// Export with normalized content (IR pipeline produces clean, consistent output).
    fn export_normalized<W: Write + Seek>(
        &self,
        book: &mut Book,
        writer: &mut W,
    ) -> io::Result<()> {
        use super::normalize::normalize_book;

        // Resolve TOC fragments before generating the NCX. Same rationale as
        // `export_raw`: AZW3 / MOBI importers leave TOC entries with bare
        // chapter hrefs until this is called.
        book.resolve_toc();

        // Snapshot every asset the importer surfaces. The normalized content
        // pipeline only records assets it actively references, which misses
        // resources like embedded fonts that are referenced from CSS rather
        // than from the IR DOM. We add those back into the manifest and ZIP
        // below.
        let all_assets: Vec<_> = book.list_assets().to_vec();

        // Normalize the book content
        let content = normalize_book(book)?;

        let mut zip = ZipWriter::new(writer);

        let compression_level = self.config.compression_level.unwrap_or(6);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(compression_level as i64));

        // 1. Write mimetype (must be first, uncompressed)
        zip.start_file("mimetype", stored).map_err(io_error)?;
        zip.write_all(b"application/epub+zip")?;

        // 2. Write container.xml
        zip.start_file("META-INF/container.xml", deflated)
            .map_err(io_error)?;
        zip.write_all(CONTAINER_XML)?;

        // 3. Build manifest
        let mut manifest_items: Vec<ManifestItem> = Vec::new();
        let mut spine_refs: Vec<String> = Vec::new();

        // Add stylesheet to manifest
        if !content.css.is_empty() {
            manifest_items.push(ManifestItem {
                id: "stylesheet".to_string(),
                href: "OEBPS/style.css".to_string(),
                media_type: "text/css",
                properties: None,
            });
        }

        // Add chapters to manifest
        for (i, _) in content.chapters.iter().enumerate() {
            let id = format!("chapter_{}", i);
            let href = format!("OEBPS/chapter_{}.xhtml", i);

            manifest_items.push(ManifestItem {
                id: id.clone(),
                href,
                media_type: "application/xhtml+xml",
                properties: None,
            });
            spine_refs.push(id);
        }

        // EPUB 3 requires exactly one manifest item with the `nav` property.
        manifest_items.push(ManifestItem {
            id: "nav".to_string(),
            href: "OEBPS/nav.xhtml".to_string(),
            media_type: "application/xhtml+xml",
            properties: Some("nav"),
        });

        // Add assets to manifest (from normalized content)
        for (asset_idx, asset_path) in content.assets.iter().enumerate() {
            let media_type = guess_media_type(asset_path);
            let id = format!("asset_{}", asset_idx);
            let href = format!("OEBPS/{}", sanitize_path(asset_path));

            manifest_items.push(ManifestItem {
                id,
                href,
                media_type,
                properties: None,
            });
        }

        // Add font assets the importer surfaced that aren't already covered
        // by normalized content. Fonts are typically referenced from CSS, not
        // from DOM nodes, so `normalize_book` doesn't pull them into
        // `content.assets`. Without this we'd emit `@font-face` rules whose
        // `src:` URLs point at files we never wrote into the ZIP.
        let mut extra_font_idx = 0;
        for asset_path in &all_assets {
            if !asset_path.starts_with("fonts/") {
                continue;
            }
            if content.assets.contains(asset_path) {
                continue;
            }
            manifest_items.push(ManifestItem {
                id: format!("font_{}", extra_font_idx),
                href: format!("OEBPS/{}", sanitize_path(asset_path)),
                media_type: guess_media_type(asset_path),
                properties: None,
            });
            extra_font_idx += 1;
        }

        // 4. Write content.opf
        let opf = generate_opf(book.metadata(), &manifest_items, &spine_refs);
        zip.start_file("OEBPS/content.opf", deflated)
            .map_err(io_error)?;
        zip.write_all(opf.as_bytes())?;

        // 5. Write toc.ncx. Rewrite the TOC hrefs (original source paths / bare
        // `#anchor`s) to the emitted chapter files so navigation resolves.
        let rewritten_toc = content.rewrite_toc(book.toc());
        let ncx = generate_ncx(book.metadata(), &rewritten_toc);
        zip.start_file("OEBPS/toc.ncx", deflated)
            .map_err(io_error)?;
        zip.write_all(ncx.as_bytes())?;

        // 5b. Write the EPUB 3 nav document (same TOC, XHTML form).
        let nav = generate_nav(&book.metadata().title, &rewritten_toc);
        zip.start_file("OEBPS/nav.xhtml", deflated)
            .map_err(io_error)?;
        zip.write_all(nav.as_bytes())?;

        // 6. Write unified stylesheet
        if !content.css.is_empty() {
            zip.start_file("OEBPS/style.css", deflated)
                .map_err(io_error)?;
            zip.write_all(content.css.as_bytes())?;
        }

        // 7. Write synthesized chapters
        for (i, chapter) in content.chapters.iter().enumerate() {
            let zip_path = format!("OEBPS/chapter_{}.xhtml", i);
            zip.start_file(&zip_path, deflated).map_err(io_error)?;
            zip.write_all(chapter.document.as_bytes())?;
        }

        // 8. Write assets referenced by normalized content
        for asset_path in &content.assets {
            let zip_path = format!("OEBPS/{}", sanitize_path(asset_path));

            // Try to load the asset from the book
            if let Ok(data) = book.load_asset(asset_path) {
                let opts = asset_options(&zip_path, &data, stored, deflated);
                zip.start_file(&zip_path, opts).map_err(io_error)?;
                zip.write_all(&data)?;
            }
        }

        // 9. Write font assets not already covered by normalized content.
        // Matches the manifest entries added above.
        for asset_path in &all_assets {
            if !asset_path.starts_with("fonts/") {
                continue;
            }
            if content.assets.contains(asset_path) {
                continue;
            }
            let zip_path = format!("OEBPS/{}", sanitize_path(asset_path));
            if let Ok(data) = book.load_asset(asset_path) {
                let opts = asset_options(&zip_path, &data, stored, deflated);
                zip.start_file(&zip_path, opts).map_err(io_error)?;
                zip.write_all(&data)?;
            }
        }

        zip.finish().map_err(io_error)?;
        Ok(())
    }
}

/// Convert zip error to io error.
fn io_error<E: std::error::Error + Send + Sync + 'static>(e: E) -> io::Error {
    io::Error::other(e)
}

/// Container.xml template.
const CONTAINER_XML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

struct ManifestItem {
    id: String,
    href: String,
    media_type: &'static str,
    /// Optional OPF `properties` (e.g. `nav`, `cover-image`).
    properties: Option<&'static str>,
}

/// Pick a ZIP compression method for an asset. Images and fonts are already
/// entropy-coded, so re-deflating them burns CPU for ~0% size gain (the
/// dominant cost of exporting an image-heavy book) — store them uncompressed.
fn asset_options(
    path: &str,
    data: &[u8],
    stored: SimpleFileOptions,
    deflated: SimpleFileOptions,
) -> SimpleFileOptions {
    let fmt = crate::util::detect_media_format(path, data);
    if fmt.is_image() || fmt.is_font() {
        stored
    } else {
        deflated
    }
}

/// Generate content.opf from metadata and manifest.
fn generate_opf(
    metadata: &crate::model::Metadata,
    manifest: &[ManifestItem],
    spine_refs: &[String],
) -> String {
    let mut opf = String::new();

    // Use EPUB3 for extended metadata support
    opf.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="BookId">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
"#,
    );

    // Track IDs for refinements
    let mut next_id = 1;

    // Title with optional file-as refinement
    let title_id = format!("title{}", next_id);
    next_id += 1;
    opf.push_str(&format!(
        "    <dc:title id=\"{}\">{}</dc:title>\n",
        title_id,
        escape_xml(&metadata.title)
    ));
    if let Some(ref title_sort) = metadata.title_sort {
        opf.push_str(&format!(
            "    <meta refines=\"#{}\" property=\"file-as\">{}</meta>\n",
            title_id,
            escape_xml(title_sort)
        ));
    }

    // Authors with optional file-as refinement for first author
    for (i, author) in metadata.authors.iter().enumerate() {
        let creator_id = format!("creator{}", next_id);
        next_id += 1;
        opf.push_str(&format!(
            "    <dc:creator id=\"{}\">{}</dc:creator>\n",
            creator_id,
            escape_xml(author)
        ));
        // Add file-as for first author if available
        if i == 0
            && let Some(ref author_sort) = metadata.author_sort
        {
            opf.push_str(&format!(
                "    <meta refines=\"#{}\" property=\"file-as\">{}</meta>\n",
                creator_id,
                escape_xml(author_sort)
            ));
        }
    }

    // Language
    if !metadata.language.is_empty() {
        opf.push_str(&format!(
            "    <dc:language>{}</dc:language>\n",
            escape_xml(&metadata.language)
        ));
    } else {
        opf.push_str("    <dc:language>en</dc:language>\n");
    }

    // Identifier
    if !metadata.identifier.is_empty() {
        opf.push_str(&format!(
            "    <dc:identifier id=\"BookId\">{}</dc:identifier>\n",
            escape_xml(&metadata.identifier)
        ));
    } else {
        opf.push_str("    <dc:identifier id=\"BookId\">urn:uuid:00000000-0000-0000-0000-000000000000</dc:identifier>\n");
    }

    // dcterms:modified (required for EPUB3)
    if let Some(ref modified) = metadata.modified_date {
        opf.push_str(&format!(
            "    <meta property=\"dcterms:modified\">{}</meta>\n",
            escape_xml(modified)
        ));
    } else {
        // Generate a timestamp for EPUB3 compliance
        opf.push_str("    <meta property=\"dcterms:modified\">2024-01-01T00:00:00Z</meta>\n");
    }

    // Contributors with role refinements
    for contrib in &metadata.contributors {
        let contrib_id = format!("contrib{}", next_id);
        next_id += 1;
        opf.push_str(&format!(
            "    <dc:contributor id=\"{}\">{}</dc:contributor>\n",
            contrib_id,
            escape_xml(&contrib.name)
        ));
        if let Some(ref role) = contrib.role {
            opf.push_str(&format!(
                "    <meta refines=\"#{}\" property=\"role\" scheme=\"marc:relators\">{}</meta>\n",
                contrib_id,
                escape_xml(role)
            ));
        }
        if let Some(ref file_as) = contrib.file_as {
            opf.push_str(&format!(
                "    <meta refines=\"#{}\" property=\"file-as\">{}</meta>\n",
                contrib_id,
                escape_xml(file_as)
            ));
        }
    }

    // Collection/series info
    if let Some(ref coll) = metadata.collection {
        let coll_id = format!("collection{}", next_id);
        next_id += 1;
        opf.push_str(&format!(
            "    <meta property=\"belongs-to-collection\" id=\"{}\">{}</meta>\n",
            coll_id,
            escape_xml(&coll.name)
        ));
        if let Some(ref coll_type) = coll.collection_type {
            opf.push_str(&format!(
                "    <meta refines=\"#{}\" property=\"collection-type\">{}</meta>\n",
                coll_id,
                escape_xml(coll_type)
            ));
        }
        if let Some(pos) = coll.position {
            let pos_str = if pos.fract() == 0.0 {
                format!("{}", pos as i64)
            } else {
                format!("{}", pos)
            };
            opf.push_str(&format!(
                "    <meta refines=\"#{}\" property=\"group-position\">{}</meta>\n",
                coll_id, pos_str
            ));
        }
    }

    // Suppress unused variable warning
    let _ = next_id;

    // Optional metadata
    if let Some(ref publisher) = metadata.publisher {
        opf.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            escape_xml(publisher)
        ));
    }
    if let Some(ref description) = metadata.description {
        opf.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            escape_xml(description)
        ));
    }
    for subject in &metadata.subjects {
        opf.push_str(&format!(
            "    <dc:subject>{}</dc:subject>\n",
            escape_xml(subject)
        ));
    }
    if let Some(ref date) = metadata.date {
        opf.push_str(&format!("    <dc:date>{}</dc:date>\n", escape_xml(date)));
    }
    if let Some(ref rights) = metadata.rights {
        opf.push_str(&format!(
            "    <dc:rights>{}</dc:rights>\n",
            escape_xml(rights)
        ));
    }

    opf.push_str("  </metadata>\n");

    // Manifest
    opf.push_str("  <manifest>\n");
    opf.push_str(
        "    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n",
    );

    for item in manifest {
        // Get relative path from OEBPS/
        let href = item.href.strip_prefix("OEBPS/").unwrap_or(&item.href);
        let properties = match item.properties {
            Some(p) => format!(" properties=\"{p}\""),
            None => String::new(),
        };
        opf.push_str(&format!(
            "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"{}/>\n",
            escape_xml(&item.id),
            escape_xml(href),
            escape_xml(item.media_type),
            properties,
        ));
    }
    opf.push_str("  </manifest>\n");

    // Spine
    opf.push_str("  <spine toc=\"ncx\">\n");
    for id in spine_refs {
        opf.push_str(&format!("    <itemref idref=\"{}\"/>\n", escape_xml(id)));
    }
    opf.push_str("  </spine>\n");

    opf.push_str("</package>\n");
    opf
}

/// Generate toc.ncx from TOC entries.
fn generate_ncx(metadata: &crate::model::Metadata, toc: &[TocEntry]) -> String {
    let mut ncx = String::new();

    ncx.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd">
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content=""#,
    );
    ncx.push_str(&escape_xml(&metadata.identifier));
    ncx.push_str(
        r#""/>
    <meta name="dtb:depth" content="1"/>
    <meta name="dtb:totalPageCount" content="0"/>
    <meta name="dtb:maxPageNumber" content="0"/>
  </head>
  <docTitle>
    <text>"#,
    );
    ncx.push_str(&escape_xml(&metadata.title));
    ncx.push_str(
        r#"</text>
  </docTitle>
  <navMap>
"#,
    );

    let mut play_order = 1;
    write_nav_points(&mut ncx, toc, &mut play_order, 2);

    ncx.push_str("  </navMap>\n</ncx>\n");
    ncx
}

/// Recursively write navPoint elements.
fn write_nav_points(ncx: &mut String, entries: &[TocEntry], play_order: &mut usize, indent: usize) {
    // `indent` grows one per nesting level; cap it so a pathologically deep TOC
    // can't overflow the stack during export.
    if indent > crate::util::MAX_TREE_DEPTH {
        return;
    }
    let indent_str = "  ".repeat(indent);

    for entry in entries {
        ncx.push_str(&format!(
            "{}<navPoint id=\"navPoint-{}\" playOrder=\"{}\">\n",
            indent_str, play_order, play_order
        ));
        ncx.push_str(&format!(
            "{}  <navLabel><text>{}</text></navLabel>\n",
            indent_str,
            escape_xml(&entry.title)
        ));
        ncx.push_str(&format!(
            "{}  <content src=\"{}\"/>\n",
            indent_str,
            escape_xml(&entry.href)
        ));

        *play_order += 1;

        if !entry.children.is_empty() {
            write_nav_points(ncx, &entry.children, play_order, indent + 1);
        }

        ncx.push_str(&format!("{}</navPoint>\n", indent_str));
    }
}

/// Generate the EPUB 3 nav document (`nav.xhtml`) from TOC entries.
fn generate_nav(title: &str, toc: &[TocEntry]) -> String {
    let mut doc = String::new();
    doc.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n\
         <head>\n  <meta charset=\"utf-8\"/>\n  <title>",
    );
    doc.push_str(&escape_xml(title));
    doc.push_str("</title>\n</head>\n<body>\n  <nav epub:type=\"toc\" id=\"toc\">\n");
    if !toc.is_empty() {
        write_nav_list(&mut doc, toc, 2);
    }
    doc.push_str("  </nav>\n</body>\n</html>\n");
    doc
}

fn write_nav_list(doc: &mut String, entries: &[TocEntry], indent: usize) {
    if indent > crate::util::MAX_TREE_DEPTH {
        return;
    }
    let pad = "  ".repeat(indent);
    doc.push_str(&pad);
    doc.push_str("<ol>\n");
    for entry in entries {
        doc.push_str(&pad);
        doc.push_str("  <li>");
        // The nav spec requires a resolvable target; entries without one use a
        // <span> label instead of an empty-href <a>.
        if entry.href.is_empty() {
            doc.push_str("<span>");
            doc.push_str(&escape_xml(&entry.title));
            doc.push_str("</span>");
        } else {
            doc.push_str("<a href=\"");
            doc.push_str(&escape_xml(&entry.href));
            doc.push_str("\">");
            doc.push_str(&escape_xml(&entry.title));
            doc.push_str("</a>");
        }
        if entry.children.is_empty() {
            doc.push_str("</li>\n");
        } else {
            doc.push('\n');
            write_nav_list(doc, &entry.children, indent + 2);
            doc.push_str(&pad);
            doc.push_str("  </li>\n");
        }
    }
    doc.push_str(&pad);
    doc.push_str("</ol>\n");
}

/// Sanitize a path for use in ZIP (remove leading slashes, normalize).
fn sanitize_path(path: &str) -> String {
    path.trim_start_matches('/')
        .replace('\\', "/")
        .replace("//", "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("Hello & World"), "Hello &amp; World");
        assert_eq!(escape_xml("<tag>"), "&lt;tag&gt;");
        assert_eq!(escape_xml("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_sanitize_path() {
        assert_eq!(sanitize_path("/path/to/file.xhtml"), "path/to/file.xhtml");
        assert_eq!(sanitize_path("path\\to\\file.xhtml"), "path/to/file.xhtml");
    }

    #[test]
    fn test_guess_media_type() {
        assert_eq!(guess_media_type("file.xhtml"), "application/xhtml+xml");
        assert_eq!(guess_media_type("style.css"), "text/css");
        assert_eq!(guess_media_type("image.jpg"), "image/jpeg");
        assert_eq!(guess_media_type("font.woff2"), "font/woff2");
    }
}
