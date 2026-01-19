#!/usr/bin/env python3
"""Analyze CSS-to-KFX symbol mappings from a test KFX file."""

import sys
from pathlib import Path
from collections import defaultdict

# Add scripts to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from kfx_dump import load_kfx, format_value


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


def main():
    if len(sys.argv) < 2:
        print("Usage: analyze_css_mapping.py <kfx_file>")
        sys.exit(1)

    kfx_path = sys.argv[1]
    fragments, method = load_kfx(kfx_path)
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

    # Find the baseline style (first paragraph, no CSS)
    baseline_style_id = text_to_style.get(0)
    baseline_style = styles.get(baseline_style_id, {})

    print("=" * 80)
    print(f"BASELINE STYLE (ID: {baseline_style_id}):")
    print("-" * 40)
    for k, v in sorted(baseline_style.items()):
        print(f"  {k}: {v}")
    print()

    # Create mapping: CSS property -> KFX symbols that differ from baseline
    print("=" * 80)
    print("CSS PROPERTY TO KFX SYMBOL MAPPING")
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
                mappings.append((css_prop_value, diffs, style_id))

    # Sort by CSS property
    mappings.sort(key=lambda x: x[0])

    # Group by KFX property for cleaner output
    by_kfx_key = defaultdict(list)
    for css, diffs, style_id in mappings:
        # Convert to string key since dict values aren't hashable
        key = str(sorted(diffs.items(), key=lambda x: str(x)))
        by_kfx_key[key].append((css, diffs))

    # Print grouped results
    for kfx_key, css_list in sorted(by_kfx_key.items(), key=lambda x: str(x[0])):
        # Get the diffs from the first item
        diffs = css_list[0][1]
        kfx_str = ", ".join(f"{k}={v}" for k, v in sorted(diffs.items(), key=lambda x: str(x)))
        print(f"KFX: {kfx_str}")
        for css, _ in css_list:
            print(f"  <- {css}")
        print()

    # Also print a direct table
    print()
    print("=" * 80)
    print("DIRECT TABLE (CSS -> KFX)")
    print("=" * 80)
    print()

    for css, diffs, style_id in mappings:
        kfx_str = ", ".join(f"{k}={v}" for k, v in sorted(diffs.items(), key=lambda x: str(x)))
        print(f"{css:45} -> {kfx_str}")

if __name__ == "__main__":
    main()
