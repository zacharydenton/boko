#!/usr/bin/env python3
"""
KFX Symbol Name Resolution - Extracts readable names from multiple sources.

This module provides symbol name lookup by combining:
1. Rust symbol constants from src/kfx/writer/symbols.rs (structural names)
2. CSS property names from kfxlib's yj_to_epub_properties.py (CSS names)

Symbol IDs (like $492) are converted to readable names (like METADATA_KEY).
"""

import re
import os
from pathlib import Path

# Cache for parsed symbols
_SYMBOL_NAMES = None
_RUST_SOURCE = None
_CSS_PROPERTY_NAMES = None


def _find_rust_source():
    """Find the symbols.rs file relative to this script."""
    script_dir = Path(__file__).parent
    # Try relative to scripts directory - check symbols.rs first (refactored location)
    candidates = [
        script_dir.parent / "src" / "kfx" / "writer" / "symbols.rs",
        script_dir.parent / "src" / "kfx" / "writer.rs",
        script_dir / ".." / "src" / "kfx" / "writer" / "symbols.rs",
        script_dir / ".." / "src" / "kfx" / "writer.rs",
    ]
    for path in candidates:
        if path.exists():
            return path.resolve()
    return None


def _find_yj_properties():
    """Find yj_to_epub_properties.py from kfxlib."""
    script_dir = Path(__file__).parent
    candidates = [
        script_dir.parent / "kfxinput" / "kfxlib" / "yj_to_epub_properties.py",
        script_dir / ".." / "kfxinput" / "kfxlib" / "yj_to_epub_properties.py",
    ]
    for path in candidates:
        if path.exists():
            return path.resolve()
    return None


def _parse_rust_symbols(source_path):
    """Parse pub const declarations from Rust source."""
    symbols = {}

    if not source_path or not os.path.exists(source_path):
        return symbols

    with open(source_path, "r") as f:
        content = f.read()

    # Match: pub const NAME: u64 = NUMBER; // comment
    pattern = r'pub const ([A-Z_0-9]+): u64 = (\d+);'

    for match in re.finditer(pattern, content):
        name = match.group(1)
        num = int(match.group(2))
        symbol = f"${num}"

        # Don't overwrite if we already have a name (first one wins)
        if symbol not in symbols:
            symbols[symbol] = name

    return symbols


def _parse_yj_properties(source_path):
    """Parse CSS property names from yj_to_epub_properties.py."""
    css_names = {}

    if not source_path or not os.path.exists(source_path):
        return css_names

    with open(source_path, "r") as f:
        content = f.read()

    # Match: "$123": Prop("css-property-name", ...)
    pattern = r'"\$(\d+)":\s*Prop\("([^"]+)"'

    for match in re.finditer(pattern, content):
        num = int(match.group(1))
        css_name = match.group(2)
        symbol = f"${num}"

        # Store CSS property name
        css_names[symbol] = css_name

    # Also parse YJ_LENGTH_UNITS
    units_match = re.search(r'YJ_LENGTH_UNITS\s*=\s*\{([^}]+)\}', content, re.DOTALL)
    if units_match:
        unit_pattern = re.compile(r'"\$(\d+)":\s*"([^"]+)"')
        for match in unit_pattern.finditer(units_match.group(1)):
            num = int(match.group(1))
            unit_name = match.group(2)
            css_names[f"${num}"] = f"UNIT_{unit_name.upper()}"

    return css_names


def get_css_property_names():
    """Get CSS property names from yj_to_epub_properties.py."""
    global _CSS_PROPERTY_NAMES

    if _CSS_PROPERTY_NAMES is None:
        yj_path = _find_yj_properties()
        _CSS_PROPERTY_NAMES = _parse_yj_properties(yj_path)

    return _CSS_PROPERTY_NAMES


def get_symbol_names():
    """Get the symbol name lookup dictionary (Rust names preferred)."""
    global _SYMBOL_NAMES, _RUST_SOURCE

    if _SYMBOL_NAMES is None:
        _RUST_SOURCE = _find_rust_source()
        _SYMBOL_NAMES = _parse_rust_symbols(_RUST_SOURCE)

        # Merge in CSS property names for symbols we don't have
        css_names = get_css_property_names()
        for sym, css_name in css_names.items():
            if sym not in _SYMBOL_NAMES:
                # Convert css-name to CSS_NAME format
                rust_style = css_name.replace('-', '_').upper()
                _SYMBOL_NAMES[sym] = rust_style

    return _SYMBOL_NAMES


def symbol_name(sym):
    """Convert a symbol ID to a readable name.

    Args:
        sym: Symbol like "$492" or IonSymbol object

    Returns:
        Readable name like "METADATA_KEY" or original if not found
    """
    names = get_symbol_names()
    sym_str = str(sym)

    if sym_str in names:
        return names[sym_str]

    # Handle IonSymbol objects that might have different string repr
    if hasattr(sym, 'tostring'):
        sym_str = sym.tostring()
        if sym_str in names:
            return names[sym_str]

    return sym_str


def format_symbol(sym):
    """Format a symbol with both ID and name for display.

    Args:
        sym: Symbol like "$492"

    Returns:
        String like "$492 (METADATA_KEY)" or just "$492" if no name found
    """
    names = get_symbol_names()
    sym_str = str(sym)

    if sym_str in names:
        return f"{sym_str} ({names[sym_str]})"

    return sym_str


def get_source_path():
    """Get the path to the Rust source file being used."""
    global _RUST_SOURCE
    if _RUST_SOURCE is None:
        get_symbol_names()  # This will populate _RUST_SOURCE
    return _RUST_SOURCE


# Pre-defined common symbols for quick reference (subset)
COMMON_SYMBOLS = {
    # Fragment types
    "$145": "TEXT_CONTENT",
    "$157": "STYLE",
    "$164": "RESOURCE",
    "$258": "METADATA",
    "$259": "CONTENT_BLOCK",
    "$260": "SECTION",
    "$264": "POSITION_MAP",
    "$265": "POSITION_ID_MAP",
    "$266": "PAGE_TEMPLATE/ANCHOR",
    "$270": "CONTAINER_INFO",
    "$389": "BOOK_NAVIGATION",
    "$391": "NAV_CONTAINER_TYPE",
    "$395": "NAV_UNIT_LIST",
    "$417": "RAW_MEDIA",
    "$419": "CONTAINER_ENTITY_MAP",
    "$490": "KINDLE_METADATA",
    "$538": "DOCUMENT_DATA",
    "$550": "LOCATION_MAP",
    "$585": "FORMAT_CAPABILITIES_OLD",
    "$593": "FORMAT_CAPABILITIES",
    "$597": "AUXILIARY_DATA",

    # Common field keys
    "$4": "ID",
    "$10": "LANGUAGE",
    "$141": "SECTION_CONTENT",
    "$142": "INLINE_STYLE_RUNS",
    "$143": "OFFSET",
    "$144": "COUNT/TEXT",
    "$146": "CONTENT_ARRAY",
    "$155": "POSITION/EID",
    "$159": "CONTENT_TYPE",
    "$161": "FORMAT",
    "$165": "LOCATION",
    "$169": "READING_ORDERS",
    "$170": "SECTIONS_LIST",
    "$173": "STYLE_NAME",
    "$174": "SECTION_NAME",
    "$175": "RESOURCE_NAME",
    "$176": "CONTENT_NAME",
    "$178": "READING_ORDER_NAME",
    "$179": "ANCHOR_REF",
    "$180": "TEMPLATE_NAME/ANCHOR_ID",
    "$183": "POSITION_INFO",
    "$186": "EXTERNAL_URL",
    "$235": "NAV_TYPE",
    "$241": "NAV_TITLE",
    "$246": "NAV_TARGET",
    "$247": "NAV_ENTRIES",
    "$306": "UNIT",
    "$307": "VALUE",
    "$392": "NAV_CONTAINER_REF",
    "$491": "METADATA_ENTRIES",
    "$492": "METADATA_KEY",
    "$495": "METADATA_GROUP",

    # Style properties
    "$11": "FONT_FAMILY",
    "$12": "FONT_STYLE",
    "$13": "FONT_WEIGHT",
    "$16": "FONT_SIZE",
    "$19": "COLOR",
    "$34": "TEXT_ALIGN",
    "$36": "TEXT_INDENT",
    "$42": "LINE_HEIGHT",
    "$47": "SPACE_BEFORE/MARGIN_TOP",
    "$48": "MARGIN_LEFT",
    "$50": "MARGIN_RIGHT",
    "$56": "STYLE_WIDTH",
    "$57": "STYLE_HEIGHT",

    # Units
    "$308": "UNIT_EM",
    "$310": "UNIT_MULTIPLIER",
    "$314": "UNIT_PERCENT",
    "$318": "UNIT_PX",

    # Content types
    "$269": "CONTENT_PARAGRAPH",
    "$271": "IMAGE_CONTENT",

    # Content item fields
    "$790": "CONTENT_ROLE",  # 2=first, 3=normal, 4=endnote

    # Values
    "$320": "ALIGN_CENTER",
    "$321": "ALIGN_JUSTIFY",
    "$348": "SINGLETON_ID",
    "$349": "NORMAL/NONE",
    "$350": "FONT_WEIGHT_NORMAL",
    "$351": "DEFAULT_READING_ORDER",
    "$361": "FONT_WEIGHT_BOLD",
    "$377": "IMAGE_FIT_CONTAIN",
    "$382": "FONT_STYLE_ITALIC",
    "$383": "BLOCK_TYPE_BLOCK",
}


if __name__ == "__main__":
    # Test the module
    names = get_symbol_names()
    print(f"Loaded {len(names)} symbols from {get_source_path()}")
    print("\nSample symbols:")
    for sym in ["$492", "$307", "$155", "$145", "$157", "$490"]:
        print(f"  {sym} -> {symbol_name(sym)}")
