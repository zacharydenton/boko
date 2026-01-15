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
use super::palmdoc;

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

    // 2. Read and parse MOBI header (record 0)
    let record0 = pdb.read_record(&mut reader, 0)?;
    let mobi = MobiHeader::parse(&record0)?;

    // 3. Check for encryption
    if mobi.encryption != 0 {
        return Err(Error::InvalidMobi(
            "Encrypted MOBI files are not supported".into(),
        ));
    }

    // 4. Parse EXTH if present
    let exth = if mobi.has_exth() && mobi.header_length > 0 {
        let exth_start = 16 + mobi.header_length as usize;
        if exth_start < record0.len() {
            ExthHeader::parse(&record0[exth_start..], mobi.encoding).ok()
        } else {
            None
        }
    } else {
        None
    };

    // Debug: Check for KF8 boundary
    if let Some(ref exth) = exth {
        if let Some(boundary) = exth.kf8_boundary {
            eprintln!("DEBUG: kf8_boundary = {}", boundary);
        }
    }
    eprintln!("DEBUG: mobi.is_kf8() = {}, skel_index = {}, div_index = {}",
              mobi.is_kf8(), mobi.skel_index, mobi.div_index);

    // 5. Build metadata
    let mut metadata = Metadata::default();

    // Title priority: EXTH 503 > MOBI header > PDB name
    metadata.title = exth
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

    if let Some(ref exth) = exth {
        metadata.authors = exth.authors.clone();
        metadata.publisher = exth.publisher.clone();
        metadata.description = exth.description.clone();
        metadata.subjects = exth.subjects.clone();
        metadata.date = exth.pub_date.clone();
        metadata.rights = exth.rights.clone();

        if let Some(ref lang) = exth.language {
            metadata.language = lang.clone();
        }
    }

    // 6. Extract text content
    let text = extract_text(&mut reader, &pdb, &mobi)?;

    // 7. Extract images
    let images = extract_images(&mut reader, &pdb, &mobi)?;

    // 8. Build Book
    let mut book = Book::new();
    book.metadata = metadata;

    // Determine codec string
    let codec = match mobi.encoding {
        Encoding::Utf8 => "utf-8",
        _ => "cp1252",
    };

    // Try KF8 parsing for proper chapters
    // For combo MOBI6+KF8 files, we need to offset index reads by (kf8_boundary - 1)
    let kf8_record_offset = exth
        .as_ref()
        .and_then(|e| e.kf8_boundary)
        .map(|b| (b as usize).saturating_sub(1))
        .unwrap_or(0);
    eprintln!("DEBUG: kf8_record_offset = {}", kf8_record_offset);

    if mobi.is_kf8() {
        if let Ok(kf8_result) = parse_kf8(&mut reader, &pdb, &mobi, &text, codec, kf8_record_offset) {
            // Add chapter HTML files
            for (i, (filename, content)) in kf8_result.parts.iter().enumerate() {
                let html = wrap_html_content(content, &mobi.title, i, &kf8_result.elems);
                book.add_resource(filename, html.into_bytes(), "application/xhtml+xml");
                book.add_spine_item(
                    &format!("part{:04}", i),
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

            // Add CSS if present
            for (i, css) in kf8_result.css_flows.iter().enumerate() {
                let filename = format!("styles/style{:04}.css", i);
                book.add_resource(&filename, css.clone(), "text/css");
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
        if let Some(ref exth) = exth {
            if exth.cover_offset == Some(i as u32) {
                book.metadata.cover_image = Some(href.clone());
            }
        }
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
    eprintln!("DEBUG: parse_kf8 with record_offset = {}", record_offset);

    // Parse FDST for flow boundaries (FDST index needs offset too)
    let flow_table = parse_fdst(reader, pdb, mobi, record_offset)?;
    eprintln!("DEBUG: flow_table = {:?}", flow_table);
    eprintln!("DEBUG: text.len() = {}", text.len());

    // Get HTML content (flow 0) - everything else is CSS/SVG
    // Calibre: text = flows[0] = raw_ml[start:end] for first flow
    let (html_start, html_end) = flow_table.first().copied().unwrap_or((0, text.len()));
    let html_text = &text[html_start..html_end.min(text.len())];
    eprintln!("DEBUG: html_text range = {}..{}, len = {}", html_start, html_end.min(text.len()), html_text.len());

    // Extract CSS flows (flows 1+)
    let mut css_flows = Vec::new();
    for (i, (start, end)) in flow_table.iter().enumerate().skip(1) {
        if *start < text.len() && *end <= text.len() {
            let flow_data = text[*start..*end].to_vec();
            // Check if it looks like CSS
            if is_css_like(&flow_data) {
                css_flows.push(flow_data);
            }
        }
        // Only get first few CSS flows
        if i > 5 {
            break;
        }
    }

    // Create record reader closure with offset for combo files
    let mut read_record = |idx: usize| -> Result<Vec<u8>> {
        let actual_idx = idx + record_offset;
        eprintln!("DEBUG: read_record {} -> {} (offset {})", idx, actual_idx, record_offset);
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
    eprintln!("DEBUG: parse_fdst reading record {} (fdst_index {} + offset {})",
              actual_idx, mobi.fdst_index, record_offset);
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

    eprintln!("DEBUG: build_parts - {} files, {} elems, text len {}", files.len(), elems.len(), text.len());

    for file in files {
        let skel_start = file.start_pos as usize;
        let skel_end = skel_start + file.length as usize;
        eprintln!("DEBUG: file {} - skel_start={}, skel_end={}, div_count={}",
                  file.file_number, skel_start, skel_end, file.div_count);

        if skel_end > text.len() {
            continue;
        }

        let mut skeleton = text[skel_start..skel_end].to_vec();
        // baseptr starts at end of skeleton - div parts are stored contiguously after
        let mut baseptr = skel_end;

        // Insert div elements into skeleton
        // NOTE: Calibre's algorithm uses insertpos - skelpos directly without cumulative
        // adjustment. This works because the KF8 format's insertpos values are designed
        // to be applied to the original skeleton structure.
        for i in 0..file.div_count {
            if div_ptr >= elems.len() {
                break;
            }

            let elem = &elems[div_ptr];
            let part_len = elem.length as usize;

            // Verify elem belongs to this file
            if elem.file_number as usize != file.file_number {
                eprintln!("DEBUG:   WARNING: elem {} has file_number {} but processing file {}!",
                          div_ptr, elem.file_number, file.file_number);
            }

            eprintln!("DEBUG:   div {} - insert_pos={}, length={}, file_number={}, baseptr={}, startpos={}",
                      div_ptr, elem.insert_pos, elem.length, elem.file_number, baseptr, elem.start_pos);

            if baseptr + part_len > text.len() {
                eprintln!("DEBUG:   SKIP - baseptr + part_len > text.len()");
                div_ptr += 1;
                continue;
            }

            let part = &text[baseptr..baseptr + part_len];
            let part_preview: String = String::from_utf8_lossy(&part[..part.len().min(80)]).to_string();
            eprintln!("DEBUG:   part preview: {:?}", part_preview);

            // Show skeleton context around insert point
            let rel_insert = (elem.insert_pos as usize).saturating_sub(skel_start);
            if rel_insert <= skeleton.len() {
                let ctx_start = rel_insert.saturating_sub(30);
                let ctx_end = (rel_insert + 30).min(skeleton.len());
                let before: String = String::from_utf8_lossy(&skeleton[ctx_start..rel_insert]).to_string();
                let after: String = String::from_utf8_lossy(&skeleton[rel_insert..ctx_end]).to_string();
                eprintln!("DEBUG:   skeleton context: ...{:?}|INSERT|{:?}...", before, after);
            }

            // Insert position is relative to skeleton start position
            // Like Calibre, we use the raw insertpos - skelpos without cumulative adjustment
            let mut insert_pos = (elem.insert_pos as usize).saturating_sub(skel_start);
            eprintln!("DEBUG:   insert_pos relative to skeleton: {}", insert_pos);

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

                if head_incomplete || tail_incomplete {
                    // Try to find a safe insert position using aid attribute from first elem
                    if i == 0 {
                        if let Some(aid) = extract_aid_from_elem(elem) {
                            if let Some(new_pos) = find_tag_end_by_aid(&skeleton, &aid) {
                                insert_pos = new_pos;
                            }
                        }
                    }
                    // If still problematic, try to find nearest tag boundary
                    if insert_pos <= skeleton.len() {
                        let test_head = &skeleton[..insert_pos];
                        let last_gt = test_head.iter().rposition(|&b| b == b'>');
                        if let Some(gt_pos) = last_gt {
                            // Move insert position to after the last complete tag
                            insert_pos = gt_pos + 1;
                        }
                    }
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

/// Extract aid attribute value from elem's toc_text if available
fn extract_aid_from_elem(elem: &DivElement) -> Option<Vec<u8>> {
    // The toc_text often contains the aid reference
    elem.toc_text.as_ref().and_then(|text| {
        // Extract aid from format like "aid:XXXXX" or just use first few chars
        if text.len() >= 4 {
            Some(text[..text.len().min(10)].as_bytes().to_vec())
        } else {
            None
        }
    })
}

/// Find the end position of a tag with specific aid attribute
fn find_tag_end_by_aid(skeleton: &[u8], aid: &[u8]) -> Option<usize> {
    // Search for aid="XXX" or aid='XXX' in skeleton
    let aid_patterns = [
        format!("aid=\"{}\"", String::from_utf8_lossy(aid)),
        format!("aid='{}'", String::from_utf8_lossy(aid)),
    ];

    for pattern in &aid_patterns {
        if let Some(pos) = skeleton
            .windows(pattern.len())
            .position(|w| w == pattern.as_bytes())
        {
            // Find the end of this tag (next '>')
            if let Some(gt_pos) = skeleton[pos..].iter().position(|&b| b == b'>') {
                return Some(pos + gt_pos + 1);
            }
        }
    }
    None
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
fn wrap_html_content(content: &[u8], _title: &str, _part_num: usize, elems: &[DivElement]) -> String {
    let content_str = String::from_utf8_lossy(content);

    // Strip encoding declarations but keep structure
    let cleaned = strip_encoding_declarations(&content_str);

    // Convert kindle: references to proper paths
    let result = clean_kindle_references(&cleaned, elems);

    // Ensure it has proper XHTML structure
    ensure_xhtml_structure(&result)
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

/// Ensure content has proper XHTML structure
fn ensure_xhtml_structure(html: &str) -> String {
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
        // Extract just the body content - strip any partial HTML tags at start
        let body_content = extract_body_content_safe(&result);

        result = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
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
        if let Some(start) = result.find("<html") {
            if let Some(end) = result[start..].find('>') {
                let html_tag = &result[start..start + end + 1];
                if !html_tag.contains("xmlns") {
                    let new_tag = html_tag.replace("<html", "<html xmlns=\"http://www.w3.org/1999/xhtml\"");
                    result = format!("{}{}{}", &result[..start], new_tag, &result[start + end + 1..]);
                }
            }
        }

        // Add meta charset after <head> if not present
        if let Some(head_pos) = result.find("<head>") {
            let after_head = head_pos + 6;
            if !result[after_head..].starts_with("<meta charset") && !result[after_head..].starts_with("<meta http-equiv") {
                result = format!(
                    "{}<meta charset=\"UTF-8\"/>{}",
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
    if let Some(body_start) = html.find("<body") {
        if let Some(body_tag_end) = html[body_start..].find('>') {
            let content_start = body_start + body_tag_end + 1;
            if let Some(body_end) = html[content_start..].rfind("</body>") {
                return html[content_start..content_start + body_end].to_string();
            }
            // No closing body, take everything after body tag
            return html[content_start..].to_string();
        }
    }

    // No body tag - try to extract content after </head> or after <html...>
    if let Some(head_end) = html.find("</head>") {
        let after_head = head_end + 7;
        // Skip any <body> tag if present
        let content = &html[after_head..];
        if let Some(body_start) = content.find("<body") {
            if let Some(body_tag_end) = content[body_start..].find('>') {
                return content[body_start + body_tag_end + 1..].to_string();
            }
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
    if !result.starts_with('<') {
        if let Some(first_tag) = result.find('<') {
            // Check if this is a real tag start or just a < in text
            let tag_content = &result[first_tag..];
            if tag_content.starts_with("<div") || tag_content.starts_with("<p") ||
               tag_content.starts_with("<h") || tag_content.starts_with("<span") ||
               tag_content.starts_with("<a") || tag_content.starts_with("<img") {
                result = result[first_tag..].to_string();
            }
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
) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    let text_end = mobi.text_record_count as usize + 1;

    // For Huffman compression, we need to load HUFF/CDIC records first
    let mut huff_reader = if mobi.compression == Compression::Huffman {
        Some(load_huffcdic(reader, pdb, mobi)?)
    } else {
        None
    };

    for i in 1..text_end {
        if i >= pdb.record_offsets.len() {
            break;
        }

        let record = pdb.read_record(reader, i)?;

        // Strip trailing bytes if extra_data_flags is set
        let record = strip_trailing_data(&record, mobi.extra_data_flags);

        let decompressed = match mobi.compression {
            Compression::PalmDoc => palmdoc::decompress(&record),
            Compression::None => record.to_vec(),
            Compression::Huffman => {
                if let Some(ref mut hr) = huff_reader {
                    hr.decompress(&record)?
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
) -> Result<HuffCdicReader> {
    if mobi.huff_record_index == NULL_INDEX || mobi.huff_record_count == 0 {
        return Err(Error::InvalidMobi(
            "Huffman compression but no HUFF/CDIC records".into(),
        ));
    }

    let huff_start = mobi.huff_record_index as usize;
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

    // Count trailing data entries based on flags
    for i in 0..16 {
        if flags & (1 << i) != 0 {
            if end == 0 {
                break;
            }
            let mut size = 0usize;
            let mut shift = 0;
            for j in (0..end).rev() {
                let byte = record[j];
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
    }

    // Handle multibyte overlap flag (bit 0)
    if flags & 1 != 0 && end > 0 {
        let overlap = (record[end - 1] & 3) as usize + 1;
        if overlap <= end {
            end -= overlap;
        }
    }

    &record[..end]
}

fn extract_images<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    mobi: &MobiHeader,
) -> Result<Vec<(Vec<u8>, String)>> {
    let mut images = Vec::new();

    let first_image = mobi.first_image_index as usize;
    if first_image == NULL_INDEX as usize {
        return Ok(images);
    }

    for i in first_image..pdb.record_offsets.len() {
        let record = pdb.read_record(reader, i)?;

        let media_type = detect_image_type(&record);
        if let Some(mt) = media_type {
            images.push((record, mt.to_string()));
        } else {
            if record.starts_with(b"FONT") || record.starts_with(b"BOUNDARY") {
                break;
            }
        }
    }

    Ok(images)
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

fn build_html(text: &[u8], mobi: &MobiHeader) -> String {
    let content = match mobi.encoding {
        Encoding::Utf8 => String::from_utf8_lossy(text).to_string(),
        _ => String::from_utf8_lossy(text).to_string(),
    };

    let body_content = extract_body_content(&content);
    let body_content = clean_kindle_references(&body_content, &[]);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
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
    if let Some(body_start) = html.find("<body") {
        if let Some(body_tag_end) = html[body_start..].find('>') {
            let after_body = body_start + body_tag_end + 1;
            if let Some(body_end) = html[after_body..].find("</body>") {
                return html[after_body..after_body + body_end].to_string();
            }
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

fn clean_kindle_references(html: &str, elems: &[DivElement]) -> String {
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

    // Replace kindle:pos:fid:XXXX:off:YYYY links with file references
    // XXXX is div table index (base32) - index into elems array
    // YYYY is offset (base32) - offset within the element
    while let Some(start) = result.find("kindle:pos:fid:") {
        if let Some(end) = result[start..].find('"') {
            // Extract fid (base32 encoded after "kindle:pos:fid:")
            let ref_str = &result[start..start + end];
            // Format: kindle:pos:fid:XXXX:off:YYYY
            let parts: Vec<&str> = ref_str.split(':').collect();
            if parts.len() >= 4 {
                let fid_str = parts[3]; // The XXXX part (elem index)
                let elem_idx = parse_kindle_base32(fid_str);

                // Look up the element to get its file_number
                let file_num = if let Some(elem) = elems.get(elem_idx) {
                    elem.file_number as usize
                } else {
                    // Fallback to elem index if out of bounds
                    0
                };

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
        if let Some(end) = result[start..].find(|c| c == '"' || c == '\'' || c == ')') {
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
        let before_start = if start > 30 { start - 30 } else { 0 };
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
