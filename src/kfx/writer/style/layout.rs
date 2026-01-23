//! Layout property conversion (dimensions, box model, visibility).

use std::collections::HashMap;

use crate::css::{Clear, CssValue, Overflow, ParsedStyle, Visibility, WordBreak};
use crate::kfx::ion::{IonValue, encode_kfx_decimal};
use crate::kfx::writer::symbols::sym;

use super::{ToKfxIon, break_value_to_symbol};

/// Add all layout properties to the style ION struct
pub fn add_all(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    add_dimensions(style_ion, style);
    add_min_max_dimensions(style_ion, style);
    add_clear(style_ion, style);
    add_word_break(style_ion, style);
    add_overflow(style_ion, style);
    add_visibility(style_ion, style);
    add_break_properties(style_ion, style);
}

/// Add width and height properties
pub fn add_dimensions(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref width) = style.width {
        let width_val = match width {
            CssValue::Percent(pct) => {
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*pct)));
                Some(IonValue::Struct(s))
            }
            CssValue::Em(v) | CssValue::Rem(v) => {
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                Some(IonValue::Struct(s))
            }
            CssValue::Px(v) => {
                let pct = *v * 0.117;
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                Some(IonValue::Struct(s))
            }
            // Handle viewport and other new units via ToKfxIon trait
            _ => width.to_kfx_ion(),
        };
        if let Some(val) = width_val {
            style_ion.insert(sym::STYLE_WIDTH, val);
        }
    }
    if let Some(ref height) = style.height {
        let height_val = match height {
            CssValue::Percent(pct) => {
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*pct)));
                Some(IonValue::Struct(s))
            }
            _ => height.to_kfx_ion(),
        };
        if let Some(val) = height_val {
            style_ion.insert(sym::STYLE_HEIGHT, val);
        }
    }
}

/// Add min/max width and height properties
pub fn add_min_max_dimensions(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref val) = style.min_width
        && let Some(ion_val) = val.to_kfx_ion()
    {
        style_ion.insert(sym::MIN_WIDTH, ion_val);
    }
    if let Some(ref val) = style.min_height
        && let Some(ion_val) = val.to_kfx_ion()
    {
        style_ion.insert(sym::MIN_HEIGHT, ion_val);
    }
    if let Some(ref val) = style.max_width
        && let Some(ion_val) = val.to_kfx_ion()
    {
        style_ion.insert(sym::MAX_WIDTH, ion_val);
    }
    if let Some(ref val) = style.max_height
        && let Some(ion_val) = val.to_kfx_ion()
    {
        style_ion.insert(sym::STYLE_HEIGHT, ion_val);
    }
}

/// Add clear property
pub fn add_clear(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(clear) = style.clear {
        let clear_sym = match clear {
            Clear::None => sym::TEXT_TRANSFORM_NONE,
            Clear::Left => sym::ALIGN_LEFT,
            Clear::Right => sym::ALIGN_RIGHT,
            Clear::Both => sym::CLEAR_BOTH,
        };
        if clear != Clear::None {
            style_ion.insert(sym::CLEAR, IonValue::Symbol(clear_sym));
        }
    }
}

/// Add word-break property
pub fn add_word_break(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(word_break) = style.word_break {
        let break_sym = match word_break {
            WordBreak::Normal => sym::FONT_WEIGHT_NORMAL,
            WordBreak::BreakAll => sym::WORD_BREAK_ALL,
            WordBreak::KeepAll => sym::FONT_WEIGHT_NORMAL,
        };
        if word_break != WordBreak::Normal {
            style_ion.insert(sym::WORD_BREAK, IonValue::Symbol(break_sym));
        }
    }
}

/// Add overflow property
pub fn add_overflow(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(overflow) = style.overflow {
        if matches!(overflow, Overflow::Hidden | Overflow::Clip) {
            style_ion.insert(sym::OVERFLOW_CLIP, IonValue::Bool(true));
        }
    }
}

/// Add visibility property
pub fn add_visibility(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(visibility) = style.visibility {
        style_ion.insert(
            sym::VISIBILITY,
            IonValue::Bool(visibility == Visibility::Visible),
        );
    }
}

/// Add break-before, break-after, break-inside properties
pub fn add_break_properties(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Use legacy page-break-* symbols ($133-$135) per KFX spec
    if let Some(break_val) = style.break_before {
        let break_sym = break_value_to_symbol(break_val);
        style_ion.insert(sym::PAGE_BREAK_BEFORE, IonValue::Symbol(break_sym));
    }
    if let Some(break_val) = style.break_after {
        let break_sym = break_value_to_symbol(break_val);
        style_ion.insert(sym::PAGE_BREAK_AFTER, IonValue::Symbol(break_sym));
    }
    if let Some(break_val) = style.break_inside {
        let break_sym = break_value_to_symbol(break_val);
        style_ion.insert(sym::BREAK_INSIDE, IonValue::Symbol(break_sym));
    }
}
