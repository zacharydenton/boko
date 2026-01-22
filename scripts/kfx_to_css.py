#!/usr/bin/env python3
"""
KFX to CSS Conversion - Convert KFX style fragments to CSS.

This module provides bidirectional mapping between KFX style properties
and CSS. Each KFX style can be converted to a CSS class definition.

Mappings are imported from kfxlib's yj_to_epub_properties.py for accuracy.

Usage:
    from kfx_to_css import kfx_style_to_css, KfxStyleConverter

    css = kfx_style_to_css(style_data)  # Returns CSS properties dict
    css_text = KfxStyleConverter().to_css_class(style_data, "style-1")
"""

import sys
import re
from pathlib import Path
from typing import Dict, Any, Optional, List, Tuple

# =============================================================================
# Import mappings from kfxlib's yj_to_epub_properties.py
# =============================================================================

def _load_yj_mappings():
    """Load property and unit mappings from yj_to_epub_properties.py."""
    script_dir = Path(__file__).parent
    yj_path = script_dir.parent / 'kfxinput' / 'kfxlib' / 'yj_to_epub_properties.py'

    property_to_css = {}
    value_mappings = {}  # property_id -> {value_id: css_value}
    unit_values = {}

    if not yj_path.exists():
        print(f"Warning: {yj_path} not found, using fallback mappings", file=sys.stderr)
        return None, None, None

    content = yj_path.read_text()

    # Parse YJ_PROPERTY_INFO
    prop_pattern = re.compile(r'"\$(\d+)":\s*Prop\("([^"]+)"(?:,\s*\{([^}]*)\})?\)')
    for match in prop_pattern.finditer(content):
        sym_id = int(match.group(1))
        css_name = match.group(2)
        values_str = match.group(3)

        # Skip -kfx- prefixed properties (internal KFX)
        if not css_name.startswith('-kfx-') and not css_name.startswith('-amzn-'):
            property_to_css[sym_id] = css_name

        # Parse value mappings
        if values_str:
            val_pattern = re.compile(r'"\$(\d+)":\s*"([^"]*)"')
            for val_match in val_pattern.finditer(values_str):
                val_id = int(val_match.group(1))
                val_name = val_match.group(2)
                if sym_id not in value_mappings:
                    value_mappings[sym_id] = {}
                value_mappings[sym_id][val_id] = val_name

    # Parse YJ_LENGTH_UNITS
    units_match = re.search(r'YJ_LENGTH_UNITS\s*=\s*\{([^}]+)\}', content, re.DOTALL)
    if units_match:
        unit_pattern = re.compile(r'"\$(\d+)":\s*"([^"]+)"')
        for match in unit_pattern.finditer(units_match.group(1)):
            sym_id = int(match.group(1))
            unit_name = match.group(2)
            unit_values[sym_id] = unit_name

    return property_to_css, value_mappings, unit_values


# Load mappings from authoritative source
_YJ_PROPERTY_TO_CSS, _YJ_VALUE_MAPPINGS, _YJ_UNIT_VALUES = _load_yj_mappings()

# =============================================================================
# KFX Symbol to CSS Property Mapping (from yj_to_epub_properties.py)
# =============================================================================

# Use imported mappings if available, otherwise fallback
if _YJ_PROPERTY_TO_CSS:
    KFX_PROPERTY_TO_CSS = _YJ_PROPERTY_TO_CSS
else:
    # Fallback if yj_to_epub_properties.py not found
    KFX_PROPERTY_TO_CSS = {
        11: "font-family", 12: "font-style", 13: "font-weight", 16: "font-size",
        19: "color", 34: "text-align", 36: "text-indent", 41: "text-transform",
        42: "line-height", 44: "vertical-align", 45: "white-space",
        47: "margin-top", 48: "margin-left", 49: "margin-bottom", 50: "margin-right",
        56: "width", 57: "height", 65: "max-width",
        135: "page-break-inside", 583: "font-variant",
    }

# =============================================================================
# KFX Value Symbol to CSS Value Mapping (from yj_to_epub_properties.py)
# =============================================================================

def _get_values_for_prop(prop_css_name: str) -> Dict[int, str]:
    """Get value mappings for a CSS property from YJ data."""
    if not _YJ_VALUE_MAPPINGS:
        return {}

    # Find property ID by CSS name
    for prop_id, css_name in KFX_PROPERTY_TO_CSS.items():
        if css_name == prop_css_name and prop_id in _YJ_VALUE_MAPPINGS:
            return _YJ_VALUE_MAPPINGS[prop_id]
    return {}


# Build value mapping dicts from imported data or use fallbacks
KFX_TEXT_ALIGN_VALUES = _get_values_for_prop("text-align") or {
    59: "left", 61: "right", 320: "center", 321: "justify",
}

KFX_FONT_WEIGHT_VALUES = _get_values_for_prop("font-weight") or {
    350: "normal", 355: "100", 356: "200", 357: "300",
    359: "500", 360: "600", 361: "bold", 362: "800", 363: "900",
}

KFX_FONT_STYLE_VALUES = _get_values_for_prop("font-style") or {
    350: "normal", 381: "oblique", 382: "italic",
}

KFX_FONT_VARIANT_VALUES = _get_values_for_prop("font-variant") or {
    349: "normal", 369: "small-caps",
}

# Vertical-align uses -kfx-baseline-style in kfxlib
KFX_VERTICAL_ALIGN_VALUES = (_YJ_VALUE_MAPPINGS or {}).get(44, {}) or {
    350: "baseline", 370: "super", 371: "sub",
    447: "text-top", 449: "text-bottom", 320: "middle",
    58: "top", 60: "bottom",
}

KFX_TEXT_TRANSFORM_VALUES = _get_values_for_prop("text-transform") or {
    349: "none", 372: "uppercase", 373: "lowercase", 374: "capitalize",
}

# Box-sizing/image-fit ($546)
KFX_IMAGE_FIT_VALUES = (_YJ_VALUE_MAPPINGS or {}).get(546, {}) or {
    377: "content-box", 378: "border-box", 379: "padding-box",
}

# Break values
KFX_BREAK_VALUES = _get_values_for_prop("page-break-inside") or {
    353: "avoid", 383: "auto",
}

# Unit symbols (from YJ_LENGTH_UNITS)
KFX_UNIT_VALUES = _YJ_UNIT_VALUES or {
    308: "em", 309: "ex", 310: "lh", 314: "%",
    315: "cm", 316: "mm", 317: "in", 318: "pt", 319: "px",
    505: "rem", 506: "ch",
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
