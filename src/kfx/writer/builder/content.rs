//! Content item building for KFX generation.
//!
//! This module handles building content items (Text, Image, Container)
//! with proper TEXT_CONTENT references and inline style runs.

use std::collections::HashMap;

use crate::css::ParsedStyle;
use crate::kfx::ion::IonValue;
use crate::kfx::writer::content::{ContentItem, ListType, StyleRun};
use crate::kfx::writer::symbols::sym;

use super::{ContentState, KfxBookBuilder, normalize_text_for_kfx};

impl KfxBookBuilder {
    /// Build content items (Text, Image, or Container) with proper TEXT_CONTENT references
    /// Returns Vec because Text items with newlines become multiple paragraphs
    pub(crate) fn build_content_items(
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
            } => self.build_text_item(text, style, inline_runs, *is_verse, state, eid_base),

            ContentItem::Image {
                resource_href,
                style,
                alt,
            } => self.build_image_item(resource_href, style, alt.as_deref(), state, eid_base),

            ContentItem::Container {
                style,
                children,
                list_type,
                tag,
                colspan,
                rowspan,
                classification,
                ..
            } => self.build_container_item(
                style,
                children,
                list_type.as_ref(),
                tag,
                *colspan,
                *rowspan,
                *classification,
                state,
                eid_base,
            ),
        }
    }

    fn build_text_item(
        &mut self,
        text: &str,
        style: &ParsedStyle,
        inline_runs: &[StyleRun],
        is_verse: bool,
        state: &mut ContentState,
        eid_base: i64,
    ) -> Vec<IonValue> {
        let lines = normalize_text_for_kfx(text, is_verse);
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
            if let Some(style_sym) = self.style_map.get(style).copied()
                && style_sym != 0
            {
                item.insert(sym::STYLE, IonValue::Symbol(style_sym));
            }

            // Add inline style runs ($142) only for the first line
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

    fn build_image_item(
        &mut self,
        resource_href: &str,
        style: &ParsedStyle,
        alt: Option<&str>,
        state: &mut ContentState,
        eid_base: i64,
    ) -> Vec<IonValue> {
        let mut item = HashMap::new();

        let resource_sym = self
            .resource_symbols
            .get(resource_href)
            .copied()
            .unwrap_or(0);

        item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::IMAGE_CONTENT));
        item.insert(sym::RESOURCE_NAME, IonValue::Symbol(resource_sym));

        // $584 = IMAGE_ALT_TEXT for accessibility
        let alt_text = alt.unwrap_or_default().to_string();
        item.insert(sym::IMAGE_ALT_TEXT, IonValue::String(alt_text));

        // Add style reference if present
        if let Some(style_sym) = self.style_map.get(style).copied()
            && style_sym != 0
        {
            item.insert(sym::STYLE, IonValue::Symbol(style_sym));
        }

        // Use consistent EID that matches position maps
        item.insert(
            sym::POSITION,
            IonValue::Int(eid_base + 1 + state.global_idx as i64),
        );
        state.global_idx += 1;

        vec![IonValue::Struct(item)]
    }

    #[allow(clippy::too_many_arguments)]
    fn build_container_item(
        &mut self,
        style: &ParsedStyle,
        children: &[ContentItem],
        list_type: Option<&ListType>,
        tag: &str,
        colspan: Option<u32>,
        rowspan: Option<u32>,
        classification: Option<u64>,
        state: &mut ContentState,
        eid_base: i64,
    ) -> Vec<IonValue> {
        let mut item = HashMap::new();

        // Add classification ($615) for footnote/endnote containers
        if let Some(class_sym) = classification {
            item.insert(sym::CLASSIFICATION, IonValue::Symbol(class_sym));
        }

        // Determine content type based on container type
        let content_type =
            crate::kfx::writer::symbols::container_content_type(tag, list_type.is_some(), tag == "li");
        item.insert(sym::CONTENT_TYPE, IonValue::Symbol(content_type));

        // Add list type property for ol/ul containers
        if let Some(lt) = list_type {
            let list_type_sym = match lt {
                ListType::Ordered => sym::LIST_TYPE_DECIMAL,
                ListType::Unordered => sym::LIST_TYPE_DISC,
            };
            item.insert(sym::LIST_TYPE, IonValue::Symbol(list_type_sym));
        }

        // Add colspan/rowspan for table cells
        if let Some(cs) = colspan
            && cs > 1
        {
            item.insert(sym::ATTRIB_COLSPAN, IonValue::Int(cs as i64));
        }
        if let Some(rs) = rowspan
            && rs > 1
        {
            item.insert(sym::ATTRIB_ROWSPAN, IonValue::Int(rs as i64));
        }

        // For list items (li), directly reference text content
        if tag == "li" {
            self.build_list_item(&mut item, children, state, style, eid_base);
        } else {
            // Build nested content array for regular containers
            let nested_items: Vec<IonValue> = children
                .iter()
                .flat_map(|child| self.build_content_items(child, state, eid_base))
                .collect();

            item.insert(sym::CONTENT_ARRAY, IonValue::List(nested_items));

            // Add style reference for the container
            if let Some(style_sym) = self.style_map.get(style).copied()
                && style_sym != 0
            {
                item.insert(sym::STYLE, IonValue::Symbol(style_sym));
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

    /// Build a list item (li) with direct $145 text reference
    pub(crate) fn build_list_item(
        &mut self,
        item: &mut HashMap<u64, IonValue>,
        children: &[ContentItem],
        state: &mut ContentState,
        style: &ParsedStyle,
        eid_base: i64,
    ) {
        // Extract text from the list item's children
        let text_content: Vec<&ContentItem> = children
            .iter()
            .flat_map(|c| c.flatten())
            .filter(|c| matches!(c, ContentItem::Text { .. }))
            .collect();

        // Collect inline runs from all text items
        let mut all_inline_runs = Vec::new();
        let mut offset_adjustment = 0usize;

        for text_item in &text_content {
            if let ContentItem::Text {
                text, inline_runs, ..
            } = text_item
            {
                for run in inline_runs {
                    all_inline_runs.push(StyleRun {
                        offset: run.offset + offset_adjustment,
                        length: run.length,
                        style: run.style.clone(),
                        anchor_href: run.anchor_href.clone(),
                        element_id: run.element_id.clone(),
                        is_noteref: run.is_noteref,
                    });
                }
                offset_adjustment += text.chars().count();
            }
        }

        // Create direct text reference ($145)
        if !text_content.is_empty() {
            let mut text_ref = HashMap::new();
            text_ref.insert(sym::ID, IonValue::Symbol(state.current_content_sym));
            text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(state.text_idx_in_chunk));
            item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));

            // Add inline style runs ($142) if present
            if !all_inline_runs.is_empty() {
                let runs = self.build_inline_runs(&all_inline_runs);
                if !runs.is_empty() {
                    item.insert(sym::INLINE_STYLE_RUNS, IonValue::List(runs));
                }
            }

            for _ in &text_content {
                state.text_idx_in_chunk += 1;
            }
        }

        // Add style reference for the list item
        if let Some(style_sym) = self.style_map.get(style).copied()
            && style_sym != 0
        {
            item.insert(sym::STYLE, IonValue::Symbol(style_sym));
        }

        // Use consistent EID that matches position maps
        item.insert(
            sym::POSITION,
            IonValue::Int(eid_base + 1 + state.global_idx as i64),
        );
        state.global_idx += 1;
    }

    /// Build inline style runs for a text item
    pub(crate) fn build_inline_runs(&mut self, runs: &[StyleRun]) -> Vec<IonValue> {
        runs.iter()
            .filter_map(|run| {
                let style_sym = self.style_map.get(&run.style).copied()?;

                let mut run_struct = HashMap::new();
                run_struct.insert(sym::OFFSET, IonValue::Int(run.offset as i64));
                run_struct.insert(sym::COUNT, IonValue::Int(run.length as i64));
                run_struct.insert(sym::STYLE, IonValue::Symbol(style_sym));

                // Add anchor reference ($179) if this run has a hyperlink
                if let Some(ref href) = run.anchor_href
                    && let Some(&anchor_sym) = self.anchor_symbols.get(href)
                {
                    run_struct.insert(sym::ANCHOR_REF, IonValue::Symbol(anchor_sym));
                }

                // Add noteref marker ($616: $617) for footnote/endnote links
                if run.is_noteref {
                    run_struct.insert(sym::NOTEREF_TYPE, IonValue::Symbol(sym::NOTEREF));
                }

                Some(IonValue::Struct(run_struct))
            })
            .collect()
    }
}
