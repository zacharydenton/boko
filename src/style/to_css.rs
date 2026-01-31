//! ToCss implementation for ComputedStyle.
//!
//! Uses macros to eliminate boilerplate in CSS property serialization.

use std::fmt::Write;

use super::ToCss;
use super::properties::FontVariant;
use super::types::ComputedStyle;

/// Emit property if different from default.
macro_rules! emit_if_changed {
    ($self:expr, $default:expr, $buf:expr, $field:ident, $css_name:expr) => {
        if $self.$field != $default.$field {
            $buf.push_str($css_name);
            $buf.push_str(": ");
            $self.$field.to_css($buf);
            $buf.push_str("; ");
        }
    };
}

/// Emit optional color if Some.
macro_rules! emit_color_if_some {
    ($self:expr, $buf:expr, $field:ident, $css_name:expr) => {
        if let Some(color) = $self.$field {
            $buf.push_str($css_name);
            $buf.push_str(": ");
            color.to_css($buf);
            $buf.push_str("; ");
        }
    };
}

/// Emit 4-sided property (margin, padding, border-style, border-width).
macro_rules! emit_4sided {
    ($self:expr, $default:expr, $buf:expr,
     $top:ident, $right:ident, $bottom:ident, $left:ident,
     $prefix:expr) => {
        emit_if_changed!($self, $default, $buf, $top, concat!($prefix, "-top"));
        emit_if_changed!($self, $default, $buf, $right, concat!($prefix, "-right"));
        emit_if_changed!($self, $default, $buf, $bottom, concat!($prefix, "-bottom"));
        emit_if_changed!($self, $default, $buf, $left, concat!($prefix, "-left"));
    };
}

/// Emit 4-sided optional color (border-color).
macro_rules! emit_4sided_color {
    ($self:expr, $buf:expr,
     $top:ident, $right:ident, $bottom:ident, $left:ident,
     $prefix:expr) => {
        emit_color_if_some!($self, $buf, $top, concat!($prefix, "-top-color"));
        emit_color_if_some!($self, $buf, $right, concat!($prefix, "-right-color"));
        emit_color_if_some!($self, $buf, $bottom, concat!($prefix, "-bottom-color"));
        emit_color_if_some!($self, $buf, $left, concat!($prefix, "-left-color"));
    };
}

/// Emit 4-corner property (border-radius).
macro_rules! emit_4corner {
    ($self:expr, $default:expr, $buf:expr,
     $tl:ident, $tr:ident, $bl:ident, $br:ident) => {
        emit_if_changed!($self, $default, $buf, $tl, "border-top-left-radius");
        emit_if_changed!($self, $default, $buf, $tr, "border-top-right-radius");
        emit_if_changed!($self, $default, $buf, $bl, "border-bottom-left-radius");
        emit_if_changed!($self, $default, $buf, $br, "border-bottom-right-radius");
    };
}

impl ToCss for ComputedStyle {
    fn to_css(&self, buf: &mut String) {
        let default = ComputedStyle::default();

        // Font properties
        if let Some(ref family) = self.font_family {
            write!(buf, "font-family: {}; ", family).unwrap();
        }
        emit_if_changed!(self, default, buf, font_size, "font-size");
        emit_if_changed!(self, default, buf, font_weight, "font-weight");
        emit_if_changed!(self, default, buf, font_style, "font-style");

        // Colors
        emit_color_if_some!(self, buf, color, "color");
        emit_color_if_some!(self, buf, background_color, "background-color");

        // Text properties
        emit_if_changed!(self, default, buf, text_align, "text-align");
        emit_if_changed!(self, default, buf, text_indent, "text-indent");
        emit_if_changed!(self, default, buf, line_height, "line-height");

        // Text decorations (special handling for combined value)
        let mut decorations = Vec::new();
        if self.text_decoration_underline {
            decorations.push("underline");
        }
        if self.text_decoration_line_through {
            decorations.push("line-through");
        }
        if !decorations.is_empty() {
            write!(buf, "text-decoration: {}; ", decorations.join(" ")).unwrap();
        }

        // Display
        emit_if_changed!(self, default, buf, display, "display");

        // Margins (4-sided)
        emit_4sided!(
            self,
            default,
            buf,
            margin_top,
            margin_right,
            margin_bottom,
            margin_left,
            "margin"
        );

        // Padding (4-sided)
        emit_4sided!(
            self,
            default,
            buf,
            padding_top,
            padding_right,
            padding_bottom,
            padding_left,
            "padding"
        );

        // Vertical alignment
        emit_if_changed!(self, default, buf, vertical_align, "vertical-align");

        // List style
        emit_if_changed!(self, default, buf, list_style_type, "list-style-type");

        // Font variant (uses FontVariant::Normal directly for comparison)
        if self.font_variant != FontVariant::Normal {
            buf.push_str("font-variant: ");
            self.font_variant.to_css(buf);
            buf.push_str("; ");
        }

        // Text spacing
        emit_if_changed!(self, default, buf, letter_spacing, "letter-spacing");
        emit_if_changed!(self, default, buf, word_spacing, "word-spacing");

        // Text transform
        emit_if_changed!(self, default, buf, text_transform, "text-transform");

        // Hyphenation
        emit_if_changed!(self, default, buf, hyphens, "hyphens");

        // White-space
        emit_if_changed!(self, default, buf, white_space, "white-space");

        // Underline style
        emit_if_changed!(self, default, buf, underline_style, "text-decoration-style");

        // Overline (special handling)
        if self.overline {
            buf.push_str("text-decoration-line: overline; ");
        }

        // Underline color
        emit_color_if_some!(self, buf, underline_color, "text-decoration-color");

        // Layout dimensions
        emit_if_changed!(self, default, buf, width, "width");
        emit_if_changed!(self, default, buf, height, "height");
        emit_if_changed!(self, default, buf, max_width, "max-width");
        emit_if_changed!(self, default, buf, min_height, "min-height");

        // Float
        emit_if_changed!(self, default, buf, float, "float");

        // Page breaks
        emit_if_changed!(self, default, buf, break_before, "break-before");
        emit_if_changed!(self, default, buf, break_after, "break-after");
        emit_if_changed!(self, default, buf, break_inside, "break-inside");

        // Border styles (4-sided)
        emit_4sided!(
            self,
            default,
            buf,
            border_style_top,
            border_style_right,
            border_style_bottom,
            border_style_left,
            "border-style"
        );

        // Border widths (4-sided)
        emit_4sided!(
            self,
            default,
            buf,
            border_width_top,
            border_width_right,
            border_width_bottom,
            border_width_left,
            "border-width"
        );

        // Border colors (4-sided optional)
        emit_4sided_color!(
            self,
            buf,
            border_color_top,
            border_color_right,
            border_color_bottom,
            border_color_left,
            "border"
        );

        // Border radius (4-corner)
        emit_4corner!(
            self,
            default,
            buf,
            border_radius_top_left,
            border_radius_top_right,
            border_radius_bottom_left,
            border_radius_bottom_right
        );

        // List style position
        emit_if_changed!(
            self,
            default,
            buf,
            list_style_position,
            "list-style-position"
        );

        // Visibility
        emit_if_changed!(self, default, buf, visibility, "visibility");

        // Note: language is stored but typically output via HTML lang attribute
    }
}
