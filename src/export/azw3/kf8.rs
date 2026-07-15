use super::guide::*;
use super::*;

pub(super) struct Kf8Builder {
    ctx: BookContext,
    records: Vec<Vec<u8>>,
    text_length: usize,
    last_text_record: u16,
    first_non_text_record: u16,
    first_resource_record: u32,
    skel_index: u32,
    frag_index: u32,
    ncx_index: u32,
    guide_index: u32,
    chunker_result: Option<ChunkerResult>,
    /// Maps resource href to 1-indexed resource record number
    resource_map: HashMap<String, usize>,
    /// CSS flows (flow 0 is text, flows 1+ are CSS)
    css_flows: Vec<String>,
    /// Total flows length (text + CSS)
    flows_length: usize,
    /// Ordered list of image hrefs
    image_hrefs: Vec<String>,
    /// Ordered list of font hrefs
    font_hrefs: Vec<String>,
    /// Counter for link placeholders
    link_counter: usize,
    /// Maps placeholder -> (target_file_href, target_fragment)
    link_map: HashMap<String, (String, String)>,
    /// NCX entries with hierarchy-aware lengths, retained for TBS calculation.
    ncx_entries: Vec<NcxBuildEntry>,
}

impl Kf8Builder {
    pub(super) fn new(book: &Book, normalize: bool) -> crate::Result<Self> {
        let ctx = BookContext::from_book(book, normalize)?;

        let mut builder = Self {
            ctx,
            records: vec![Vec::new()], // Placeholder for record 0
            text_length: 0,
            last_text_record: 0,
            first_non_text_record: 0,
            first_resource_record: NULL_INDEX,
            skel_index: NULL_INDEX,
            frag_index: NULL_INDEX,
            ncx_index: NULL_INDEX,
            guide_index: NULL_INDEX,
            chunker_result: None,
            resource_map: HashMap::new(),
            css_flows: Vec::new(),
            flows_length: 0,
            image_hrefs: Vec::new(),
            font_hrefs: Vec::new(),
            link_counter: 0,
            link_map: HashMap::new(),
            ncx_entries: Vec::new(),
        };

        builder.collect_resources()?;
        builder.build_text_records()?;
        builder.build_kf8_indices()?;
        builder.apply_trailing_byte_sequences();
        builder.write_resource_records()?;
        builder.build_fdst_record()?;
        builder.build_flis_fcis_eof()?;
        builder.build_record0()?;

        Ok(builder)
    }

    pub(super) fn collect_resources(&mut self) -> io::Result<()> {
        // Collect images
        self.image_hrefs = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| r.media_type.starts_with("image/"))
            .map(|(href, _)| href.clone())
            .collect();
        self.image_hrefs.sort();

        // Collect fonts
        self.font_hrefs = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| {
                r.media_type.contains("font")
                    || r.media_type == "application/x-font-ttf"
                    || r.media_type == "application/x-font-opentype"
                    || r.media_type == "application/vnd.ms-opentype"
                    || r.media_type == "font/ttf"
                    || r.media_type == "font/otf"
                    || r.media_type == "font/woff"
            })
            .map(|(href, _)| href.clone())
            .collect();
        self.font_hrefs.sort();

        // Collect CSS
        let mut css_hrefs: Vec<_> = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .map(|(href, _)| href.clone())
            .collect();
        css_hrefs.sort();

        for href in &css_hrefs {
            if let Some(resource) = self.ctx.resources.get(href) {
                let css = String::from_utf8_lossy(&resource.data).to_string();
                self.css_flows.push(css);
            }
        }

        // Build resource_map
        let mut resource_idx = 1usize;

        for href in &self.image_hrefs {
            self.resource_map.insert(href.clone(), resource_idx);
            resource_idx += 1;
        }

        for href in &self.font_hrefs {
            self.resource_map.insert(href.clone(), resource_idx);
            resource_idx += 1;
        }

        Ok(())
    }

    pub(super) fn build_text_records(&mut self) -> io::Result<()> {
        // Build CSS href -> flow index map
        let mut css_hrefs: Vec<_> = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .map(|(href, _)| href.clone())
            .collect();
        css_hrefs.sort();

        let mut css_flow_map: HashMap<String, usize> = HashMap::new();
        for (i, href) in css_hrefs.iter().enumerate() {
            css_flow_map.insert(href.clone(), i + 1);
        }

        // Build spine hrefs set
        let spine_hrefs: HashSet<&str> = self.ctx.spine.iter().map(|s| s.href.as_str()).collect();

        // Process HTML files
        let mut html_files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut link_counter = 0usize;

        for spine_item in &self.ctx.spine {
            if let Some(resource) = self.ctx.resources.get(&spine_item.href)
                && resource.media_type == "application/xhtml+xml"
            {
                let result = rewrite_html_references_fast(
                    &resource.data,
                    &spine_item.href,
                    &css_flow_map,
                    &self.resource_map,
                    &spine_hrefs,
                    &self.ctx.resources,
                    link_counter,
                );

                // Collect links
                for link in result.links {
                    let mut base32_buf = [0u8; 10];
                    write_base32_10(link_counter + 1, &mut base32_buf);
                    let placeholder = format!(
                        "kindle:pos:fid:0000:off:{}",
                        std::str::from_utf8(&base32_buf).unwrap()
                    );
                    self.link_map
                        .insert(placeholder, (link.target_file, link.fragment));
                    link_counter += 1;
                }

                html_files.push((spine_item.href.clone(), result.html));
            }
        }
        self.link_counter = link_counter;

        // Rewrite CSS
        let rewritten_css: Vec<Vec<u8>> = self
            .css_flows
            .iter()
            .map(|css| rewrite_css_references_fast(css.as_bytes(), &self.resource_map))
            .collect();

        // Process with chunker
        let mut chunker = Chunker::new();
        let chunker_result = chunker.process(&html_files);

        // Resolve link placeholders
        let resolved_text = self.resolve_link_placeholders(
            &chunker_result.text,
            &chunker_result.id_map,
            &chunker_result.aid_offset_map,
            &chunker_result.filepos_map,
        );
        self.text_length = resolved_text.len();

        // Combine flows
        let mut all_flows = resolved_text;
        for css in &rewritten_css {
            all_flows.extend_from_slice(css);
        }
        self.flows_length = all_flows.len();

        // Split into records and PalmDoc-compress. Records are compressed
        // independently, so this fans out across chunks (the compression is
        // the bulk of AZW3 export time on large books).
        fn compress_record(chunk: &[u8]) -> Vec<u8> {
            let mut record = crate::mobi::palmdoc::compress(chunk);
            record.push(0); // multibyte indicator (0 = no UTF-8 overlap)
            record
        }
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        {
            use rayon::prelude::*;
            self.records
                .par_extend(all_flows.par_chunks(RECORD_SIZE).map(compress_record));
        }
        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        {
            self.records
                .extend(all_flows.chunks(RECORD_SIZE).map(compress_record));
        }

        self.last_text_record = (self.records.len() - 1) as u16;
        // Insert a zero-padding record after text so the next (non-text)
        // record starts at a 4-byte boundary in the rawml stream. Calibre
        // does this in writer8/main.py:361-363 and bumps
        // `first_non_text_record_idx` past the pad.
        let total_text_bytes: usize = self.records[1..=self.last_text_record as usize]
            .iter()
            .map(|r| r.len())
            .sum();
        let remainder = total_text_bytes % 4;
        if remainder != 0 {
            self.records.push(vec![0u8; 4 - remainder]);
            self.first_non_text_record = self.last_text_record + 2;
        } else {
            self.first_non_text_record = self.last_text_record + 1;
        }
        self.chunker_result = Some(chunker_result);

        self.css_flows = rewritten_css
            .into_iter()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect();

        Ok(())
    }

    pub(super) fn resolve_link_placeholders(
        &self,
        text: &[u8],
        id_map: &HashMap<(String, String), String>,
        aid_offset_map: &HashMap<String, (usize, usize, usize)>,
        filepos_map: &HashMap<String, Vec<(usize, String)>>,
    ) -> Vec<u8> {
        use memchr::memmem;

        const PREFIX: &[u8] = b"kindle:pos:fid:0000:off:";
        const PLACEHOLDER_LEN: usize = 34;

        let finder = memmem::Finder::new(PREFIX);
        let mut replacements: Vec<(usize, usize, [u8; 34])> = Vec::new();

        let mut search_start = 0;
        while let Some(pos) = finder.find(&text[search_start..]) {
            let start = search_start + pos;
            let end = start + PLACEHOLDER_LEN;

            if end <= text.len() {
                let placeholder = std::str::from_utf8(&text[start..end]).unwrap_or("");

                if let Some((target_file, fragment)) = self.link_map.get(placeholder) {
                    // Try to resolve the link target
                    let resolved = if fragment.starts_with("filepos") {
                        // MOBI filepos reference - use filepos_map
                        resolve_filepos_to_offset(
                            target_file,
                            fragment,
                            filepos_map,
                            aid_offset_map,
                        )
                    } else {
                        // Standard element ID - use id_map
                        let key = (target_file.clone(), fragment.clone());
                        let aid = id_map
                            .get(&key)
                            .or_else(|| id_map.get(&(target_file.clone(), String::new())));

                        aid.and_then(|aid| {
                            aid_offset_map
                                .get(aid)
                                .map(|&(seq_num, offset_in_chunk, _)| (seq_num, offset_in_chunk))
                        })
                    };

                    if let Some((seq_num, offset_in_chunk)) = resolved {
                        let mut replacement = [0u8; 34];
                        replacement[..15].copy_from_slice(b"kindle:pos:fid:");
                        let mut fid_buf = [0u8; 4];
                        write_base32_4(seq_num, &mut fid_buf);
                        replacement[15..19].copy_from_slice(&fid_buf);
                        replacement[19..24].copy_from_slice(b":off:");
                        let mut off_buf = [0u8; 10];
                        write_base32_10(offset_in_chunk, &mut off_buf);
                        replacement[24..34].copy_from_slice(&off_buf);
                        replacements.push((start, end, replacement));
                    }
                }
            }
            search_start = start + 1;
        }

        if replacements.is_empty() {
            return text.to_vec();
        }

        replacements.sort_by_key(|(start, _, _)| *start);

        let mut result = Vec::with_capacity(text.len());
        let mut last_end = 0;

        for (start, end, replacement) in replacements {
            result.extend_from_slice(&text[last_end..start]);
            result.extend_from_slice(&replacement);
            last_end = end;
        }
        result.extend_from_slice(&text[last_end..]);

        result
    }

    pub(super) fn write_resource_records(&mut self) -> io::Result<()> {
        if !self.image_hrefs.is_empty() || !self.font_hrefs.is_empty() {
            self.first_resource_record = self.records.len() as u32;
        }

        // Write images. This is the last reader of `ctx.resources` (the
        // build sequence ends with fdst/flis/record0, none of which touch
        // it), so move each image's bytes out instead of cloning multi-MB
        // payloads.
        for i in 0..self.image_hrefs.len() {
            if let Some(resource) = self.ctx.resources.get_mut(&self.image_hrefs[i]) {
                self.records.push(std::mem::take(&mut resource.data));
            }
        }

        // Write fonts as FONT records
        for i in 0..self.font_hrefs.len() {
            if let Some(resource) = self.ctx.resources.get(&self.font_hrefs[i]) {
                let font_record = write_font_record(&resource.data)?;
                self.records.push(font_record);
            }
        }

        Ok(())
    }

    pub(super) fn build_kf8_indices(&mut self) -> io::Result<()> {
        if let Some(ref chunker_result) = self.chunker_result {
            // Build SKEL index
            if !chunker_result.skel_table.is_empty() {
                self.skel_index = self.records.len() as u32;
                let skel_records = build_skel_indx(&chunker_result.skel_table)?;
                for record in skel_records {
                    self.records.push(record);
                }
            }

            // Build Fragment/Chunk index
            if !chunker_result.chunk_table.is_empty() {
                let selectors: Vec<String> = chunker_result
                    .chunk_table
                    .iter()
                    .map(|c| c.selector.clone())
                    .collect();
                // `calculate_cncx_offsets` and `build_cncx` share the same
                // record-rollover logic, so the offsets already encode
                // `record_number << 16 | offset_within_record`.
                let cncx_offsets = calculate_cncx_offsets(&selectors);
                let cncx_records = build_cncx(&selectors)?;

                self.frag_index = self.records.len() as u32;
                let chunk_records = build_chunk_indx(
                    &chunker_result.chunk_table,
                    &cncx_offsets,
                    cncx_records.len() as u32,
                )?;
                for record in chunk_records {
                    self.records.push(record);
                }
                for record in cncx_records {
                    self.records.push(record);
                }
            }
        }

        // Build NCX index.
        if !self.ctx.toc.is_empty()
            && let Some(ref chunker_result) = self.chunker_result
        {
            let ncx_entries = flatten_toc(
                &self.ctx.toc,
                self.text_length as u32,
                &chunker_result.id_map,
                &chunker_result.aid_offset_map,
                &chunker_result.filepos_map,
            );

            if !ncx_entries.is_empty() {
                self.ncx_index = self.records.len() as u32;
                let (ncx_records, ncx_cncx) = build_ncx_indx(&ncx_entries)?;
                for record in ncx_records {
                    self.records.push(record);
                }
                for record in ncx_cncx {
                    self.records.push(record);
                }
                self.ncx_entries = ncx_entries;
            }
        }

        // Build K8 guide index from landmarks. Kindle reads this to discover
        // cover / start-of-content / endnotes / etc.
        if let Some(ref chunker_result) = self.chunker_result {
            let guide_entries = collect_guide_entries(
                self.ctx.landmarks(),
                self.ctx.metadata.cover_image.as_deref(),
                &chunker_result.id_map,
                &chunker_result.aid_offset_map,
            );
            if !guide_entries.is_empty() {
                self.guide_index = self.records.len() as u32;
                let (guide_records, guide_cncx) = build_guide_indx(&guide_entries)?;
                for record in guide_records {
                    self.records.push(record);
                }
                for record in guide_cncx {
                    self.records.push(record);
                }
            }
        }

        Ok(())
    }

    /// Append per-record Trailing Byte Sequences to every text record.
    ///
    /// Kindle uses TBS to map text positions to NCX entries; firmware versions
    /// since at least Paperwhite 3 refuse to open books that declare TBS-style
    /// `extra_data_flags` (0x02) but omit the actual trailers, or that declare
    /// no TBS at all on multi-chapter books.
    pub(super) fn apply_trailing_byte_sequences(&mut self) {
        if self.ncx_entries.is_empty() || self.last_text_record == 0 {
            return;
        }

        // Text records are records[1..=last_text_record]. Each is RECORD_SIZE
        // bytes of uncompressed text/css except possibly the last, which is
        // the remainder of `flows_length`.
        let last_idx = self.last_text_record as usize;
        let total = self.flows_length;
        let mut lengths: Vec<u64> = Vec::with_capacity(last_idx);
        for i in 1..=last_idx {
            let start = (i - 1) * RECORD_SIZE;
            let end = (start + RECORD_SIZE).min(total);
            lengths.push((end - start) as u64);
        }

        let tbs_entries: Vec<crate::mobi::tbs::TbsEntry> = self
            .ncx_entries
            .iter()
            .enumerate()
            .map(|(i, e)| crate::mobi::tbs::TbsEntry {
                index: i as u32,
                start: e.pos as u64,
                length: e.length as u64,
                depth: e.depth,
                parent: e.parent,
            })
            .collect();

        let tbs_data = crate::mobi::tbs::build_tbs_for_records(&tbs_entries, &lengths);

        // Each record currently looks like `[compressed_text..., 0x00]` where
        // the trailing `0x00` is the multibyte indicator (no UTF-8 overlap).
        // The on-disk order Kindle expects is `[compressed][multibyte][tbs]`
        // — readers strip TBS from the end first, then strip multibyte (see
        // calibre `getRawML` and our own `strip_trailing_data`). So append
        // TBS *after* the multibyte byte, not before.
        for (i, tbs) in tbs_data.into_iter().enumerate() {
            let rec_idx = i + 1;
            self.records[rec_idx].extend_from_slice(&tbs);
        }
    }

    pub(super) fn build_fdst_record(&mut self) -> io::Result<()> {
        let num_flows = 1 + self.css_flows.len();

        let mut fdst = Vec::new();
        fdst.extend_from_slice(b"FDST");
        fdst.extend_from_slice(&12u32.to_be_bytes());
        fdst.extend_from_slice(&(num_flows as u32).to_be_bytes());

        // Flow 0: text
        fdst.extend_from_slice(&0u32.to_be_bytes());
        fdst.extend_from_slice(&(self.text_length as u32).to_be_bytes());

        // CSS flows
        let mut offset = self.text_length;
        for css in &self.css_flows {
            let start = offset;
            let end = offset + css.len();
            fdst.extend_from_slice(&(start as u32).to_be_bytes());
            fdst.extend_from_slice(&(end as u32).to_be_bytes());
            offset = end;
        }

        self.records.push(fdst);
        Ok(())
    }

    pub(super) fn build_flis_fcis_eof(&mut self) -> io::Result<()> {
        // FLIS
        let flis = b"FLIS\0\0\0\x08\0\x41\0\0\0\0\0\0\xff\xff\xff\xff\0\x01\0\x03\0\0\0\x03\0\0\0\x01\xff\xff\xff\xff";
        self.records.push(flis.to_vec());

        // FCIS
        let mut fcis = Vec::new();
        fcis.extend_from_slice(
            b"FCIS\x00\x00\x00\x14\x00\x00\x00\x10\x00\x00\x00\x02\x00\x00\x00\x00",
        );
        fcis.extend_from_slice(&(self.text_length as u32).to_be_bytes());
        fcis.extend_from_slice(b"\x00\x00\x00\x00\x00\x00\x00\x28\x00\x00\x00\x00\x00\x00\x00");
        fcis.extend_from_slice(b"\x28\x00\x00\x00\x08\x00\x01\x00\x01\x00\x00\x00\x00");
        self.records.push(fcis);

        // EOF
        self.records.push(b"\xe9\x8e\r\n".to_vec());

        Ok(())
    }

    pub(super) fn build_record0(&mut self) -> io::Result<()> {
        let title = &self.ctx.metadata.title;
        let title_bytes = title.as_bytes();

        let exth = self.build_exth();
        let exth_len = exth.len();

        let mobi_header_len: u32 = 264;
        let title_offset = 16 + mobi_header_len + exth_len as u32;
        // No `+ 2` separator between title and trailing padding — calibre
        // writes `exth + full_title + zeroes(8192)` with nothing in between.
        let full_record_len = title_offset as usize + title_bytes.len();

        let mut record0 = Vec::with_capacity(full_record_len + 8192);

        // PalmDOC header (16 bytes)
        record0.extend_from_slice(&2u16.to_be_bytes()); // Compression: PalmDOC
        record0.extend_from_slice(&[0, 0]);
        record0.extend_from_slice(&(self.text_length as u32).to_be_bytes());
        record0.extend_from_slice(&self.last_text_record.to_be_bytes());
        record0.extend_from_slice(&(RECORD_SIZE as u16).to_be_bytes());
        record0.extend_from_slice(&0u16.to_be_bytes()); // Encryption
        record0.extend_from_slice(&0u16.to_be_bytes());

        // MOBI header
        record0.extend_from_slice(b"MOBI");
        record0.extend_from_slice(&mobi_header_len.to_be_bytes());
        record0.extend_from_slice(&2u32.to_be_bytes()); // Book type
        record0.extend_from_slice(&65001u32.to_be_bytes()); // UTF-8
        record0.extend_from_slice(&rand_uid().to_be_bytes());
        record0.extend_from_slice(&8u32.to_be_bytes()); // KF8 version

        // Meta indices (40-80)
        for _ in 0..10 {
            record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        }

        // First non-text record — set by build_text_records, accounts for
        // any 4-byte alignment padding record we inserted after the text.
        record0.extend_from_slice(&(self.first_non_text_record as u32).to_be_bytes());

        // Title offset and length
        record0.extend_from_slice(&title_offset.to_be_bytes());
        record0.extend_from_slice(&(title_bytes.len() as u32).to_be_bytes());

        // Language
        record0.extend_from_slice(&0x09u32.to_be_bytes());

        // Dictionary in/out
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Min version
        record0.extend_from_slice(&8u32.to_be_bytes());

        // First resource record
        record0.extend_from_slice(&self.first_resource_record.to_be_bytes());

        // Huffman records
        for _ in 0..4 {
            record0.extend_from_slice(&0u32.to_be_bytes());
        }

        // EXTH flags
        record0.extend_from_slice(&0x50u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0u8; 32]);

        // Unknown index
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());

        // DRM
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0u8; 8]);

        // FDST
        let fdst_record = (self.records.len() - 4) as u32;
        record0.extend_from_slice(&fdst_record.to_be_bytes());
        let fdst_count = 1 + self.css_flows.len() as u32;
        record0.extend_from_slice(&fdst_count.to_be_bytes());

        // FCIS
        let fcis_record = (self.records.len() - 2) as u32;
        record0.extend_from_slice(&fcis_record.to_be_bytes());
        record0.extend_from_slice(&1u32.to_be_bytes());

        // FLIS
        let flis_record = (self.records.len() - 3) as u32;
        record0.extend_from_slice(&flis_record.to_be_bytes());
        record0.extend_from_slice(&1u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0u8; 8]);

        // SRCS
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0xFF; 8]);

        // Extra data flags: 0x01 (multibyte trailing byte) | 0x02 (TBS trailer).
        // Must match what apply_trailing_byte_sequences actually appends.
        let extra_data_flags: u32 = if self.ncx_entries.is_empty() { 1 } else { 3 };
        record0.extend_from_slice(&extra_data_flags.to_be_bytes());

        // KF8 indices (at MOBI offsets 0xF4–0x108):
        //   0xF4 ncx, 0xF8 frag, 0xFC skel, 0x100 datp, 0x104 guide
        record0.extend_from_slice(&self.ncx_index.to_be_bytes());
        record0.extend_from_slice(&self.frag_index.to_be_bytes());
        record0.extend_from_slice(&self.skel_index.to_be_bytes());
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // DATP
        record0.extend_from_slice(&self.guide_index.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0xFF; 4]);
        record0.extend_from_slice(&[0; 4]);
        record0.extend_from_slice(&[0xFF; 4]);
        record0.extend_from_slice(&[0; 4]);

        // EXTH
        record0.extend_from_slice(&exth);

        // Title
        record0.extend_from_slice(title_bytes);

        // Padding — calibre's MOBIHeader DSL ends with `padding = zeroes(8192)`
        // (writer8/mobi.py:191). 8KB of trailing zeros is what Amazon's DTP
        // service expects to find for in-place metadata edits; firmwares
        // also scan this region during a sanity-check pass.
        let target_len = full_record_len + 8192;
        while record0.len() < target_len {
            record0.push(0);
        }

        self.records[0] = record0;
        Ok(())
    }

    pub(super) fn build_exth(&self) -> Vec<u8> {
        let mut records: Vec<(u32, Vec<u8>)> = Vec::new();

        // Authors
        for author in &self.ctx.metadata.authors {
            records.push((100, author.as_bytes().to_vec()));
        }

        // Publisher
        if let Some(ref publisher) = self.ctx.metadata.publisher {
            records.push((101, publisher.as_bytes().to_vec()));
        }

        // Description
        if let Some(ref desc) = self.ctx.metadata.description {
            records.push((103, desc.as_bytes().to_vec()));
        }

        // Subjects
        for subject in &self.ctx.metadata.subjects {
            records.push((105, subject.as_bytes().to_vec()));
        }

        // Date
        if let Some(ref date) = self.ctx.metadata.date {
            records.push((106, date.as_bytes().to_vec()));
        }

        // Contributors (108).
        for contributor in &self.ctx.metadata.contributors {
            records.push((108, contributor.name.as_bytes().to_vec()));
        }

        // Rights
        if let Some(ref rights) = self.ctx.metadata.rights {
            records.push((109, rights.as_bytes().to_vec()));
        }

        // Source identifier (112). Kindle expects a calibre-style URN-ish
        // string here; if the book has an identifier use it, otherwise fall
        // back to a synthesized one. Calibre's own files always emit this.
        let source_id = if self.ctx.metadata.identifier.is_empty() {
            format!("boko:{}", self.ctx.metadata.title)
        } else {
            format!("calibre:{}", self.ctx.metadata.identifier)
        };
        records.push((112, source_id.into_bytes()));

        // Cover offset (201) — record-relative position of cover image.
        let mut cover_record_offset: Option<u32> = None;
        if let Some(ref cover_path) = self.ctx.metadata.cover_image
            && let Some(cover_idx) = self.image_hrefs.iter().position(|h| h == cover_path)
        {
            cover_record_offset = Some(cover_idx as u32);
            records.push((201, (cover_idx as u32).to_be_bytes().to_vec()));
            // hasfakecover = 0 (we have a real cover image)
            records.push((203, 0u32.to_be_bytes().to_vec()));
        }

        // Thumbnail offset (202) — Kindle uses the same image for thumbs if
        // no separate thumb is provided. Also emit the matching kindle:embed
        // URI (129) so the home-screen thumbnail works.
        if let Some(off) = cover_record_offset {
            records.push((202, off.to_be_bytes().to_vec()));
            // EXTH 129 wants the resource record index as a base32 string
            // The thumbnail URI in EXTH 129 must base32-encode the *same*
            // value as EXTH 202 (thumbnail_offset). Calibre:
            //   `kindle:embed:{to_base(thumbnail_offset, base=32, min_num_digits=4)}`
            // Earlier we had `off + 1` here, off-by-one against EXTH 202;
            // Kindle then spins trying to resolve a nonexistent resource for
            // the home-screen thumbnail.
            let mut buf = [0u8; 4];
            write_base32_4(off as usize, &mut buf);
            let uri = format!(
                "kindle:embed:{}",
                std::str::from_utf8(&buf).unwrap_or("0000")
            );
            records.push((129, uri.into_bytes()));
        }

        // Title (503)
        records.push((503, self.ctx.metadata.title.as_bytes().to_vec()));

        // ASIN placeholder (113)
        records.push((113, b"EBOK000000".to_vec()));

        // Document type (501)
        records.push((501, b"EBOK".to_vec()));

        // (EXTH 504 intentionally omitted — calibre does not emit it and
        // it confused some Kindle firmware versions in our testing.)

        // Language (524) — ISO 639-1 code. Kindle uses this to pick fonts and
        // hyphenation, and won't render correctly without it for non-default
        // languages. Strip any region suffix ("en-GB" → "en").
        if !self.ctx.metadata.language.is_empty() {
            let primary = self.ctx.metadata.language.split('-').next().unwrap_or("en");
            records.push((524, primary.as_bytes().to_vec()));
        }

        // KF8 housekeeping fields (calibre emits all of these on every book;
        // omitting them is correlated with Kindle refusing to open the file).
        //   125 = num_of_resources
        //   131 = kf8_unknown_count (always 0 in calibre)
        let num_resources = (self.image_hrefs.len() + self.font_hrefs.len()) as u32;
        records.push((125, num_resources.to_be_bytes().to_vec()));
        records.push((131, 0u32.to_be_bytes().to_vec()));

        // Creator software stamp (204–207) and kindlegen revision (535).
        // Pretend to be kindlegen 2 — matches what working KF8 files in the
        // wild advertise, and Kindle firmware uses these to route the file
        // through the KF8 reader rather than legacy MOBI.
        for (code, val) in [(204u32, 201u32), (205, 2), (206, 9), (207, 0)] {
            records.push((code, val.to_be_bytes().to_vec()));
        }
        records.push((535, b"0730-890adc2".to_vec()));

        // Override Kindle fonts (528) — KF8 flag telling Kindle to honour
        // embedded font @font-face rules.
        records.push((528, b"true".to_vec()));

        // Build EXTH
        let mut exth = Vec::new();
        exth.extend_from_slice(b"EXTH");

        let mut content = Vec::new();
        content.extend_from_slice(&(records.len() as u32).to_be_bytes());
        for (rec_type, data) in &records {
            let rec_len = 8 + data.len() as u32;
            content.extend_from_slice(&rec_type.to_be_bytes());
            content.extend_from_slice(&rec_len.to_be_bytes());
            content.extend_from_slice(data);
        }

        // Pad to 4-byte boundary
        while !content.len().is_multiple_of(4) {
            content.push(0);
        }

        let header_len = 12 + content.len() as u32;
        exth.extend_from_slice(&header_len.to_be_bytes());
        exth.extend_from_slice(&content);

        exth
    }

    pub(super) fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        // Calculate offsets
        let mut offsets = Vec::new();
        let pdb_header_size = 78 + 8 * self.records.len() + 2;
        let mut offset = pdb_header_size;

        for record in &self.records {
            offsets.push(offset as u32);
            offset += record.len();
        }

        // Write PDB header
        let title = sanitize_title(&self.ctx.metadata.title);
        let mut title_bytes = [0u8; 32];
        let title_slice = title.as_bytes();
        let copy_len = title_slice.len().min(31);
        title_bytes[..copy_len].copy_from_slice(&title_slice[..copy_len]);
        writer.write_all(&title_bytes)?;

        // Timestamps
        let now = crate::util::time_now_secs();
        writer.write_all(&0u16.to_be_bytes())?;
        writer.write_all(&0u16.to_be_bytes())?;
        writer.write_all(&now.to_be_bytes())?;
        writer.write_all(&now.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;

        // Type and Creator
        writer.write_all(b"BOOKMOBI")?;

        // UID seed, next record
        writer.write_all(&((2 * self.records.len() - 1) as u32).to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;

        // Number of records
        writer.write_all(&(self.records.len() as u16).to_be_bytes())?;

        // Record info list
        for (i, &offset) in offsets.iter().enumerate() {
            writer.write_all(&offset.to_be_bytes())?;
            let id_bytes = ((2 * i) as u32).to_be_bytes();
            writer.write_all(&[0, id_bytes[1], id_bytes[2], id_bytes[3]])?;
        }

        // Gap
        writer.write_all(&[0, 0])?;

        // Write records
        for record in &self.records {
            writer.write_all(record)?;
        }

        Ok(())
    }
}

/// Create a FONT record from raw font data.
pub(super) fn write_font_record(data: &[u8]) -> io::Result<Vec<u8>> {
    let usize_val = data.len() as u32;
    let mut flags: u32 = 0;

    // Compress
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(data)?;
    let mut compressed = encoder.finish()?;
    flags |= 0b01;

    // XOR obfuscation
    let mut xor_key = Vec::new();
    if compressed.len() >= 1040 {
        flags |= 0b10;

        let seed = crate::util::time_seed_nanos();
        xor_key = (0..XOR_KEY_LEN)
            .map(|i| {
                let mut x = seed.wrapping_add(i as u64);
                x = x.wrapping_mul(6364136223846793005);
                x = x.wrapping_add(1442695040888963407);
                (x >> 33) as u8
            })
            .collect();

        for i in 0..1040.min(compressed.len()) {
            compressed[i] ^= xor_key[i % XOR_KEY_LEN];
        }
    }

    let key_start: u32 = 24;
    let data_start: u32 = key_start + xor_key.len() as u32;

    let mut record = Vec::with_capacity(24 + xor_key.len() + compressed.len());

    // Header
    record.extend_from_slice(b"FONT");
    record.extend_from_slice(&usize_val.to_be_bytes());
    record.extend_from_slice(&flags.to_be_bytes());
    record.extend_from_slice(&data_start.to_be_bytes());
    record.extend_from_slice(&(xor_key.len() as u32).to_be_bytes());
    record.extend_from_slice(&key_start.to_be_bytes());

    record.extend_from_slice(&xor_key);
    record.extend_from_slice(&compressed);

    Ok(record)
}

pub(super) fn rand_uid() -> u32 {
    let seed = crate::util::time_seed_nanos() as u32;
    seed.wrapping_mul(1103515245).wrapping_add(12345)
}

pub(super) fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-')
        .collect::<String>()
        .replace(' ', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title() {
        assert_eq!(sanitize_title("Hello World"), "Hello_World");
        assert_eq!(sanitize_title("Test <Book>"), "Test_Book");
    }
}
