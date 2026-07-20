//! KFX storyline parsing and IR building.
//!
//! This module handles bidirectional conversion between KFX storyline
//! structures and boko's IR, using a schema-driven approach:
//!
//! Import: Ion → TokenStream → IR
//! Export: IR → TokenStream → Ion
//!
//! ## Key Design: Generic Interpreter
//!
//! The interpreter is completely generic - it knows nothing about KFX semantics.
//! All mapping logic is driven by the schema:
//!
//! 1. Read element type symbol ID
//! 2. Fetch Strategy from schema
//! 3. Execute Strategy to determine role
//! 4. Extract ALL attributes using schema's AttrRules
//! 5. Apply transformers to convert values

use crate::kfx::container::get_field;
use crate::kfx::context::ExportContext;
use crate::kfx::ion::IonValue;
use crate::kfx::schema::{SemanticTarget, schema};
use crate::kfx::symbols::KfxSymbol;
use crate::kfx::tokens::{ContentRef, ElementStart, KfxToken, SpanStart, TokenStream};
use crate::kfx::transforms::ImportContext;
use crate::model::Role;
use crate::model::{Chapter, Node, NodeId};
use crate::style::{BorderStyle, ComputedStyle, Length};
use std::collections::HashMap;

/// Shorthand for getting a KfxSymbol as u64.
macro_rules! sym {
    ($variant:ident) => {
        KfxSymbol::$variant as u64
    };
}

/// Build a storyline Ion structure from an IR chapter.
///
/// **Note**: This is now internal - use `build_chapter_entities_grouped` for
/// the full three-entity architecture (Content, Storyline, Section).
pub fn build_storyline_ion(chapter: &Chapter, ctx: &mut ExportContext) -> IonValue {
    let tokens = ir_to_tokens(chapter, ctx);
    tokens_to_ion(&tokens, ctx)
}

mod collapse;
mod export;
mod import;
mod ion_synth;
#[cfg(test)]
mod tests;

pub use collapse::{MarginAdjust, MarginAdjustMap, compute_margin_collapse};
pub use export::ir_to_tokens;
pub use import::{build_ir_from_tokens, parse_storyline_to_ir, tokenize_storyline};
pub use ion_synth::tokens_to_ion;
