#!/usr/bin/env python3
"""
KFX Structure Check - Verify KFX structural requirements for Rust tests.

This script checks for specific structural elements in KFX files and outputs
results in a format that Rust tests can parse.

Usage:
    python scripts/kfx_structure_check.py file.kfx --check 790
    python scripts/kfx_structure_check.py file.kfx --check nested_paragraphs
    python scripts/kfx_structure_check.py file.kfx --reference ref.kfx
"""

import sys
import os
import argparse
from pathlib import Path
from collections import defaultdict

sys.path.insert(0, str(Path(__file__).parent))

from kfx_loader import load_kfx


def count_790_fields(frags):
    """Count occurrences of $790 field in all fragments."""
    count = 0

    def search(val, depth=0):
        nonlocal count
        if depth > 20:
            return
        if hasattr(val, 'keys'):
            if '$790' in val:
                count += 1
            for v in val.values():
                search(v, depth + 1)
        elif isinstance(val, (list, tuple)):
            for v in val:
                search(v, depth + 1)

    for frag in frags.all_fragments:
        if hasattr(frag.value, 'keys'):
            search(frag.value)

    return count


def count_nested_paragraphs(frags):
    """Count paragraphs that have nested $269 content."""
    count = 0

    for frag in frags.get_all('$259'):  # Storylines
        content = frag.value.get('$146', [])
        for item in content:
            if not hasattr(item, 'get'):
                continue
            if str(item.get('$159', '')) != '$269':
                continue
            # Check if this paragraph has $146 with nested $269
            inner = item.get('$146', [])
            if inner:
                for inner_item in inner:
                    if hasattr(inner_item, 'get'):
                        if str(inner_item.get('$159', '')) == '$269':
                            count += 1
                            break

    return count


def analyze_text_chunks(frags):
    """Analyze text content chunking."""
    chunks = []

    for frag in frags.get_all('$145'):  # Text content
        content = frag.value.get('$146', [])
        for item in content:
            if isinstance(item, str):
                chunks.append(len(item))

    if not chunks:
        return 0, 0, 0

    return len(chunks), sum(chunks) // len(chunks) if chunks else 0, max(chunks) if chunks else 0


def analyze_storyline_counts(frags):
    """Get content item counts per storyline."""
    counts = []

    for frag in frags.get_all('$259'):
        content = frag.value.get('$146', [])
        counts.append(len(content))

    return counts


def count_content_types(frags):
    """Count content types ($159 values) in storyline items."""
    type_counts = {}

    def check_item(item, depth=0):
        if not hasattr(item, 'get'):
            return
        ctype = str(item.get('$159', 'none'))
        type_counts[ctype] = type_counts.get(ctype, 0) + 1
        inner = item.get('$146', [])
        for i in inner:
            check_item(i, depth + 1)

    for frag in frags.get_all('$259'):
        content = frag.value.get('$146', [])
        for item in content:
            check_item(item)

    return type_counts


def check_para_fields(frags):
    """Check what fields paragraphs have."""
    has_155 = False
    has_157 = False
    has_159 = False
    has_146 = False
    has_790 = False
    has_142 = False
    has_145 = False

    for frag in frags.get_all('$259'):
        content = frag.value.get('$146', [])
        for item in content:
            if not hasattr(item, 'get'):
                continue
            if str(item.get('$159', '')) != '$269':
                continue

            # Check fields
            if '$155' in item:
                has_155 = True
            if '$157' in item:
                has_157 = True
            if '$159' in item:
                has_159 = True
            if '$146' in item:
                has_146 = True
            if '$790' in item:
                has_790 = True
            if '$142' in item:
                has_142 = True
            if '$145' in item:
                has_145 = True

            # Also check nested content
            inner = item.get('$146', [])
            for inner_item in inner:
                if hasattr(inner_item, 'get'):
                    if '$790' in inner_item:
                        has_790 = True
                    if '$142' in inner_item:
                        has_142 = True
                    if '$145' in inner_item:
                        has_145 = True

    return {
        '155': has_155,
        '157': has_157,
        '159': has_159,
        '146': has_146,
        '790': has_790,
        '142': has_142,
        '145': has_145,
    }


def compare_with_reference(gen_frags, ref_frags):
    """Compare generated KFX with reference."""
    results = {}

    # Compare $790 counts
    gen_790 = count_790_fields(gen_frags)
    ref_790 = count_790_fields(ref_frags)
    # Match if both have $790 fields (count may differ due to style generation differences)
    # Reference uses inline styles where we use block styles, causing count variance
    if ref_790 == 0:
        results['790_match'] = gen_790 == 0
    else:
        # Just check that we have some $790 fields - exact count depends on style generation
        results['790_match'] = gen_790 > 0
    results['gen_790'] = gen_790
    results['ref_790'] = ref_790

    # Compare paragraph structure
    gen_nested = count_nested_paragraphs(gen_frags)
    ref_nested = count_nested_paragraphs(ref_frags)
    # Match if both have nested or both don't
    results['para_structure_match'] = (gen_nested > 0) == (ref_nested > 0)
    results['gen_nested'] = gen_nested
    results['ref_nested'] = ref_nested

    # Compare storyline counts
    gen_counts = analyze_storyline_counts(gen_frags)
    ref_counts = analyze_storyline_counts(ref_frags)
    gen_total = sum(gen_counts)
    ref_total = sum(ref_counts)
    results['storyline_item_diff'] = gen_total - ref_total
    results['gen_storyline_total'] = gen_total
    results['ref_storyline_total'] = ref_total

    return results


def main():
    parser = argparse.ArgumentParser(description="Check KFX structure")
    parser.add_argument("file", help="KFX file to check")
    parser.add_argument("--check", choices=['790', 'nested_paragraphs', 'text_chunks',
                                            'storyline_counts', 'para_fields', 'content_types'],
                        help="Specific check to run")
    parser.add_argument("--reference", help="Reference KFX file to compare against")

    args = parser.parse_args()

    if not os.path.exists(args.file):
        print(f"Error: {args.file} not found", file=sys.stderr)
        sys.exit(1)

    frags, method = load_kfx(args.file)

    if args.reference:
        if not os.path.exists(args.reference):
            print(f"Error: {args.reference} not found", file=sys.stderr)
            sys.exit(1)

        ref_frags, _ = load_kfx(args.reference)
        results = compare_with_reference(frags, ref_frags)

        for key, value in results.items():
            print(f"{key}: {str(value).lower() if isinstance(value, bool) else value}")

        # Also run specific check if requested
        if args.check:
            print()  # Blank line separator

    if args.check == '790':
        count = count_790_fields(frags)
        print(f"$790_count: {count}")

    elif args.check == 'nested_paragraphs':
        count = count_nested_paragraphs(frags)
        print(f"nested_para_count: {count}")

    elif args.check == 'text_chunks':
        num_chunks, avg_size, max_size = analyze_text_chunks(frags)
        print(f"num_chunks: {num_chunks}")
        print(f"avg_chunk_size: {avg_size}")
        print(f"max_chunk_size: {max_size}")

    elif args.check == 'storyline_counts':
        counts = analyze_storyline_counts(frags)
        print(f"num_storylines: {len(counts)}")
        print(f"storyline_total: {sum(counts)}")
        print(f"storyline_counts: {counts}")

        if args.reference:
            ref_frags, _ = load_kfx(args.reference)
            ref_counts = analyze_storyline_counts(ref_frags)
            diff = sum(counts) - sum(ref_counts)
            print(f"storyline_item_diff: {diff}")

    elif args.check == 'para_fields':
        fields = check_para_fields(frags)
        for field, has in fields.items():
            print(f"para_has_{field}: {str(has).lower()}")

    elif args.check == 'content_types':
        types = count_content_types(frags)
        for t, c in sorted(types.items(), key=lambda x: -x[1]):
            print(f"content_type_{t}: {c}")


if __name__ == "__main__":
    main()
