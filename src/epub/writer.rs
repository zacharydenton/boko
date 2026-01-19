use std::io::{self, Cursor, Seek, Write};
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::book::{Book, TocEntry};

/// Write a [`Book`] to an EPUB file on disk.
///
/// Creates a valid EPUB 2 file with OPF package document, NCX table of contents,
/// and all resources properly packaged.
///
/// # Example
///
/// ```no_run
/// use boko::{Book, Metadata, write_epub};
///
/// let mut book = Book::new();
/// book.metadata = Metadata::new("My Book").with_author("Me");
/// write_epub(&book, "output.epub")?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn write_epub<P: AsRef<Path>>(book: &Book, path: P) -> io::Result<()> {
    let file = std::fs::File::create(path)?;
    write_epub_to_writer(book, file)
}

/// Write a [`Book`] to any [`Write`] + [`Seek`] destination.
///
/// Useful for writing to memory buffers or network streams.
pub fn write_epub_to_writer<W: Write + Seek>(book: &Book, writer: W) -> io::Result<()> {
    let mut zip = ZipWriter::new(writer);

    // 1. Write mimetype (must be first, uncompressed)
    let options_stored =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // Use compression level 1 (fastest) - level 6 default is 10x slower with minimal size benefit
    let options_deflate = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1));

    zip.start_file("mimetype", options_stored)?;
    zip.write_all(b"application/epub+zip")?;

    // 2. Write META-INF/container.xml
    zip.start_file("META-INF/container.xml", options_deflate)?;
    zip.write_all(CONTAINER_XML.as_bytes())?;

    // Generate identifier once for consistency between OPF and NCX
    let identifier = if book.metadata.identifier.is_empty() {
        format!("urn:uuid:{}", crate::util::uuid_v4())
    } else {
        book.metadata.identifier.clone()
    };

    // 3. Write content.opf
    let opf = generate_opf(book, &identifier);
    zip.start_file("OEBPS/content.opf", options_deflate)?;
    zip.write_all(opf.as_bytes())?;

    // 4. Write toc.ncx
    let ncx = generate_ncx(book, &identifier);
    zip.start_file("OEBPS/toc.ncx", options_deflate)?;
    zip.write_all(ncx.as_bytes())?;

    // 5. Write all resources (skip generated files)
    for (href, resource) in &book.resources {
        // Skip files we generate ourselves
        if href == "toc.ncx" || href == "content.opf" {
            continue;
        }
        let path = format!("OEBPS/{}", href);
        // Use STORED for already-compressed formats (images, fonts)
        let opts = if is_precompressed(&resource.media_type) {
            options_stored
        } else {
            options_deflate
        };
        zip.start_file(&path, opts)?;
        zip.write_all(&resource.data)?;
    }

    zip.finish()?;
    Ok(())
}

/// Check if a media type is already compressed (no benefit from deflate)
#[inline]
fn is_precompressed(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/jpeg"
            | "image/png"
            | "image/gif"
            | "image/webp"
            | "audio/mpeg"
            | "audio/mp4"
            | "video/mp4"
            | "application/font-woff"
            | "application/font-woff2"
            | "font/woff"
            | "font/woff2"
    )
}

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

fn generate_opf(book: &Book, identifier: &str) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    
    // Write XML declaration manually as quick-xml's API is a bit verbose for this simple case
    // or use write_event(Event::Decl)
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None))).unwrap();

    // <package>
    let mut package = BytesStart::new("package");
    package.push_attribute(("xmlns", "http://www.idpf.org/2007/opf"));
    package.push_attribute(("version", "2.0"));
    package.push_attribute(("unique-identifier", "BookId"));
    writer.write_event(Event::Start(package)).unwrap();

    // <metadata>
    let mut metadata = BytesStart::new("metadata");
    metadata.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    metadata.push_attribute(("xmlns:opf", "http://www.idpf.org/2007/opf"));
    writer.write_event(Event::Start(metadata)).unwrap();

    // Helper to write simple elements
    let mut write_elem = |name: &str, content: &str, id: Option<&str>| {
        let mut elem = BytesStart::new(name);
        if let Some(id_val) = id {
            elem.push_attribute(("id", id_val));
        }
        writer.write_event(Event::Start(elem)).unwrap();
        writer.write_event(Event::Text(BytesText::new(content))).unwrap();
        writer.write_event(Event::End(BytesEnd::new(name))).unwrap();
    };

    // Dublin Core metadata
    write_elem("dc:title", &book.metadata.title, None);
    write_elem("dc:identifier", identifier, Some("BookId"));

    let language = if book.metadata.language.is_empty() {
        "en"
    } else {
        &book.metadata.language
    };
    write_elem("dc:language", language, None);

    for author in &book.metadata.authors {
        write_elem("dc:creator", author, None);
    }

    if let Some(ref publisher) = book.metadata.publisher {
        write_elem("dc:publisher", publisher, None);
    }

    if let Some(ref description) = book.metadata.description {
        write_elem("dc:description", description, None);
    }

    for subject in &book.metadata.subjects {
        write_elem("dc:subject", subject, None);
    }

    if let Some(ref date) = book.metadata.date {
        write_elem("dc:date", date, None);
    }

    if let Some(ref rights) = book.metadata.rights {
        write_elem("dc:rights", rights, None);
    }

    // Cover image meta
    if book.metadata.cover_image.is_some() {
        let mut meta = BytesStart::new("meta");
        meta.push_attribute(("name", "cover"));
        meta.push_attribute(("content", "cover-image"));
        writer.write_event(Event::Empty(meta)).unwrap();
    }

    writer.write_event(Event::End(BytesEnd::new("metadata"))).unwrap();

    // <manifest>
    writer.write_event(Event::Start(BytesStart::new("manifest"))).unwrap();

    // NCX item
    let mut ncx_item = BytesStart::new("item");
    ncx_item.push_attribute(("id", "ncx"));
    ncx_item.push_attribute(("href", "toc.ncx"));
    ncx_item.push_attribute(("media-type", "application/x-dtbncx+xml"));
    writer.write_event(Event::Empty(ncx_item)).unwrap();

    // Manifest items
    for (href, resource) in &book.resources {
        let id = href_to_id(href);
        let item_id = if book.metadata.cover_image.as_deref() == Some(href) {
            "cover-image"
        } else {
            &id
        };
        
        let mut item = BytesStart::new("item");
        item.push_attribute(("id", item_id));
        item.push_attribute(("href", href.as_str()));
        item.push_attribute(("media-type", resource.media_type.as_str()));
        writer.write_event(Event::Empty(item)).unwrap();
    }

    writer.write_event(Event::End(BytesEnd::new("manifest"))).unwrap();

    // <spine>
    let mut spine = BytesStart::new("spine");
    spine.push_attribute(("toc", "ncx"));
    writer.write_event(Event::Start(spine)).unwrap();

    // Spine items
    for item in &book.spine {
        let id = href_to_id(&item.href);
        let mut itemref = BytesStart::new("itemref");
        itemref.push_attribute(("idref", id.as_str()));
        writer.write_event(Event::Empty(itemref)).unwrap();
    }

    writer.write_event(Event::End(BytesEnd::new("spine"))).unwrap();
    writer.write_event(Event::End(BytesEnd::new("package"))).unwrap();

    String::from_utf8(writer.into_inner().into_inner()).unwrap()
}

fn generate_ncx(book: &Book, identifier: &str) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    
    // <?xml ... ?>
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None))).unwrap();

    // <!DOCTYPE ncx ...>
    writer.write_event(Event::DocType(BytesText::from_escaped(
        r#"ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd""#
    ))).unwrap();

    // <ncx>
    let mut ncx = BytesStart::new("ncx");
    ncx.push_attribute(("xmlns", "http://www.daisy.org/z3986/2005/ncx/"));
    ncx.push_attribute(("version", "2005-1"));
    writer.write_event(Event::Start(ncx)).unwrap();

    // <head>
    writer.write_event(Event::Start(BytesStart::new("head"))).unwrap();

    let mut meta_uid = BytesStart::new("meta");
    meta_uid.push_attribute(("name", "dtb:uid"));
    meta_uid.push_attribute(("content", identifier));
    writer.write_event(Event::Empty(meta_uid)).unwrap();

    let mut meta_depth = BytesStart::new("meta");
    meta_depth.push_attribute(("name", "dtb:depth"));
    meta_depth.push_attribute(("content", "1"));
    writer.write_event(Event::Empty(meta_depth)).unwrap();

    let mut meta_total = BytesStart::new("meta");
    meta_total.push_attribute(("name", "dtb:totalPageCount"));
    meta_total.push_attribute(("content", "0"));
    writer.write_event(Event::Empty(meta_total)).unwrap();

    let mut meta_max = BytesStart::new("meta");
    meta_max.push_attribute(("name", "dtb:maxPageNumber"));
    meta_max.push_attribute(("content", "0"));
    writer.write_event(Event::Empty(meta_max)).unwrap();

    writer.write_event(Event::End(BytesEnd::new("head"))).unwrap();

    // <docTitle>
    writer.write_event(Event::Start(BytesStart::new("docTitle"))).unwrap();
    writer.write_event(Event::Start(BytesStart::new("text"))).unwrap();
    writer.write_event(Event::Text(BytesText::new(&book.metadata.title))).unwrap();
    writer.write_event(Event::End(BytesEnd::new("text"))).unwrap();
    writer.write_event(Event::End(BytesEnd::new("docTitle"))).unwrap();

    // <navMap>
    writer.write_event(Event::Start(BytesStart::new("navMap"))).unwrap();

    // Generate navPoints
    let mut play_order = 1;
    for entry in &book.toc {
        write_nav_point_recursive(&mut writer, entry, &mut play_order);
    }

    writer.write_event(Event::End(BytesEnd::new("navMap"))).unwrap();
    writer.write_event(Event::End(BytesEnd::new("ncx"))).unwrap();

    String::from_utf8(writer.into_inner().into_inner()).unwrap()
}

fn write_nav_point_recursive<W: Write>(writer: &mut Writer<W>, entry: &TocEntry, play_order: &mut usize) {
    let mut nav_point = BytesStart::new("navPoint");
    nav_point.push_attribute(("id", format!("navpoint-{}", play_order).as_str()));
    nav_point.push_attribute(("playOrder", play_order.to_string().as_str()));
    writer.write_event(Event::Start(nav_point)).unwrap();

    *play_order += 1;

    // <navLabel><text>...</text></navLabel>
    writer.write_event(Event::Start(BytesStart::new("navLabel"))).unwrap();
    writer.write_event(Event::Start(BytesStart::new("text"))).unwrap();
    writer.write_event(Event::Text(BytesText::new(&entry.title))).unwrap();
    writer.write_event(Event::End(BytesEnd::new("text"))).unwrap();
    writer.write_event(Event::End(BytesEnd::new("navLabel"))).unwrap();

    // <content src="..."/>
    let mut content = BytesStart::new("content");
    content.push_attribute(("src", entry.href.as_str()));
    writer.write_event(Event::Empty(content)).unwrap();

    for child in &entry.children {
        write_nav_point_recursive(writer, child, play_order);
    }

    writer.write_event(Event::End(BytesEnd::new("navPoint"))).unwrap();
}

fn href_to_id(href: &str) -> String {
    href.replace(['/', '.', ' ', '-'], "_")
}
