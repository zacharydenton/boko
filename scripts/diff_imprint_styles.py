#!/usr/bin/env python3
"""Detailed diff of imprint styles between generated and reference KFX."""

import sys
import os
from pathlib import Path
from decimal import Decimal

SCRIPT_DIR = Path(__file__).parent
KFXLIB_PATH = os.environ.get('KFXLIB_PATH', str(SCRIPT_DIR))
sys.path.insert(0, KFXLIB_PATH)

try:
    from kfxlib.ion_binary import IonBinary
    from kfxlib.yj_symbol_catalog import YJ_SYMBOLS
except ImportError as e:
    print(f"Error: Could not import kfxlib from {KFXLIB_PATH}")
    sys.exit(1)

# Symbol mappings for readability
SYMBOL_NAMES = {
    '$10': 'language',
    '$12': 'font-size',
    '$13': 'line-height',
    '$16': 'text-indent',
    '$34': 'text-align',
    '$36': 'margin-top',
    '$42': 'margin-bottom',
    '$44': 'font-family',
    '$45': 'bold',
    '$46': 'italic',
    '$47': 'margin-left',
    '$48': 'margin-right',
    '$56': 'width',
    '$127': 'display/block-type',
    '$135': 'font-variant',
    '$173': 'style-name',
    '$306': 'unit',
    '$307': 'value',
    '$308': 'em',
    '$310': 'zero',
    '$314': 'px',
    '$320': 'justify',
    '$321': 'center',
    '$322': 'left',
    '$323': 'right',
    '$349': 'block-inline?',
    '$350': '100%',
    '$353': 'small-caps',
    '$361': '1em',
    '$370': 'serif',
    '$377': 'contain',
    '$382': 'smaller',
    '$383': 'block',
    '$505': '1.5em',
    '$546': 'image-fit',
    '$580': 'image-layout',
    '$788': '?788',
}

def sym_name(s):
    """Get human-readable name for a symbol."""
    if isinstance(s, str) and s.startswith('$'):
        return SYMBOL_NAMES.get(s, s)
    return str(s)

def format_value(v, indent=0):
    """Format a value for display."""
    prefix = "  " * indent
    if isinstance(v, dict):
        if '$306' in v and '$307' in v:
            # Unit-value pair
            unit = sym_name(v.get('$306', '?'))
            val = v.get('$307', '?')
            if isinstance(val, Decimal):
                val = float(val)
            return f"{val} {unit}"
        parts = []
        for k, vv in sorted(v.items()):
            parts.append(f"{sym_name(k)}={format_value(vv)}")
        return "{" + ", ".join(parts) + "}"
    elif isinstance(v, Decimal):
        return str(float(v))
    return str(v)


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


def search_value(val, text):
    if isinstance(val, str):
        return text.lower() in val.lower()
    elif isinstance(val, dict):
        return any(search_value(v, text) for v in val.values())
    elif isinstance(val, list):
        return any(search_value(v, text) for v in val)
    return False


def get_item_type(item):
    """Determine if item is header container, image, or text."""
    if not isinstance(item, dict):
        return "unknown"
    if '$146' in item:  # has content array = container
        return "container"
    if '$175' in item or '$584' in item:  # resource ref or alt text = image
        return "image"
    if '$145' in item:  # text reference
        return "text"
    return "unknown"


def analyze_content_structure(block_value):
    """Analyze the content structure and extract style info per element."""
    results = []

    if not isinstance(block_value, dict) or '$146' not in block_value:
        return results

    outer_style = block_value.get('$157', '?')
    results.append(('outer-container', outer_style, block_value.get('$155', '?')))

    items = block_value['$146']
    for i, item in enumerate(items):
        if not isinstance(item, dict):
            continue

        item_type = get_item_type(item)
        item_style = item.get('$157', '?')
        item_eid = item.get('$155', '?')

        if item_type == 'container':
            results.append((f'header-container', item_style, item_eid))
            # Check children
            if '$146' in item:
                for j, child in enumerate(item['$146']):
                    if isinstance(child, dict):
                        child_type = get_item_type(child)
                        child_style = child.get('$157', '?')
                        child_eid = child.get('$155', '?')
                        results.append((f'  {child_type}', child_style, child_eid))
        elif item_type == 'image':
            results.append((f'image', item_style, item_eid))
        elif item_type == 'text':
            results.append((f'paragraph-{i}', item_style, item_eid))

    return results


def main():
    gen_frags = load_kfx_fragments("/tmp/epictetus-boko.kfx")
    ref_frags = load_kfx_fragments("tests/fixtures/epictetus.kfx")

    # Find imprint blocks
    gen_blocks = [f for f in gen_frags if f['ftype'] == '$259' and search_value(f['value'], 'Standard Ebooks logo')]
    ref_blocks = [f for f in ref_frags if f['ftype'] == '$259' and search_value(f['value'], 'Standard Ebooks logo')]

    gen_block = gen_blocks[0] if gen_blocks else None
    ref_block = ref_blocks[0] if ref_blocks else None

    # Build style lookups
    gen_styles = {f['fid']: f['value'] for f in gen_frags if f['ftype'] == '$157'}
    ref_styles = {f['fid']: f['value'] for f in ref_frags if f['ftype'] == '$157'}

    print("="*80)
    print("IMPRINT CONTENT STRUCTURE COMPARISON")
    print("="*80)

    print("\n--- GENERATED ---")
    gen_struct = analyze_content_structure(gen_block['value']) if gen_block else []
    for elem_type, style_ref, eid in gen_struct:
        print(f"  {elem_type:20} style={style_ref:8} eid={eid}")

    print("\n--- REFERENCE ---")
    ref_struct = analyze_content_structure(ref_block['value']) if ref_block else []
    for elem_type, style_ref, eid in ref_struct:
        print(f"  {elem_type:20} style={style_ref:8} eid={eid}")

    print("\n" + "="*80)
    print("STYLE PROPERTY COMPARISON")
    print("="*80)

    # Map generated structure to reference by position/type
    print("\n--- Element-by-element style comparison ---\n")

    for i, (gen_elem, ref_elem) in enumerate(zip(gen_struct, ref_struct)):
        gen_type, gen_style_ref, gen_eid = gen_elem
        ref_type, ref_style_ref, ref_eid = ref_elem

        print(f"\n[{i}] {gen_type}")
        print(f"    Generated: {gen_style_ref}  |  Reference: {ref_style_ref}")
        print("-" * 60)

        gen_style = gen_styles.get(gen_style_ref, {})
        ref_style = ref_styles.get(ref_style_ref, {})

        if not isinstance(gen_style, dict):
            gen_style = {}
        if not isinstance(ref_style, dict):
            ref_style = {}

        all_keys = set(gen_style.keys()) | set(ref_style.keys())
        all_keys.discard('$173')  # Skip style-name

        for key in sorted(all_keys):
            gen_val = gen_style.get(key)
            ref_val = ref_style.get(key)

            gen_fmt = format_value(gen_val) if gen_val else "-"
            ref_fmt = format_value(ref_val) if ref_val else "-"

            match = "✓" if gen_val == ref_val else "✗"
            prop_name = sym_name(key)

            if gen_val != ref_val:
                print(f"    {match} {prop_name:20} gen={gen_fmt:20} ref={ref_fmt}")


if __name__ == "__main__":
    main()
