#!/usr/bin/env python3
"""Compare style verbosity between generated and reference KFX."""

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


def analyze_styles(filepath, label):
    """Analyze style fragments in a KFX file."""
    fragments = load_kfx_fragments(filepath)

    # Filter to style fragments ($157)
    styles = [f for f in fragments if f['ftype'] == '$157']

    print(f"\n=== {label} ===")
    print(f"Total style fragments: {len(styles)}")

    # Analyze property counts
    prop_counts = []
    for s in styles:
        if isinstance(s['value'], dict):
            prop_counts.append(len(s['value']))

    if prop_counts:
        print(f"Properties per style: min={min(prop_counts)}, max={max(prop_counts)}, avg={sum(prop_counts)/len(prop_counts):.1f}")

    # Show a few example styles
    print("\nExample styles:")
    for i, s in enumerate(styles[:5]):
        print(f"\n  [{i}] {s['fid']}:")
        if isinstance(s['value'], dict):
            for k, v in list(s['value'].items())[:10]:
                print(f"    {k}: {v}")
            if len(s['value']) > 10:
                print(f"    ... and {len(s['value']) - 10} more properties")


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Compare KFX style verbosity")
    parser.add_argument("generated", help="Generated KFX file")
    parser.add_argument("reference", help="Reference KFX file")
    args = parser.parse_args()

    analyze_styles(args.generated, "Generated")
    analyze_styles(args.reference, "Reference")


if __name__ == "__main__":
    main()
