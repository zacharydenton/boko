//! Pass: detect CSS dropcaps and mark them for native KFX rendering.
//!
//! The common ebook dropcap idiom is a floated, large-font inline span
//! holding the paragraph's first letter(s):
//!
//! ```html
//! <p><span class="dropcaps">T</span>he kid looked at Tobin…</p>
//! ```
//! ```css
//! .dropcaps { float: left; font-size: 55px; line-height: 50px; }
//! ```
//!
//! Kindle Previewer renders this as a native KFX dropcap
//! (`dropcap_lines`/`dropcap_chars` on the paragraph) rather than a floated
//! box — the float otherwise reflows awkwardly on a small screen. This pass
//! recognizes the pattern and records the dropcap metrics on the
//! paragraph's style.
//!
//! The pass is deliberately **non-destructive**: it leaves the floated span
//! in place. The IR is shared across every output format, and EPUB / AZW3 /
//! MOBI have no native dropcap — for them, honoring the source CSS means
//! keeping the float span (`dropcap_lines`/`dropcap_chars` are not real CSS
//! and never reach their output). Only the KFX exporter consumes the
//! markers, and it suppresses the leading span's float and large font there
//! so the letters read as the paragraph's ordinary first characters (KFX
//! renders the first `dropcap_chars` large from the paragraph markers).

use super::pass::walk_bottom_up;
use super::predicates::is_inline_role;
use crate::model::{Chapter, NodeId, Role};

/// Detect dropcap spans and annotate their paragraphs.
pub fn detect_dropcaps(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, id| {
        detect_one(chapter, id);
    });
}

fn detect_one(chapter: &mut Chapter, para_id: NodeId) {
    let Some(para) = chapter.node(para_id) else {
        return;
    };
    if !matches!(para.role, Role::Paragraph | Role::Container) {
        return;
    }

    // The dropcap must be the paragraph's first child.
    let Some(first) = chapter.node(para_id).and_then(|n| n.first_child) else {
        return;
    };
    let Some(span) = chapter.node(first) else {
        return;
    };
    if !is_inline_role(span.role) || span.role == Role::Break {
        return;
    }

    let Some(span_style) = chapter.styles.get(span.style) else {
        return;
    };
    // Positive evidence for a dropcap: a float plus a font markedly larger
    // than the surrounding text.
    if span_style.float == crate::style::Float::None {
        return;
    }
    let para_abs = chapter
        .styles
        .get(chapter.node(para_id).unwrap().style)
        .map(|s| s.font_size_abs.0)
        .unwrap_or(1.0);
    let span_abs = span_style.font_size_abs.0;
    if span_abs < para_abs * 1.6 {
        return;
    }

    // Count the dropcap characters (short leading run) and the line span.
    let chars = dropcap_char_count(chapter, first);
    if chars == 0 || chars > 3 {
        return;
    }
    // Lines the dropcap spans: its glyph height (font size) over the body
    // font size, rounded and clamped. The dropcap's line-height is
    // deliberately ≤ its font size (it suppresses extra leading), so the
    // font-size ratio is the true line span; line-height is not a factor.
    let lines = (span_abs / para_abs).round().clamp(2.0, 5.0) as u8;

    // Annotate the paragraph. The floated span is left untouched: non-KFX
    // formats keep it as the CSS dropcap, and the KFX exporter suppresses
    // it when it sees the paragraph markers.
    let para_style_id = chapter.node(para_id).unwrap().style;
    if let Some(base) = chapter.styles.get(para_style_id).cloned() {
        let mut styled = base;
        styled.dropcap_lines = lines;
        styled.dropcap_chars = chars as u8;
        let new_id = chapter.styles.intern(styled);
        if let Some(node) = chapter.node_mut(para_id) {
            node.style = new_id;
        }
    }
}

/// Count leading characters inside the span subtree (text leaves only).
fn dropcap_char_count(chapter: &Chapter, span_id: NodeId) -> usize {
    let mut count = 0;
    let mut stack = vec![span_id];
    let mut ids = Vec::new();
    // Depth-first, left-to-right collection of text.
    while let Some(id) = stack.pop() {
        ids.push(id);
        let mut kids: Vec<NodeId> = chapter.children(id).collect();
        kids.reverse();
        stack.extend(kids);
    }
    for id in ids {
        if let Some(node) = chapter.node(id)
            && node.role == Role::Text
        {
            count += chapter.text(node.text).trim().chars().count();
        }
    }
    count
}
