use std::io::{self, Seek, Write};
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

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
    let options_deflate =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", options_stored)?;
    zip.write_all(b"application/epub+zip")?;

    // 2. Write META-INF/container.xml
    zip.start_file("META-INF/container.xml", options_deflate)?;
    zip.write_all(CONTAINER_XML.as_bytes())?;

    // Generate identifier once for consistency between OPF and NCX
    let identifier = if book.metadata.identifier.is_empty() {
        format!("urn:uuid:{}", uuid_v4())
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
        zip.start_file(&path, options_deflate)?;
        zip.write_all(&resource.data)?;
    }

    zip.finish()?;
    Ok(())
}

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

fn generate_opf(book: &Book, identifier: &str) -> String {
    let mut opf = String::new();

    opf.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="BookId">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
"#,
    );

    // Dublin Core metadata
    opf.push_str(&format!(
        "    <dc:title>{}</dc:title>\n",
        escape_xml(&book.metadata.title)
    ));

    opf.push_str(&format!(
        "    <dc:identifier id=\"BookId\">{}</dc:identifier>\n",
        escape_xml(identifier)
    ));

    let language = if book.metadata.language.is_empty() {
        "en"
    } else {
        &book.metadata.language
    };
    opf.push_str(&format!("    <dc:language>{}</dc:language>\n", language));

    for author in &book.metadata.authors {
        opf.push_str(&format!(
            "    <dc:creator>{}</dc:creator>\n",
            escape_xml(author)
        ));
    }

    if let Some(ref publisher) = book.metadata.publisher {
        opf.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            escape_xml(publisher)
        ));
    }

    if let Some(ref description) = book.metadata.description {
        opf.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            escape_xml(description)
        ));
    }

    for subject in &book.metadata.subjects {
        opf.push_str(&format!(
            "    <dc:subject>{}</dc:subject>\n",
            escape_xml(subject)
        ));
    }

    if let Some(ref date) = book.metadata.date {
        opf.push_str(&format!("    <dc:date>{}</dc:date>\n", escape_xml(date)));
    }

    if let Some(ref rights) = book.metadata.rights {
        opf.push_str(&format!(
            "    <dc:rights>{}</dc:rights>\n",
            escape_xml(rights)
        ));
    }

    // Cover image meta
    if book.metadata.cover_image.is_some() {
        opf.push_str("    <meta name=\"cover\" content=\"cover-image\"/>\n");
    }

    opf.push_str("  </metadata>\n  <manifest>\n");

    // NCX item
    opf.push_str(
        "    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n",
    );

    // Manifest items
    for (href, resource) in &book.resources {
        let id = href_to_id(href);
        let item_id = if book.metadata.cover_image.as_deref() == Some(href) {
            "cover-image"
        } else {
            &id
        };
        opf.push_str(&format!(
            "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"/>\n",
            item_id,
            escape_xml(href),
            escape_xml(&resource.media_type)
        ));
    }

    opf.push_str("  </manifest>\n  <spine toc=\"ncx\">\n");

    // Spine items
    for item in &book.spine {
        let id = href_to_id(&item.href);
        opf.push_str(&format!("    <itemref idref=\"{}\"/>\n", id));
    }

    opf.push_str("  </spine>\n</package>\n");
    opf
}

fn generate_ncx(book: &Book, identifier: &str) -> String {
    let mut ncx = String::new();

    ncx.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd">
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content=""#,
    );

    ncx.push_str(&escape_xml(identifier));
    ncx.push_str(
        r#""/>
    <meta name="dtb:depth" content="1"/>
    <meta name="dtb:totalPageCount" content="0"/>
    <meta name="dtb:maxPageNumber" content="0"/>
  </head>
  <docTitle>
    <text>"#,
    );
    ncx.push_str(&escape_xml(&book.metadata.title));
    ncx.push_str(
        r#"</text>
  </docTitle>
  <navMap>
"#,
    );

    // Generate navPoints
    let mut play_order = 1;
    for entry in &book.toc {
        write_nav_point(&mut ncx, entry, &mut play_order, 2);
    }

    ncx.push_str("  </navMap>\n</ncx>\n");
    ncx
}

fn write_nav_point(ncx: &mut String, entry: &TocEntry, play_order: &mut usize, indent: usize) {
    let indent_str = "  ".repeat(indent);

    ncx.push_str(&format!(
        "{}<navPoint id=\"navpoint-{}\" playOrder=\"{}\">\n",
        indent_str, play_order, play_order
    ));
    ncx.push_str(&format!(
        "{}  <navLabel>\n{}    <text>{}</text>\n{}  </navLabel>\n",
        indent_str,
        indent_str,
        escape_xml(&entry.title),
        indent_str
    ));
    ncx.push_str(&format!(
        "{}  <content src=\"{}\"/>\n",
        indent_str,
        escape_xml(&entry.href)
    ));

    *play_order += 1;

    for child in &entry.children {
        write_nav_point(ncx, child, play_order, indent + 1);
    }

    ncx.push_str(&format!("{}</navPoint>\n", indent_str));
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn href_to_id(href: &str) -> String {
    href.replace(['/', '.', ' ', '-'], "_")
}

/// Generate a simple UUID v4 (random)
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    // Simple PRNG for UUID generation (not cryptographically secure, but fine for identifiers)
    let mut state = seed;
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 33) as u8;
    }

    // Set version (4) and variant (2)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}
