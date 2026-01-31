//! Pure markdown generation from IR.
//!
//! This module provides utilities for rendering the internal book representation
//! to Markdown format. The design separates pure rendering logic from I/O:
//!
//! - [`escape`]: Pure string transformation utilities for Markdown escaping
//! - [`slugify`]: GitHub-style slug generation for heading anchors
//! - [`render`]: Core IR → Markdown rendering
//!
//! The export layer ([`crate::export::text`]) handles I/O orchestration, calling
//! these pure functions to generate content.
//!
//! ## Design Notes
//!
//! The rendering follows Pandoc's Markdown writer patterns:
//!
//! - **Text escaping**: Special Markdown characters (`*`, `_`, `[`, `` ` ``, etc.)
//!   are escaped to prevent unintended formatting
//! - **Tight/loose list detection**: Lists with single-paragraph items render
//!   without blank lines between items (tight), while lists with multiple blocks
//!   per item get blank line separation (loose)
//! - **Footnote accumulation**: Footnotes are collected during rendering and
//!   emitted at the end of each chapter as `[^n]: content`
//! - **Dynamic code fence length**: Code blocks use the minimum fence length
//!   (backticks or tildes) that doesn't conflict with content
//! - **Internal link resolution**: Links to headings use GitHub-style slugs
//!   (`#chapter-one`), while other internal links use node IDs (`#c0n42`)

mod escape;
mod render;
mod slugify;

pub use escape::{calculate_fence_length, calculate_inline_code_ticks, escape_markdown};
pub use render::{Footnote, RenderContext, RenderResult, render_chapter};
pub use slugify::{build_heading_slugs, collect_heading_text, slugify};
