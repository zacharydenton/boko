//! Static margin collapsing for KFX export.
//!
//! The Kindle renderer does not collapse adjoining vertical margins the way
//! CSS engines do — Kindle Previewer resolves collapsing at conversion time
//! and bakes the result into the styles (its output has margin-bottom on
//! almost no styles; the collapsed gap rides the following block's
//! margin-top, and only a section's last block keeps a bottom margin).
//! Without this pass, every `margin: 1em 0` paragraph sequence renders with
//! double gaps on device.
//!
//! The rules follow CSS adjoining-margin semantics, verified against Kindle
//! Previewer gold masters with probe books:
//!
//! - Adjoining margins take `max(positives) + min(negatives)`, resolved in
//!   absolute units (em of the root size) so different font scales compare
//!   correctly.
//! - The collapsed value is assigned to the *following* block's margin-top;
//!   every other participating margin is zeroed.
//! - Collapsing reaches through plain container boundaries (first/last
//!   child chains) and across empty blocks, and is blocked by borders,
//!   padding, and intervening inline content.
//! - A container's last pending bottom margin is kept (compressed onto the
//!   outermost element) — sections keep their final bottom margin.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::{Chapter, NodeId, Role};
use crate::style::{ComputedStyle, Length};

/// Margin override for one node, in absolute em (multiples of the root font
/// size). `Some(0.0)` removes the margin; `None` leaves the authored value.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MarginAdjust {
    pub top_abs_em: Option<f32>,
    pub bottom_abs_em: Option<f32>,
}

impl MarginAdjust {
    pub fn is_identity(&self) -> bool {
        self.top_abs_em.is_none() && self.bottom_abs_em.is_none()
    }
}

/// Per-node margin overrides produced by [`compute_margin_collapse`].
pub type MarginAdjustMap = FxHashMap<NodeId, MarginAdjust>;

/// Which margin of a node participates in an adjoining set.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Top,
    Bottom,
}

struct Collapser<'a> {
    chapter: &'a Chapter,
    map: MarginAdjustMap,
    /// Margins already consumed by a collapse (never touched twice).
    done: FxHashSet<(NodeId, u8)>,
}

/// Compute the margin-collapse overrides for a chapter.
pub fn compute_margin_collapse(chapter: &Chapter) -> MarginAdjustMap {
    let mut c = Collapser {
        chapter,
        map: MarginAdjustMap::default(),
        done: FxHashSet::default(),
    };
    c.process_container(chapter.root());
    c.map.retain(|_, adj| !adj.is_identity());
    c.map
}

impl<'a> Collapser<'a> {
    fn style(&self, id: NodeId) -> Option<&'a ComputedStyle> {
        let node = self.chapter.node(id)?;
        self.chapter.styles.get(node.style)
    }

    fn mark(&mut self, id: NodeId, side: Side) {
        self.done.insert((id, side as u8));
    }

    fn is_done(&self, id: NodeId, side: Side) -> bool {
        self.done.contains(&(id, side as u8))
    }

    /// The resolvable absolute margin (root-em) of one side, `None` when the
    /// margin is `auto` or the style is missing.
    fn abs_margin(&self, id: NodeId, side: Side) -> Option<f32> {
        let style = self.style(id)?;
        let len = match side {
            Side::Top => style.margin_top,
            Side::Bottom => style.margin_bottom,
        };
        let abs = style.font_size_abs.0;
        match len {
            Length::Auto => None,
            Length::Em(x) => Some(x * abs),
            Length::Rem(x) => Some(x),
            Length::Px(x) => Some(x / 16.0),
            // Kindle Previewer resolves percent spacing against its 512px
            // layout viewport (32em at the root size).
            Length::Percent(p) => Some(p / 100.0 * 32.0),
        }
    }

    /// Whether a node participates in block flow at all.
    fn is_flow_block(&self, id: NodeId) -> bool {
        let Some(node) = self.chapter.node(id) else {
            return false;
        };
        let block = matches!(
            node.role,
            Role::Paragraph
                | Role::Heading(_)
                | Role::Container
                | Role::BlockQuote
                | Role::OrderedList
                | Role::UnorderedList
                | Role::ListItem
                | Role::Table
                | Role::Image
                | Role::Figure
                | Role::Sidebar
                | Role::Footnote
                | Role::DefinitionList
                | Role::DefinitionTerm
                | Role::DefinitionDescription
                | Role::CodeBlock
                | Role::Caption
                | Role::Rule
        );
        if !block {
            return false;
        }
        match self.style(id) {
            Some(s) => {
                s.display != crate::style::Display::None && s.float == crate::style::Float::None
            }
            None => true,
        }
    }

    /// Inline flow with visible content breaks margin adjacency. Empty
    /// inline anchors (`<a id="..."/>` between paragraphs) render nothing
    /// and must not break it.
    fn is_blocking_inline(&self, id: NodeId) -> bool {
        let Some(node) = self.chapter.node(id) else {
            return false;
        };
        match node.role {
            Role::Text => !self.chapter.text(node.text).trim().is_empty(),
            Role::Break => true,
            Role::Link | Role::Inline => self.has_visible_content(id),
            _ => false,
        }
    }

    /// An empty plain block: no visible content at all — a bare block or an
    /// anchor carrier like `<p><a id="..."/></p>`. Its margins vanish
    /// (reference output drops such blocks entirely).
    fn is_empty_block(&self, id: NodeId) -> bool {
        let Some(node) = self.chapter.node(id) else {
            return false;
        };
        matches!(
            node.role,
            Role::Paragraph | Role::Container | Role::Heading(_) | Role::BlockQuote
        ) && !self.has_visible_content(id)
    }

    /// Whether the subtree renders anything: non-empty text, a break, or
    /// replaced/structural content. Empty inline anchors don't count.
    fn has_visible_content(&self, id: NodeId) -> bool {
        let Some(node) = self.chapter.node(id) else {
            return false;
        };
        match node.role {
            Role::Break | Role::Image | Role::Rule | Role::Table | Role::Math => return true,
            Role::Text => return !self.chapter.text(node.text).trim().is_empty(),
            _ => {}
        }
        if !self.chapter.text(node.text).trim().is_empty() {
            return true;
        }
        self.chapter
            .children(id)
            .any(|c| self.has_visible_content(c))
    }

    /// Whether collapsing may reach from this node into its first/last
    /// child's margins: plain grouping containers only, without an
    /// intervening barrier or own text.
    fn extends_into_children(&self, id: NodeId, side: Side) -> bool {
        let Some(node) = self.chapter.node(id) else {
            return false;
        };
        let grouping = matches!(
            node.role,
            Role::Container
                | Role::BlockQuote
                | Role::Figure
                | Role::Sidebar
                | Role::Footnote
                | Role::DefinitionList
                | Role::OrderedList
                | Role::UnorderedList
                | Role::ListItem
                | Role::DefinitionDescription
        );
        if !grouping || !self.chapter.text(node.text).is_empty() {
            return false;
        }
        let Some(style) = self.style(id) else {
            return true;
        };
        let (border, padding) = match side {
            Side::Top => (
                style.border_style_top != crate::style::BorderStyle::None
                    && !matches!(style.border_width_top, Length::Auto | Length::Px(0.0)),
                !matches!(style.padding_top, Length::Auto | Length::Px(0.0)),
            ),
            Side::Bottom => (
                style.border_style_bottom != crate::style::BorderStyle::None
                    && !matches!(style.border_width_bottom, Length::Auto | Length::Px(0.0)),
                !matches!(style.padding_bottom, Length::Auto | Length::Px(0.0)),
            ),
        };
        !(border || padding)
    }

    /// The chain of adjoining margins entering `id` from the given side:
    /// the node's own margin plus, when nothing separates them, the first
    /// (or last) in-flow child's chain.
    fn margin_chain(&self, id: NodeId, side: Side, out: &mut Vec<NodeId>) {
        out.push(id);
        if !self.extends_into_children(id, side) {
            return;
        }
        let flow: Vec<NodeId> = self.flow_children(id);
        let next = match side {
            Side::Top => flow.first(),
            Side::Bottom => flow.last(),
        };
        // Any blocking inline content on that edge stops the chain.
        let kids: Vec<NodeId> = self.chapter.children(id).collect();
        let edge_clear = match (side, next) {
            (_, None) => false,
            (Side::Top, Some(&f)) => kids
                .iter()
                .take_while(|&&k| k != f)
                .all(|&k| !self.is_blocking_inline(k)),
            (Side::Bottom, Some(&l)) => kids
                .iter()
                .skip_while(|&&k| k != l)
                .skip(1)
                .all(|&k| !self.is_blocking_inline(k)),
        };
        if let Some(&child) = next
            && edge_clear
        {
            self.margin_chain(child, side, out);
        }
    }

    fn flow_children(&self, id: NodeId) -> Vec<NodeId> {
        self.chapter
            .children(id)
            .filter(|&c| self.is_flow_block(c))
            .collect()
    }

    /// CSS adjoining-margin value: largest positive plus smallest negative.
    fn collapse_value(&self, set: &[(NodeId, Side)]) -> f32 {
        let mut max_pos = 0.0f32;
        let mut min_neg = 0.0f32;
        for &(id, side) in set {
            if let Some(v) = self.abs_margin(id, side) {
                if v > max_pos {
                    max_pos = v;
                }
                if v < min_neg {
                    min_neg = v;
                }
            }
        }
        max_pos + min_neg
    }

    fn zero(&mut self, id: NodeId, side: Side) {
        if self.is_done(id, side) {
            return;
        }
        self.mark(id, side);
        // Only record a change when there is an authored margin to remove.
        if self.abs_margin(id, side).is_some_and(|v| v != 0.0) {
            let adj = self.map.entry(id).or_default();
            match side {
                Side::Top => adj.top_abs_em = Some(0.0),
                Side::Bottom => adj.bottom_abs_em = Some(0.0),
            }
        }
    }

    fn assign(&mut self, id: NodeId, side: Side, value: f32) {
        if self.is_done(id, side) {
            return;
        }
        self.mark(id, side);
        let authored = self.abs_margin(id, side).unwrap_or(0.0);
        if (authored - value).abs() > 1e-4 {
            let adj = self.map.entry(id).or_default();
            match side {
                Side::Top => adj.top_abs_em = Some(value),
                Side::Bottom => adj.bottom_abs_em = Some(value),
            }
        }
    }

    /// Collapse a full adjoining set, assigning the result to `carrier`.
    fn collapse_set(&mut self, set: Vec<(NodeId, Side)>, carrier: (NodeId, Side)) {
        if set.iter().any(|&(id, side)| self.is_done(id, side)) {
            // Part of this set was already consumed by an outer collapse;
            // leave the remainder alone rather than double-collapsing.
            return;
        }
        let value = self.collapse_value(&set);
        for &(id, side) in &set {
            if (id, side) != carrier {
                self.zero(id, side);
            }
        }
        self.assign(carrier.0, carrier.1, value);
    }

    /// Process one container: collapse boundaries between its in-flow block
    /// children, then recurse. Called top-down so outer chains consume deep
    /// margins before inner containers look at them.
    fn process_container(&mut self, id: NodeId) {
        let Some(node) = self.chapter.node(id) else {
            return;
        };
        if matches!(node.role, Role::Image | Role::Rule) {
            return;
        }

        let kids: Vec<NodeId> = self.chapter.children(id).collect();
        let has_blocks = kids.iter().any(|&k| self.is_flow_block(k));
        if has_blocks {
            // Pending adjoining margins waiting for the next block: the
            // previous block's trailing chain plus any empty blocks since.
            let mut pending: Vec<(NodeId, Side)> = Vec::new();
            let mut first_at_level = true;

            for &child in &kids {
                if self.is_blocking_inline(child) {
                    // Inline content ends adjacency: compress what's pending
                    // onto its outermost bottom margin.
                    self.flush_pending(&mut pending);
                    first_at_level = true;
                    continue;
                }
                if !self.is_flow_block(child) {
                    continue;
                }
                if self.is_empty_block(child) {
                    // Reference output drops empty (anchor-only) blocks
                    // entirely: their margins vanish rather than joining
                    // the adjoining set.
                    self.zero(child, Side::Top);
                    self.zero(child, Side::Bottom);
                    continue;
                }

                let mut leading = Vec::new();
                self.margin_chain(child, Side::Top, &mut leading);
                if !pending.is_empty() {
                    let mut set = std::mem::take(&mut pending);
                    set.extend(leading.iter().map(|&n| (n, Side::Top)));
                    self.collapse_set(set, (child, Side::Top));
                } else if first_at_level && leading.len() > 1 {
                    // First block of a barrier-rooted level: compress its
                    // leading chain onto the outermost element.
                    let set: Vec<_> = leading.iter().map(|&n| (n, Side::Top)).collect();
                    self.collapse_set(set, (child, Side::Top));
                }
                first_at_level = false;

                let mut trailing = Vec::new();
                self.margin_chain(child, Side::Bottom, &mut trailing);
                pending = trailing.into_iter().map(|n| (n, Side::Bottom)).collect();
            }
            // Section/container end: keep the compressed bottom margin.
            self.flush_pending(&mut pending);
        }

        for &child in &kids {
            self.process_container(child);
        }
    }

    /// Compress a pending trailing set onto its outermost bottom margin
    /// (the first entry), zeroing the rest.
    fn flush_pending(&mut self, pending: &mut Vec<(NodeId, Side)>) {
        let set = std::mem::take(pending);
        if set.len() < 2 {
            return;
        }
        let carrier = set[0];
        self.collapse_set(set, carrier);
    }
}
