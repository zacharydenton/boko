//! Box model shorthand parsing (margin, padding).

use cssparser::Parser;

use crate::style::properties::Length;

use super::values::parse_length;

/// Parse margin/padding shorthand with 1-4 values.
/// Returns (top, right, bottom, left) following CSS box model rules.
pub(crate) fn parse_box_shorthand_values(
    input: &mut Parser<'_, '_>,
) -> Option<(Length, Length, Length, Length)> {
    let mut values = Vec::with_capacity(4);

    // Parse up to 4 length values
    while values.len() < 4 {
        if let Some(len) = parse_length(input) {
            values.push(len);
        } else {
            break;
        }
    }

    // Expand according to CSS shorthand rules:
    // 1 value: all sides
    // 2 values: top/bottom, left/right
    // 3 values: top, left/right, bottom
    // 4 values: top, right, bottom, left
    expand_shorthand_4(values)
}

/// Expand 1-4 values to (top, right, bottom, left) following CSS shorthand rules.
pub(crate) fn expand_shorthand_4<T: Copy>(values: Vec<T>) -> Option<(T, T, T, T)> {
    match values.len() {
        1 => {
            let v = values[0];
            Some((v, v, v, v))
        }
        2 => {
            let (tb, lr) = (values[0], values[1]);
            Some((tb, lr, tb, lr))
        }
        3 => {
            let (t, lr, b) = (values[0], values[1], values[2]);
            Some((t, lr, b, lr))
        }
        4 => Some((values[0], values[1], values[2], values[3])),
        _ => None,
    }
}
