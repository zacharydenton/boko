#!/usr/bin/env python3
"""Compare styles used in the imprint section between generated and reference KFX."""

import sys
import os
from pathlib import Path

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

    header_len = int.from_bytes(data[6:10], 'little')
    ci_offset = int.from_bytes(data[10:14], 'little')
    ci_len = int.from_bytes(data[14:18], 'little')

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

    ci_data = data[ci_offset:ci_offset+ci_len]
    ci = ion.deserialize_single_value(ci_data, 0)

    index_offset = ci.get('$413')
    index_length = ci.get('$414')

    if index_offset is None or index_length is None:
        raise ValueError("Could not find entity index table")

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


def collect_style_refs(val, refs=None):
    """Collect all style references ($157) from a content structure."""
    if refs is None:
        refs = set()

    if isinstance(val, dict):
        # Check for $157 (style reference)
        if '$157' in val:
            style_ref = val['$157']
            if isinstance(style_ref, str):
                refs.add(style_ref)
        # Recurse into all values
        for v in val.values():
            collect_style_refs(v, refs)
    elif isinstance(val, list):
        for item in val:
            collect_style_refs(item, refs)

    return refs


def format_style(style_val):
    """Format a style value for display."""
    if not isinstance(style_val, dict):
        return str(style_val)

    lines = []
    for k, v in sorted(style_val.items(), key=lambda x: x[0]):
        lines.append(f"    {k}: {v}")
    return "\n".join(lines)


def analyze_imprint_styles(filepath, label):
    """Analyze styles used in the imprint content block."""
    fragments = load_kfx_fragments(filepath)

    # Find imprint content block
    content_blocks = [f for f in fragments if f['ftype'] == '$259']
    imprint_blocks = [f for f in content_blocks if search_value(f['value'], 'Standard Ebooks logo')]

    if not imprint_blocks:
        print(f"No imprint block found in {label}")
        return {}, []

    # Use first matching block (the simpler imprint, not colophon)
    imprint = imprint_blocks[0]

    print(f"\n{'='*70}")
    print(f"{label}: {imprint['fid']}")
    print('='*70)

    # Collect style references from imprint content
    style_refs = collect_style_refs(imprint['value'])
    print(f"Style references in imprint: {sorted(style_refs)}")

    # Build style lookup
    styles = {f['fid']: f['value'] for f in fragments if f['ftype'] == '$157'}

    # Show each style used
    style_details = {}
    for ref in sorted(style_refs):
        if ref in styles:
            print(f"\n  Style {ref}:")
            print(format_style(styles[ref]))
            style_details[ref] = styles[ref]
        else:
            print(f"\n  Style {ref}: NOT FOUND")

    return style_details, sorted(style_refs)


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Compare imprint styles")
    parser.add_argument("generated", help="Generated KFX file")
    parser.add_argument("reference", help="Reference KFX file")
    args = parser.parse_args()

    gen_styles, gen_refs = analyze_imprint_styles(args.generated, "Generated")
    ref_styles, ref_refs = analyze_imprint_styles(args.reference, "Reference")

    print(f"\n{'='*70}")
    print("COMPARISON SUMMARY")
    print('='*70)
    print(f"Generated uses {len(gen_refs)} styles: {gen_refs}")
    print(f"Reference uses {len(ref_refs)} styles: {ref_refs}")


if __name__ == "__main__":
    main()
