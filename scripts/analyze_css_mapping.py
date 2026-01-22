#!/usr/bin/env python3
"""Analyze CSS-to-KFX symbol mappings from a test KFX file.

Outputs KFX styles as CSS for easy comparison with input CSS.

Usage:
    python analyze_css_mapping.py <kfx_file>           # Full analysis
    python analyze_css_mapping.py <kfx_file> --css     # Just output CSS styles
"""

import sys
from pathlib import Path
from collections import defaultdict

# Add scripts to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from kfx_loader import load_kfx
from kfx_symbols import format_symbol, get_symbol_names
from kfx_to_css import KfxStyleConverter, kfx_style_to_css_string


def format_kfx_value(v):
    """Format a KFX value with symbol names where applicable."""
    if isinstance(v, str) and v.startswith('$') and v[1:].isdigit():
        return format_symbol(v)
    elif isinstance(v, dict):
        # Format dict values recursively
        parts = []
        for dk, dv in v.items():
            dk_fmt = format_symbol(dk) if isinstance(dk, str) and dk.startswith('$') else dk
            dv_fmt = format_kfx_value(dv)
            parts.append(f"{dk_fmt}: {dv_fmt}")
        return "{" + ", ".join(parts) + "}"
    elif isinstance(v, list):
        return "[" + ", ".join(format_kfx_value(item) for item in v) + "]"
    else:
        return str(v)


def ion_to_dict(ion_val):
    """Convert Ion values to Python dicts/lists for easier manipulation."""
    if hasattr(ion_val, 'items'):
        # IonStruct
        result = {}
        for k, v in ion_val.items():
            # Convert key - could be IonSymbol or string
            if hasattr(k, 'text'):
                key = k.text
            else:
                key = str(k)
            result[key] = ion_to_dict(v)
        return result
    elif hasattr(ion_val, '__iter__') and not isinstance(ion_val, (str, bytes)):
        # IonList
        return [ion_to_dict(v) for v in ion_val]
    elif hasattr(ion_val, 'text'):
        # IonSymbol
        return ion_val.text
    else:
        return ion_val


def output_css_only(kfx_path):
    """Output all styles as CSS classes."""
    frags, _ = load_kfx(kfx_path)
    converter = KfxStyleConverter()

    print(f"/* Styles from {kfx_path} */")
    print()

    for frag in frags.get_all("$157"):  # STYLE fragments
        style_data = {}
        raw_val = frag.value
        if hasattr(raw_val, 'value'):
            raw_val = raw_val.value

        if hasattr(raw_val, 'items'):
            for k, v in raw_val.items():
                key_str = str(k)
                if key_str.startswith('$'):
                    try:
                        key_int = int(key_str[1:])
                        style_data[key_int] = v
                    except ValueError:
                        pass

        # Get style name
        style_name = style_data.get(173, frag.fid)
        if isinstance(style_name, str) and style_name.startswith('$'):
            class_name = style_name[1:]
        else:
            class_name = str(style_name).replace('$', '')

        # Convert to CSS
        css_str = converter.to_css_string(style_data)
        if css_str:
            print(f".{class_name} {{ {css_str}; }}")
        else:
            print(f".{class_name} {{ /* empty */ }}")


def main():
    if len(sys.argv) < 2:
        print("Usage: analyze_css_mapping.py <kfx_file> [--css]")
        sys.exit(1)

    kfx_path = sys.argv[1]

    # Simple --css mode
    if "--css" in sys.argv:
        output_css_only(kfx_path)
        return

    frags, method = load_kfx(kfx_path)
    fragments = list(frags)
    print(f"Loaded {len(fragments)} fragments using {method}")

    # Build fragment lookup by ID
    frag_by_id = {}
    for frag in fragments:
        fid = frag.fid
        if fid:
            frag_by_id[fid] = frag

    # Find fragments by type
    frags_by_type = defaultdict(list)
    for frag in fragments:
        ftype = frag.ftype
        frags_by_type[ftype].append(frag)

    # Find the text content fragment ($145 type)
    text_content = None
    for frag in frags_by_type.get('$145', []):
        val = ion_to_dict(frag.value)
        text_content = val.get('$146', [])
        break

    if not text_content:
        print("Could not find text content fragment")
        sys.exit(1)

    print(f"Found {len(text_content)} text entries")

    # Find the paragraph structure fragment ($259 type)
    para_structures = []
    for frag in frags_by_type.get('$259', []):
        val = ion_to_dict(frag.value)
        para_list = val.get('$146', [])
        para_structures.extend(para_list)

    print(f"Found {len(para_structures)} paragraph structures")

    # Build mapping: text index -> style ID
    text_to_style = {}
    for para in para_structures:
        if isinstance(para, dict):
            text_ref = para.get('$145', {})
            if isinstance(text_ref, dict):
                text_idx = text_ref.get('$403')
                style_id = para.get('$157')
                if text_idx is not None and style_id is not None:
                    text_to_style[text_idx] = style_id

    # Find all style fragments ($157 type)
    styles = {}
    for frag in frags_by_type.get('$157', []):
        style_id = frag.fid
        style_value = ion_to_dict(frag.value)
        if style_id:
            styles[style_id] = style_value

    print(f"Found {len(styles)} style fragments")
    print()

    # Convert styles to int keys for kfx_to_css
    def normalize_style(style_dict):
        """Convert $NNN string keys to int keys for kfx_to_css."""
        result = {}
        for k, v in style_dict.items():
            if isinstance(k, str) and k.startswith('$'):
                try:
                    key = int(k[1:])
                except ValueError:
                    key = k
            else:
                key = k
            # Recursively normalize nested dicts
            if isinstance(v, dict):
                v = normalize_style(v)
            result[key] = v
        return result

    converter = KfxStyleConverter()

    # Find the baseline style (first paragraph, no CSS)
    baseline_style_id = text_to_style.get(0)
    baseline_style = styles.get(baseline_style_id, {})
    baseline_normalized = normalize_style(baseline_style)
    baseline_css = converter.to_css_string(baseline_normalized)

    print("=" * 80)
    print(f"BASELINE STYLE (ID: {format_symbol(baseline_style_id)}):")
    print("-" * 40)
    print(f"  CSS: {baseline_css if baseline_css else '(empty)'}")
    print()
    print("  Raw KFX properties:")
    for k, v in sorted(baseline_style.items()):
        print(f"    {format_symbol(k)}: {format_kfx_value(v)}")
    print()

    # Create mapping: CSS property -> KFX symbols that differ from baseline
    print("=" * 80)
    print("CSS INPUT → CSS OUTPUT (from KFX)")
    print("=" * 80)
    print()

    mappings = []

    for idx, text in enumerate(text_content):
        if 'BASELINE' in text or ':' not in text:
            continue

        style_id = text_to_style.get(idx)
        if not style_id:
            continue

        style = styles.get(style_id, {})
        style_normalized = normalize_style(style)
        style_css = converter.to_css_string(style_normalized)

        # Parse CSS from text
        # Format: "class-name: property-name: value"
        parts = text.split(':')
        if len(parts) >= 2:
            css_class = parts[0].strip()
            css_prop_value = ':'.join(parts[1:]).strip()

            # Find what changed from baseline
            diffs = {}
            for k, v in style.items():
                if k not in baseline_style or baseline_style[k] != v:
                    diffs[k] = v

            # Note what was removed
            for k in baseline_style:
                if k not in style:
                    diffs[k] = "(removed)"

            if diffs:
                mappings.append((css_prop_value, diffs, style_id, style_css))

    # Sort by CSS property
    mappings.sort(key=lambda x: x[0])

    # Group by output CSS for cleaner output
    by_css_output = defaultdict(list)
    for css_input, diffs, style_id, css_output in mappings:
        by_css_output[css_output].append((css_input, diffs))

    # Print grouped results - show which CSS inputs produce the same output
    for css_output, css_list in sorted(by_css_output.items(), key=lambda x: x[0]):
        print(f"OUTPUT: {css_output if css_output else '(no CSS properties)'}")
        for css_input, _ in css_list:
            print(f"  <- INPUT: {css_input}")
        print()

    # Also print a direct table
    print()
    print("=" * 80)
    print("DIRECT TABLE (CSS INPUT → CSS OUTPUT)")
    print("=" * 80)
    print()

    for css_input, diffs, style_id, css_output in mappings:
        # Show input vs output CSS
        print(f"IN:  {css_input}")
        print(f"OUT: {css_output if css_output else '(empty)'}")
        if css_input.strip() == css_output.strip():
            print("     ✓ MATCH")
        print()

    # Collect all symbols used in styles and identify unknown ones
    print()
    print("=" * 80)
    print("SYMBOL ANALYSIS")
    print("=" * 80)
    print()

    known_symbols = get_symbol_names()
    all_style_symbols = set()

    def collect_symbols(obj):
        if isinstance(obj, str) and obj.startswith('$') and obj[1:].isdigit():
            all_style_symbols.add(obj)
        elif isinstance(obj, dict):
            for k, v in obj.items():
                if isinstance(k, str) and k.startswith('$') and k[1:].isdigit():
                    all_style_symbols.add(k)
                collect_symbols(v)
        elif isinstance(obj, list):
            for item in obj:
                collect_symbols(item)

    for style in styles.values():
        collect_symbols(style)

    unknown_symbols = [s for s in sorted(all_style_symbols, key=lambda x: int(x[1:])) if s not in known_symbols]
    known_used = [s for s in sorted(all_style_symbols, key=lambda x: int(x[1:])) if s in known_symbols]

    print(f"Total unique symbols in styles: {len(all_style_symbols)}")
    print(f"Known symbols: {len(known_used)}")
    print(f"Unknown symbols: {len(unknown_symbols)}")
    print()

    if unknown_symbols:
        print("UNKNOWN SYMBOLS (need to add to writer.rs):")
        for sym in unknown_symbols:
            print(f"  {sym}")
        print()

    print("Known symbols used:")
    for sym in known_used:
        print(f"  {format_symbol(sym)}")

if __name__ == "__main__":
    main()
