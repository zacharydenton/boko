//! KFX book builder - orchestrates the conversion of a Book to KFX format.
//!
//! This module is split into submodules:
//! - `content`: Content item building (text, images, containers)
//! - `chunks`: Chunk management and content block creation
//! - `fragments`: Fragment creation (metadata, navigation, sections, etc.)

mod chunks;
mod content;
mod fragments;

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::book::Book;
use crate::css::{ParsedStyle, Stylesheet};
use crate::kfx::ion::IonValue;

use super::content::{
    ChapterData, ContentChunk, ContentItem, collect_referenced_images, count_content_items,
    extract_content_from_xhtml, extract_css_hrefs_from_xhtml,
};
use super::fragment::KfxFragment;
use super::navigation::build_anchor_symbols;
use super::position::{build_anchor_eids, build_section_eids};
use super::resources::{
    build_resource_symbols, create_resource_fragments, get_image_dimensions,
    populate_image_dimensions,
};
use super::serialization::{
    SerializedEntity, create_entity_data, create_raw_media_data, generate_container_id,
    serialize_annotated_ion, serialize_container_v2,
};
use super::symbols::{SymbolTable, sym};

/// State for tracking text indexing across content chunks
pub(crate) struct ContentState {
    pub(crate) global_idx: usize,
    pub(crate) text_idx_in_chunk: i64,
    pub(crate) current_content_sym: u64,
}

/// Normalize text for KFX output based on verse context.
/// - Verse: split by newlines to create separate paragraph entries
/// - Non-verse: keep text as-is (preserve newlines for proper inline run offset alignment)
pub(crate) fn normalize_text_for_kfx(text: &str, is_verse: bool) -> Vec<String> {
    if is_verse {
        text.split('\n')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        if text.trim().is_empty() {
            vec![]
        } else {
            vec![text.to_string()]
        }
    }
}

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
        if let Some(cover_href) = &book.metadata.cover_image
            && let Some(cover_sym) = builder.resource_symbols.get(cover_href).copied()
        {
            let (cover_width, cover_height) = book
                .resources
                .get(cover_href)
                .and_then(|r| get_image_dimensions(&r.data))
                .unwrap_or((1400, 2100));

            builder.add_cover_section(cover_sym, cover_width, cover_height, eid_base);
            builder.add_auxiliary_data_for_section("cover");
            eid_base += 2;
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
            builder.add_auxiliary_data_for_section(&chapter.id);

            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
        }

        // Add position/location maps
        builder.add_position_map(&chapters, has_cover);
        builder.add_position_id_map(&chapters, has_cover);
        builder.add_location_map(&chapters, has_cover);

        // Add anchor fragments
        builder.add_anchor_fragments();

        // Collect referenced images from content (skip unreferenced mobi fallbacks)
        let mut referenced_hrefs = std::collections::HashSet::new();
        for chapter in &chapters {
            referenced_hrefs.extend(collect_referenced_images(&chapter.content));
        }

        // Add resources (only referenced images, cover, and fonts)
        let (resource_fragments, resource_to_media) = create_resource_fragments(
            book,
            &mut builder.symtab,
            &builder.resource_symbols,
            &referenced_hrefs,
        );
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
mod tests;
