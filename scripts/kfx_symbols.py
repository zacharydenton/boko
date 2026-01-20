#!/usr/bin/env python3
"""
KFX Symbol Name Resolution - Extracts readable names from boko's Rust source.

This module parses the symbol constants from src/kfx/writer.rs and provides
a lookup table for converting symbol IDs (like $492) to readable names
(like METADATA_KEY).
"""

import re
import os
from pathlib import Path

# Cache for parsed symbols
_SYMBOL_NAMES = None
_RUST_SOURCE = None


def _find_rust_source():
    """Find the writer.rs file relative to this script."""
    script_dir = Path(__file__).parent
    # Try relative to scripts directory
    candidates = [
        script_dir.parent / "src" / "kfx" / "writer.rs",
        script_dir / ".." / "src" / "kfx" / "writer.rs",
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


def get_symbol_names():
    """Get the symbol name lookup dictionary."""
    global _SYMBOL_NAMES, _RUST_SOURCE

    if _SYMBOL_NAMES is None:
        _RUST_SOURCE = _find_rust_source()
        _SYMBOL_NAMES = _parse_rust_symbols(_RUST_SOURCE)

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
