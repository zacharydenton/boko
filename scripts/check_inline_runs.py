#!/usr/bin/env python3
"""Check inline style runs in imprint section."""

import sys
import os
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
KFXLIB_PATH = os.environ.get('KFXLIB_PATH', str(SCRIPT_DIR))
sys.path.insert(0, KFXLIB_PATH)

from kfxlib.ion_binary import IonBinary
from kfxlib.yj_symbol_catalog import YJ_SYMBOLS


def load_kfx_fragments(filepath):
    with open(filepath, 'rb') as f:
        data = f.read()

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


def show_text_item(item, label, styles):
    """Show details of a TEXT item including inline runs."""
    print(f"\n{label}")
    print(f"  Style: {item.get('$157', '-')}")

    # Show inline runs ($142)
    runs = item.get('$142', [])
    if runs:
        print(f"  Inline runs ({len(runs)}):")
        for i, run in enumerate(runs):
            if isinstance(run, dict):
                run_style = run.get('$157', '-')
                offset = run.get('$176', '?')
                length = run.get('$178', '?')
                anchor = run.get('$183') or run.get('$186')
                anchor_type = '$183' if '$183' in run else '$186' if '$186' in run else '-'

                # Look up the style
                style_props = styles.get(run_style, {})
                style_summary = []
                if '$127' in style_props:
                    style_summary.append(f"block-type={style_props['$127']}")
                if '$45' in style_props:
                    style_summary.append("bold")
                if '$46' in style_props:
                    style_summary.append("italic")

                print(f"    [{i}] style={run_style} offset={offset} len={length} anchor={anchor_type}")
                if style_summary:
                    print(f"         style-props: {', '.join(style_summary)}")
    else:
        print("  No inline runs")


def main():
    print("="*80)
    print("REFERENCE INLINE RUNS")
    print("="*80)

    ref_frags = load_kfx_fragments("tests/fixtures/epictetus.kfx")
    ref_styles = {f['fid']: f['value'] for f in ref_frags if f['ftype'] == '$157'}

    # Find imprint
    for frag in ref_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'Standard Ebooks logo'):
            val = frag['value']
            if '$146' in val:
                for i, item in enumerate(val['$146']):
                    if isinstance(item, dict) and '$145' in item:
                        show_text_item(item, f"Paragraph {i}", ref_styles)
            break

    print("\n" + "="*80)
    print("GENERATED INLINE RUNS")
    print("="*80)

    gen_frags = load_kfx_fragments("/tmp/epictetus-boko.kfx")
    gen_styles = {f['fid']: f['value'] for f in gen_frags if f['ftype'] == '$157'}

    for frag in gen_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'Standard Ebooks logo'):
            val = frag['value']
            if '$146' in val:
                for i, item in enumerate(val['$146']):
                    if isinstance(item, dict) and '$145' in item:
                        show_text_item(item, f"Paragraph {i}", gen_styles)
            break

    print("\n" + "="*80)
    print("STYLE COMPARISON: Link styles")
    print("="*80)

    print("\nReference $1125 (link style):")
    if '$1125' in ref_styles:
        for k, v in ref_styles['$1125'].items():
            print(f"  {k}: {v}")

    print("\nGenerated inline run styles:")
    # Find what styles are used for inline runs in generated
    for frag in gen_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'Standard Ebooks logo'):
            val = frag['value']
            if '$146' in val:
                for item in val['$146']:
                    if isinstance(item, dict) and '$142' in item:
                        for run in item['$142']:
                            if isinstance(run, dict):
                                run_style = run.get('$157')
                                if run_style and run_style in gen_styles:
                                    print(f"\n  {run_style}:")
                                    for k, v in gen_styles[run_style].items():
                                        print(f"    {k}: {v}")
                                    break
                        break
            break


if __name__ == "__main__":
    main()
