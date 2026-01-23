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
        let content_type = crate::kfx::writer::symbols::container_content_type(
            tag,
            list_type.is_some(),
            tag == "li",
        );
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
            self.build_list_item(classification, children, state, style, eid_base)
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

            vec![IonValue::Struct(item)]
        }
    }

    /// Build list item content.
    ///
    /// For simple list items (just text), returns multiple CONTENT_LIST_ITEM entries.
    /// For complex list items (with nested containers like blockquote), returns ONE
    /// CONTENT_LIST_ITEM container with CONTENT_ARRAY of nested CONTENT_PARAGRAPH items.
    pub(crate) fn build_list_item(
        &mut self,
        classification: Option<u64>,
        children: &[ContentItem],
        state: &mut ContentState,
        style: &ParsedStyle,
        eid_base: i64,
    ) -> Vec<IonValue> {
        // Check if we have nested containers (blockquote, div, etc.)
        // Note: This matches ContentItem::has_nested_containers() but operates on children slice
        let has_nested_containers = children
            .iter()
            .any(|c| matches!(c, ContentItem::Container { .. }));

        if has_nested_containers {
            // Complex list item: create ONE container with nested content
            self.build_complex_list_item(classification, children, state, style, eid_base)
        } else {
            // Simple list item: create flat CONTENT_LIST_ITEM entries
            // Note: simple list items don't support classification as they're flat text entries
            self.build_simple_list_item(children, state, style, eid_base)
        }
    }

    /// Build simple list item (no nested containers) - creates flat CONTENT_LIST_ITEM entries
    fn build_simple_list_item(
        &mut self,
        children: &[ContentItem],
        state: &mut ContentState,
        _style: &ParsedStyle,
        eid_base: i64,
    ) -> Vec<IonValue> {
        let mut items = Vec::new();

        for child in children {
            match child {
                ContentItem::Text {
                    text,
                    style: text_style,
                    inline_runs,
                    is_verse,
                    ..
                } => {
                    let lines = super::normalize_text_for_kfx(text, *is_verse);

                    for (i, _line) in lines.iter().enumerate() {
                        let mut content_item = HashMap::new();
                        content_item
                            .insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_LIST_ITEM));

                        let mut text_ref = HashMap::new();
                        text_ref.insert(sym::ID, IonValue::Symbol(state.current_content_sym));
                        text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(state.text_idx_in_chunk));
                        content_item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));

                        if i == 0 && !inline_runs.is_empty() {
                            let runs = self.build_inline_runs(inline_runs);
                            if !runs.is_empty() {
                                content_item.insert(sym::INLINE_STYLE_RUNS, IonValue::List(runs));
                            }
                        }

                        if let Some(style_sym) = self.style_map.get(text_style).copied()
                            && style_sym != 0
                        {
                            content_item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                        }

                        content_item.insert(
                            sym::POSITION,
                            IonValue::Int(eid_base + 1 + state.global_idx as i64),
                        );

                        state.text_idx_in_chunk += 1;
                        state.global_idx += 1;
                        items.push(IonValue::Struct(content_item));
                    }
                }
                ContentItem::Image { .. } => {
                    let img_items = self.build_content_items(child, state, eid_base);
                    items.extend(img_items);
                }
                _ => {}
            }
        }

        items
    }

    /// Build complex list item (with nested containers) - creates ONE CONTENT_LIST_ITEM
    /// container with CONTENT_ARRAY of nested CONTENT_PARAGRAPH items
    fn build_complex_list_item(
        &mut self,
        classification: Option<u64>,
        children: &[ContentItem],
        state: &mut ContentState,
        style: &ParsedStyle,
        eid_base: i64,
    ) -> Vec<IonValue> {
        let mut nested_items = Vec::new();

        // Recursively build all nested content as CONTENT_PARAGRAPH items
        self.build_nested_paragraphs(children, state, eid_base, &mut nested_items);

        // Create the outer CONTENT_LIST_ITEM container
        let mut container = HashMap::new();
        container.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_LIST_ITEM));
        container.insert(sym::CONTENT_ARRAY, IonValue::List(nested_items));

        // Add classification ($615) for footnote/endnote containers
        if let Some(class_sym) = classification {
            container.insert(sym::CLASSIFICATION, IonValue::Symbol(class_sym));
        }

        if let Some(style_sym) = self.style_map.get(style).copied()
            && style_sym != 0
        {
            container.insert(sym::STYLE, IonValue::Symbol(style_sym));
        }

        container.insert(
            sym::POSITION,
            IonValue::Int(eid_base + 1 + state.global_idx as i64),
        );
        state.global_idx += 1;

        vec![IonValue::Struct(container)]
    }

    /// Recursively build nested content items as CONTENT_PARAGRAPH ($269)
    fn build_nested_paragraphs(
        &mut self,
        children: &[ContentItem],
        state: &mut ContentState,
        eid_base: i64,
        items: &mut Vec<IonValue>,
    ) {
        for child in children {
            match child {
                ContentItem::Text {
                    text,
                    style: text_style,
                    inline_runs,
                    is_verse,
                    ..
                } => {
                    let lines = super::normalize_text_for_kfx(text, *is_verse);

                    for (i, _line) in lines.iter().enumerate() {
                        let mut content_item = HashMap::new();
                        // Use CONTENT_PARAGRAPH for nested items, not CONTENT_LIST_ITEM
                        content_item
                            .insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));

                        let mut text_ref = HashMap::new();
                        text_ref.insert(sym::ID, IonValue::Symbol(state.current_content_sym));
                        text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(state.text_idx_in_chunk));
                        content_item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));

                        if i == 0 && !inline_runs.is_empty() {
                            let runs = self.build_inline_runs(inline_runs);
                            if !runs.is_empty() {
                                content_item.insert(sym::INLINE_STYLE_RUNS, IonValue::List(runs));
                            }
                        }

                        if let Some(style_sym) = self.style_map.get(text_style).copied()
                            && style_sym != 0
                        {
                            content_item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                        }

                        content_item.insert(
                            sym::POSITION,
                            IonValue::Int(eid_base + 1 + state.global_idx as i64),
                        );

                        state.text_idx_in_chunk += 1;
                        state.global_idx += 1;
                        items.push(IonValue::Struct(content_item));
                    }
                }
                ContentItem::Container {
                    children: nested, ..
                } => {
                    // Recursively process nested containers
                    self.build_nested_paragraphs(nested, state, eid_base, items);
                }
                ContentItem::Image { .. } => {
                    let img_items = self.build_content_items(child, state, eid_base);
                    items.extend(img_items);
                }
            }
        }
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
