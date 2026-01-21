//! KFX book builder - orchestrates the conversion of a Book to KFX format.

use std::collections::HashMap;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::fs::File;

use crate::book::Book;
use crate::css::{ParsedStyle, Stylesheet};
use crate::kfx::ion::IonValue;

use super::content::{
    count_content_items, extract_content_from_xhtml, extract_css_hrefs_from_xhtml, ChapterData,
    ContentChunk, ContentItem, ListType, StyleRun,
};
use super::fragment::KfxFragment;
use super::navigation::{build_anchor_symbols, build_book_navigation, build_nav_unit_list};
use super::position::{
    build_anchor_eids, build_location_map, build_position_id_map, build_position_map,
    build_section_eids,
};
use super::resources::{
    build_resource_symbols, create_resource_fragments, get_image_dimensions,
    populate_image_dimensions,
};
use super::serialization::{
    create_entity_data, create_raw_media_data, generate_container_id, serialize_annotated_ion,
    serialize_container_v2, SerializedEntity,
};
use super::style::style_to_ion;

/// State for tracking text indexing across content chunks
struct ContentState {
    global_idx: usize,
    text_idx_in_chunk: i64,
    current_content_sym: u64,
}

/// Normalize text for KFX output based on verse context.
/// - Verse: split by newlines to create separate paragraph entries
/// - Non-verse: keep text as-is (preserve newlines for proper inline run offset alignment)
fn normalize_text_for_kfx(text: &str, is_verse: bool) -> Vec<String> {
    if is_verse {
        text.split('\n')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        // Keep text as-is, including newlines - this preserves inline run offsets
        // The Kindle reader interprets \n as line breaks within the paragraph
        if text.trim().is_empty() {
            vec![]
        } else {
            vec![text.to_string()]
        }
    }
}
use super::symbols::{sym, SymbolTable};

/// Builder for creating a complete KFX book
pub struct KfxBookBuilder {
    pub(crate) symtab: SymbolTable,
    pub(crate) fragments: Vec<KfxFragment>,
    pub(crate) container_id: String,
    /// Map from parsed style to style symbol
    pub(crate) style_map: HashMap<ParsedStyle, u64>,
    /// Map from resource href to resource symbol (for image references)
    pub(crate) resource_symbols: HashMap<String, u64>,
    /// Map from resource symbol to raw media symbol (for P253 entity dependencies)
    pub(crate) resource_to_media: Vec<(u64, u64)>,
    /// Map from section symbol to resource symbol (for P253 entity dependencies)
    pub(crate) section_to_resource: Vec<(u64, u64)>,
    /// Map from anchor href (URL or internal path) to anchor fragment symbol
    pub(crate) anchor_symbols: HashMap<String, u64>,
    /// Map from XHTML path to section EID (for internal link targets)
    pub(crate) section_eids: HashMap<String, i64>,
    /// Map from full anchor href (path#fragment) to (EID, offset) for TOC navigation
    pub(crate) anchor_eids: HashMap<String, (i64, i64)>,
}

impl KfxBookBuilder {
    pub fn new() -> Self {
        Self {
            symtab: SymbolTable::new(),
            fragments: Vec::new(),
            container_id: generate_container_id(),
            style_map: HashMap::new(),
            resource_symbols: HashMap::new(),
            resource_to_media: Vec::new(),
            section_to_resource: Vec::new(),
            anchor_symbols: HashMap::new(),
            section_eids: HashMap::new(),
            anchor_eids: HashMap::new(),
        }
    }

    /// Build a KFX book from a Book structure
    pub fn from_book(book: &Book) -> Self {
        let mut builder = Self::new();

        // Build resource symbol mapping
        builder.resource_symbols = build_resource_symbols(book, &mut builder.symtab);

        // Build TOC title map
        let toc_titles: HashMap<&str, &str> = book
            .toc
            .iter()
            .map(|entry| (entry.href.as_str(), entry.title.as_str()))
            .collect();

        // Extract content from spine
        let mut chapters = builder.extract_chapters(book, &toc_titles);

        // Populate image dimensions
        for chapter in &mut chapters {
            for content_item in &mut chapter.content {
                populate_image_dimensions(content_item, &book.resources);
            }
        }

        let has_cover = book.metadata.cover_image.is_some();

        // Build EID mappings
        builder.section_eids = build_section_eids(&chapters, has_cover);
        builder.anchor_eids = build_anchor_eids(&chapters, has_cover);

        // Add fragments
        builder.add_format_capabilities();
        builder.add_metadata(book);
        builder.add_reading_order_metadata(&chapters, has_cover);
        builder.add_document_data(&chapters, has_cover);

        let first_content_eid = chapters
            .first()
            .and_then(|ch| builder.section_eids.get(&ch.source_path).copied());
        builder.add_book_navigation_fragment(&book.toc, has_cover, first_content_eid);
        builder.add_nav_unit_list();

        // Add styles
        builder.add_all_styles(&chapters);

        // Process chunks
        let all_chunks = builder.create_chunks(&chapters);

        for (_, chunk) in &all_chunks {
            builder.add_text_content_chunk(chunk);
        }

        builder.anchor_symbols = build_anchor_symbols(&chapters, &mut builder.symtab);

        // Add cover section if present
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if let Some(cover_href) = &book.metadata.cover_image {
            if let Some(cover_sym) = builder.resource_symbols.get(cover_href).copied() {
                let (cover_width, cover_height) = book
                    .resources
                    .get(cover_href)
                    .and_then(|r| get_image_dimensions(&r.data))
                    .unwrap_or((1400, 2100));

                builder.add_cover_section(cover_sym, cover_width, cover_height, eid_base);
                eid_base += 2;
            }
        }

        // Add content fragments for each chapter
        for (chapter_idx, chapter) in chapters.iter().enumerate() {
            let chapter_chunks: Vec<&ContentChunk> = all_chunks
                .iter()
                .filter(|(idx, _)| *idx == chapter_idx)
                .map(|(_, chunk)| chunk)
                .collect();

            builder.add_content_block_chunked(chapter, &chapter_chunks, eid_base);
            builder.add_section(chapter, eid_base);
            builder.add_auxiliary_data(chapter);

            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
        }

        // Add position/location maps
        builder.add_position_map(&chapters, has_cover);
        builder.add_position_id_map(&chapters, has_cover);
        builder.add_location_map(&chapters, has_cover);

        // Add page templates
        builder.add_page_templates(&chapters, has_cover);

        // Add anchor fragments
        builder.add_anchor_fragments();

        // Add resources
        let (resource_fragments, resource_to_media) =
            create_resource_fragments(book, &mut builder.symtab, &builder.resource_symbols);
        builder.fragments.extend(resource_fragments);
        builder.resource_to_media = resource_to_media;

        // Add container entity map
        builder.add_container_entity_map();

        // Add header fragments
        builder.add_container_info_fragment();
        builder.add_symbol_table_fragment();

        builder
    }

    fn extract_chapters<'a>(
        &mut self,
        book: &Book,
        toc_titles: &HashMap<&'a str, &'a str>,
    ) -> Vec<ChapterData> {
        let mut chapters = Vec::new();
        let mut chapter_num = 1;

        for (idx, spine_item) in book.spine.iter().enumerate() {
            let resource = match book.resources.get(&spine_item.href) {
                Some(r) => r,
                None => continue,
            };

            let css_hrefs = extract_css_hrefs_from_xhtml(&resource.data, &spine_item.href);

            let mut combined_css = String::new();
            for css_href in &css_hrefs {
                if let Some(css_resource) = book.resources.get(css_href) {
                    combined_css.push_str(&String::from_utf8_lossy(&css_resource.data));
                    combined_css.push('\n');
                }
            }

            let stylesheet = Stylesheet::parse_with_defaults(&combined_css);

            let content =
                extract_content_from_xhtml(&resource.data, &stylesheet, &spine_item.href);

            if content.is_empty() {
                continue;
            }

            let title = toc_titles
                .get(spine_item.href.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    content
                        .iter()
                        .flat_map(|item| item.flatten())
                        .find_map(|item| {
                            if let ContentItem::Text { text, .. } = item
                                && text.len() < 100
                                && !text.contains('.')
                            {
                                return Some(text.clone());
                            }
                            None
                        })
                })
                .unwrap_or_else(|| format!("Chapter {chapter_num}"));

            chapters.push(ChapterData {
                id: format!("chapter-{idx}"),
                title,
                content,
                source_path: spine_item.href.clone(),
            });
            chapter_num += 1;
        }

        chapters
    }

    fn create_chunks(&self, chapters: &[ChapterData]) -> Vec<(usize, ContentChunk)> {
        let mut all_chunks = Vec::new();
        for (chapter_idx, chapter) in chapters.iter().enumerate() {
            let chapter_clone = ChapterData {
                id: chapter.id.clone(),
                title: chapter.title.clone(),
                content: chapter.content.clone(),
                source_path: chapter.source_path.clone(),
            };
            for chunk in chapter_clone.into_chunks() {
                all_chunks.push((chapter_idx, chunk));
            }
        }
        all_chunks
    }

    fn add_container_info_fragment(&mut self) {
        let mut info = HashMap::new();
        info.insert(
            sym::CONTAINER_ID,
            IonValue::String(self.container_id.clone()),
        );
        info.insert(sym::CHUNK_SIZE, IonValue::Int(4096));
        info.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
        info.insert(sym::DRM_SCHEME, IonValue::Int(0));
        info.insert(
            sym::MIN_VERSION,
            IonValue::String(format!("boko-{}", env!("CARGO_PKG_VERSION"))),
        );
        info.insert(
            sym::VERSION,
            IonValue::String(env!("CARGO_PKG_VERSION").to_string()),
        );
        info.insert(sym::FORMAT, IonValue::String("KFX main".to_string()));

        self.fragments.push(KfxFragment::singleton(
            sym::CONTAINER_INFO,
            IonValue::Struct(info),
        ));
    }

    fn add_symbol_table_fragment(&mut self) {
        let symtab_value = self.symtab.create_import();
        self.fragments.push(KfxFragment::new(
            3,
            "$ion_symbol_table",
            symtab_value,
        ));
    }

    fn add_format_capabilities(&mut self) {
        let capabilities = [
            ("com.amazon.yjconversion", "reflow-style", 6, 0),
            ("SDK.Marker", "CanonicalFormat", 1, 0),
            ("com.amazon.yjconversion", "yj_hdv", 1, 0),
        ];

        let caps_list: Vec<IonValue> = capabilities
            .iter()
            .map(|(provider, feature, min_version, version)| {
                let mut ver_struct = HashMap::new();
                ver_struct.insert(sym::MIN_VERSION, IonValue::Int(*min_version));
                ver_struct.insert(sym::VERSION, IonValue::Int(*version));

                let mut ver_wrapper = HashMap::new();
                ver_wrapper.insert(5, IonValue::Struct(ver_struct));

                let mut cap = HashMap::new();
                cap.insert(sym::CAPABILITY_NAME, IonValue::String(provider.to_string()));
                cap.insert(sym::METADATA_KEY, IonValue::String(feature.to_string()));
                cap.insert(sym::CAPABILITY_VERSION, IonValue::Struct(ver_wrapper));
                IonValue::Struct(cap)
            })
            .collect();

        let mut caps_struct = HashMap::new();
        caps_struct.insert(sym::CAPABILITIES_LIST, IonValue::List(caps_list));

        self.fragments.push(KfxFragment::singleton(
            sym::FORMAT_CAPABILITIES_OLD,
            IonValue::Struct(caps_struct),
        ));
    }

    fn add_metadata(&mut self, book: &Book) {
        let mut all_groups = Vec::new();

        // kindle_audit_metadata
        {
            let mut entries = Vec::new();
            let mut add_entry = |key: &str, value: IonValue| {
                let mut entry = HashMap::new();
                entry.insert(sym::METADATA_KEY, IonValue::String(key.to_string()));
                entry.insert(sym::VALUE, value);
                entries.push(IonValue::Struct(entry));
            };

            add_entry("file_creator", IonValue::String("boko".to_string()));
            add_entry(
                "creator_version",
                IonValue::String(env!("CARGO_PKG_VERSION").to_string()),
            );

            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_audit_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(entries));
            all_groups.push(IonValue::Struct(group));
        }

        // kindle_title_metadata
        {
            let mut entries = Vec::new();
            let mut add_entry = |key: &str, value: IonValue| {
                let mut entry = HashMap::new();
                entry.insert(sym::METADATA_KEY, IonValue::String(key.to_string()));
                entry.insert(sym::VALUE, value);
                entries.push(IonValue::Struct(entry));
            };

            add_entry("title", IonValue::String(book.metadata.title.clone()));

            for author in &book.metadata.authors {
                add_entry("author", IonValue::String(author.clone()));
            }

            if !book.metadata.language.is_empty() {
                add_entry("language", IonValue::String(book.metadata.language.clone()));
            }

            if let Some(ref publisher) = book.metadata.publisher {
                add_entry("publisher", IonValue::String(publisher.clone()));
            }

            if let Some(ref description) = book.metadata.description {
                add_entry("description", IonValue::String(description.clone()));
            }

            let asin = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                book.metadata.title.hash(&mut h);
                book.metadata.authors.hash(&mut h);
                book.metadata.identifier.hash(&mut h);
                format!("{:032X}", h.finish())
            };
            add_entry("ASIN", IonValue::String(asin.clone()));
            add_entry("content_id", IonValue::String(asin));
            add_entry("cde_content_type", IonValue::String("EBOK".to_string()));

            if let Some(cover_href) = &book.metadata.cover_image {
                if let Some(&cover_sym) = self.resource_symbols.get(cover_href) {
                    add_entry("cover_image", IonValue::Symbol(cover_sym));
                }
            }

            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_title_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(entries));
            all_groups.push(IonValue::Struct(group));
        }

        // kindle_ebook_metadata
        {
            let mut entries = Vec::new();
            let mut add_entry = |key: &str, value: IonValue| {
                let mut entry = HashMap::new();
                entry.insert(sym::METADATA_KEY, IonValue::String(key.to_string()));
                entry.insert(sym::VALUE, value);
                entries.push(IonValue::Struct(entry));
            };

            add_entry("selection", IonValue::String("enabled".to_string()));
            add_entry("nested_span", IonValue::String("enabled".to_string()));

            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_ebook_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(entries));
            all_groups.push(IonValue::Struct(group));
        }

        // kindle_capability_metadata
        {
            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_capability_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(Vec::new()));
            all_groups.push(IonValue::Struct(group));
        }

        let mut root = HashMap::new();
        root.insert(sym::METADATA_ENTRIES, IonValue::List(all_groups));

        self.fragments.push(KfxFragment::singleton(
            sym::KINDLE_METADATA,
            IonValue::Struct(root),
        ));
    }

    fn add_reading_order_metadata(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut section_refs: Vec<IonValue> = Vec::new();

        if has_cover {
            let cover_section_sym = self.symtab.get_or_intern("cover-section");
            section_refs.push(IonValue::Symbol(cover_section_sym));
        }

        for ch in chapters {
            let section_id = format!("section-{}", ch.id);
            let sym_id = self.symtab.get_or_intern(&section_id);
            section_refs.push(IonValue::Symbol(sym_id));
        }

        let mut reading_order = HashMap::new();
        reading_order.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        reading_order.insert(sym::SECTIONS_LIST, IonValue::List(section_refs));

        let mut metadata_258 = HashMap::new();
        metadata_258.insert(
            sym::READING_ORDERS,
            IonValue::List(vec![IonValue::Struct(reading_order)]),
        );

        self.fragments.push(KfxFragment::singleton(
            sym::METADATA,
            IonValue::Struct(metadata_258),
        ));
    }

    fn add_document_data(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut section_refs: Vec<IonValue> = Vec::new();

        if has_cover {
            let cover_section_sym = self.symtab.get_or_intern("cover-section");
            section_refs.push(IonValue::Symbol(cover_section_sym));
        }

        for ch in chapters {
            let section_id = format!("section-{}", ch.id);
            let sym_id = self.symtab.get_or_intern(&section_id);
            section_refs.push(IonValue::Symbol(sym_id));
        }

        let mut reading_order = HashMap::new();
        reading_order.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        reading_order.insert(sym::SECTIONS_LIST, IonValue::List(section_refs));

        let total_items: usize = chapters.iter().map(|ch| ch.content.len()).sum();

        let typed_null = || {
            let mut s = HashMap::new();
            s.insert(307, IonValue::Decimal(vec![0x80, 0x01]));
            s.insert(306, IonValue::Symbol(308));
            IonValue::Struct(s)
        };

        let mut doc_data = HashMap::new();
        doc_data.insert(
            sym::READING_ORDERS,
            IonValue::List(vec![IonValue::Struct(reading_order)]),
        );
        doc_data.insert(8, IonValue::Int(total_items as i64));
        doc_data.insert(16, typed_null());
        doc_data.insert(42, typed_null());
        doc_data.insert(112, IonValue::Symbol(383));
        doc_data.insert(192, IonValue::Symbol(376));
        doc_data.insert(436, IonValue::Symbol(441));
        doc_data.insert(477, IonValue::Symbol(56));
        doc_data.insert(560, IonValue::Symbol(557));

        self.fragments.push(KfxFragment::singleton(
            sym::DOCUMENT_DATA,
            IonValue::Struct(doc_data),
        ));
    }

    fn add_book_navigation_fragment(
        &mut self,
        toc: &[crate::book::TocEntry],
        has_cover: bool,
        first_content_eid: Option<i64>,
    ) {
        let nav_value = build_book_navigation(
            toc,
            &self.section_eids,
            &self.anchor_eids,
            &mut self.symtab,
            has_cover,
            first_content_eid,
        );

        self.fragments.push(KfxFragment::singleton(
            sym::BOOK_NAVIGATION,
            nav_value,
        ));
    }

    fn add_nav_unit_list(&mut self) {
        self.fragments.push(KfxFragment::singleton(
            sym::NAV_UNIT_LIST,
            build_nav_unit_list(),
        ));
    }

    fn add_all_styles(&mut self, chapters: &[ChapterData]) {
        fn collect_styles(item: &ContentItem, styles: &mut std::collections::HashSet<ParsedStyle>) {
            styles.insert(item.style().clone());

            match item {
                ContentItem::Container { children, .. } => {
                    for child in children {
                        collect_styles(child, styles);
                    }
                }
                ContentItem::Text { inline_runs, .. } => {
                    for run in inline_runs {
                        styles.insert(run.style.clone());
                    }
                }
                ContentItem::Image { .. } => {}
            }
        }

        let mut unique_styles = std::collections::HashSet::new();
        for chapter in chapters {
            for item in &chapter.content {
                collect_styles(item, &mut unique_styles);
            }
        }

        for (i, style) in unique_styles.into_iter().enumerate() {
            let style_id = format!("style-{i}");
            let style_sym = self.symtab.get_or_intern(&style_id);

            let style_ion = style_to_ion(&style, style_sym, &mut self.symtab);

            self.fragments.push(KfxFragment::new(sym::STYLE, &style_id, style_ion));
            self.style_map.insert(style, style_sym);
        }
    }

    /// Add a text content chunk fragment.
    ///
    /// Note: We use CONTENT_ARRAY ($146) with a list of strings rather than TEXT ($244)
    /// with a single concatenated string. The KFX reader expects $146 format to extract
    /// text content for the spine (see `extract_text_content` in reader.rs). Reference
    /// KFX files from Kindle Previewer also use the $146 list format.
    fn add_text_content_chunk(&mut self, chunk: &ContentChunk) {
        let content_id = format!("content-{}", chunk.id);
        let content_sym = self.symtab.get_or_intern(&content_id);

        // Use flatten() to extract text from nested containers
        // Normalize based on verse context (see normalize_text_for_kfx)
        let text_values: Vec<IonValue> = chunk
            .items
            .iter()
            .flat_map(|item| item.flatten())
            .flat_map(|item| {
                if let ContentItem::Text { text, is_verse, .. } = item {
                    normalize_text_for_kfx(text, *is_verse)
                        .into_iter()
                        .map(IonValue::String)
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect();

        // Don't create an empty text content fragment
        if text_values.is_empty() {
            return;
        }

        let mut content = HashMap::new();
        content.insert(sym::ID, IonValue::Symbol(content_sym));
        content.insert(sym::CONTENT_ARRAY, IonValue::List(text_values));

        self.fragments.push(KfxFragment::new(
            sym::TEXT_CONTENT,
            &content_id,
            IonValue::Struct(content),
        ));
    }

    fn add_cover_section(&mut self, cover_sym: u64, width: u32, height: u32, eid_base: i64) {
        let cover_block_id = "cover-block";
        let cover_block_sym = self.symtab.get_or_intern(cover_block_id);
        let cover_section_id = "cover-section";
        let cover_section_sym = self.symtab.get_or_intern(cover_section_id);
        let cover_style_id = "cover-style";
        let cover_style_sym = self.symtab.get_or_intern(cover_style_id);

        // Create cover image style
        let mut cover_style = HashMap::new();
        cover_style.insert(sym::STYLE_NAME, IonValue::Symbol(cover_style_sym));
        cover_style.insert(sym::IMAGE_FIT, IonValue::Symbol(sym::IMAGE_FIT_CONTAIN));
        cover_style.insert(sym::IMAGE_LAYOUT, IonValue::Symbol(sym::ALIGN_CENTER));

        self.fragments.push(KfxFragment::new(
            sym::STYLE,
            cover_style_id,
            IonValue::Struct(cover_style),
        ));

        // Create content block with the cover image
        let mut image_item = HashMap::new();
        image_item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::IMAGE_CONTENT));
        image_item.insert(sym::RESOURCE_NAME, IonValue::Symbol(cover_sym));
        image_item.insert(sym::STYLE_NAME, IonValue::Symbol(cover_style_sym));

        let mut block = HashMap::new();
        block.insert(sym::CONTENT_NAME, IonValue::Symbol(cover_block_sym));
        block.insert(
            sym::CONTENT_ARRAY,
            IonValue::List(vec![IonValue::Struct(image_item)]),
        );

        self.fragments.push(KfxFragment::new(
            sym::CONTENT_BLOCK,
            cover_block_id,
            IonValue::Struct(block),
        ));

        // Create section referencing the cover content block
        let mut section = HashMap::new();
        section.insert(sym::SECTION_NAME, IonValue::Symbol(cover_section_sym));
        section.insert(sym::PAGE_LAYOUT, IonValue::Symbol(sym::LAYOUT_FULL_PAGE));
        section.insert(sym::SECTION_WIDTH, IonValue::Int(width as i64));
        section.insert(sym::SECTION_HEIGHT, IonValue::Int(height as i64));
        section.insert(
            sym::SECTION_CONTENT,
            IonValue::List(vec![IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid_base)),
                (sym::CONTENT_NAME, IonValue::Symbol(cover_block_sym)),
            ])]),
        );

        self.fragments.push(KfxFragment::new(
            sym::SECTION,
            cover_section_id,
            IonValue::Struct(section),
        ));

        // Track section -> resource dependency for P253
        self.section_to_resource.push((cover_section_sym, cover_sym));
    }

    fn add_content_block_chunked(
        &mut self,
        chapter: &ChapterData,
        chunks: &[&ContentChunk],
        eid_base: i64,
    ) {
        let block_id = format!("block-{}", chapter.id);
        let block_sym = self.symtab.get_or_intern(&block_id);

        // Create content items referencing text content chunks or images
        let mut content_items = Vec::new();
        let mut state = ContentState {
            global_idx: 0,
            text_idx_in_chunk: 0,
            current_content_sym: 0,
        };

        for chunk in chunks.iter() {
            let content_id = format!("content-{}", chunk.id);
            state.current_content_sym = self.symtab.get_or_intern(&content_id);
            state.text_idx_in_chunk = 0;

            for content_item in chunk.items.iter() {
                let ion_items =
                    self.build_content_items(content_item, &mut state, eid_base);
                content_items.extend(ion_items);
            }
        }

        let mut block = HashMap::new();
        block.insert(sym::CONTENT_NAME, IonValue::Symbol(block_sym));
        block.insert(sym::CONTENT_ARRAY, IonValue::List(content_items));

        self.fragments.push(KfxFragment::new(
            sym::CONTENT_BLOCK,
            &block_id,
            IonValue::Struct(block),
        ));
    }

    /// Build content items (Text, Image, or Container) with proper TEXT_CONTENT references
    /// Returns Vec because Text items with newlines become multiple paragraphs
    fn build_content_items(
        &mut self,
        content_item: &ContentItem,
        state: &mut ContentState,
        eid_base: i64,
    ) -> Vec<IonValue> {
        match content_item {
            ContentItem::Text {
                text,
                style,
                inline_runs,
                is_verse,
                ..
            } => {
                // Normalize text based on verse context (see normalize_text_for_kfx)
                // IMPORTANT: Don't collapse whitespace - inline run offsets depend on
                // character positions matching the original text (with \n -> space being 1:1)
                let lines = normalize_text_for_kfx(text, *is_verse);

                let mut items = Vec::new();

                for (i, _line) in lines.iter().enumerate() {
                    let mut item = HashMap::new();

                    // Text content: reference the text chunk
                    let mut text_ref = HashMap::new();
                    text_ref.insert(sym::ID, IonValue::Symbol(state.current_content_sym));
                    text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(state.text_idx_in_chunk));

                    item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));
                    item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));

                    // Add base style reference
                    if let Some(style_sym) = self.style_map.get(style).copied() {
                        if style_sym != 0 {
                            item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                        }
                    }

                    // Add inline style runs ($142) only for the first line
                    // (inline runs reference character offsets in the original combined text)
                    let has_inline_runs = if i == 0 && !inline_runs.is_empty() {
                        let runs = self.build_inline_runs(inline_runs);
                        if !runs.is_empty() {
                            item.insert(sym::INLINE_STYLE_RUNS, IonValue::List(runs));
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    // Add content role indicator ($790)
                    // Only on items WITHOUT inline style runs
                    // First item in content block gets 2, normal paragraphs get 3
                    if !has_inline_runs {
                        let role = if state.global_idx == 0 { 2 } else { 3 };
                        item.insert(sym::CONTENT_ROLE, IonValue::Int(role));
                    }

                    // Use consistent EID that matches position maps
                    item.insert(
                        sym::POSITION,
                        IonValue::Int(eid_base + 1 + state.global_idx as i64),
                    );

                    state.text_idx_in_chunk += 1;
                    state.global_idx += 1;
                    items.push(IonValue::Struct(item));
                }

                items
            }
            ContentItem::Image {
                resource_href,
                style,
                alt,
            } => {
                let mut item = HashMap::new();

                // Image content: reference the resource directly
                let resource_sym = self
                    .resource_symbols
                    .get(resource_href)
                    .copied()
                    .unwrap_or(0);

                item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::IMAGE_CONTENT));
                item.insert(sym::RESOURCE_NAME, IonValue::Symbol(resource_sym));

                // $584 = IMAGE_ALT_TEXT for accessibility
                let alt_text = alt.clone().unwrap_or_default();
                item.insert(sym::IMAGE_ALT_TEXT, IonValue::String(alt_text));

                // Add style reference if present
                if let Some(style_sym) = self.style_map.get(style).copied() {
                    if style_sym != 0 {
                        item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                    }
                }

                // Use consistent EID that matches position maps
                item.insert(
                    sym::POSITION,
                    IonValue::Int(eid_base + 1 + state.global_idx as i64),
                );
                state.global_idx += 1;

                vec![IonValue::Struct(item)]
            }
            ContentItem::Container {
                style,
                children,
                list_type,
                tag,
                ..
            } => {
                let mut item = HashMap::new();

                // Determine content type based on container type
                let content_type = if list_type.is_some() {
                    // ol/ul list container uses $276 (CONTENT_LIST)
                    sym::CONTENT_LIST
                } else if tag == "li" {
                    // li list item uses $277 (CONTENT_LIST_ITEM)
                    sym::CONTENT_LIST_ITEM
                } else {
                    // Regular container uses $269 (CONTENT_PARAGRAPH)
                    sym::CONTENT_PARAGRAPH
                };
                item.insert(sym::CONTENT_TYPE, IonValue::Symbol(content_type));
                // Note: Containers do NOT get $790 - only leaf text items do

                // Add list type property for ol/ul containers
                if let Some(lt) = list_type {
                    let list_type_sym = match lt {
                        ListType::Ordered => sym::LIST_TYPE_DECIMAL,
                        ListType::Unordered => sym::LIST_TYPE_DECIMAL, // TODO: find correct symbol for bullet list
                    };
                    item.insert(sym::LIST_TYPE, IonValue::Symbol(list_type_sym));
                }

                // For list items (li), directly reference text content with $145
                // instead of creating nested $146 array
                if tag == "li" {
                    // Build list item with direct text reference
                    self.build_list_item(&mut item, children, state, style, eid_base);
                } else {
                    // Build nested content array for regular containers
                    let nested_items: Vec<IonValue> = children
                        .iter()
                        .flat_map(|child| self.build_content_items(child, state, eid_base))
                        .collect();

                    item.insert(sym::CONTENT_ARRAY, IonValue::List(nested_items));

                    // Add style reference for the container
                    if let Some(style_sym) = self.style_map.get(style).copied() {
                        if style_sym != 0 {
                            item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                        }
                    }

                    // Use consistent EID that matches position maps
                    item.insert(
                        sym::POSITION,
                        IonValue::Int(eid_base + 1 + state.global_idx as i64),
                    );
                    state.global_idx += 1;
                }

                vec![IonValue::Struct(item)]
            }
        }
    }

    /// Build a list item (li) with direct $145 text reference
    /// List items in KFX directly reference their text content, not nested containers
    fn build_list_item(
        &mut self,
        item: &mut HashMap<u64, IonValue>,
        children: &[ContentItem],
        state: &mut ContentState,
        style: &ParsedStyle,
        eid_base: i64,
    ) {
        // Extract text from the list item's children
        // List items typically have a single Text child or nested inline elements
        // We flatten to get all text content
        let text_content: Vec<&ContentItem> = children
            .iter()
            .flat_map(|c| c.flatten())
            .filter(|c| matches!(c, ContentItem::Text { .. }))
            .collect();

        // Create direct text reference ($145) for the first text item
        // This matches reference KFX where list items directly contain text ref
        if !text_content.is_empty() {
            let mut text_ref = HashMap::new();
            text_ref.insert(sym::ID, IonValue::Symbol(state.current_content_sym));
            text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(state.text_idx_in_chunk));
            item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));

            // Increment text index for each text item in the list item
            for _ in &text_content {
                state.text_idx_in_chunk += 1;
            }
        }

        // Add style reference for the list item
        if let Some(style_sym) = self.style_map.get(style).copied() {
            if style_sym != 0 {
                item.insert(sym::STYLE, IonValue::Symbol(style_sym));
            }
        }

        // Use consistent EID that matches position maps
        item.insert(
            sym::POSITION,
            IonValue::Int(eid_base + 1 + state.global_idx as i64),
        );
        state.global_idx += 1;
    }

    #[allow(dead_code)]
    fn content_item_to_ion_deprecated(
        &mut self,
        item: &ContentItem,
        eid: i64,
        is_first: bool,
    ) -> (Vec<IonValue>, i64) {
        match item {
            ContentItem::Text {
                text,
                style,
                inline_runs,
                ..
            } => {
                let style_sym = self.style_map.get(style).copied().unwrap_or(0);

                // Split text by newlines (from BR tags) to create separate paragraphs
                let lines: Vec<&str> = text.split('\n').collect();
                let mut all_items = Vec::new();
                let mut current_eid = eid;
                let mut is_first_para = is_first;

                for line in lines {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let mut para = HashMap::new();
                    para.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));
                    para.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));
                    para.insert(sym::TEXT_OFFSET, IonValue::Int(0));
                    para.insert(sym::COUNT, IonValue::Int(trimmed.chars().count() as i64));
                    para.insert(sym::POSITION, IonValue::Int(current_eid));

                    let role = if is_first_para { 2 } else { 3 };
                    para.insert(sym::CONTENT_ROLE, IonValue::Int(role));

                    // Only apply inline runs to first paragraph (they're relative to original text)
                    // Note: This is a simplification; proper inline run handling would need adjustment
                    if is_first_para && !inline_runs.is_empty() {
                        let runs = self.build_inline_runs(inline_runs);
                        para.insert(sym::INLINE_STYLE_RUNS, IonValue::List(runs));
                    }

                    let annotated = IonValue::Annotated(
                        vec![sym::CONTENT_PARAGRAPH],
                        Box::new(IonValue::Struct(para)),
                    );

                    all_items.push(annotated);
                    current_eid += 1;
                    is_first_para = false;
                }

                // If no lines were produced, still increment eid to maintain consistency
                if all_items.is_empty() {
                    (all_items, eid)
                } else {
                    (all_items, current_eid)
                }
            }
            ContentItem::Image {
                resource_href,
                style,
                alt,
            } => {
                let style_sym = self.style_map.get(style).copied().unwrap_or(0);
                let resource_sym = self.resource_symbols.get(resource_href).copied().unwrap_or(0);

                let mut img = HashMap::new();
                img.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::IMAGE_CONTENT));
                img.insert(sym::RESOURCE_NAME, IonValue::Symbol(resource_sym));
                img.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));
                img.insert(sym::POSITION, IonValue::Int(eid));

                // CONTENT_ROLE: 2=first, 3=normal
                let role = if is_first { 2 } else { 3 };
                img.insert(sym::CONTENT_ROLE, IonValue::Int(role));

                if let Some(alt_text) = alt {
                    img.insert(sym::IMAGE_ALT_TEXT, IonValue::String(alt_text.clone()));
                }

                let annotated = IonValue::Annotated(
                    vec![sym::CONTENT_PARAGRAPH],
                    Box::new(IonValue::Struct(img)),
                );

                (vec![annotated], eid + 1)
            }
            ContentItem::Container {
                children, style, ..
            } => {
                let style_sym = self.style_map.get(style).copied().unwrap_or(0);

                let mut all_items = Vec::new();
                let mut current_eid = eid;
                let mut is_first_child = is_first;

                for child in children {
                    let (child_items, new_eid) =
                        self.content_item_to_ion_deprecated(child, current_eid, is_first_child);
                    all_items.extend(child_items);
                    current_eid = new_eid;
                    is_first_child = false;
                }

                let container_eid = current_eid;

                let mut container = HashMap::new();
                container.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::BLOCK_TYPE_BLOCK));
                container.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));
                container.insert(sym::POSITION, IonValue::Int(container_eid));

                // CONTENT_ROLE: 2=first, 3=normal (containers use the same role as their first child)
                let role = if is_first { 2 } else { 3 };
                container.insert(sym::CONTENT_ROLE, IonValue::Int(role));

                let first_child_eid = eid;
                let last_child_eid = container_eid - 1;
                container.insert(
                    sym::CONTENT_ARRAY,
                    IonValue::List(vec![
                        IonValue::Int(first_child_eid),
                        IonValue::Int(last_child_eid),
                    ]),
                );

                let annotated = IonValue::Annotated(
                    vec![sym::CONTENT_PARAGRAPH],
                    Box::new(IonValue::Struct(container)),
                );

                all_items.push(annotated);

                (all_items, container_eid + 1)
            }
        }
    }

    fn build_inline_runs(&mut self, runs: &[StyleRun]) -> Vec<IonValue> {
        runs.iter()
            .filter_map(|run| {
                // Get style symbol (required for run)
                let style_sym = self.style_map.get(&run.style).copied()?;

                let mut run_struct = HashMap::new();
                run_struct.insert(sym::OFFSET, IonValue::Int(run.offset as i64));
                run_struct.insert(sym::COUNT, IonValue::Int(run.length as i64));
                run_struct.insert(sym::STYLE, IonValue::Symbol(style_sym));

                // Add anchor reference ($179) if this run has a hyperlink
                if let Some(ref href) = run.anchor_href {
                    if let Some(&anchor_sym) = self.anchor_symbols.get(href) {
                        run_struct.insert(sym::ANCHOR_REF, IonValue::Symbol(anchor_sym));
                    }
                }

                Some(IonValue::Struct(run_struct))
            })
            .collect()
    }

    fn add_section(&mut self, chapter: &ChapterData, eid_base: i64) {
        let section_id = format!("section-{}", chapter.id);
        let section_sym = self.symtab.get_or_intern(&section_id);

        let block_id = format!("block-{}", chapter.id);
        let block_sym = self.symtab.get_or_intern(&block_id);

        let mut section = HashMap::new();
        section.insert(sym::SECTION_NAME, IonValue::Symbol(section_sym));

        section.insert(
            sym::SECTION_CONTENT,
            IonValue::List(vec![IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid_base)),
                (sym::CONTENT_NAME, IonValue::Symbol(block_sym)),
            ])]),
        );

        self.fragments.push(KfxFragment::new(
            sym::SECTION,
            &section_id,
            IonValue::Struct(section),
        ));
    }

    fn add_auxiliary_data(&mut self, chapter: &ChapterData) {
        let aux_id = format!("aux-{}", chapter.id);
        let aux_sym = self.symtab.get_or_intern(&aux_id);

        let mut aux = HashMap::new();
        aux.insert(sym::AUX_DATA_REF, IonValue::Symbol(aux_sym));
        aux.insert(sym::DESCRIPTION, IonValue::String(chapter.title.clone()));

        self.fragments.push(KfxFragment::new(
            sym::AUXILIARY_DATA,
            &aux_id,
            IonValue::Struct(aux),
        ));
    }

    fn add_position_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let position_map = build_position_map(chapters, &mut self.symtab, has_cover);
        self.fragments.push(KfxFragment::singleton(
            sym::POSITION_MAP,
            position_map,
        ));
    }

    fn add_position_id_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let position_id_map = build_position_id_map(chapters, has_cover);
        self.fragments.push(KfxFragment::singleton(
            sym::POSITION_ID_MAP,
            position_id_map,
        ));
    }

    fn add_location_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let location_map = build_location_map(chapters, has_cover);
        self.fragments.push(KfxFragment::singleton(
            sym::LOCATION_MAP,
            location_map,
        ));
    }

    fn add_page_templates(&mut self, chapters: &[ChapterData], has_cover: bool) {
        const CHARS_PER_PAGE: usize = 2000;

        let mut template_idx = 0;
        let mut total_chars: usize = 0;
        let mut next_page_at: usize = 0;

        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if has_cover {
            let cover_content_eid = eid_base + 1;
            self.add_page_template_with_offset(template_idx, cover_content_eid, 0);
            template_idx += 1;
            next_page_at = CHARS_PER_PAGE;
            eid_base += 2;
        }

        for chapter in chapters {
            let total_items = count_content_items(&chapter.content);
            for (i, item) in chapter.content.iter().enumerate() {
                let content_eid = eid_base + 1 + i as i64;
                let item_len = match item {
                    ContentItem::Image { .. } => CHARS_PER_PAGE,
                    _ => item.total_text_size(),
                };

                let item_start = total_chars;
                let item_end = total_chars + item_len;

                while next_page_at < item_end {
                    let offset_in_item = if next_page_at > item_start {
                        (next_page_at - item_start) as i64
                    } else {
                        0
                    };
                    self.add_page_template_with_offset(template_idx, content_eid, offset_in_item);
                    template_idx += 1;
                    next_page_at += CHARS_PER_PAGE;
                }

                total_chars = item_end;
            }

            eid_base += 1 + total_items as i64;
        }
    }

    fn add_page_template_with_offset(&mut self, idx: usize, eid: i64, offset: i64) {
        let template_id = format!("template-{idx}");
        let template_sym = self.symtab.get_or_intern(&template_id);

        let pos_info = if offset > 0 {
            IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid)),
                (sym::OFFSET, IonValue::Int(offset)),
            ])
        } else {
            IonValue::OrderedStruct(vec![(sym::POSITION, IonValue::Int(eid))])
        };

        let mut template = HashMap::new();
        template.insert(sym::TEMPLATE_NAME, IonValue::Symbol(template_sym));
        template.insert(sym::POSITION_INFO, pos_info);

        self.fragments.push(KfxFragment::new(
            sym::PAGE_TEMPLATE,
            &template_id,
            IonValue::Struct(template),
        ));
    }

    fn add_anchor_fragments(&mut self) {
        let section_eids = self.section_eids.clone();
        let anchor_eids = self.anchor_eids.clone();

        for (href, anchor_sym) in &self.anchor_symbols {
            let anchor_id = format!("${anchor_sym}");
            let mut anchor_struct = HashMap::new();
            anchor_struct.insert(sym::TEMPLATE_NAME, IonValue::Symbol(*anchor_sym));

            if href.starts_with("http://") || href.starts_with("https://") {
                anchor_struct.insert(sym::EXTERNAL_URL, IonValue::String(href.clone()));
            } else {
                let (path_without_fragment, has_fragment) = if let Some(hash_pos) = href.find('#') {
                    (&href[..hash_pos], true)
                } else {
                    (href.as_str(), false)
                };

                let target = if has_fragment {
                    anchor_eids
                        .get(href)
                        .copied()
                        .or_else(|| section_eids.get(path_without_fragment).map(|&e| (e, 0)))
                } else {
                    section_eids.get(path_without_fragment).map(|&e| (e, 0))
                };

                if let Some((eid, offset)) = target {
                    let pos_info = if offset > 0 {
                        IonValue::OrderedStruct(vec![
                            (sym::POSITION, IonValue::Int(eid)),
                            (sym::OFFSET, IonValue::Int(offset)),
                        ])
                    } else {
                        IonValue::OrderedStruct(vec![(sym::POSITION, IonValue::Int(eid))])
                    };
                    anchor_struct.insert(sym::POSITION_INFO, pos_info);
                } else {
                    continue;
                }
            }

            self.fragments.push(KfxFragment::new(
                sym::PAGE_TEMPLATE,
                &anchor_id,
                IonValue::Struct(anchor_struct),
            ));
        }
    }

    fn add_container_entity_map(&mut self) {
        let entity_ids: Vec<IonValue> = self
            .fragments
            .iter()
            .filter(|f| !f.is_singleton())
            .map(|f| {
                let sym_id = self.symtab.get_or_intern(&f.fid);
                IonValue::Symbol(sym_id)
            })
            .collect();

        let mut container_contents = HashMap::new();
        container_contents.insert(sym::POSITION, IonValue::String(self.container_id.clone()));
        container_contents.insert(sym::ENTITY_LIST, IonValue::List(entity_ids));

        let mut entity_map = HashMap::new();
        entity_map.insert(
            sym::CONTAINER_CONTENTS,
            IonValue::List(vec![IonValue::Struct(container_contents)]),
        );

        let mut all_deps: Vec<IonValue> = Vec::new();

        for (section_sym, resource_sym) in &self.section_to_resource {
            let mut dep = HashMap::new();
            dep.insert(sym::POSITION, IonValue::Symbol(*section_sym));
            dep.insert(
                sym::MANDATORY_DEPS,
                IonValue::List(vec![IonValue::Symbol(*resource_sym)]),
            );
            all_deps.push(IonValue::Struct(dep));
        }

        for (resource_sym, media_sym) in &self.resource_to_media {
            let mut dep = HashMap::new();
            dep.insert(sym::POSITION, IonValue::Symbol(*resource_sym));
            dep.insert(
                sym::MANDATORY_DEPS,
                IonValue::List(vec![IonValue::Symbol(*media_sym)]),
            );
            all_deps.push(IonValue::Struct(dep));
        }

        if !all_deps.is_empty() {
            entity_map.insert(sym::ENTITY_DEPS, IonValue::List(all_deps));
        }

        self.fragments.push(KfxFragment::singleton(
            sym::CONTAINER_ENTITY_MAP,
            IonValue::Struct(entity_map),
        ));
    }

    /// Build and serialize to bytes
    pub fn build(self) -> Vec<u8> {
        let symtab_ion = self
            .fragments
            .iter()
            .find(|f| f.ftype == 3)
            .map(|f| serialize_annotated_ion(3, &f.value))
            .unwrap_or_else(|| {
                let symtab_value = self.symtab.create_import();
                serialize_annotated_ion(3, &symtab_value)
            });

        let format_caps_ion = {
            let mut cap = HashMap::new();
            cap.insert(
                sym::METADATA_KEY,
                IonValue::String("kfxgen.textBlock".to_string()),
            );
            cap.insert(5, IonValue::Int(1));

            let caps_list = IonValue::List(vec![IonValue::Struct(cap)]);
            serialize_annotated_ion(sym::FORMAT_CAPABILITIES, &caps_list)
        };

        let mut entities: Vec<SerializedEntity> = Vec::new();

        for frag in &self.fragments {
            if frag.ftype == sym::FORMAT_CAPABILITIES
                || frag.ftype == sym::CONTAINER_INFO
                || frag.ftype == 3
            {
                continue;
            }

            let entity_id = if frag.is_singleton() {
                sym::SINGLETON_ID as u32
            } else {
                self.symtab.get(&frag.fid).unwrap_or(sym::SINGLETON_ID) as u32
            };

            let data = if frag.ftype == sym::RAW_MEDIA {
                if let IonValue::Blob(bytes) = &frag.value {
                    create_raw_media_data(bytes)
                } else {
                    create_entity_data(&frag.value)
                }
            } else {
                create_entity_data(&frag.value)
            };

            entities.push(SerializedEntity {
                id: entity_id,
                entity_type: frag.ftype as u32,
                data,
            });
        }

        serialize_container_v2(&self.container_id, &entities, &symtab_ion, &format_caps_ion)
    }
}

impl Default for KfxBookBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Write a KFX file from a Book
pub fn write_kfx(book: &Book, path: impl AsRef<Path>) -> io::Result<()> {
    let file = File::create(path)?;
    write_kfx_to_writer(book, BufWriter::new(file))
}

/// Write KFX to any writer
pub fn write_kfx_to_writer<W: Write>(book: &Book, mut writer: W) -> io::Result<()> {
    let builder = KfxBookBuilder::from_book(book);
    let data = builder.build();
    writer.write_all(&data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::ParsedStyle;

    /// Helper to find a value in an IonValue::Struct by key
    fn get_struct_field(value: &IonValue, key: u64) -> Option<&IonValue> {
        match value {
            IonValue::Struct(map) => map.get(&key),
            _ => None,
        }
    }

    /// Helper to get symbol value from IonValue
    fn get_symbol_value(value: &IonValue) -> Option<u64> {
        match value {
            IonValue::Symbol(s) => Some(*s),
            _ => None,
        }
    }

    #[test]
    fn test_list_container_uses_content_list_type() {
        // Create a list container with list items
        let list_item_1 = ContentItem::Container {
            style: ParsedStyle::default(),
            children: vec![ContentItem::Text {
                text: "First item".to_string(),
                style: ParsedStyle::default(),
                inline_runs: Vec::new(),
                anchor_href: None,
                element_id: None,
                is_verse: false,
            }],
            tag: "li".to_string(),
            element_id: None,
            list_type: None, // li elements don't have list_type
        };

        let list_item_2 = ContentItem::Container {
            style: ParsedStyle::default(),
            children: vec![ContentItem::Text {
                text: "Second item".to_string(),
                style: ParsedStyle::default(),
                inline_runs: Vec::new(),
                anchor_href: None,
                element_id: None,
                is_verse: false,
            }],
            tag: "li".to_string(),
            element_id: None,
            list_type: None,
        };

        let list_container = ContentItem::Container {
            style: ParsedStyle::default(),
            children: vec![list_item_1, list_item_2],
            tag: "ol".to_string(),
            element_id: None,
            list_type: Some(ListType::Ordered),
        };

        // Build the content items
        let mut builder = KfxBookBuilder::new();
        // Add a default style to the style_map
        builder.style_map.insert(ParsedStyle::default(), 860);

        let content_sym = builder.symtab.get_or_intern("content-test");
        let mut state = ContentState {
            global_idx: 0,
            text_idx_in_chunk: 0,
            current_content_sym: content_sym,
        };

        let ion_items = builder.build_content_items(&list_container, &mut state, 860);

        // Should have one list container
        assert_eq!(ion_items.len(), 1, "Should produce one list container");

        let list_ion = &ion_items[0];

        // Verify list container has content type $276 (CONTENT_LIST)
        let content_type = get_struct_field(list_ion, sym::CONTENT_TYPE)
            .and_then(get_symbol_value);
        assert_eq!(
            content_type,
            Some(sym::CONTENT_LIST),
            "List container should have content type $276 (CONTENT_LIST), got {:?}",
            content_type
        );

        // Verify list container has $100 (LIST_TYPE) property
        let list_type_prop = get_struct_field(list_ion, sym::LIST_TYPE)
            .and_then(get_symbol_value);
        assert_eq!(
            list_type_prop,
            Some(sym::LIST_TYPE_DECIMAL),
            "List container should have $100: $343 (decimal list type)"
        );

        // Get the children ($146 CONTENT_ARRAY)
        let children = get_struct_field(list_ion, sym::CONTENT_ARRAY);
        assert!(children.is_some(), "List container should have $146 (CONTENT_ARRAY)");

        if let Some(IonValue::List(child_items)) = children {
            assert_eq!(child_items.len(), 2, "List should have 2 items");

            // Verify each list item has content type $277 (CONTENT_LIST_ITEM)
            for (i, child_ion) in child_items.iter().enumerate() {
                let child_content_type = get_struct_field(child_ion, sym::CONTENT_TYPE)
                    .and_then(get_symbol_value);
                assert_eq!(
                    child_content_type,
                    Some(sym::CONTENT_LIST_ITEM),
                    "List item {} should have content type $277 (CONTENT_LIST_ITEM), got {:?}",
                    i,
                    child_content_type
                );

                // Verify list item has $145 (TEXT_CONTENT) directly, not nested $146
                let text_ref = get_struct_field(child_ion, sym::TEXT_CONTENT);
                assert!(
                    text_ref.is_some(),
                    "List item {} should have $145 (TEXT_CONTENT) directly",
                    i
                );

                // Verify list item does NOT have nested $146 (CONTENT_ARRAY)
                let nested_array = get_struct_field(child_ion, sym::CONTENT_ARRAY);
                assert!(
                    nested_array.is_none(),
                    "List item {} should NOT have nested $146 (CONTENT_ARRAY)",
                    i
                );
            }
        } else {
            panic!("Expected List for CONTENT_ARRAY");
        }
    }
}
