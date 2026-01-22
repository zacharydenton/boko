#!/usr/bin/env python3
"""
KFX to CSS Conversion - Convert KFX style fragments to CSS.

This module provides bidirectional mapping between KFX style properties
and CSS. Each KFX style can be converted to a CSS class definition.

Usage:
    from kfx_to_css import kfx_style_to_css, KfxStyleConverter

    css = kfx_style_to_css(style_data)  # Returns CSS properties dict
    css_text = KfxStyleConverter().to_css_class(style_data, "style-1")
"""

from typing import Dict, Any, Optional, List, Tuple

# =============================================================================
# KFX Symbol to CSS Property Mapping
# =============================================================================

# KFX property symbols -> CSS property names
KFX_PROPERTY_TO_CSS = {
    # Font properties
    11: "font-family",      # $11 FONT_FAMILY
    12: "font-style",       # $12 FONT_STYLE
    13: "font-weight",      # $13 FONT_WEIGHT
    16: "font-size",        # $16 FONT_SIZE
    583: "font-variant",    # $583 FONT_VARIANT

    # Text properties
    19: "color",            # $19 COLOR
    34: "text-align",       # $34 TEXT_ALIGN
    36: "text-indent",      # $36 TEXT_INDENT
    41: "text-transform",   # $41 TEXT_TRANSFORM
    42: "line-height",      # $42 LINE_HEIGHT
    44: "vertical-align",   # $44 VERTICAL_ALIGN
    45: "white-space",      # $45 WHITE_SPACE (nowrap)

    # Spacing/margins
    47: "margin-top",       # $47 SPACE_BEFORE
    48: "margin-left",      # $48 MARGIN_LEFT
    49: "margin-bottom",    # $49 SPACE_AFTER
    50: "margin-right",     # $50 MARGIN_RIGHT

    # Dimensions
    56: "width",            # $56 STYLE_WIDTH
    57: "height",           # $57 STYLE_HEIGHT
    65: "max-width",        # $65 MAX_WIDTH
    66: "max-height",       # $66 MAX_HEIGHT
    67: "min-width",        # $67 MIN_WIDTH
    68: "min-height",       # $68 MIN_HEIGHT

    # Image properties
    546: "object-fit",      # $546 IMAGE_FIT
    580: "object-position", # $580 IMAGE_LAYOUT (approximation)

    # Text decoration
    20: "text-decoration",  # $20 TEXT_DECORATION_UNDERLINE (partial)
    21: "text-decoration",  # $21 TEXT_DECORATION_OVERLINE (partial)
    22: "text-decoration",  # $22 TEXT_DECORATION_LINE_THROUGH (partial)

    # Break properties
    135: "break-inside",    # $135 BREAK_INSIDE
    788: "break-after",     # $788 BREAK_AFTER

    # Language
    10: "lang",             # $10 LANGUAGE (as attribute, not CSS)

    # Block type (informational, not direct CSS)
    127: "-kfx-block-type", # $127 STYLE_BLOCK_TYPE
    173: "-kfx-style-name", # $173 STYLE_NAME
}

# =============================================================================
# KFX Value Symbol to CSS Value Mapping
# =============================================================================

# Text-align values
KFX_TEXT_ALIGN_VALUES = {
    59: "left",       # $59 ALIGN_LEFT
    61: "right",      # $61 ALIGN_RIGHT
    320: "center",    # $320 ALIGN_CENTER
    321: "justify",   # $321 ALIGN_JUSTIFY
}

# Font-weight values
KFX_FONT_WEIGHT_VALUES = {
    350: "normal",    # $350 FONT_WEIGHT_NORMAL (400)
    355: "100",       # $355
    356: "200",       # $356
    357: "300",       # $357
    359: "500",       # $359
    360: "600",       # $360
    361: "bold",      # $361 FONT_WEIGHT_BOLD (700)
    362: "800",       # $362
    363: "900",       # $363
}

# Font-style values
KFX_FONT_STYLE_VALUES = {
    350: "normal",    # $350 (shared with font-weight)
    381: "oblique",   # $381 FONT_STYLE_OBLIQUE
    382: "italic",    # $382 FONT_STYLE_ITALIC
}

# Font-variant values
KFX_FONT_VARIANT_VALUES = {
    369: "small-caps",  # $369 FONT_VARIANT_SMALL_CAPS
}

# Vertical-align values
KFX_VERTICAL_ALIGN_VALUES = {
    350: "baseline",   # $350
    370: "super",      # $370 VERTICAL_SUPER
    371: "sub",        # $371 VERTICAL_SUB
    372: "text-top",   # $372 VERTICAL_TEXT_TOP
    373: "text-bottom",# $373 VERTICAL_TEXT_BOTTOM
    320: "middle",     # $320 (shared)
}

# Text-transform values
KFX_TEXT_TRANSFORM_VALUES = {
    349: "none",       # $349 TEXT_TRANSFORM_NONE
    373: "lowercase",  # $373
    374: "uppercase",  # $374
    375: "capitalize", # $375
}

# Image-fit values (object-fit)
KFX_IMAGE_FIT_VALUES = {
    377: "contain",    # $377 IMAGE_FIT_CONTAIN
    378: "cover",      # $378 IMAGE_FIT_COVER
    379: "fill",       # $379 IMAGE_FIT_FILL
}

# Break values
KFX_BREAK_VALUES = {
    353: "avoid",      # $353 BREAK_AVOID
    383: "auto",       # $383 BLOCK_TYPE_BLOCK (default)
}

# Unit symbols
KFX_UNIT_VALUES = {
    308: "em",         # $308 UNIT_EM
    310: "em",         # $310 UNIT_MULTIPLIER (treat as em for margins)
    314: "%",          # $314 UNIT_PERCENT
    318: "px",         # $318 UNIT_PX
    505: "em",         # $505 UNIT_EM_FONTSIZE
}

# Properties where UNIT_MULTIPLIER should be unitless (like line-height)
UNITLESS_MULTIPLIER_PROPS = {42}  # LINE_HEIGHT

# =============================================================================
# Converter Class
# =============================================================================

class KfxStyleConverter:
    """Convert KFX style fragments to CSS."""

    def __init__(self):
        self.text_decorations = []  # Accumulate text-decoration values

    def _parse_symbol(self, val: Any) -> Optional[int]:
        """Extract symbol number from various formats."""
        if isinstance(val, int):
            return val
        s = str(val)
        if s.startswith('$'):
            try:
                return int(s[1:])
            except ValueError:
                pass
        return None

    def _parse_value_with_unit(self, val: Dict, prop_sym: int = 0) -> Optional[str]:
        """Parse a KFX value struct with unit and value."""
        if not isinstance(val, dict):
            return None

        # Get unit and value
        unit_val = val.get(306) or val.get('$306')  # UNIT
        num_val = val.get(307) or val.get('$307')   # VALUE

        if num_val is None:
            return None

        # Parse unit symbol
        unit_sym = self._parse_symbol(unit_val)
        unit_str = KFX_UNIT_VALUES.get(unit_sym, "")

        # UNIT_MULTIPLIER is unitless for line-height but em for margins
        if unit_sym == 310 and prop_sym in UNITLESS_MULTIPLIER_PROPS:
            unit_str = ""

        # Format number
        if isinstance(num_val, float):
            # Remove trailing zeros
            num_str = f"{num_val:.6f}".rstrip('0').rstrip('.')
        else:
            num_str = str(num_val)

        return f"{num_str}{unit_str}"

    def _convert_color(self, val: int) -> str:
        """Convert KFX color (0x00RRGGBB) to CSS."""
        r = (val >> 16) & 0xFF
        g = (val >> 8) & 0xFF
        b = val & 0xFF
        if r == 0 and g == 0 and b == 0:
            return "black"
        if r == 255 and g == 255 and b == 255:
            return "white"
        return f"#{r:02x}{g:02x}{b:02x}"

    def _convert_property(self, prop_sym: int, val: Any) -> Optional[Tuple[str, str]]:
        """Convert a single KFX property to CSS property: value."""
        css_prop = KFX_PROPERTY_TO_CSS.get(prop_sym)
        if not css_prop:
            return None

        # Handle different property types
        if prop_sym == 10:  # LANGUAGE - not CSS, skip
            return None

        if prop_sym == 173:  # STYLE_NAME - informational only
            return None

        if prop_sym == 127:  # STYLE_BLOCK_TYPE - informational
            return None

        if prop_sym == 19:  # COLOR
            if isinstance(val, int):
                return (css_prop, self._convert_color(val))
            return None

        if prop_sym == 34:  # TEXT_ALIGN
            sym = self._parse_symbol(val)
            css_val = KFX_TEXT_ALIGN_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 12:  # FONT_STYLE
            sym = self._parse_symbol(val)
            css_val = KFX_FONT_STYLE_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 13:  # FONT_WEIGHT
            sym = self._parse_symbol(val)
            css_val = KFX_FONT_WEIGHT_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 583:  # FONT_VARIANT
            sym = self._parse_symbol(val)
            css_val = KFX_FONT_VARIANT_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 44:  # VERTICAL_ALIGN
            sym = self._parse_symbol(val)
            css_val = KFX_VERTICAL_ALIGN_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 41:  # TEXT_TRANSFORM
            sym = self._parse_symbol(val)
            css_val = KFX_TEXT_TRANSFORM_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 546:  # IMAGE_FIT
            sym = self._parse_symbol(val)
            css_val = KFX_IMAGE_FIT_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym in (135, 788):  # BREAK_INSIDE, BREAK_AFTER
            sym = self._parse_symbol(val)
            css_val = KFX_BREAK_VALUES.get(sym)
            if css_val:
                return (css_prop, css_val)
            return None

        if prop_sym == 45:  # WHITE_SPACE_NOWRAP
            if val:
                return (css_prop, "nowrap")
            return None

        if prop_sym == 11:  # FONT_FAMILY
            if isinstance(val, str):
                return (css_prop, val)
            return None

        # Properties with unit+value struct
        if prop_sym in (16, 36, 42, 47, 48, 49, 50, 56, 57, 65, 66, 67, 68):
            if isinstance(val, dict):
                css_val = self._parse_value_with_unit(val, prop_sym)
                if css_val:
                    return (css_prop, css_val)
            return None

        # Text decoration - accumulate
        if prop_sym in (20, 21, 22):
            return None  # Handled separately

        return None

    def to_css_properties(self, style_data: Dict) -> Dict[str, str]:
        """Convert KFX style data to CSS properties dict."""
        result = {}

        for key, val in style_data.items():
            # Parse key as symbol
            if isinstance(key, str) and key.startswith('$'):
                prop_sym = int(key[1:])
            elif isinstance(key, int):
                prop_sym = key
            else:
                continue

            converted = self._convert_property(prop_sym, val)
            if converted:
                prop, value = converted
                result[prop] = value

        return result

    def to_css_string(self, style_data: Dict) -> str:
        """Convert KFX style data to CSS property declarations string."""
        props = self.to_css_properties(style_data)
        if not props:
            return ""

        # Sort for consistent output
        parts = [f"{k}: {v}" for k, v in sorted(props.items())]
        return "; ".join(parts)

    def to_css_class(self, style_data: Dict, class_name: str) -> str:
        """Convert KFX style data to a full CSS class definition."""
        css_str = self.to_css_string(style_data)
        if not css_str:
            return f".{class_name} {{ }}"
        return f".{class_name} {{ {css_str}; }}"


def kfx_style_to_css(style_data: Dict) -> Dict[str, str]:
    """Convenience function to convert KFX style to CSS properties."""
    return KfxStyleConverter().to_css_properties(style_data)


def kfx_style_to_css_string(style_data: Dict) -> str:
    """Convenience function to convert KFX style to CSS string."""
    return KfxStyleConverter().to_css_string(style_data)


# =============================================================================
# CLI for testing
# =============================================================================

if __name__ == "__main__":
    # Test with sample KFX style data
    test_style = {
        173: "$style-1",           # STYLE_NAME
        34: "$321",                # TEXT_ALIGN: justify
        16: {306: "$505", 307: 1.17},  # FONT_SIZE: 1.17em
        42: {306: "$310", 307: 1.2},   # LINE_HEIGHT: 1.2
        47: {306: "$310", 307: 1},     # MARGIN_TOP: 1
        13: "$361",                # FONT_WEIGHT: bold
        12: "$382",                # FONT_STYLE: italic
    }

    converter = KfxStyleConverter()

    print("Test KFX style conversion:")
    print(f"  Input: {test_style}")
    print(f"  CSS properties: {converter.to_css_properties(test_style)}")
    print(f"  CSS string: {converter.to_css_string(test_style)}")
    print(f"  CSS class: {converter.to_css_class(test_style, 'test-style')}")
