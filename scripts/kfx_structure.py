#!/usr/bin/env python3
"""
Visualize the nesting structure of KFX content blocks.

Usage:
    python scripts/kfx_structure.py <kfx_file> [--text SEARCH] [--index N]

Examples:
    python scripts/kfx_structure.py book.kfx --text "Standard Ebooks logo"
    python scripts/kfx_structure.py book.kfx --index 2
"""

import sys
import os
import argparse
from pathlib import Path

# Add kfxlib to path
SCRIPT_DIR = Path(__file__).parent
KFXLIB_PATH = os.environ.get('KFXLIB_PATH', str(SCRIPT_DIR))
sys.path.insert(0, KFXLIB_PATH)

try:
    from kfxlib.ion_binary import IonBinary
    from kfxlib.yj_symbol_catalog import YJ_SYMBOLS
except ImportError as e:
    print(f"Error: Could not import kfxlib from {KFXLIB_PATH}")
    print(f"Set KFXLIB_PATH environment variable to the kfxlib directory")
    sys.exit(1)


def load_kfx_fragments(filepath):
    """Load fragments from a KFX file."""
    with open(filepath, 'rb') as f:
        data = f.read()

    if data[:4] != b'CONT':
        raise ValueError(f"Not a valid KFX container")

    # Parse header
    header_len = int.from_bytes(data[6:10], 'little')
    ci_offset = int.from_bytes(data[10:14], 'little')
    ci_len = int.from_bytes(data[14:18], 'little')

    # Symbol table wrapper
    class SymbolTableWrapper:
        def __init__(self, shared):
            self.shared = shared
            self.local_symbols = {}
            self.shared_base = 10

        def get_symbol(self, sid):
            if sid in self.local_symbols:
                return self.local_symbols[sid]
            idx = sid - self.shared_base
            if 0 <= idx < len(self.shared.symbols):
                sym = self.shared.symbols[idx]
                return f"${sid}" if sym is None else sym
            return f"${sid}"

        def add_symbol(self, sid, name):
            self.local_symbols[sid] = name

    symtab = SymbolTableWrapper(YJ_SYMBOLS)
    ion = IonBinary()
    ion.symtab = symtab

    # Parse container info
    ci_data = data[ci_offset:ci_offset+ci_len]
    ci = ion.deserialize_single_value(ci_data, 0)

    index_offset = ci.get('$413')
    index_length = ci.get('$414')

    if index_offset is None or index_length is None:
        raise ValueError("Could not find entity index table")

    # Parse entity index
    entry_size = 24
    num_entries = index_length // entry_size
    payload_start = header_len

    fragments = []
    for i in range(num_entries):
        idx_pos = index_offset + i * entry_size
        eid = int.from_bytes(data[idx_pos:idx_pos+4], 'little')
        etype = int.from_bytes(data[idx_pos+4:idx_pos+8], 'little')
        eoffset = int.from_bytes(data[idx_pos+8:idx_pos+16], 'little')
        elength = int.from_bytes(data[idx_pos+16:idx_pos+24], 'little')

        entity_data = data[payload_start + eoffset : payload_start + eoffset + elength]

        if entity_data[:4] == b'ENTY':
            ent_header_len = int.from_bytes(entity_data[6:10], 'little')
            entity_data = entity_data[ent_header_len:]

        try:
            ion2 = IonBinary()
            ion2.symtab = symtab
            value = ion2.deserialize_single_value(entity_data, 0)
            fragments.append({
                'fid': f"${eid}",
                'ftype': f"${etype}",
                'value': value
            })
        except:
            pass

    return fragments


def search_value(val, text):
    """Search for text in a value recursively."""
    if isinstance(val, str):
        return text.lower() in val.lower()
    elif isinstance(val, dict):
        return any(search_value(v, text) for v in val.values())
    elif isinstance(val, list):
        return any(search_value(v, text) for v in val)
    return False


def show_structure(val, indent=0, max_depth=10):
    """Print the nesting structure of a content block."""
    if indent > max_depth:
        print("  " * indent + "...")
        return

    prefix = "  " * indent

    if isinstance(val, dict):
        # Check what kind of item this is
        has_146 = '$146' in val
        has_145 = '$145' in val
        has_175 = '$175' in val  # resource reference (image)
        has_142 = '$142' in val  # inline style runs
        has_584 = '$584' in val  # alt text

        if has_175:
            # Image item
            alt = val.get('$584', '')
            eid = val.get('$155', '?')
            style = val.get('$157', '?')
            print(f"{prefix}IMAGE (eid={eid}, style={style}): {alt[:40]}")
        elif has_145 and not has_146:
            # Text reference item
            text_ref = val['$145']
            if isinstance(text_ref, dict):
                text_id = text_ref.get('$4', '?')
                idx = text_ref.get('$403', '?')
                eid = val.get('$155', '?')
                style = val.get('$157', '?')
                links = len(val.get('$142', []))
                link_str = f", {links} links" if links else ""
                print(f"{prefix}TEXT (eid={eid}, style={style}, ref={text_id}[{idx}]{link_str})")
            else:
                print(f"{prefix}TEXT: {text_ref}")
        elif has_146:
            # Container with children
            eid = val.get('$155', '')
            style = val.get('$157', '')
            eid_str = f"eid={eid}, " if eid else ""
            style_str = f"style={style}" if style else ""
            meta = f" ({eid_str}{style_str})" if eid_str or style_str else ""
            print(f"{prefix}CONTAINER{meta}:")
            children = val['$146']
            for i, child in enumerate(children):
                show_structure(child, indent + 1, max_depth)
        else:
            # Other struct - show keys
            keys = [k for k in val.keys() if not k.startswith('$1')]  # Skip high symbol IDs
            eid = val.get('$155', '')
            style = val.get('$157', '')
            print(f"{prefix}STRUCT (eid={eid}, style={style}, keys={keys[:5]})")

    elif isinstance(val, list):
        print(f"{prefix}LIST[{len(val)}]:")
        for i, item in enumerate(val):
            show_structure(item, indent + 1, max_depth)
    else:
        print(f"{prefix}{type(val).__name__}: {str(val)[:50]}")


def main():
    parser = argparse.ArgumentParser(description="Visualize KFX content block structure")
    parser.add_argument("kfx_file", help="KFX file to analyze")
    parser.add_argument("--text", "-t", help="Search for content blocks containing this text")
    parser.add_argument("--index", "-i", type=int, help="Show content block at this index")
    parser.add_argument("--depth", "-d", type=int, default=10, help="Max nesting depth to show")
    args = parser.parse_args()

    if not Path(args.kfx_file).exists():
        print(f"Error: File not found: {args.kfx_file}")
        sys.exit(1)

    print(f"Loading {args.kfx_file}...")
    fragments = load_kfx_fragments(args.kfx_file)

    # Filter to content blocks ($259)
    content_blocks = [f for f in fragments if f['ftype'] == '$259']
    print(f"Found {len(content_blocks)} content blocks ($259)")

    # Apply filters
    if args.text:
        content_blocks = [f for f in content_blocks if search_value(f['value'], args.text)]
        print(f"Filtered to {len(content_blocks)} blocks containing '{args.text}'")

    if args.index is not None:
        if args.index < 0 or args.index >= len(content_blocks):
            print(f"Error: Index {args.index} out of range (0-{len(content_blocks)-1})")
            sys.exit(1)
        content_blocks = [content_blocks[args.index]]

    # Show structure
    for i, block in enumerate(content_blocks):
        print(f"\n{'='*60}")
        print(f"[{i}] Content Block: {block['fid']}")
        print('='*60)

        val = block['value']
        if isinstance(val, dict) and '$146' in val:
            show_structure(val, max_depth=args.depth)
        else:
            print(f"  (not a container structure)")


if __name__ == "__main__":
    main()
