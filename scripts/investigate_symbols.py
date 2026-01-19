#!/usr/bin/env python3
"""Investigate symbol usage patterns in reference KFX to understand mappings."""

import sys
import os
from pathlib import Path
from collections import defaultdict

SCRIPT_DIR = Path(__file__).parent
KFXLIB_PATH = os.environ.get('KFXLIB_PATH', str(SCRIPT_DIR))
sys.path.insert(0, KFXLIB_PATH)

try:
    from kfxlib.ion_binary import IonBinary
    from kfxlib.yj_symbol_catalog import YJ_SYMBOLS
except ImportError as e:
    print(f"Error: Could not import kfxlib from {KFXLIB_PATH}")
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


def find_all_style_refs(val, refs=None):
    """Find ALL $157 style references in a structure."""
    if refs is None:
        refs = []

    if isinstance(val, dict):
        if '$157' in val:
            refs.append(val['$157'])
        for v in val.values():
            find_all_style_refs(v, refs)
    elif isinstance(val, list):
        for item in val:
            find_all_style_refs(item, refs)

    return refs


def search_value(val, text):
    if isinstance(val, str):
        return text.lower() in val.lower()
    elif isinstance(val, dict):
        return any(search_value(v, text) for v in val.values())
    elif isinstance(val, list):
        return any(search_value(v, text) for v in val)
    return False


def main():
    ref_frags = load_kfx_fragments("tests/fixtures/epictetus.kfx")
    gen_frags = load_kfx_fragments("/tmp/epictetus-boko.kfx")

    # Build style lookups
    ref_styles = {f['fid']: f['value'] for f in ref_frags if f['ftype'] == '$157'}
    gen_styles = {f['fid']: f['value'] for f in gen_frags if f['ftype'] == '$157'}

    print("="*80)
    print("1. INVESTIGATING $349 vs $383 (block type values)")
    print("="*80)

    # Find all styles using $349 or $383
    styles_349 = []
    styles_383 = []

    for fid, style in ref_styles.items():
        if isinstance(style, dict):
            block_type = style.get('$127')
            if block_type == '$349':
                styles_349.append(fid)
            elif block_type == '$383':
                styles_383.append(fid)

    print(f"\nReference styles with $127=$349: {len(styles_349)}")
    for fid in styles_349[:5]:
        print(f"  {fid}: {ref_styles[fid]}")

    print(f"\nReference styles with $127=$383: {len(styles_383)}")
    for fid in styles_383[:5]:
        print(f"  {fid}: {ref_styles[fid]}")

    print("\n" + "="*80)
    print("2. INVESTIGATING $1125 - WHERE IS IT USED?")
    print("="*80)

    # Find all content blocks that reference $1125
    for frag in ref_frags:
        if frag['ftype'] == '$259':  # content block
            refs = find_all_style_refs(frag['value'])
            if '$1125' in refs:
                print(f"\nContent block {frag['fid']} uses $1125")
                # Show the structure
                if isinstance(frag['value'], dict) and '$146' in frag['value']:
                    items = frag['value']['$146']
                    for i, item in enumerate(items[:10]):
                        if isinstance(item, dict):
                            item_style = item.get('$157', '-')
                            item_type = 'container' if '$146' in item else 'text' if '$145' in item else 'image' if '$175' in item else '?'
                            print(f"  [{i}] {item_type}: style={item_style}")

    print("\n" + "="*80)
    print("3. WHAT DOES $1125 CONTAIN?")
    print("="*80)
    if '$1125' in ref_styles:
        print(f"$1125 = {ref_styles['$1125']}")

    print("\n" + "="*80)
    print("4. COMPARING STYLE COUNTS BY ELEMENT TYPE")
    print("="*80)

    # Find imprint blocks
    ref_imprint = None
    gen_imprint = None
    for frag in ref_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'Standard Ebooks logo'):
            ref_imprint = frag
            break
    for frag in gen_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'Standard Ebooks logo'):
            gen_imprint = frag
            break

    print("\nReference imprint - all style refs:", find_all_style_refs(ref_imprint['value']) if ref_imprint else [])
    print("Generated imprint - all style refs:", find_all_style_refs(gen_imprint['value']) if gen_imprint else [])

    print("\n" + "="*80)
    print("5. DETAILED STRUCTURE WITH ALL STYLES")
    print("="*80)

    def show_structure(val, indent=0, label=""):
        prefix = "  " * indent
        if isinstance(val, dict):
            style = val.get('$157', '')
            eid = val.get('$155', '')

            if '$146' in val:  # container
                print(f"{prefix}CONTAINER style={style} eid={eid} {label}")
                for i, item in enumerate(val['$146']):
                    show_structure(item, indent+1, f"[{i}]")
            elif '$145' in val:  # text ref
                text_ref = val['$145']
                if isinstance(text_ref, dict):
                    text_id = text_ref.get('$4', '?')
                    print(f"{prefix}TEXT style={style} eid={eid} ref={text_id} {label}")
                else:
                    print(f"{prefix}TEXT style={style} eid={eid} {label}")
            elif '$175' in val or '$584' in val:  # image
                alt = val.get('$584', '')[:30] if val.get('$584') else ''
                print(f"{prefix}IMAGE style={style} eid={eid} alt=\"{alt}\" {label}")
            else:
                print(f"{prefix}OTHER style={style} eid={eid} keys={list(val.keys())[:5]} {label}")

    print("\n--- Reference imprint structure ---")
    if ref_imprint:
        show_structure(ref_imprint['value'])

    print("\n--- Generated imprint structure ---")
    if gen_imprint:
        show_structure(gen_imprint['value'])

    print("\n" + "="*80)
    print("6. YJ_SYMBOLS LOOKUP FOR KEY VALUES")
    print("="*80)

    # Try to find symbol meanings
    key_symbols = [349, 383, 546, 580, 788]
    for sym_id in key_symbols:
        idx = sym_id - 10  # shared_base is 10
        if 0 <= idx < len(YJ_SYMBOLS.symbols):
            name = YJ_SYMBOLS.symbols[idx]
            print(f"  ${sym_id} = {name}")
        else:
            print(f"  ${sym_id} = (not in YJ_SYMBOLS)")


if __name__ == "__main__":
    main()
