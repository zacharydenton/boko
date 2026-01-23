//! Chunk management for KFX generation.
//!
//! This module handles creating content chunks and building content blocks
//! with proper text content references.

use std::collections::HashMap;

use crate::kfx::ion::IonValue;
use crate::kfx::writer::content::{ChapterData, ContentChunk, ContentItem};
use crate::kfx::writer::fragment::KfxFragment;
use crate::kfx::writer::symbols::sym;

use super::{ContentState, KfxBookBuilder, normalize_text_for_kfx};

impl KfxBookBuilder {
    /// Create content chunks from all chapters
    pub(crate) fn create_chunks(&self, chapters: &[ChapterData]) -> Vec<(usize, ContentChunk)> {
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

    /// Add a text content chunk fragment.
    ///
    /// Note: We use CONTENT_ARRAY ($146) with a list of strings rather than TEXT ($244)
    /// with a single concatenated string. The KFX reader expects $146 format to extract
    /// text content for the spine. Reference KFX files from Kindle Previewer also use
    /// the $146 list format.
    pub(crate) fn add_text_content_chunk(&mut self, chunk: &ContentChunk) {
        let content_id = format!("content-{}", chunk.id);
        let content_sym = self.symtab.get_or_intern(&content_id);

        // Use flatten() to extract text from nested containers
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

    /// Add a cover section with image
    pub(crate) fn add_cover_section(
        &mut self,
        cover_sym: u64,
        width: u32,
        height: u32,
        eid_base: i64,
    ) {
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
        self.section_to_resource
            .push((cover_section_sym, cover_sym));
    }

    /// Add a content block with chunked text content
    pub(crate) fn add_content_block_chunked(
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
                let ion_items = self.build_content_items(content_item, &mut state, eid_base);
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
}
