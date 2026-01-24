//! Fragment creation for KFX generation.
//!
//! This module handles creating various KFX fragments including metadata,
//! navigation, sections, position maps, and anchors.

use std::collections::{HashMap, HashSet};

use crate::book::Book;
use crate::css::ParsedStyle;
use crate::kfx::ion::IonValue;
use crate::kfx::writer::content::{ChapterData, ContentItem, count_content_eids};
use crate::kfx::writer::fragment::KfxFragment;
use crate::kfx::writer::navigation::{build_book_navigation, build_nav_unit_list};
use crate::kfx::writer::position::{build_location_map, build_position_id_map, build_position_map};
use crate::kfx::writer::style::style_to_ion;
use crate::kfx::writer::symbols::{SymbolTable, sym};

use super::KfxBookBuilder;

impl KfxBookBuilder {
    /// Add container info fragment
    pub(crate) fn add_container_info_fragment(&mut self) {
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

    /// Add symbol table fragment
    pub(crate) fn add_symbol_table_fragment(&mut self) {
        let symtab_value = self.symtab.create_import();
        self.fragments
            .push(KfxFragment::new(3, "$ion_symbol_table", symtab_value));
    }

    /// Add format capabilities fragment
    pub(crate) fn add_format_capabilities(&mut self) {
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

    /// Add metadata fragment with book information
    pub(crate) fn add_metadata(&mut self, book: &Book) {
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

            if let Some(cover_href) = &book.metadata.cover_image
                && let Some(&cover_sym) = self.resource_symbols.get(cover_href)
            {
                add_entry("cover_image", IonValue::Symbol(cover_sym));
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

    /// Add reading order metadata fragment
    pub(crate) fn add_reading_order_metadata(&mut self, chapters: &[ChapterData], has_cover: bool) {
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

    /// Add document data fragment
    pub(crate) fn add_document_data(&mut self, chapters: &[ChapterData], has_cover: bool) {
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

    /// Add book navigation fragment ($389)
    ///
    /// Creates a $389 fragment with INLINE $391:: nav containers and $393:: nav entries.
    /// This matches the reference KFX format where navigation is a single fragment
    /// with annotated inline values, not separate fragments.
    pub(crate) fn add_book_navigation_fragment(
        &mut self,
        toc: &[crate::book::TocEntry],
        chapters: &[ChapterData],
        has_cover: bool,
        first_content_eid: Option<i64>,
    ) {
        let nav_value = build_book_navigation(
            toc,
            chapters,
            &self.section_eids,
            &self.anchor_eids,
            &mut self.symtab,
            has_cover,
            first_content_eid,
        );

        self.fragments
            .push(KfxFragment::singleton(sym::BOOK_NAVIGATION, nav_value));
    }

    /// Add navigation unit list fragment
    pub(crate) fn add_nav_unit_list(&mut self) {
        self.fragments.push(KfxFragment::singleton(
            sym::NAV_UNIT_LIST,
            build_nav_unit_list(),
        ));
    }

    /// Collect and add all unique styles from chapters
    pub(crate) fn add_all_styles(&mut self, chapters: &[ChapterData]) {
        fn collect_styles(item: &ContentItem, styles: &mut HashSet<ParsedStyle>) {
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
                ContentItem::Image { .. } | ContentItem::Svg { .. } => {}
            }
        }

        let mut unique_styles = HashSet::new();
        for chapter in chapters {
            for item in &chapter.content {
                collect_styles(item, &mut unique_styles);
            }
        }

        for (i, style) in unique_styles.into_iter().enumerate() {
            let style_id = format!("style-{i}");
            let style_sym = self.symtab.get_or_intern(&style_id);

            let style_ion = style_to_ion(&style, style_sym, &mut self.symtab);

            self.fragments
                .push(KfxFragment::new(sym::STYLE, &style_id, style_ion));
            self.style_map.insert(style, style_sym);
        }
    }

    /// Add section fragment for a chapter
    pub(crate) fn add_section(&mut self, chapter: &ChapterData, eid_base: i64) {
        let section_id = format!("section-{}", chapter.id);
        let section_sym = self.symtab.get_or_intern(&section_id);

        let block_id = format!("block-{}", chapter.id);
        let block_sym = self.symtab.get_or_intern(&block_id);

        let mut section = HashMap::new();
        section.insert(sym::SECTION_NAME, IonValue::Symbol(section_sym));

        // Section content entry must include CONTENT_TYPE ($159) for TOC navigation to work
        // Reference KFX files have $159: $269 (CONTENT_PARAGRAPH) for text sections
        section.insert(
            sym::SECTION_CONTENT,
            IonValue::List(vec![IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid_base)),
                (sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH)),
                (sym::CONTENT_NAME, IonValue::Symbol(block_sym)),
            ])]),
        );

        self.fragments.push(KfxFragment::new(
            sym::SECTION,
            &section_id,
            IonValue::Struct(section),
        ));
    }

    /// Add auxiliary data fragment for a section
    ///
    /// `section_fid` should be the exact FID string used when creating the section fragment.
    /// For cover: "cover-section"
    /// For chapters: "section-chapter-N"
    pub(crate) fn add_auxiliary_data_for_section(&mut self, section_fid: &str) {
        let aux_id = format!("aux-{}", section_fid);
        // Get the section symbol directly - no transformation needed
        let section_sym = self.symtab.get_or_intern(section_fid);

        let mut metadata_entry = HashMap::new();
        metadata_entry.insert(sym::VALUE, IonValue::Bool(true));
        metadata_entry.insert(
            sym::METADATA_KEY,
            IonValue::String("IS_TARGET_SECTION".to_string()),
        );

        let mut aux = HashMap::new();
        // Reference the section symbol
        aux.insert(sym::AUX_DATA_REF, IonValue::Symbol(section_sym));
        aux.insert(
            sym::METADATA,
            IonValue::List(vec![IonValue::Struct(metadata_entry)]),
        );

        self.fragments.push(KfxFragment::new(
            sym::AUXILIARY_DATA,
            &aux_id,
            IonValue::Struct(aux),
        ));
    }

    /// Add position map fragment
    pub(crate) fn add_position_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let position_map = build_position_map(chapters, &mut self.symtab, has_cover);
        self.fragments
            .push(KfxFragment::singleton(sym::POSITION_MAP, position_map));
    }

    /// Add position ID map fragment
    pub(crate) fn add_position_id_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let position_id_map = build_position_id_map(chapters, has_cover);
        self.fragments.push(KfxFragment::singleton(
            sym::POSITION_ID_MAP,
            position_id_map,
        ));
    }

    /// Add location map fragment
    pub(crate) fn add_location_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let location_map = build_location_map(chapters, has_cover);
        self.fragments
            .push(KfxFragment::singleton(sym::LOCATION_MAP, location_map));
    }

    /// Add pagination templates based on character count.
    /// Note: Currently not called as Kindle uses POSITION_MAP instead.
    #[allow(dead_code)]
    pub(crate) fn add_page_templates(&mut self, chapters: &[ChapterData], has_cover: bool) {
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
            let total_items = count_content_eids(&chapter.content);
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

    #[allow(dead_code)]
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

    /// Add anchor fragments for internal and external links
    pub(crate) fn add_anchor_fragments(&mut self) {
        let section_eids = self.section_eids.clone();
        let anchor_eids = self.anchor_eids.clone();

        for (href, anchor_sym) in &self.anchor_symbols {
            let anchor_id = format!("${anchor_sym}");
            let mut anchor_struct = HashMap::new();
            anchor_struct.insert(sym::TEMPLATE_NAME, IonValue::Symbol(*anchor_sym));

            if href.starts_with("http://")
                || href.starts_with("https://")
                || href.starts_with("mailto:")
            {
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

    /// Add container entity map fragment
    pub(crate) fn add_container_entity_map(&mut self) {
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
}
