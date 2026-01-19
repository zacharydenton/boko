#!/usr/bin/env python3
"""Full comparison of imprint section between generated and reference KFX."""

import sys
import os
from pathlib import Path
from decimal import Decimal
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
    '$142': 'inline-runs',
    '$145': 'text-ref',
    '$146': 'content-list',
    '$155': 'element-id',
    '$157': 'style-ref',
    '$173': 'style-name',
    '$175': 'resource-ref',
    '$176': 'offset',
    '$178': 'length',
    '$183': 'anchor-local',
    '$186': 'anchor-external',
    '$259': 'content-block',
    '$306': 'unit',
    '$307': 'value',
    '$308': 'em',
    '$310': 'zero',
    '$314': 'px',
    '$320': 'justify',
    '$321': 'center',
    '$322': 'left',
    '$323': 'right',
    '$349': 'inline',
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
    '$584': 'alt-text',
    '$788': 'unknown-788',
}


def sym_name(s):
    """Get human-readable name for a symbol."""
    if isinstance(s, str) and s.startswith('$'):
        return SYMBOL_NAMES.get(s, s)
    return str(s)


def format_value(v, compact=False):
    """Format a value for display."""
    if isinstance(v, dict):
        if '$306' in v and '$307' in v:
            # Unit-value pair
            unit = sym_name(v.get('$306', '?'))
            val = v.get('$307', '?')
            if isinstance(val, Decimal):
                val = float(val)
            return f"{val}{unit}"
        parts = []
        for k, vv in sorted(v.items()):
            parts.append(f"{sym_name(k)}={format_value(vv, compact=True)}")
        return "{" + ", ".join(parts) + "}"
    elif isinstance(v, Decimal):
        return str(float(v))
    elif isinstance(v, str) and len(v) > 40 and not compact:
        return f"\"{v[:40]}...\""
    elif isinstance(v, str):
        return f"\"{v}\""
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
    """Search for text within a nested structure."""
    if isinstance(val, str):
        return text.lower() in val.lower()
    elif isinstance(val, dict):
        return any(search_value(v, text) for v in val.values())
    elif isinstance(val, list):
        return any(search_value(v, text) for v in val)
    return False


def get_text_content(frag_value, text_frags):
    """Extract all text content from a content block."""
    texts = []

    def extract_text_refs(val):
        if isinstance(val, dict):
            if '$145' in val:  # text reference
                text_ref = val['$145']
                if isinstance(text_ref, dict) and '$4' in text_ref:
                    ref_id = f"${text_ref['$4']}"
                    for tf in text_frags:
                        if tf['fid'] == ref_id:
                            if '$146' in tf['value']:
                                texts.append(tf['value']['$146'])
            for v in val.values():
                extract_text_refs(v)
        elif isinstance(val, list):
            for item in val:
                extract_text_refs(item)

    extract_text_refs(frag_value)
    return texts


def get_inline_runs(content_item):
    """Get inline runs from a content item."""
    if isinstance(content_item, dict) and '$142' in content_item:
        return content_item['$142']
    return []


def show_structure(val, styles, indent=0, prefix=""):
    """Show the structure of a content block."""
    ind = "  " * indent

    if isinstance(val, dict):
        style_ref = val.get('$157', '-')
        eid = val.get('$155', '-')
        style = styles.get(style_ref, {}) if isinstance(style_ref, str) else {}

        # Determine element type
        if '$146' in val:  # container
            children = val['$146']
            block_type = style.get('$127', '-') if isinstance(style, dict) else '-'
            print(f"{ind}{prefix}CONTAINER style={style_ref} block={sym_name(block_type)} eid={eid}")
            for i, child in enumerate(children):
                show_structure(child, styles, indent+1, f"[{i}] ")
        elif '$145' in val:  # text reference
            text_ref = val['$145']
            ref_id = "-"
            if isinstance(text_ref, dict) and '$4' in text_ref:
                ref_id = f"${text_ref['$4']}"
            inline_runs = get_inline_runs(val)
            run_info = f" ({len(inline_runs)} runs)" if inline_runs else ""
            print(f"{ind}{prefix}TEXT style={style_ref} ref={ref_id}{run_info}")

            # Show inline runs
            for i, run in enumerate(inline_runs):
                if isinstance(run, dict):
                    run_style = run.get('$157', '-')
                    offset = run.get('$176', '?')
                    length = run.get('$178', '?')
                    anchor_local = run.get('$183')
                    anchor_ext = run.get('$186')
                    anchor = anchor_ext or anchor_local or '-'
                    print(f"{ind}  └─ run[{i}] style={run_style} off={offset} len={length} anchor={anchor[:30] if isinstance(anchor, str) else anchor}...")
        elif '$175' in val:  # image resource
            res_ref = val.get('$175', {})
            res_id = res_ref.get('$4', '?') if isinstance(res_ref, dict) else '?'
            alt = val.get('$584', '')[:30] if val.get('$584') else ''
            print(f"{ind}{prefix}IMAGE style={style_ref} res=${res_id} alt=\"{alt}\"")
        else:
            print(f"{ind}{prefix}OTHER style={style_ref} keys={list(val.keys())[:5]}")


def compare_styles(gen_style, ref_style, label):
    """Compare two styles and show differences."""
    if not isinstance(gen_style, dict):
        gen_style = {}
    if not isinstance(ref_style, dict):
        ref_style = {}

    all_keys = set(gen_style.keys()) | set(ref_style.keys())
    all_keys.discard('$173')  # Skip style-name

    diffs = []
    for key in sorted(all_keys):
        gen_val = gen_style.get(key)
        ref_val = ref_style.get(key)

        if gen_val != ref_val:
            gen_fmt = format_value(gen_val) if gen_val else "-"
            ref_fmt = format_value(ref_val) if ref_val else "-"
            diffs.append((sym_name(key), gen_fmt, ref_fmt))

    if diffs:
        print(f"\n  {label}:")
        for prop, gen, ref in diffs:
            print(f"    {prop:25} gen={gen:25} ref={ref}")
    return len(diffs)


def main():
    print("Loading KFX files...")
    gen_frags = load_kfx_fragments("/tmp/epictetus-boko.kfx")
    ref_frags = load_kfx_fragments("tests/fixtures/epictetus.kfx")

    # Build lookups
    gen_styles = {f['fid']: f['value'] for f in gen_frags if f['ftype'] == '$157'}
    ref_styles = {f['fid']: f['value'] for f in ref_frags if f['ftype'] == '$157'}
    gen_texts = [f for f in gen_frags if f['ftype'] == '$417']  # text fragments
    ref_texts = [f for f in ref_frags if f['ftype'] == '$417']

    # Find imprint blocks (search for distinctive text)
    gen_imprint = None
    ref_imprint = None

    for frag in gen_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'book is the product'):
            gen_imprint = frag
            break

    for frag in ref_frags:
        if frag['ftype'] == '$259' and search_value(frag['value'], 'book is the product'):
            ref_imprint = frag
            break

    if not gen_imprint:
        print("ERROR: Could not find imprint in generated KFX")
        return
    if not ref_imprint:
        print("ERROR: Could not find imprint in reference KFX")
        return

    print(f"\nFound imprint blocks:")
    print(f"  Generated: {gen_imprint['fid']}")
    print(f"  Reference: {ref_imprint['fid']}")

    # ============== STRUCTURE COMPARISON ==============
    print("\n" + "="*80)
    print("1. STRUCTURE COMPARISON")
    print("="*80)

    print("\n--- GENERATED STRUCTURE ---")
    show_structure(gen_imprint['value'], gen_styles)

    print("\n--- REFERENCE STRUCTURE ---")
    show_structure(ref_imprint['value'], ref_styles)

    # ============== TEXT CONTENT COMPARISON ==============
    print("\n" + "="*80)
    print("2. TEXT CONTENT COMPARISON")
    print("="*80)

    gen_texts_content = get_text_content(gen_imprint['value'], gen_texts)
    ref_texts_content = get_text_content(ref_imprint['value'], ref_texts)

    print(f"\nGenerated text blocks: {len(gen_texts_content)}")
    for i, text in enumerate(gen_texts_content):
        preview = str(text)[:80].replace('\n', ' ')
        print(f"  [{i}] {preview}...")

    print(f"\nReference text blocks: {len(ref_texts_content)}")
    for i, text in enumerate(ref_texts_content):
        preview = str(text)[:80].replace('\n', ' ')
        print(f"  [{i}] {preview}...")

    # ============== STYLE COLLECTION ==============
    def collect_style_refs(val):
        refs = []
        if isinstance(val, dict):
            if '$157' in val:
                refs.append(val['$157'])
            # Also check inline runs
            if '$142' in val:
                for run in val['$142']:
                    if isinstance(run, dict) and '$157' in run:
                        refs.append(run['$157'])
            for v in val.values():
                refs.extend(collect_style_refs(v))
        elif isinstance(val, list):
            for item in val:
                refs.extend(collect_style_refs(item))
        return refs

    gen_style_refs = list(dict.fromkeys(collect_style_refs(gen_imprint['value'])))
    ref_style_refs = list(dict.fromkeys(collect_style_refs(ref_imprint['value'])))

    print("\n" + "="*80)
    print("3. STYLES USED")
    print("="*80)

    print(f"\nGenerated styles: {gen_style_refs}")
    print(f"Reference styles: {ref_style_refs}")

    print(f"\nGenerated style count: {len(gen_style_refs)}")
    print(f"Reference style count: {len(ref_style_refs)}")

    # ============== DETAILED STYLE COMPARISON ==============
    print("\n" + "="*80)
    print("4. STYLE PROPERTY DETAILS")
    print("="*80)

    print("\n--- Generated styles ---")
    for style_ref in gen_style_refs:
        style = gen_styles.get(style_ref, {})
        if isinstance(style, dict):
            print(f"\n{style_ref}:")
            for k, v in sorted(style.items()):
                if k != '$173':  # skip style-name
                    print(f"  {sym_name(k):25} = {format_value(v)}")

    print("\n--- Reference styles ---")
    for style_ref in ref_style_refs:
        style = ref_styles.get(style_ref, {})
        if isinstance(style, dict):
            print(f"\n{style_ref}:")
            for k, v in sorted(style.items()):
                if k != '$173':  # skip style-name
                    print(f"  {sym_name(k):25} = {format_value(v)}")

    # ============== INLINE RUNS COMPARISON ==============
    print("\n" + "="*80)
    print("5. INLINE RUNS COMPARISON")
    print("="*80)

    def collect_inline_runs(val, path=""):
        runs = []
        if isinstance(val, dict):
            if '$142' in val:
                runs.append((path or "root", val['$142']))
            if '$146' in val:
                for i, child in enumerate(val['$146']):
                    runs.extend(collect_inline_runs(child, f"{path}[{i}]"))
        return runs

    gen_runs = collect_inline_runs(gen_imprint['value'])
    ref_runs = collect_inline_runs(ref_imprint['value'])

    print(f"\nGenerated inline run groups: {len(gen_runs)}")
    for path, runs in gen_runs:
        print(f"\n  {path}: {len(runs)} runs")
        for i, run in enumerate(runs):
            if isinstance(run, dict):
                style = run.get('$157', '-')
                offset = run.get('$176', '?')
                length = run.get('$178', '?')
                anchor = run.get('$186') or run.get('$183') or '-'
                print(f"    [{i}] style={style} off={offset} len={length}")
                print(f"        anchor={anchor[:50] if isinstance(anchor, str) else anchor}...")

    print(f"\nReference inline run groups: {len(ref_runs)}")
    for path, runs in ref_runs:
        print(f"\n  {path}: {len(runs)} runs")
        for i, run in enumerate(runs):
            if isinstance(run, dict):
                style = run.get('$157', '-')
                offset = run.get('$176', '?')
                length = run.get('$178', '?')
                anchor = run.get('$186') or run.get('$183') or '-'
                print(f"    [{i}] style={style} off={offset} len={length}")
                print(f"        anchor={anchor[:50] if isinstance(anchor, str) else anchor}...")

    # ============== SUMMARY ==============
    print("\n" + "="*80)
    print("6. SUMMARY")
    print("="*80)

    # Structure match
    gen_items = len(gen_imprint['value'].get('$146', []))
    ref_items = len(ref_imprint['value'].get('$146', []))
    print(f"\nContent item count: Generated={gen_items}, Reference={ref_items}")

    # Style count match
    print(f"Style count: Generated={len(gen_style_refs)}, Reference={len(ref_style_refs)}")

    # Inline run count
    gen_run_total = sum(len(runs) for _, runs in gen_runs)
    ref_run_total = sum(len(runs) for _, runs in ref_runs)
    print(f"Total inline runs: Generated={gen_run_total}, Reference={ref_run_total}")


if __name__ == "__main__":
    main()
