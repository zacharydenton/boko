use std::io::{self, Read, Seek};
use std::path::Path;

use crate::book::{Book, Metadata, TocEntry};

use super::headers::{Compression, Encoding, ExthHeader, MobiHeader, NULL_INDEX, PdbHeader};
use super::huffcdic::HuffCdicReader;
use super::index::{
    Cncx, DivElement, NcxEntry, SkeletonFile, parse_div_index, parse_ncx_index, parse_skel_index,
    read_index,
};

/// Detected MOBI format variant
enum MobiFormat {
    /// Pure KF8 (AZW3) - version 8, skeleton/div structure
    Kf8 { record_offset: usize },
    /// Combo file with both MOBI6 and KF8 sections
    Combo { kf8_record_offset: usize },
    /// Legacy MOBI6 - single HTML stream
    Mobi6,
}

impl MobiFormat {
    /// Record offset for text extraction (0 for pure files, >0 for combo KF8)
    fn record_offset(&self) -> usize {
        match self {
            MobiFormat::Kf8 { record_offset } => *record_offset,
            MobiFormat::Combo { kf8_record_offset } => *kf8_record_offset,
            MobiFormat::Mobi6 => 0,
        }
    }

    fn is_kf8(&self) -> bool {
        matches!(self, MobiFormat::Kf8 { .. } | MobiFormat::Combo { .. })
    }
}

/// Extracted resources from MOBI file
struct ExtractedResources {
    /// Images as (data, media_type)
    images: Vec<(Vec<u8>, String)>,
    /// Fonts as (data, extension)
    fonts: Vec<(Vec<u8>, String)>,
    /// Maps resource index to href (for CSS reference resolution)
    resource_map: Vec<Option<String>>,
}

/// Read a MOBI or AZW3 file from disk into a [`Book`].
///
/// Supports both MOBI (KF7) and AZW3 (KF8) formats, including combo files.
/// Extracts metadata, spine, table of contents, images, fonts, and CSS.
///
/// # Example
///
/// ```no_run
/// use boko::read_mobi;
///
/// let book = read_mobi("path/to/book.azw3")?;
/// println!("Title: {}", book.metadata.title);
/// println!("Chapters: {}", book.toc.len());
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn read_mobi<P: AsRef<Path>>(path: P) -> io::Result<Book> {
    let file = std::fs::File::open(path)?;
    read_mobi_from_reader(file)
}

/// Read a MOBI/AZW3 from any [`Read`] + [`Seek`] source.
///
/// Useful for reading from memory buffers or network streams.
pub fn read_mobi_from_reader<R: Read + Seek>(mut reader: R) -> io::Result<Book> {
    let pdb = PdbHeader::read(&mut reader)?;
    if pdb.num_records < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Not enough records",
        ));
    }

    // Parse record 0 header (may be MOBI6 header for combo files)
    let record0 = pdb.read_record(&mut reader, 0)?;
    let header0 = MobiHeader::parse(&record0)?;

    if header0.encryption != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Encrypted MOBI files are not supported",
        ));
    }

    // Parse EXTH metadata
    let exth = parse_exth(&record0, &header0);

    // Detect format and get the appropriate header
    // For combo files, resource_header uses MOBI6's first_image_index (shared resources)
    let (format, mobi, resource_header) = detect_format(&mut reader, &pdb, header0, &exth)?;

    // Build metadata
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

    // Extract text content
    let record_offset = format.record_offset();
    let text = extract_text(&mut reader, &pdb, &mobi, record_offset)?;

    // Extract resources (images and fonts)
    // For combo files, use resource_header which has the correct first_image_index
    let ExtractedResources {
        images,
        fonts,
        resource_map,
    } = extract_resources(&mut reader, &pdb, &resource_header)?;

    // Build Book
    let mut book = Book::new();
    book.metadata = metadata;

    let codec = match mobi.encoding {
        Encoding::Utf8 => "utf-8",
        _ => "cp1252",
    };

    // KF8 format: use skeleton/div structure for proper chapters
    if format.is_kf8() {
        if let Ok(kf8_result) = parse_kf8(&mut reader, &pdb, &mobi, &text, codec, record_offset) {
            // Build file_starts array: (start_pos, file_number) for ID lookup
            let file_starts: Vec<(u32, u32)> = kf8_result
                .files
                .iter()
                .map(|f| (f.start_pos, f.file_number as u32))
                .collect();

            // Add chapter HTML files
            for (i, (filename, content)) in kf8_result.parts.iter().enumerate() {
                let html = wrap_html_content(
                    content,
                    &mobi.title,
                    i,
                    &kf8_result.elems,
                    &text,
                    &file_starts,
                );
                book.add_resource(filename, html.into_bytes(), "application/xhtml+xml");
                book.add_spine_item(format!("part{i:04}"), filename, "application/xhtml+xml");
            }

            // Add TOC entries from NCX, reconstructing hierarchy from parent indices
            book.toc = build_toc_from_ncx(&kf8_result.ncx, &kf8_result.elems, &kf8_result.files);

            // Add CSS with resolved kindle:embed references
            // Use original flow index for filename to match kindle:flow:N references
            // kindle:flow:N maps to styles/style{N-1:04}.css in transform.rs
            for (flow_idx, css) in kf8_result.css_flows.iter() {
                let css_str = String::from_utf8_lossy(css);
                let resolved_css = resolve_css_kindle_embeds(&css_str, &resource_map);
                // flow_idx is 1-based (flow 0 is HTML), so style index = flow_idx - 1
                let filename = format!("styles/style{:04}.css", flow_idx - 1);
                book.add_resource(&filename, resolved_css.into_bytes(), "text/css");
            }
        } else {
            // KF8 index parsing failed, fall back to single file
            add_single_file_content(&mut book, &text, &mobi);
        }
    } else {
        // MOBI6: single HTML stream
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
        let href = format!("images/image_{i:04}.{ext}");
        book.add_resource(&href, data, &media_type);

        // Check if this is the cover
        if let Some(ref exth) = exth
            && exth.cover_offset == Some(i as u32)
        {
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
        let href = format!("fonts/font_{i:04}.{ext}");
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

/// Parse EXTH header if present
fn parse_exth(record0: &[u8], header: &MobiHeader) -> Option<ExthHeader> {
    if header.has_exth() && header.header_length > 0 {
        let exth_start = 16 + header.header_length as usize;
        if exth_start < record0.len() {
            return ExthHeader::parse(&record0[exth_start..], header.encoding).ok();
        }
    }
    None
}

/// Detect MOBI format variant and return appropriate headers
/// Returns: (format, content_header, resource_header)
/// - content_header: used for text extraction (KF8 for combo/pure KF8, MOBI6 for legacy)
/// - resource_header: used for resource extraction (MOBI6 for combo since resources are shared)
fn detect_format<R: Read + Seek>(
    reader: &mut R,
    pdb: &PdbHeader,
    header0: MobiHeader,
    exth: &Option<ExthHeader>,
) -> io::Result<(MobiFormat, MobiHeader, MobiHeader)> {
    // Pure KF8: record 0 is already version 8
    if header0.mobi_version == 8 {
        return Ok((
            MobiFormat::Kf8 { record_offset: 0 },
            header0.clone(),
            header0,
        ));
    }

    // Check for combo file: EXTH 121 points to KF8 section after BOUNDARY marker
    if let Some(kf8_idx) = exth.as_ref().and_then(|e| e.kf8_boundary) {
        let kf8_idx = kf8_idx as usize;
        if kf8_idx > 0 && kf8_idx < pdb.num_records as usize {
            // Verify BOUNDARY marker exists
            let boundary = pdb.read_record(reader, kf8_idx - 1)?;
            if boundary.starts_with(b"BOUNDARY") {
                // Parse KF8 header
                if let Ok(kf8_header) = MobiHeader::parse(&pdb.read_record(reader, kf8_idx)?) {
                    return Ok((
                        MobiFormat::Combo {
                            kf8_record_offset: kf8_idx,
                        },
                        kf8_header,
                        header0, // Use MOBI6 header for resources (shared resources)
                    ));
                }
            }
        }
    }

    // Legacy MOBI6
    Ok((MobiFormat::Mobi6, header0.clone(), header0))
}

/// KF8 parsing result
struct Kf8Result {
    parts: Vec<(String, Vec<u8>)>, // (filename, content)
    ncx: Vec<NcxEntry>,
    css_flows: Vec<(usize, Vec<u8>)>, // (flow_index, content) - preserves original indices
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
    record_offset: usize, // Offset for combo MOBI6+KF8 files
) -> io::Result<Kf8Result> {
    // Parse FDST for flow boundaries (FDST index needs offset too)
    let flow_table = parse_fdst(reader, pdb, mobi, record_offset)?;

    // Get HTML content (flow 0) - everything else is CSS/SVG
    // Calibre: text = flows[0] = raw_ml[start:end] for first flow
    let (html_start, html_end) = flow_table.first().copied().unwrap_or((0, text.len()));
    let html_text = &text[html_start..html_end.min(text.len())];

    // Extract CSS flows (flows 1+), preserving original flow indices
    // kindle:flow:N references flow N (1-based), where flow 0 is HTML
    let mut css_flows = Vec::new();
    for (i, (start, end)) in flow_table.iter().enumerate().skip(1) {
        if *start < text.len() && *end <= text.len() {
            let flow_data = text[*start..*end].to_vec();
            // Check if it looks like CSS (or SVG)
            if is_css_like(&flow_data) {
                css_flows.push((i, flow_data)); // Store original flow index
            }
        }
    }

    // Create record reader closure with offset for combo files
    let mut read_record = |idx: usize| -> io::Result<Vec<u8>> {
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
) -> io::Result<Vec<(usize, usize)>> {
    if mobi.fdst_index == NULL_INDEX {
        return Ok(Vec::new());
    }

    let actual_idx = mobi.fdst_index as usize + record_offset;
    let fdst_record = pdb.read_record(reader, actual_idx)?;

    if fdst_record.len() < 12 || &fdst_record[0..4] != b"FDST" {
        return Ok(Vec::new());
    }

    let sec_start = u32::from_be_bytes([
        fdst_record[4],
        fdst_record[5],
        fdst_record[6],
        fdst_record[7],
    ]) as usize;
    let num_sections = u32::from_be_bytes([
        fdst_record[8],
        fdst_record[9],
        fdst_record[10],
        fdst_record[11],
    ]) as usize;

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
) -> io::Result<Vec<(String, Vec<u8>)>> {
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
                        (Some(lt), Some(gt)) => lt > gt, // '<' after '>' means unclosed tag
                        (Some(_), None) => true,         // '<' with no '>' means unclosed tag
                        _ => false,
                    }
                };

                // Check for incomplete tag in tail: first '>' should be after first '<'
                let tail_incomplete = {
                    let first_lt = tail.iter().position(|&b| b == b'<');
                    let first_gt = tail.iter().position(|&b| b == b'>');
                    match (first_lt, first_gt) {
                        (Some(lt), Some(gt)) => gt < lt, // '>' before '<' means we're inside a tag
                        (None, Some(_)) => true,         // '>' with no '<' means we're inside a tag
                        _ => false,
                    }
                };

                // Note: KF8 intentionally splits tags like "a" + "id=" = "aid="
                // across skeleton and div content. This is NOT an error - don't "fix" it.
                // Calibre warns but uses a different correction method involving
                // locate_beg_end_of_tag() which we don't implement.
                // For now, trust the insert positions from the div table.
                if head_incomplete || tail_incomplete {}
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

/// Build hierarchical TOC from flat NCX entries using parent indices
fn build_toc_from_ncx(
    ncx: &[NcxEntry],
    elems: &[DivElement],
    files: &[SkeletonFile],
) -> Vec<TocEntry> {
    use quick_xml::escape::unescape;

    // First pass: create TocEntry for each NCX entry
    let entries: Vec<TocEntry> = ncx
        .iter()
        .map(|ncx_entry| {
            // Map NCX entry to filename
            let href = if let Some((elem_idx, _offset)) = ncx_entry.pos_fid {
                if let Some(elem) = elems.get(elem_idx as usize) {
                    format!("part{:04}.html", elem.file_number)
                } else {
                    format!("part{:04}.html", 0)
                }
            } else {
                find_file_for_position(files, ncx_entry.pos)
                    .map(|f| format!("part{:04}.html", f.file_number))
                    .unwrap_or_else(|| "part0000.html".to_string())
            };

            let title = unescape(&ncx_entry.text)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| ncx_entry.text.clone());

            TocEntry::new(&title, &href)
        })
        .collect();

    // If no hierarchy info, return flat list
    if ncx.iter().all(|e| e.parent < 0) {
        return entries;
    }

    // Second pass: build hierarchy using parent indices
    // We need to own entries so we can move them into children
    let mut entries: Vec<Option<TocEntry>> = entries.into_iter().map(Some).collect();
    let mut roots: Vec<usize> = Vec::new();

    // Identify roots and collect children by parent
    let mut children_map: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();

    for (i, ncx_entry) in ncx.iter().enumerate() {
        if ncx_entry.parent < 0 {
            roots.push(i);
        } else {
            children_map
                .entry(ncx_entry.parent as usize)
                .or_default()
                .push(i);
        }
    }

    // Recursively build tree, taking entries out of the Option vec
    fn take_with_children(
        idx: usize,
        entries: &mut [Option<TocEntry>],
        children_map: &std::collections::HashMap<usize, Vec<usize>>,
    ) -> Option<TocEntry> {
        let mut entry = entries[idx].take()?;

        if let Some(children_indices) = children_map.get(&idx) {
            for &child_idx in children_indices {
                if let Some(child) = take_with_children(child_idx, entries, children_map) {
                    entry.children.push(child);
                }
            }
        }

        Some(entry)
    }

    // Build result from roots
    roots
        .into_iter()
        .filter_map(|idx| take_with_children(idx, &mut entries, &children_map))
        .collect()
}

/// Process KF8 content: clean up declarations and convert kindle: references
fn wrap_html_content(
    content: &[u8],
    _title: &str,
    part_num: usize,
    elems: &[DivElement],
    raw_text: &[u8],
    file_starts: &[(u32, u32)],
) -> String {
    use super::transform::{strip_kindle_attributes_fast, transform_kindle_refs};

    // Step 1: Strip encoding declarations (quick byte scan)
    let cleaned = strip_encoding_declarations_fast(content);

    // Step 2: Transform kindle: references (single pass with SIMD search)
    let transformed = transform_kindle_refs(&cleaned, elems, raw_text, file_starts);

    // Step 3: Strip Amazon attributes (single pass)
    let stripped = strip_kindle_attributes_fast(&transformed);

    // Step 4: Ensure proper XHTML structure
    let stripped_str = String::from_utf8_lossy(&stripped);
    ensure_xhtml_structure(&stripped_str, part_num)
}

/// Fast byte-level encoding declaration stripping
fn strip_encoding_declarations_fast(html: &[u8]) -> Vec<u8> {
    use memchr::memmem;

    let mut result = Vec::with_capacity(html.len());
    let mut pos = 0;

    // Remove <?xml ... ?> declarations
    let xml_finder = memmem::Finder::new(b"<?xml");
    let xml_end_finder = memmem::Finder::new(b"?>");

    while let Some(start) = xml_finder.find(&html[pos..]) {
        let abs_start = pos + start;
        result.extend_from_slice(&html[pos..abs_start]);

        if let Some(end) = xml_end_finder.find(&html[abs_start..]) {
            pos = abs_start + end + 2;
        } else {
            pos = abs_start;
            break;
        }
    }

    result.extend_from_slice(&html[pos..]);

    // Clean up double << that might occur from skeleton placeholder issues
    // Do this efficiently by scanning for << and replacing with <
    let mut final_result = Vec::with_capacity(result.len());
    let mut i = 0;
    while i < result.len() {
        if i + 1 < result.len() && result[i] == b'<' && result[i + 1] == b'<' {
            final_result.push(b'<');
            i += 2;
        } else {
            final_result.push(result[i]);
            i += 1;
        }
    }

    final_result
}

/// Strip Amazon-specific attributes and fix XHTML compliance issues
fn strip_kindle_attributes(html: &str) -> String {
    use super::transform::strip_kindle_attributes_fast;
    let bytes = html.as_bytes();
    let stripped = strip_kindle_attributes_fast(bytes);
    String::from_utf8_lossy(&stripped).into_owned()
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
            let stripped = before_html.trim().trim_start_matches(|c: char| {
                c == '<'
                    || c == '?'
                    || c == '!'
                    || c.is_alphanumeric()
                    || c == '"'
                    || c == '='
                    || c == '/'
                    || c == '>'
                    || c == '-'
                    || c == '.'
                    || c.is_whitespace()
            });
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
{body_content}
</body>
</html>"#
        );
    } else {
        // Add XML declaration if missing
        if !result.starts_with("<?xml") {
            result = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{result}");
        }

        // Add xmlns to html tag if missing
        if let Some(start) = result.find("<html")
            && let Some(end) = result[start..].find('>')
        {
            let html_tag = &result[start..start + end + 1];
            if !html_tag.contains("xmlns") {
                let new_tag =
                    html_tag.replace("<html", "<html xmlns=\"http://www.w3.org/1999/xhtml\"");
                result = format!(
                    "{}{}{}",
                    &result[..start],
                    new_tag,
                    &result[start + end + 1..]
                );
            }
        }

        // Add meta charset after <head> if not present (use EPUB2-compatible format)
        if let Some(head_pos) = result.find("<head>") {
            let after_head = head_pos + 6;
            if !result[after_head..].starts_with("<meta charset")
                && !result[after_head..].starts_with("<meta http-equiv")
            {
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
        && let Some(body_tag_end) = html[body_start..].find('>')
    {
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
            && let Some(body_tag_end) = content[body_start..].find('>')
        {
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
        && let Some(first_tag) = result.find('<')
    {
        // Check if this is a real tag start or just a < in text
        let tag_content = &result[first_tag..];
        if tag_content.starts_with("<div")
            || tag_content.starts_with("<p")
            || tag_content.starts_with("<h")
            || tag_content.starts_with("<span")
            || tag_content.starts_with("<a")
            || tag_content.starts_with("<img")
        {
            result = result[first_tag..].to_string();
        }
    }

    result
}

/// Add content as a single file (fallback mode)
fn add_single_file_content(book: &mut Book, text: &[u8], mobi: &MobiHeader) {
    let first_image_index = mobi.first_image_index as usize;
    let html_content = build_html(text, mobi, first_image_index);
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
) -> io::Result<Vec<u8>> {
    // Pre-allocate based on expected text length (count * size per record)
    let estimated_size = mobi.text_record_count as usize * mobi.text_record_size as usize;
    let mut text = Vec::with_capacity(estimated_size);
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
            Compression::PalmDoc => super::palmdoc::decompress(record)?,
            Compression::None => record.to_vec(),
            Compression::Huffman => {
                if let Some(ref mut hr) = huff_reader {
                    hr.decompress(record)?
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "Huffman reader not initialized",
                    ));
                }
            }
            Compression::Unknown(n) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("Unknown compression type: {n}"),
                ));
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
) -> io::Result<HuffCdicReader> {
    if mobi.huff_record_index == NULL_INDEX || mobi.huff_record_count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Huffman compression but no HUFF/CDIC records",
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
) -> io::Result<ExtractedResources> {
    let mut images = Vec::new();
    let mut fonts = Vec::new();
    let mut resource_map: Vec<Option<String>> = Vec::new();

    let first_image = mobi.first_image_index as usize;
    if first_image == NULL_INDEX as usize {
        return Ok(ExtractedResources {
            images,
            fonts,
            resource_map,
        });
    }

    let mut image_idx = 0usize;
    let mut font_idx = 0usize;

    for i in first_image..pdb.record_offsets.len() {
        let record = pdb.read_record(reader, i)?;

        // Check for FONT record
        if record.starts_with(b"FONT") {
            if let Some((font_data, ext)) = read_font_record(&record) {
                let href = format!("fonts/font_{font_idx:04}.{ext}");
                resource_map.push(Some(href.clone()));
                fonts.push((font_data, ext));
                font_idx += 1;
            } else {
                resource_map.push(None);
            }
            continue;
        }

        // Skip metadata records
        if record.starts_with(b"FLIS")
            || record.starts_with(b"FCIS")
            || record.starts_with(b"SRCS")
            || record.starts_with(b"BOUN")
            || record.starts_with(b"FDST")
            || record.starts_with(b"DATP")
            || record.starts_with(b"AUDI")
            || record.starts_with(b"VIDE")
            || record.starts_with(b"RESC")
            || record.starts_with(b"CMET")
            || record.starts_with(b"PAGE")
            || record.starts_with(b"CONT")
            || record.starts_with(b"CRES")
            || record.starts_with(b"BOUNDARY")
        {
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
            let href = format!("images/image_{image_idx:04}.{ext}");
            resource_map.push(Some(href));
            images.push((record, mt.to_string()));
            image_idx += 1;
        } else {
            resource_map.push(None);
        }
    }

    Ok(ExtractedResources {
        images,
        fonts,
        resource_map,
    })
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
    use bstr::ByteSlice;
    use memchr::memmem;

    let css_bytes = css.as_bytes();
    let mut output = Vec::with_capacity(css_bytes.len());
    let mut pos = 0;

    // Finders for patterns
    let fontface_finder = memmem::Finder::new(b"@font-face");
    let embed_finder = memmem::Finder::new(b"kindle:embed:");

    // First pass: strip @font-face blocks with placeholder URLs (XXXX patterns)
    while let Some(ff_start) = fontface_finder.find(&css_bytes[pos..]) {
        let abs_start = pos + ff_start;

        // Find the opening brace
        if let Some(brace_start) = css_bytes[abs_start..].find_byte(b'{') {
            let abs_brace = abs_start + brace_start;
            // Find the closing brace
            if let Some(brace_end) = css_bytes[abs_brace..].find_byte(b'}') {
                let abs_end = abs_brace + brace_end + 1;
                let block = &css_bytes[abs_start..abs_end];

                // Check if this block has placeholder URL (10+ X's)
                if block.find(b"XXXXXXXXXX").is_some() {
                    // Skip this @font-face block
                    output.extend_from_slice(&css_bytes[pos..abs_start]);
                    pos = abs_end;
                    continue;
                }
            }
        }
        // Not a placeholder block, include it and move past @font-face
        output.extend_from_slice(&css_bytes[pos..abs_start + 10]);
        pos = abs_start + 10;
    }
    output.extend_from_slice(&css_bytes[pos..]);

    // Second pass: replace kindle:embed:XXXX references
    let result = output;
    let mut output = Vec::with_capacity(result.len());
    pos = 0;

    while let Some(embed_start) = embed_finder.find(&result[pos..]) {
        let abs_start = pos + embed_start;
        output.extend_from_slice(&result[pos..abs_start]);

        // Parse the base32 value after "kindle:embed:"
        let after_prefix = &result[abs_start + 13..];
        let mut base32_end = 0;
        for &b in after_prefix {
            if b.is_ascii_alphanumeric() {
                base32_end += 1;
            } else {
                break;
            }
        }

        if base32_end > 0 {
            let base32_str = &after_prefix[..base32_end];
            let idx = super::parse_base32(base32_str);
            let resource_idx = if idx > 0 { idx - 1 } else { 0 };

            // Look up resource path
            let replacement = if let Some(Some(href)) = resource_map.get(resource_idx) {
                format!("../{href}")
            } else {
                "missing-resource".to_string()
            };

            output.extend_from_slice(replacement.as_bytes());

            // Skip past the kindle:embed:XXXX and optional ?mime=... part
            let mut skip_end = 13 + base32_end;
            if after_prefix.get(base32_end) == Some(&b'?') {
                // Skip ?mime=... until quote or paren
                for &b in &after_prefix[base32_end..] {
                    if b == b'"' || b == b'\'' || b == b')' {
                        break;
                    }
                    skip_end += 1;
                }
            }
            pos = abs_start + skip_end;
        } else {
            // No valid base32, copy as-is
            output.extend_from_slice(b"kindle:embed:");
            pos = abs_start + 13;
        }
    }
    output.extend_from_slice(&result[pos..]);

    String::from_utf8_lossy(&output).into_owned()
}

fn build_html(text: &[u8], mobi: &MobiHeader, first_image_index: usize) -> String {
    let content = match mobi.encoding {
        Encoding::Utf8 => String::from_utf8_lossy(text).to_string(),
        _ => String::from_utf8_lossy(text).to_string(),
    };

    let body_content = extract_body_content(&content);
    // Process MOBI6-specific markup (recindex, filepos, mbp:pagebreak)
    let body_content = process_mobi6_markup(&body_content, text, first_image_index);
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

/// Process MOBI6-specific markup
/// - Convert recindex="N" to actual image paths
/// - Convert filepos="N" links to #fileposN anchors
/// - Insert anchors at filepos positions
/// - Convert mbp:pagebreak tags
fn process_mobi6_markup(html: &str, raw_text: &[u8], first_image_index: usize) -> String {
    use std::collections::HashSet;

    let mut result = html.to_string();

    // Step 1: Collect all filepos targets that are linked to
    let mut filepos_targets: HashSet<usize> = HashSet::new();
    let mut search_pos = 0;
    while let Some(pos) = result[search_pos..].find("filepos=") {
        let abs_pos = search_pos + pos;
        let after = &result[abs_pos + 8..];
        // Parse the filepos value (may be quoted or unquoted)
        let (value_str, _) = parse_attribute_value(after);
        if let Ok(filepos) = value_str.parse::<usize>() {
            filepos_targets.insert(filepos);
        }
        search_pos = abs_pos + 8;
    }

    // Step 2: Insert anchors at filepos positions in the raw text
    // We need to map byte positions to positions in the processed HTML
    // For now, we add anchors inline where tags would naturally occur
    result = insert_filepos_anchors(&result, raw_text, &filepos_targets);

    // Step 3: Convert filepos="N" to href="#fileposN"
    result = convert_filepos_links(&result);

    // Step 4: Convert recindex="N" to actual image paths
    result = convert_recindex_images(&result, first_image_index);

    // Step 5: Convert mbp:pagebreak tags
    result = convert_mbp_pagebreaks(&result);

    result
}

/// Parse an attribute value (handles quoted and unquoted)
fn parse_attribute_value(s: &str) -> (&str, usize) {
    let s = s.trim_start();
    if let Some(stripped) = s.strip_prefix('"') {
        if let Some(end) = stripped.find('"') {
            return (&stripped[..end], 1 + end + 1);
        }
    } else if let Some(stripped) = s.strip_prefix('\'') {
        if let Some(end) = stripped.find('\'') {
            return (&stripped[..end], 1 + end + 1);
        }
    } else {
        // Unquoted - ends at whitespace or >
        let end = s
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(s.len());
        return (&s[..end], end);
    }
    ("", 0)
}

/// Insert anchors at filepos positions
fn insert_filepos_anchors(
    html: &str,
    _raw_text: &[u8],
    targets: &std::collections::HashSet<usize>,
) -> String {
    if targets.is_empty() {
        return html.to_string();
    }

    // For MOBI6, filepos values are byte positions in the raw text
    // We need to insert <a id="fileposN"></a> anchors at those positions
    // Since we're working with processed HTML, we approximate by inserting
    // anchors at the start of paragraphs/divs near those positions

    let mut result = String::with_capacity(html.len() + targets.len() * 30);
    let mut sorted_targets: Vec<usize> = targets.iter().copied().collect();
    sorted_targets.sort();

    let bytes = html.as_bytes();
    let mut pos = 0;
    let mut target_idx = 0;

    while pos < bytes.len() {
        // Look for tag starts that could be good anchor points
        if bytes[pos] == b'<' {
            // Check if this is a block element that could be an anchor point
            let remaining = &html[pos..];
            let is_block_start = remaining.starts_with("<p")
                || remaining.starts_with("<div")
                || remaining.starts_with("<h1")
                || remaining.starts_with("<h2")
                || remaining.starts_with("<h3")
                || remaining.starts_with("<h4")
                || remaining.starts_with("<h5")
                || remaining.starts_with("<h6")
                || remaining.starts_with("<section")
                || remaining.starts_with("<article");

            // Insert any pending anchors for positions we've passed
            while target_idx < sorted_targets.len() && sorted_targets[target_idx] <= pos {
                let target = sorted_targets[target_idx];
                result.push_str(&format!("<a id=\"filepos{target}\"></a>"));
                target_idx += 1;
            }

            if is_block_start && target_idx < sorted_targets.len() {
                // Check if next target is close to current position
                let next_target = sorted_targets[target_idx];
                if next_target <= pos + 100 {
                    result.push_str(&format!("<a id=\"filepos{next_target}\"></a>"));
                    target_idx += 1;
                }
            }
        }

        result.push(bytes[pos] as char);
        pos += 1;
    }

    // Insert any remaining anchors at the end
    while target_idx < sorted_targets.len() {
        let target = sorted_targets[target_idx];
        result.push_str(&format!("<a id=\"filepos{target}\"></a>"));
        target_idx += 1;
    }

    result
}

/// Convert filepos="N" links to href="#fileposN"
fn convert_filepos_links(html: &str) -> String {
    use memchr::memmem;

    let finder = memmem::Finder::new(b"filepos=");
    let bytes = html.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut pos = 0;

    while let Some(found) = finder.find(&bytes[pos..]) {
        let abs_pos = pos + found;
        result.extend_from_slice(&bytes[pos..abs_pos]);

        // Parse the filepos value
        let after = &html[abs_pos + 8..];
        let (value_str, consumed) = parse_attribute_value(after);

        if let Ok(filepos) = value_str.parse::<usize>() {
            // Replace with href="#fileposN"
            result.extend_from_slice(format!("href=\"#filepos{filepos}\"").as_bytes());
        } else {
            // Keep original if can't parse
            result.extend_from_slice(b"filepos=");
            result.extend_from_slice(&after.as_bytes()[..consumed]);
        }

        pos = abs_pos + 8 + consumed;
    }

    result.extend_from_slice(&bytes[pos..]);
    String::from_utf8_lossy(&result).into_owned()
}

/// Convert recindex="N" to actual image paths
/// In MOBI6, recindex is 1-based from the first image record
fn convert_recindex_images(html: &str, _first_image_index: usize) -> String {
    use memchr::memmem;

    let finder = memmem::Finder::new(b"recindex=");
    let bytes = html.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut pos = 0;

    while let Some(found) = finder.find(&bytes[pos..]) {
        let abs_pos = pos + found;
        result.extend_from_slice(&bytes[pos..abs_pos]);

        // Parse the recindex value
        let after = &html[abs_pos + 9..];
        let (value_str, consumed) = parse_attribute_value(after);

        if let Ok(recindex) = value_str.parse::<usize>() {
            // recindex is 1-based, so subtract 1 for 0-based image index
            // Our images are named image_NNNN.ext starting from 0
            let img_idx = recindex.saturating_sub(1);
            result.extend_from_slice(format!("src=\"images/image_{img_idx:04}.jpg\"").as_bytes());
        } else {
            // Keep original if can't parse
            result.extend_from_slice(b"recindex=");
            result.extend_from_slice(&after.as_bytes()[..consumed]);
        }

        pos = abs_pos + 9 + consumed;
    }

    result.extend_from_slice(&bytes[pos..]);
    String::from_utf8_lossy(&result).into_owned()
}

/// Convert mbp:pagebreak tags to div elements
fn convert_mbp_pagebreaks(html: &str) -> String {
    html.replace("<mbp:pagebreak/>", "<div class=\"pagebreak\"></div>")
        .replace("<mbp:pagebreak />", "<div class=\"pagebreak\"></div>")
        .replace("<mbp:pagebreak>", "<div class=\"pagebreak\">")
        .replace("</mbp:pagebreak>", "</div>")
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
        && let Some(body_tag_end) = html[body_start..].find('>')
    {
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

fn escape_xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
