use std::io::{Read, Seek};
use std::path::Path;

use quick_xml::escape::unescape;

use crate::book::{Book, Metadata, TocEntry};
use crate::error::{Error, Result};

use super::headers::{Compression, Encoding, ExthHeader, MobiHeader, PdbHeader, NULL_INDEX};
use super::huffcdic::HuffCdicReader;
use super::index::{
    parse_div_index, parse_ncx_index, parse_skel_index, read_index, Cncx, DivElement, NcxEntry,
    SkeletonFile,
};

/// Extracted resources from MOBI file
struct ExtractedResources {
    /// Images as (data, media_type)
    images: Vec<(Vec<u8>, String)>,
    /// Fonts as (data, extension)
    fonts: Vec<(Vec<u8>, String)>,
    /// Maps resource index to href (for CSS reference resolution)
    resource_map: Vec<Option<String>>,
}

/// Read a MOBI/AZW3 file into a Book
pub fn read_mobi<P: AsRef<Path>>(path: P) -> Result<Book> {
    let file = std::fs::File::open(path)?;
    read_mobi_from_reader(file)
}

/// Read a MOBI from any Read + Seek source
pub fn read_mobi_from_reader<R: Read + Seek>(mut reader: R) -> Result<Book> {
    // 1. Parse PDB header
    let pdb = PdbHeader::read(&mut reader)?;

    if pdb.num_records < 2 {
        return Err(Error::InvalidMobi("Not enough records".into()));
    }

    // 2. Read and parse MOBI header (record 0 - might be MOBI6 for combo files)
    let record0 = pdb.read_record(&mut reader, 0)?;
    let mobi6_header = MobiHeader::parse(&record0)?;

    // 3. Check for encryption
    if mobi6_header.encryption != 0 {
        return Err(Error::InvalidMobi(
            "Encrypted MOBI files are not supported".into(),
        ));
    }

    // 4. Parse EXTH if present
    let exth = if mobi6_header.has_exth() && mobi6_header.header_length > 0 {
        let exth_start = 16 + mobi6_header.header_length as usize;
        if exth_start < record0.len() {
            ExthHeader::parse(&record0[exth_start..], mobi6_header.encoding).ok()
        } else {
            None
        }
    } else {
        None
    };

    // 5. For combo MOBI6+KF8 files, we need to find the KF8 header
    // This is indicated by a BOUNDARY marker record followed by a KF8 header
    // EXTH 121 MIGHT point to the KF8 header, but only if there's a BOUNDARY marker before it
    let (mobi, kf8_record_offset) = {
        // First check if record 0 header is already KF8 (pure KF8 file)
        if mobi6_header.mobi_version == 8 {
            (mobi6_header, 0)
        } else if let Some(kf8_header_idx) = exth.as_ref().and_then(|e| e.kf8_boundary) {
            // Verify BOUNDARY marker exists at kf8_header_idx - 1
            if kf8_header_idx > 0 && (kf8_header_idx as usize) < pdb.num_records as usize {
                let boundary_record = pdb.read_record(&mut reader, kf8_header_idx as usize - 1)?;
                if boundary_record.starts_with(b"BOUNDARY") {
                    // Read KF8 header from kf8_header_idx
                    let kf8_record = pdb.read_record(&mut reader, kf8_header_idx as usize)?;
                    match MobiHeader::parse(&kf8_record) {
                        Ok(kf8_mobi) => {
                            // KF8 indices are relative to (kf8_header_idx - 1) as the base
                            (kf8_mobi, (kf8_header_idx as usize).saturating_sub(1))
                        }
                        Err(_) => {
                            (mobi6_header, 0)
                        }
                    }
                } else {
                    (mobi6_header, 0)
                }
            } else {
                (mobi6_header, 0)
            }
        } else {
            (mobi6_header, 0)
        }
    };

    // 5. Build metadata
    // Title priority: EXTH 503 > MOBI header > PDB name
    let title = exth
        .as_ref()
        .and_then(|e| e.title.clone())
        .or_else(|| {
            if !mobi.title.is_empty() {
                Some(mobi.title.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| pdb.name.clone());

    let metadata = if let Some(ref exth) = exth {
        Metadata {
            title,
            authors: exth.authors.clone(),
            publisher: exth.publisher.clone(),
            description: exth.description.clone(),
            subjects: exth.subjects.clone(),
            date: exth.pub_date.clone(),
            rights: exth.rights.clone(),
            language: exth.language.clone().unwrap_or_default(),
            ..Default::default()
        }
    } else {
        Metadata {
            title,
            ..Default::default()
        }
    };

    // 6. Extract text content (use kf8_record_offset for combo files)
    let text = extract_text(&mut reader, &pdb, &mobi, kf8_record_offset)?;

    // 7. Extract resources (images and fonts)
    let ExtractedResources { images, fonts, resource_map } = extract_resources(&mut reader, &pdb, &mobi)?;

    // 8. Build Book
    let mut book = Book::new();
    book.metadata = metadata;

    // Determine codec string
    let codec = match mobi.encoding {
        Encoding::Utf8 => "utf-8",
        _ => "cp1252",
    };

    // Try KF8 parsing for proper chapters
    if mobi.is_kf8() {
        if let Ok(kf8_result) = parse_kf8(&mut reader, &pdb, &mobi, &text, codec, kf8_record_offset) {
            // Build file_starts array: (start_pos, file_number) for ID lookup
            let file_starts: Vec<(u32, u32)> = kf8_result.files.iter()
                .map(|f| (f.start_pos, f.file_number as u32))
                .collect();

            // Add chapter HTML files
            for (i, (filename, content)) in kf8_result.parts.iter().enumerate() {
                let html = wrap_html_content(content, &mobi.title, i, &kf8_result.elems, &text, &file_starts);
                book.add_resource(filename, html.into_bytes(), "application/xhtml+xml");
                book.add_spine_item(
                    format!("part{:04}", i),
                    filename,
                    "application/xhtml+xml",
                );
            }

            // Add TOC entries from NCX
            for ncx_entry in &kf8_result.ncx {
                // Map NCX entry to filename
                // pos_fid is (elem_idx, offset) - need to look up elem's file_number
                let href = if let Some((elem_idx, _offset)) = ncx_entry.pos_fid {
                    // Look up the elem to get its file_number
                    if let Some(elem) = kf8_result.elems.get(elem_idx as usize) {
                        format!("part{:04}.html", elem.file_number)
                    } else {
                        format!("part{:04}.html", 0)
                    }
                } else {
                    // Fallback: use position to find file
                    let pos = ncx_entry.pos;
                    find_file_for_position(&kf8_result.files, pos)
                        .map(|f| format!("part{:04}.html", f.file_number))
                        .unwrap_or_else(|| "part0000.html".to_string())
                };

                let title = unescape(&ncx_entry.text)
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| ncx_entry.text.clone());
                book.toc.push(TocEntry::new(&title, &href));
            }

            // Add CSS with resolved kindle:embed references
            for (i, css) in kf8_result.css_flows.iter().enumerate() {
                let css_str = String::from_utf8_lossy(css);
                let resolved_css = resolve_css_kindle_embeds(&css_str, &resource_map);
                let filename = format!("styles/style{:04}.css", i);
                book.add_resource(&filename, resolved_css.into_bytes(), "text/css");
            }
        } else {
            // Fallback to single-file mode
            add_single_file_content(&mut book, &text, &mobi);
        }
    } else {
        // Non-KF8 (MOBI6) - single file
        add_single_file_content(&mut book, &text, &mobi);
    }

    // Add images
    for (i, (data, media_type)) in images.into_iter().enumerate() {
        let ext = match media_type.as_str() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            _ => "bin",
        };
        let href = format!("images/image_{:04}.{}", i, ext);
        book.add_resource(&href, data, &media_type);

        // Check if this is the cover
        if let Some(ref exth) = exth
            && exth.cover_offset == Some(i as u32) {
                book.metadata.cover_image = Some(href.clone());
            }
    }

    // Add fonts
    for (i, (data, ext)) in fonts.into_iter().enumerate() {
        let media_type = match ext.as_str() {
            "ttf" => "application/x-font-truetype",
            "otf" => "application/vnd.ms-opentype",
            "woff" => "application/font-woff",
            _ => "application/octet-stream",
        };
        let href = format!("fonts/font_{:04}.{}", i, ext);
        book.add_resource(&href, data, media_type);
    }

    // Ensure at least one TOC entry
    if book.toc.is_empty() {
        let first_href = book
            .spine
            .first()
            .map(|s| s.href.clone())
            .unwrap_or_else(|| "content.html".to_string());
        book.toc
            .push(TocEntry::new(&book.metadata.title, &first_href));
    }

    Ok(book)
}

/// KF8 parsing result
struct Kf8Result {
    parts: Vec<(String, Vec<u8>)>, // (filename, content)
    ncx: Vec<NcxEntry>,
    css_flows: Vec<Vec<u8>>,
    files: Vec<SkeletonFile>,
    elems: Vec<DivElement>,
}

/// Parse KF8 structure for chapter splitting
fn parse_kf8<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    mobi: &MobiHeader,
    text: &[u8],
    codec: &str,
    record_offset: usize,  // Offset for combo MOBI6+KF8 files
) -> Result<Kf8Result> {

    // Parse FDST for flow boundaries (FDST index needs offset too)
    let flow_table = parse_fdst(reader, pdb, mobi, record_offset)?;

    // Get HTML content (flow 0) - everything else is CSS/SVG
    // Calibre: text = flows[0] = raw_ml[start:end] for first flow
    let (html_start, html_end) = flow_table.first().copied().unwrap_or((0, text.len()));
    let html_text = &text[html_start..html_end.min(text.len())];

    // Extract CSS flows (flows 1+)
    let mut css_flows = Vec::new();
    for (_i, (start, end)) in flow_table.iter().enumerate().skip(1) {
        if *start < text.len() && *end <= text.len() {
            let flow_data = text[*start..*end].to_vec();
            // Check if it looks like CSS (or SVG)
            if is_css_like(&flow_data) {
                css_flows.push(flow_data);
            }
        }
    }

    // Create record reader closure with offset for combo files
    let mut read_record = |idx: usize| -> Result<Vec<u8>> {
        let actual_idx = idx + record_offset;
        pdb.read_record(reader, actual_idx)
    };

    // Parse skeleton index
    let files = if mobi.skel_index != NULL_INDEX {
        let (entries, _) = read_index(&mut read_record, mobi.skel_index as usize, codec)?;
        parse_skel_index(&entries)
    } else {
        Vec::new()
    };

    // Parse div index
    let (elems, _div_cncx) = if mobi.div_index != NULL_INDEX {
        let (entries, cncx) = read_index(&mut read_record, mobi.div_index as usize, codec)?;
        (parse_div_index(&entries, &cncx), cncx)
    } else {
        (Vec::new(), Cncx::new())
    };

    // Parse NCX index
    let ncx = if mobi.ncx_index != NULL_INDEX {
        let (entries, cncx) = read_index(&mut read_record, mobi.ncx_index as usize, codec)?;
        parse_ncx_index(&entries, &cncx)
    } else {
        Vec::new()
    };

    // Build parts from skeleton + div
    let parts = build_parts(html_text, &files, &elems)?;

    Ok(Kf8Result {
        parts,
        ncx,
        css_flows,
        files,
        elems,
    })
}

/// Parse FDST (Flow Descriptor Table) record
fn parse_fdst<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    mobi: &MobiHeader,
    record_offset: usize,
) -> Result<Vec<(usize, usize)>> {
    if mobi.fdst_index == NULL_INDEX {
        return Ok(Vec::new());
    }

    let actual_idx = mobi.fdst_index as usize + record_offset;
    let fdst_record = pdb.read_record(reader, actual_idx)?;

    if fdst_record.len() < 12 || &fdst_record[0..4] != b"FDST" {
        return Ok(Vec::new());
    }

    let sec_start =
        u32::from_be_bytes([fdst_record[4], fdst_record[5], fdst_record[6], fdst_record[7]])
            as usize;
    let num_sections =
        u32::from_be_bytes([fdst_record[8], fdst_record[9], fdst_record[10], fdst_record[11]])
            as usize;

    let mut flows = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
        let offset = sec_start + i * 8;
        if offset + 8 > fdst_record.len() {
            break;
        }
        let start = u32::from_be_bytes([
            fdst_record[offset],
            fdst_record[offset + 1],
            fdst_record[offset + 2],
            fdst_record[offset + 3],
        ]) as usize;
        let end = u32::from_be_bytes([
            fdst_record[offset + 4],
            fdst_record[offset + 5],
            fdst_record[offset + 6],
            fdst_record[offset + 7],
        ]) as usize;
        flows.push((start, end));
    }

    Ok(flows)
}

/// Build chapter parts by combining skeletons with div content
/// Based on Calibre's mobi8.py reconstruction algorithm
fn build_parts(
    text: &[u8],
    files: &[SkeletonFile],
    elems: &[DivElement],
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut parts = Vec::new();
    let mut div_ptr = 0;


    for file in files {
        let skel_start = file.start_pos as usize;
        let skel_end = skel_start + file.length as usize;

        if skel_end > text.len() {
            continue;
        }

        let mut skeleton = text[skel_start..skel_end].to_vec();
        // baseptr starts at end of skeleton - div parts are stored contiguously after
        let mut baseptr = skel_end;

        // Insert div elements into skeleton
        // The insert positions in the index are CUMULATIVE - they account for
        // previously inserted content. So we apply them directly without adjustment.
        for _i in 0..file.div_count {
            if div_ptr >= elems.len() {
                break;
            }

            let elem = &elems[div_ptr];
            let part_len = elem.length as usize;

            if baseptr + part_len > text.len() {
                div_ptr += 1;
                continue;
            }

            let part = &text[baseptr..baseptr + part_len];

            // Insert position is relative to skeleton start position
            // The positions in the index are cumulative (account for previous insertions)
            let insert_pos = (elem.insert_pos as usize).saturating_sub(skel_start);

            // Calibre check: verify insert_pos doesn't split a tag
            // If head ends with incomplete tag or tail starts with incomplete tag, fix it
            if insert_pos <= skeleton.len() {
                let head = &skeleton[..insert_pos];
                let tail = &skeleton[insert_pos..];

                // Check for incomplete tag in head: last '<' should be before last '>'
                let head_incomplete = {
                    let last_lt = head.iter().rposition(|&b| b == b'<');
                    let last_gt = head.iter().rposition(|&b| b == b'>');
                    match (last_lt, last_gt) {
                        (Some(lt), Some(gt)) => lt > gt,  // '<' after '>' means unclosed tag
                        (Some(_), None) => true,  // '<' with no '>' means unclosed tag
                        _ => false,
                    }
                };

                // Check for incomplete tag in tail: first '>' should be after first '<'
                let tail_incomplete = {
                    let first_lt = tail.iter().position(|&b| b == b'<');
                    let first_gt = tail.iter().position(|&b| b == b'>');
                    match (first_lt, first_gt) {
                        (Some(lt), Some(gt)) => gt < lt,  // '>' before '<' means we're inside a tag
                        (None, Some(_)) => true,  // '>' with no '<' means we're inside a tag
                        _ => false,
                    }
                };

                // Note: KF8 intentionally splits tags like "a" + "id=" = "aid="
                // across skeleton and div content. This is NOT an error - don't "fix" it.
                // Calibre warns but uses a different correction method involving
                // locate_beg_end_of_tag() which we don't implement.
                // For now, trust the insert positions from the div table.
                if head_incomplete || tail_incomplete {
                }
            }

            // Insert part into skeleton at insert_pos
            if insert_pos <= skeleton.len() {
                let mut new_skeleton = Vec::with_capacity(skeleton.len() + part.len());
                new_skeleton.extend_from_slice(&skeleton[..insert_pos]);
                new_skeleton.extend_from_slice(part);
                new_skeleton.extend_from_slice(&skeleton[insert_pos..]);
                skeleton = new_skeleton;
            }

            baseptr += part_len;
            div_ptr += 1;
        }

        let filename = format!("part{:04}.html", file.file_number);
        parts.push((filename, skeleton));
    }

    // If no parts were built, create a single part with all content
    if parts.is_empty() && !text.is_empty() {
        parts.push(("part0000.html".to_string(), text.to_vec()));
    }

    Ok(parts)
}

/// Check if data looks like CSS
fn is_css_like(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    s.contains('{') && s.contains('}') && (s.contains(':') || s.contains('@'))
}

/// Find skeleton file containing a given position
fn find_file_for_position(files: &[SkeletonFile], pos: u32) -> Option<&SkeletonFile> {
    for file in files {
        if pos >= file.start_pos && pos < file.start_pos + file.length {
            return Some(file);
        }
    }
    // Fallback: return first file
    files.first()
}

/// Process KF8 content: clean up declarations and convert kindle: references
fn wrap_html_content(content: &[u8], _title: &str, part_num: usize, elems: &[DivElement], raw_text: &[u8], file_starts: &[(u32, u32)]) -> String {
    let content_str = String::from_utf8_lossy(content);

    // Strip encoding declarations but keep structure
    let cleaned = strip_encoding_declarations(&content_str);

    // Convert kindle: references to proper paths
    let result = clean_kindle_references(&cleaned, elems, raw_text, file_starts);

    // Strip Amazon-specific attributes (aid, data-AmznRemoved, etc.)
    let stripped = strip_kindle_attributes(&result);

    // Ensure it has proper XHTML structure
    
    ensure_xhtml_structure(&stripped, part_num)
}

/// Strip XML/encoding declarations but keep the HTML structure
fn strip_encoding_declarations(html: &str) -> String {
    let mut result = html.to_string();

    // Remove XML declarations
    while let Some(start) = result.find("<?xml") {
        if let Some(end) = result[start..].find("?>") {
            result = format!("{}{}", &result[..start], &result[start + end + 2..]);
        } else {
            break;
        }
    }

    // Clean up double << that might occur from skeleton placeholder issues
    result = result.replace("<<", "<");

    result
}

/// Strip Amazon-specific attributes and fix XHTML compliance issues
fn strip_kindle_attributes(html: &str) -> String {
    use super::patterns::{AID_ATTR_RE, AMZN_REMOVED_RE, AMZN_PAGE_RE, IMG_TAG_RE, META_CHARSET_RE};

    // Strip aid="..." and aid='...' attributes (Amazon IDs)
    let result = AID_ATTR_RE.replace_all(html, "");

    // Strip data-AmznRemoved and data-AmznRemoved-M8="..." attributes
    let result2 = AMZN_REMOVED_RE.replace_all(&result, "");

    // Strip data-AmznPageBreak="..." attributes
    let result2 = AMZN_PAGE_RE.replace_all(&result2, "");

    // Add alt="" to img tags that don't have alt attribute
    // Match <img that doesn't already have alt= and add alt="" before the closing
    let result3 = IMG_TAG_RE.replace_all(&result2, |caps: &regex_lite::Captures| {
        let attrs = &caps[1];
        let close = &caps[2];
        if attrs.contains("alt=") {
            format!("<img {}{}", attrs, close)
        } else {
            format!("<img {} alt=\"\"{}", attrs, close)
        }
    });

    // Replace HTML5 <meta charset="..."/> with EPUB2-compatible version
    // Also handle variants like <meta charset="UTF-8"/> (no spaces)
    let result4 = META_CHARSET_RE.replace_all(&result3, r#"<meta http-equiv="Content-Type" content="text/html; charset=$1"/>"#);

    result4.to_string()
}

/// Ensure content has proper XHTML structure
fn ensure_xhtml_structure(html: &str, _part_num: usize) -> String {
    let mut result = html.trim().to_string();

    // Check if structure is valid: should start with <?xml or <!DOCTYPE or <html
    // If there's text content before any HTML structure, we need to wrap it
    let needs_wrapping = if let Some(first_tag) = result.find('<') {
        // Check what comes before the first tag
        let before_tag = &result[..first_tag];
        // If there's non-whitespace content before the first tag, structure is broken
        !before_tag.trim().is_empty() && !before_tag.trim().starts_with("<?xml")
    } else {
        // No tags at all
        true
    };

    // Also check if <html> appears at a reasonable position (within first 500 chars)
    let html_pos = result.find("<html");
    let has_valid_html_structure = match html_pos {
        Some(pos) => {
            // Check if content between start and <html> is only xml declaration/doctype/whitespace
            let before_html = &result[..pos];
            let stripped = before_html
                .trim()
                .trim_start_matches(|c: char| c == '<' || c == '?' || c == '!' || c.is_alphanumeric() || c == '"' || c == '=' || c == '/' || c == '>' || c == '-' || c == '.' || c.is_whitespace());
            stripped.is_empty() || pos < 500
        }
        None => false,
    };

    if needs_wrapping || !has_valid_html_structure {
        // For KF8 content that doesn't start with proper structure,
        // try to find existing body content or just wrap the whole thing
        let body_pos = result.find("<body");
        // Only use body extraction if body tag is near the beginning (first 10%)
        let body_content = if let Some(pos) = body_pos {
            let threshold = result.len() / 10;
            if pos < threshold {
                extract_body_content_safe(&result)
            } else {
                // Body tag is late in content - probably from next file's structure
                // Keep the full content
                result.clone()
            }
        } else if result.contains("<html") {
            // Has html but no body - extract after html opening tag
            if let Some(html_pos) = result.find("<html") {
                let threshold = result.len() / 10;
                if html_pos < threshold {
                    if let Some(end) = result[html_pos..].find('>') {
                        result[html_pos + end + 1..].to_string()
                    } else {
                        result.clone()
                    }
                } else {
                    result.clone()
                }
            } else {
                result.clone()
            }
        } else {
            // No proper structure - use the content as-is (KF8 skeleton fragment)
            result.clone()
        };

        result = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Content</title></head>
<body>
{}
</body>
</html>"#,
            body_content
        );
    } else {
        // Add XML declaration if missing
        if !result.starts_with("<?xml") {
            result = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", result);
        }

        // Add xmlns to html tag if missing
        if let Some(start) = result.find("<html")
            && let Some(end) = result[start..].find('>') {
                let html_tag = &result[start..start + end + 1];
                if !html_tag.contains("xmlns") {
                    let new_tag = html_tag.replace("<html", "<html xmlns=\"http://www.w3.org/1999/xhtml\"");
                    result = format!("{}{}{}", &result[..start], new_tag, &result[start + end + 1..]);
                }
            }

        // Add meta charset after <head> if not present (use EPUB2-compatible format)
        if let Some(head_pos) = result.find("<head>") {
            let after_head = head_pos + 6;
            if !result[after_head..].starts_with("<meta charset") && !result[after_head..].starts_with("<meta http-equiv") {
                result = format!(
                    "{}<meta http-equiv=\"Content-Type\" content=\"text/html; charset=UTF-8\"/>{}",
                    &result[..after_head],
                    &result[after_head..]
                );
            }
        }
    }

    result
}

/// Safely extract body content, handling corrupted HTML
fn extract_body_content_safe(html: &str) -> String {
    // Try to find <body> tag
    if let Some(body_start) = html.find("<body")
        && let Some(body_tag_end) = html[body_start..].find('>') {
            let content_start = body_start + body_tag_end + 1;
            if let Some(body_end) = html[content_start..].rfind("</body>") {
                return html[content_start..content_start + body_end].to_string();
            }
            // No closing body, take everything after body tag
            return html[content_start..].to_string();
        }

    // No body tag - try to extract content after </head> or after <html...>
    if let Some(head_end) = html.find("</head>") {
        let after_head = head_end + 7;
        // Skip any <body> tag if present
        let content = &html[after_head..];
        if let Some(body_start) = content.find("<body")
            && let Some(body_tag_end) = content[body_start..].find('>') {
                return content[body_start + body_tag_end + 1..].to_string();
            }
        return content.to_string();
    }

    // Just return as-is, stripping any leading partial tags
    let mut result = html.to_string();

    // Remove XML declaration if present
    if let Some(xml_end) = result.find("?>") {
        result = result[xml_end + 2..].trim().to_string();
    }

    // If still starts with partial text (not a tag), find first complete tag
    if !result.starts_with('<')
        && let Some(first_tag) = result.find('<') {
            // Check if this is a real tag start or just a < in text
            let tag_content = &result[first_tag..];
            if tag_content.starts_with("<div") || tag_content.starts_with("<p") ||
               tag_content.starts_with("<h") || tag_content.starts_with("<span") ||
               tag_content.starts_with("<a") || tag_content.starts_with("<img") {
                result = result[first_tag..].to_string();
            }
        }

    result
}

/// Add content as a single file (fallback mode)
fn add_single_file_content(book: &mut Book, text: &[u8], mobi: &MobiHeader) {
    let html_content = build_html(text, mobi);
    book.add_resource(
        "content.html",
        html_content.into_bytes(),
        "application/xhtml+xml",
    );
    book.add_spine_item("content", "content.html", "application/xhtml+xml");
    book.toc
        .push(TocEntry::new(&book.metadata.title, "content.html"));
}

fn extract_text<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    mobi: &MobiHeader,
    record_offset: usize, // Offset for combo MOBI6+KF8 files
) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    let text_end = mobi.text_record_count as usize + 1;

    // For Huffman compression, we need to load HUFF/CDIC records first
    let mut huff_reader = if mobi.compression == Compression::Huffman {
        Some(load_huffcdic(reader, pdb, mobi, record_offset)?)
    } else {
        None
    };

    // Extract text from records 1 to text_end-1

    for i in 1..text_end {
        // Apply record offset for combo files (KF8 text starts after BOUNDARY marker)
        let actual_idx = i + record_offset;
        if actual_idx >= pdb.record_offsets.len() {
            break;
        }

        let record = pdb.read_record(reader, actual_idx)?;

        // Strip trailing bytes if extra_data_flags is set
        let record = strip_trailing_data(&record, mobi.extra_data_flags);

        let decompressed = match mobi.compression {
            Compression::PalmDoc => palmdoc_compression::decompress(record)
                .map_err(|e| Error::UnsupportedFormat(format!("PalmDoc decompression failed: {:?}", e)))?,
            Compression::None => record.to_vec(),
            Compression::Huffman => {
                if let Some(ref mut hr) = huff_reader {
                    hr.decompress(record)?
                } else {
                    return Err(Error::UnsupportedFormat(
                        "Huffman reader not initialized".into(),
                    ));
                }
            }
            Compression::Unknown(n) => {
                return Err(Error::UnsupportedFormat(format!(
                    "Unknown compression type: {}",
                    n
                )));
            }
        };

        text.extend_from_slice(&decompressed);
    }

    // Post-processing like Calibre's mobi6.py:
    // Remove trailing '#'
    if text.ends_with(b"#") {
        text.pop();
    }
    // Remove null bytes (can interfere with position calculations)
    text.retain(|&b| b != 0);

    Ok(text)
}

/// Load HUFF and CDIC records for Huffman decompression
fn load_huffcdic<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    mobi: &MobiHeader,
    record_offset: usize, // Offset for combo MOBI6+KF8 files
) -> Result<HuffCdicReader> {
    if mobi.huff_record_index == NULL_INDEX || mobi.huff_record_count == 0 {
        return Err(Error::InvalidMobi(
            "Huffman compression but no HUFF/CDIC records".into(),
        ));
    }

    // Apply record offset for combo files
    let huff_start = mobi.huff_record_index as usize + record_offset;
    let huff_count = mobi.huff_record_count as usize;

    // First record is HUFF
    let huff = pdb.read_record(reader, huff_start)?;

    // Remaining records are CDIC
    let mut cdics: Vec<Vec<u8>> = Vec::with_capacity(huff_count - 1);
    for i in 1..huff_count {
        let cdic = pdb.read_record(reader, huff_start + i)?;
        cdics.push(cdic);
    }

    // Convert to references for the constructor
    let cdic_refs: Vec<&[u8]> = cdics.iter().map(|v| v.as_slice()).collect();

    HuffCdicReader::new(&huff, &cdic_refs)
}

fn strip_trailing_data(record: &[u8], flags: u16) -> &[u8] {
    if flags == 0 || record.is_empty() {
        return record;
    }

    let mut end = record.len();

    // Count trailing data entries based on flags (skip bit 0, it's handled separately)
    // Calibre does: flags >> 1, then loops checking each bit
    let mut shifted_flags = flags >> 1;
    while shifted_flags != 0 {
        if shifted_flags & 1 != 0 {
            if end == 0 {
                break;
            }
            // Read variable-length size from end of record
            let mut size = 0usize;
            let mut shift = 0;
            let mut pos = end;
            while pos > 0 {
                pos -= 1;
                let byte = record[pos];
                size |= ((byte & 0x7F) as usize) << shift;
                shift += 7;
                if byte & 0x80 != 0 || shift >= 28 {
                    break;
                }
            }
            if size > 0 && size <= end {
                end -= size;
            }
        }
        shifted_flags >>= 1;
    }

    // Handle multibyte overlap flag (bit 0) - this is processed LAST
    if flags & 1 != 0 && end > 0 {
        let overlap = (record[end - 1] & 3) as usize + 1;
        if overlap <= end {
            end -= overlap;
        }
    }

    &record[..end]
}

/// Extract all resources (images and fonts) from the MOBI file
fn extract_resources<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    mobi: &MobiHeader,
) -> Result<ExtractedResources> {
    let mut images = Vec::new();
    let mut fonts = Vec::new();
    let mut resource_map: Vec<Option<String>> = Vec::new();

    let first_image = mobi.first_image_index as usize;
    if first_image == NULL_INDEX as usize {
        return Ok(ExtractedResources { images, fonts, resource_map });
    }

    let mut image_idx = 0usize;
    let mut font_idx = 0usize;

    for i in first_image..pdb.record_offsets.len() {
        let record = pdb.read_record(reader, i)?;

        // Check for FONT record
        if record.starts_with(b"FONT") {
            if let Some((font_data, ext)) = read_font_record(&record) {
                let href = format!("fonts/font_{:04}.{}", font_idx, ext);
                resource_map.push(Some(href.clone()));
                fonts.push((font_data, ext));
                font_idx += 1;
            } else {
                resource_map.push(None);
            }
            continue;
        }

        // Skip metadata records
        if record.starts_with(b"FLIS") || record.starts_with(b"FCIS") ||
           record.starts_with(b"SRCS") || record.starts_with(b"BOUN") ||
           record.starts_with(b"FDST") || record.starts_with(b"DATP") ||
           record.starts_with(b"AUDI") || record.starts_with(b"VIDE") ||
           record.starts_with(b"RESC") || record.starts_with(b"CMET") ||
           record.starts_with(b"PAGE") || record.starts_with(b"CONT") ||
           record.starts_with(b"CRES") || record.starts_with(b"BOUNDARY") {
            resource_map.push(None);
            continue;
        }

        // Check for image
        let media_type = detect_image_type(&record);
        if let Some(mt) = media_type {
            let ext = match mt {
                "image/jpeg" => "jpg",
                "image/png" => "png",
                "image/gif" => "gif",
                "image/bmp" => "bmp",
                _ => "bin",
            };
            let href = format!("images/image_{:04}.{}", image_idx, ext);
            resource_map.push(Some(href));
            images.push((record, mt.to_string()));
            image_idx += 1;
        } else {
            resource_map.push(None);
        }
    }

    Ok(ExtractedResources { images, fonts, resource_map })
}

/// Read and decode a FONT record from MOBI
/// Returns (font_data, extension) or None if failed
fn read_font_record(data: &[u8]) -> Option<(Vec<u8>, String)> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    if data.len() < 24 || !data.starts_with(b"FONT") {
        return None;
    }

    // Parse header (big-endian)
    let usize_val = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let flags = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let dstart = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let xor_len = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let xor_start = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as usize;

    if dstart >= data.len() {
        return None;
    }

    let mut font_data = data[dstart..].to_vec();

    // XOR obfuscation (flag bit 1)
    if flags & 0b10 != 0 && xor_len > 0 && xor_start + xor_len <= data.len() {
        let key = &data[xor_start..xor_start + xor_len];
        let extent = 1040.min(font_data.len());
        for n in 0..extent {
            font_data[n] ^= key[n % xor_len];
        }
    }

    // Zlib compression (flag bit 0)
    if flags & 0b1 != 0 {
        let mut decoder = ZlibDecoder::new(&font_data[..]);
        let mut decompressed = Vec::with_capacity(usize_val);
        if decoder.read_to_end(&mut decompressed).is_ok() {
            font_data = decompressed;
        } else {
            return None;
        }
    }

    // Detect font type
    let ext = if font_data.len() >= 4 {
        let sig = &font_data[..4];
        if sig == b"\x00\x01\x00\x00" || sig == b"true" || sig == b"ttcf" {
            "ttf"
        } else if sig == b"OTTO" {
            "otf"
        } else if sig == b"wOFF" {
            "woff"
        } else {
            "dat"
        }
    } else {
        "dat"
    };

    Some((font_data, ext.to_string()))
}

fn detect_image_type(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }

    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }

    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("image/png");
    }

    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("image/gif");
    }

    if data.starts_with(b"BM") {
        return Some("image/bmp");
    }

    None
}

/// Resolve kindle:embed:XXXX references in CSS to actual resource paths
/// Also strips invalid @font-face declarations with placeholder URLs
fn resolve_css_kindle_embeds(css: &str, resource_map: &[Option<String>]) -> String {
    use super::patterns::{FONTFACE_PLACEHOLDER_RE, KINDLE_EMBED_RE};

    let mut result = css.to_string();

    // First, strip @font-face declarations with XXXXXXXXXXXXXXXX placeholder URLs
    // These are Amazon placeholders when fonts aren't actually embedded
    result = FONTFACE_PLACEHOLDER_RE.replace_all(&result, "").to_string();

    // Replace all kindle:embed:XXXX matches
    for cap in KINDLE_EMBED_RE.captures_iter(css) {
        let full_match = cap.get(0).unwrap().as_str();
        let base32_str = cap.get(1).unwrap().as_str();

        // Parse base32 index (1-indexed, so subtract 1)
        let idx = parse_kindle_base32(base32_str);
        let resource_idx = if idx > 0 { idx - 1 } else { 0 };

        // Look up resource path
        let replacement = if let Some(Some(href)) = resource_map.get(resource_idx) {
            // Use relative path from styles/ directory
            format!("../{}", href)
        } else {
            // Fallback: keep a placeholder that won't break CSS parsing
            "missing-resource".to_string()
        };

        // Replace in result, handling the full match including quotes
        let new_value = if full_match.ends_with('"') {
            format!("{}\"", replacement)
        } else if full_match.ends_with('\'') {
            format!("{}'", replacement)
        } else {
            replacement
        };

        result = result.replacen(full_match, &new_value, 1);
    }

    result
}

fn build_html(text: &[u8], mobi: &MobiHeader) -> String {
    let content = match mobi.encoding {
        Encoding::Utf8 => String::from_utf8_lossy(text).to_string(),
        _ => String::from_utf8_lossy(text).to_string(),
    };

    let body_content = extract_body_content(&content);
    // For MOBI6, we don't have KF8 structures for ID lookup, pass empty slices
    let body_content = clean_kindle_references(&body_content, &[], text, &[]);
    // Strip Amazon-specific attributes (aid, data-AmznRemoved, etc.)
    let body_content = strip_kindle_attributes(&body_content);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>{}</title>
<style type="text/css">
body {{ font-family: serif; }}
</style>
</head>
<body>
{}
</body>
</html>"#,
        escape_xml_text(&mobi.title),
        body_content
    )
}

fn extract_body_content(html: &str) -> String {
    // KF8 format has skeleton HTML followed by actual content
    if let Some(first_html_end) = html.find("</html>") {
        let after_first_html = first_html_end + 7;
        if after_first_html < html.len() {
            let content = &html[after_first_html..];
            return strip_html_structure(content);
        }
    }

    // Fallback: try to extract from body
    if let Some(body_start) = html.find("<body")
        && let Some(body_tag_end) = html[body_start..].find('>') {
            let after_body = body_start + body_tag_end + 1;
            if let Some(body_end) = html[after_body..].find("</body>") {
                return html[after_body..after_body + body_end].to_string();
            }
        }

    html.to_string()
}

fn strip_html_structure(content: &str) -> String {
    let mut result = content.to_string();

    // Remove XML declarations
    while let Some(start) = result.find("<?xml") {
        if let Some(end) = result[start..].find("?>") {
            result = format!("{}{}", &result[..start], &result[start + end + 2..]);
        } else {
            break;
        }
    }

    // Remove DOCTYPE
    while let Some(start) = result.find("<!DOCTYPE") {
        if let Some(end) = result[start..].find('>') {
            result = format!("{}{}", &result[..start], &result[start + end + 1..]);
        } else {
            break;
        }
    }

    // Remove <html> tags
    while let Some(start) = result.find("<html") {
        if let Some(end) = result[start..].find('>') {
            result = format!("{}{}", &result[..start], &result[start + end + 1..]);
        } else {
            break;
        }
    }
    result = result.replace("</html>", "");

    // Remove <head>...</head>
    while let Some(start) = result.find("<head") {
        if let Some(end) = result[start..].find("</head>") {
            result = format!("{}{}", &result[..start], &result[start + end + 7..]);
        } else {
            break;
        }
    }

    // Remove <body> tags
    while let Some(start) = result.find("<body") {
        if let Some(end) = result[start..].find('>') {
            result = format!("{}{}", &result[..start], &result[start + end + 1..]);
        } else {
            break;
        }
    }
    result = result.replace("</body>", "");

    result
}

/// Find the nearest id= attribute at or after a given position in the raw text
/// The kindle:pos:fid links point to positions that are at or just before the target element
fn find_nearest_id(raw_text: &[u8], pos: usize, _file_starts: &[(u32, u32)]) -> Option<String> {
    use super::patterns::ID_ATTR_RE;

    if pos >= raw_text.len() {
        return None;
    }

    // Search forward from pos to find the next tag with an id= attribute
    // Limit search to a reasonable window (e.g., 2000 bytes forward)
    let end_pos = (pos + 2000).min(raw_text.len());
    let search_text = &raw_text[pos..end_pos];
    let search_str = String::from_utf8_lossy(search_text);

    // Find the first id= in the forward search
    if let Some(caps) = ID_ATTR_RE.captures(&search_str)
        && let Some(m) = caps.get(1) {
            return Some(m.as_str().to_string());
        }

    // If no ID found forward, try searching backwards as fallback
    let start_pos = pos.saturating_sub(500);
    let search_back = &raw_text[start_pos..pos];
    for tag in reverse_tag_iter(search_back) {
        let tag_str = String::from_utf8_lossy(tag);
        if let Some(caps) = ID_ATTR_RE.captures(&tag_str)
            && let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
    }

    None
}

/// Iterate over all tags in a byte slice in reverse order (last tag to first tag)
/// This is a Rust port of Calibre's reverse_tag_iter function
fn reverse_tag_iter(block: &[u8]) -> ReverseTagIterator<'_> {
    ReverseTagIterator { block, end: block.len() }
}

struct ReverseTagIterator<'a> {
    block: &'a [u8],
    end: usize,
}

impl<'a> Iterator for ReverseTagIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        // Find the last '>' before end
        let pgt = self.block[..self.end].iter().rposition(|&b| b == b'>')?;
        // Find the last '<' before the '>'
        let plt = self.block[..pgt].iter().rposition(|&b| b == b'<')?;
        // Extract the tag
        let tag = &self.block[plt..=pgt];
        // Update end for next iteration
        self.end = plt;
        Some(tag)
    }
}

fn clean_kindle_references(html: &str, elems: &[DivElement], raw_text: &[u8], file_starts: &[(u32, u32)]) -> String {
    let mut result = html.to_string();

    // Replace kindle:flow references (e.g., kindle:flow:0001?mime=text/css)
    // Flow index is base32 encoded, flow 0 is HTML content, flow 1+ are CSS/SVG
    while let Some(start) = result.find("kindle:flow:") {
        if let Some(end) = result[start..].find('"') {
            // Extract flow ID (base32 encoded)
            let ref_str = &result[start..start + end];
            let flow_id_end = ref_str.find('?').unwrap_or(ref_str.len());
            let flow_id_str = &ref_str[12..flow_id_end]; // After "kindle:flow:"

            let flow_num = parse_kindle_base32(flow_id_str);
            // Flow 1 becomes style0000.css, flow 2 becomes style0001.css, etc.
            let css_idx = if flow_num > 0 { flow_num - 1 } else { 0 };
            let replacement = format!("styles/style{:04}.css", css_idx);
            result = format!("{}{}{}", &result[..start], replacement, &result[start + end..]);
        } else {
            break;
        }
    }

    // Replace kindle:pos:fid:XXXX:off:YYYY links with file references + anchors
    // XXXX is div table index (base32) - index into elems array
    // YYYY is offset (base32) - offset within the element
    while let Some(start) = result.find("kindle:pos:fid:") {
        if let Some(end) = result[start..].find('"') {
            // Extract fid (base32 encoded after "kindle:pos:fid:")
            let ref_str = &result[start..start + end];
            // Format: kindle:pos:fid:XXXX:off:YYYY
            let parts: Vec<&str> = ref_str.split(':').collect();
            if parts.len() >= 6 && parts[4] == "off" {
                let fid_str = parts[3]; // The XXXX part (elem index)
                let off_str = parts[5]; // The YYYY part (offset)
                let elem_idx = parse_kindle_base32(fid_str);
                let offset = parse_kindle_base32(off_str);

                // Look up the element to get its file_number and position
                let (file_num, target_pos) = if let Some(elem) = elems.get(elem_idx) {
                    (elem.file_number as usize, elem.insert_pos + offset as u32)
                } else {
                    (0, 0u32)
                };

                // Search backwards in the raw text to find the nearest id= attribute
                // Like Calibre's get_id_tag() function
                let anchor = find_nearest_id(raw_text, target_pos as usize, file_starts);

                let replacement = if let Some(id) = anchor {
                    format!("part{:04}.html#{}", file_num, id)
                } else {
                    format!("part{:04}.html", file_num)
                };
                result = format!("{}{}{}", &result[..start], replacement, &result[start + end..]);
            } else if parts.len() >= 4 {
                // Old format without offset
                let fid_str = parts[3];
                let elem_idx = parse_kindle_base32(fid_str);
                let file_num = elems.get(elem_idx).map(|e| e.file_number as usize).unwrap_or(0);
                let replacement = format!("part{:04}.html", file_num);
                result = format!("{}{}{}", &result[..start], replacement, &result[start + end..]);
            } else {
                // Can't parse, replace with placeholder
                result = format!("{}part0000.html{}", &result[..start], &result[start + end..]);
            }
        } else {
            break;
        }
    }

    // Replace kindle:embed image references
    // Format: kindle:embed:XXXX?mime=image/jpeg
    // XXXX is 1-indexed base32, so embed:0001 is image 0
    while let Some(start) = result.find("kindle:embed:") {
        if let Some(end) = result[start..].find(['"', '\'', ')']) {
            let ref_str = &result[start..start + end];
            // Extract the base32 ID
            let id_end = ref_str[13..].find('?').map(|p| 13 + p).unwrap_or(ref_str.len());
            let embed_id = &ref_str[13..id_end]; // After "kindle:embed:"

            let img_num = parse_kindle_base32(embed_id);
            // Calibre uses 1-indexed, so subtract 1
            let img_idx = if img_num > 0 { img_num - 1 } else { 0 };

            let ext = if ref_str.contains("image/png") {
                "png"
            } else if ref_str.contains("image/gif") {
                "gif"
            } else {
                "jpg" // Default to jpeg
            };

            let img_path = format!("images/image_{:04}.{}", img_idx, ext);
            result = format!("{}{}{}", &result[..start], img_path, &result[start + end..]);
        } else {
            break;
        }
    }

    // Clean up any remaining malformed kindle: references
    // These can occur from corrupted skeleton reconstruction
    result = clean_malformed_kindle_refs(&result);

    result
}

/// Remove any malformed kindle: references that weren't properly converted
fn clean_malformed_kindle_refs(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000; // Prevent infinite loops

    // Remove standalone "kindle:" fragments that got corrupted
    // Match patterns like kindle:xxx that don't have proper structure
    while let Some(start) = result.find("kindle:") {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            break;
        }

        // Find the extent of this malformed reference
        let after = &result[start..];

        // Find the end of this kindle reference
        let end = after
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"' || c == '\'')
            .unwrap_or(after.len());

        // Check if this is inside an href="" or src=""
        let before_start = start.saturating_sub(30);
        let before = &result[before_start..start];

        if before.contains("href=\"") || before.contains("src=\"") {
            // Inside a properly quoted attribute - replace with safe placeholder
            result = format!("{}#{}", &result[..start], &result[start + end..]);
        } else if before.contains("href=") || before.contains("src=") {
            // Attribute without proper quotes - still replace
            result = format!("{}#{}", &result[..start], &result[start + end..]);
        } else {
            // Not in an attribute - just remove the kindle: text
            result = format!("{}{}", &result[..start], &result[start + end..]);
        }
    }

    // Also clean up any malformed tags that have kindle references embedded
    // Pattern: style<a href= or similar corrupted structures
    result = result.replace("style<a", "style=\"\"><a");

    result
}

fn escape_xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Parse Kindle base32 encoding
/// Kindle uses custom base32: 0-9 (0-9), A-V (10-31)
fn parse_kindle_base32(s: &str) -> usize {
    let mut result = 0usize;
    // Kindle IDs are typically 4-10 chars, limit to prevent overflow
    for c in s.chars().take(10) {
        result = result.saturating_mul(32);
        let val = match c {
            '0'..='9' => c as usize - '0' as usize,
            'A'..='V' => c as usize - 'A' as usize + 10,
            'a'..='v' => c as usize - 'a' as usize + 10,
            _ => continue, // Skip invalid chars
        };
        result = result.saturating_add(val);
    }
    result
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_image_type() {
        assert_eq!(
            detect_image_type(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_image_type(&[0x89, 0x50, 0x4E, 0x47]),
            Some("image/png")
        );
        assert_eq!(detect_image_type(b"GIF89a"), Some("image/gif"));
        assert_eq!(detect_image_type(&[0x00, 0x00, 0x00, 0x00]), None);
    }
}
