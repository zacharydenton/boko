#!/usr/bin/env python3
"""
KFX Smart Diff - Compare KFX files by matching similar fragments.

Unlike kfx_diff.py which compares by fragment ID, this tool matches
fragments by their content structure and shows meaningful differences.

Usage:
    python scripts/kfx_smart_diff.py reference.kfx generated.kfx [--section SECTION]
"""

import sys
import os
from pathlib import Path
from collections import defaultdict
from difflib import SequenceMatcher

sys.path.insert(0, str(Path(__file__).parent))

from kfx_loader import load_kfx
from kfx_symbols import format_symbol


def sym(s):
    """Format symbol with readable name."""
    return format_symbol(s)


def get_text_content(frag):
    """Extract text from a content fragment."""
    texts = []

    def extract(val):
        if isinstance(val, str):
            texts.append(val)
        elif hasattr(val, 'keys'):  # dict-like
            # Check for text field ($144)
            if "$144" in val:
                texts.append(str(val["$144"]))
            for v in val.values():
                extract(v)
        elif isinstance(val, (list, tuple)):
            for item in val:
                extract(item)

    extract(frag.value)
    return " ".join(texts)


def get_style_name(frag):
    """Get style name from style fragment."""
    if isinstance(frag.value, dict):
        return frag.value.get("$173", str(frag.fid))
    return str(frag.fid)


def get_section_name(frag):
    """Get section identifier."""
    if isinstance(frag.value, dict):
        # Try to get section name or content name
        name = frag.value.get("$174") or frag.value.get("$176")
        if name:
            return str(name)
    return str(frag.fid)


def get_storyline_structure(frag):
    """Get storyline content structure for matching."""
    if not isinstance(frag.value, dict):
        return ""

    content = frag.value.get("$146", [])
    structure = []

    for item in content:
        if isinstance(item, dict):
            ctype = str(item.get("$159", "?"))
            structure.append(ctype)

    return "|".join(structure)


def similarity(s1, s2):
    """Calculate string similarity ratio."""
    if not s1 and not s2:
        return 1.0
    if not s1 or not s2:
        return 0.0
    return SequenceMatcher(None, s1, s2).ratio()


def match_fragments_by_content(frags1, frags2, ftype):
    """Match fragments of a type by content similarity."""
    list1 = frags1.get_all(ftype)
    list2 = frags2.get_all(ftype)

    if not list1 or not list2:
        return [], list1, list2

    # Get content signatures based on fragment type
    def get_signature(frag):
        if ftype == "$157":  # Style
            return get_style_name(frag)
        elif ftype == "$259":  # Storyline
            return get_storyline_structure(frag)
        elif ftype == "$260":  # Section
            return get_section_name(frag)
        elif ftype == "$145":  # Text content
            return get_text_content(frag)[:500]
        elif ftype == "$266":  # Anchor
            if isinstance(frag.value, dict):
                if "$186" in frag.value:  # External URL
                    return f"ext:{frag.value['$186']}"
                elif "$183" in frag.value:  # Internal
                    pos = frag.value["$183"]
                    return f"int:{pos.get('$155', '?')}"
            return str(frag.fid)
        else:
            return str(frag.value)[:200]

    sigs1 = [(frag, get_signature(frag)) for frag in list1]
    sigs2 = [(frag, get_signature(frag)) for frag in list2]

    matched = []
    used2 = set()
    unmatched1 = []

    for frag1, sig1 in sigs1:
        best_match = None
        best_score = 0.5  # Minimum threshold
        best_idx = -1

        for idx, (frag2, sig2) in enumerate(sigs2):
            if idx in used2:
                continue
            score = similarity(sig1, sig2)
            if score > best_score:
                best_score = score
                best_match = frag2
                best_idx = idx

        if best_match:
            matched.append((frag1, best_match, best_score))
            used2.add(best_idx)
        else:
            unmatched1.append(frag1)

    unmatched2 = [frag for idx, (frag, _) in enumerate(sigs2) if idx not in used2]

    return matched, unmatched1, unmatched2


def format_value_compact(val, max_depth=3, max_len=100):
    """Format value compactly for display."""
    if max_depth <= 0:
        return "..."

    if isinstance(val, dict):
        if not val:
            return "{}"
        items = []
        for k, v in list(val.items())[:5]:
            k_str = sym(k) if str(k).startswith("$") else str(k)
            v_str = format_value_compact(v, max_depth - 1, max_len // 2)
            items.append(f"{k_str}: {v_str}")
        result = "{ " + ", ".join(items)
        if len(val) > 5:
            result += f", ...+{len(val)-5}"
        result += " }"
        return result[:max_len] + "..." if len(result) > max_len else result

    elif isinstance(val, list):
        if not val:
            return "[]"
        if len(val) <= 3:
            items = [format_value_compact(v, max_depth - 1, max_len // 3) for v in val]
            return "[" + ", ".join(items) + "]"
        return f"[{len(val)} items]"

    elif isinstance(val, bytes):
        return f"<{len(val)} bytes>"

    elif isinstance(val, str):
        if len(val) > 50:
            return f'"{val[:50]}..."'
        return f'"{val}"'

    elif str(val).startswith("$"):
        return sym(val)

    else:
        s = repr(val)
        return s[:max_len] + "..." if len(s) > max_len else s


def diff_values(val1, val2, path="", max_depth=5):
    """Deep diff two values, returning list of differences."""
    diffs = []

    if max_depth <= 0:
        if val1 != val2:
            diffs.append((path, "differs", format_value_compact(val1), format_value_compact(val2)))
        return diffs

    if type(val1) != type(val2):
        diffs.append((path, "type_diff", type(val1).__name__, type(val2).__name__))
        return diffs

    if isinstance(val1, dict):
        all_keys = set(val1.keys()) | set(val2.keys())
        for k in sorted(all_keys, key=str):
            k_str = sym(k) if str(k).startswith("$") else str(k)
            k_path = f"{path}.{k_str}" if path else k_str

            if k not in val1:
                diffs.append((k_path, "added", None, format_value_compact(val2[k])))
            elif k not in val2:
                diffs.append((k_path, "removed", format_value_compact(val1[k]), None))
            elif val1[k] != val2[k]:
                diffs.extend(diff_values(val1[k], val2[k], k_path, max_depth - 1))

    elif isinstance(val1, list):
        if len(val1) != len(val2):
            diffs.append((path, "length", len(val1), len(val2)))
        # Compare element by element up to min length
        for i in range(min(len(val1), len(val2), 10)):  # Limit to first 10
            if val1[i] != val2[i]:
                diffs.extend(diff_values(val1[i], val2[i], f"{path}[{i}]", max_depth - 1))

    elif val1 != val2:
        diffs.append((path, "value", format_value_compact(val1), format_value_compact(val2)))

    return diffs


def print_fragment_diff(frag1, frag2, score, show_detail=True):
    """Print differences between two matched fragments."""
    fid1 = str(frag1.fid) if frag1.fid else "?"
    fid2 = str(frag2.fid) if frag2.fid else "?"

    print(f"\n  [{fid1}] <-> [{fid2}] (similarity: {score:.1%})")

    if not show_detail:
        return

    diffs = diff_values(frag1.value, frag2.value)

    if not diffs:
        print("    (identical content)")
        return

    for path, diff_type, v1, v2 in diffs[:15]:  # Limit output
        if diff_type == "added":
            print(f"    + {path}: {v2}")
        elif diff_type == "removed":
            print(f"    - {path}: {v1}")
        elif diff_type == "length":
            print(f"    ~ {path}: {v1} items vs {v2} items")
        elif diff_type == "type_diff":
            print(f"    ! {path}: type {v1} vs {v2}")
        else:
            print(f"    ~ {path}:")
            print(f"        ref: {v1}")
            print(f"        gen: {v2}")

    if len(diffs) > 15:
        print(f"    ... and {len(diffs) - 15} more differences")


def analyze_text_content(frags1, frags2):
    """Analyze text content differences in detail."""
    print("\n" + "=" * 70)
    print(" TEXT CONTENT ANALYSIS ($145)")
    print("=" * 70)

    # Get all text from each file
    texts1 = []
    texts2 = []

    for frag in frags1.get_all("$145"):
        text = get_text_content(frag)
        if text:
            texts1.append((str(frag.fid), text))

    for frag in frags2.get_all("$145"):
        text = get_text_content(frag)
        if text:
            texts2.append((str(frag.fid), text))

    print(f"\n  Reference: {len(texts1)} text blocks")
    print(f"  Generated: {len(texts2)} text blocks")

    # Total character count
    total1 = sum(len(t) for _, t in texts1)
    total2 = sum(len(t) for _, t in texts2)
    print(f"\n  Total chars: {total1:,} vs {total2:,}")

    # Combine all text and compare
    full1 = " ".join(t for _, t in texts1)
    full2 = " ".join(t for _, t in texts2)

    if full1 == full2:
        print("\n  Text content is IDENTICAL")
    else:
        sim = similarity(full1, full2)
        print(f"\n  Overall text similarity: {sim:.1%}")

        # Find first difference
        for i, (c1, c2) in enumerate(zip(full1, full2)):
            if c1 != c2:
                context1 = full1[max(0, i-20):i+50]
                context2 = full2[max(0, i-20):i+50]
                print(f"\n  First difference at char {i}:")
                print(f"    ref: ...{context1}...")
                print(f"    gen: ...{context2}...")
                break

        if len(full1) != len(full2):
            print(f"\n  Length difference: {len(full1)} vs {len(full2)} ({len(full2) - len(full1):+d})")


def analyze_styles(frags1, frags2):
    """Analyze style differences in detail."""
    print("\n" + "=" * 70)
    print(" STYLE ANALYSIS ($157)")
    print("=" * 70)

    styles1 = {get_style_name(f): f for f in frags1.get_all("$157")}
    styles2 = {get_style_name(f): f for f in frags2.get_all("$157")}

    print(f"\n  Reference styles: {len(styles1)}")
    print(f"  Generated styles: {len(styles2)}")

    common = set(styles1.keys()) & set(styles2.keys())
    only1 = set(styles1.keys()) - set(styles2.keys())
    only2 = set(styles2.keys()) - set(styles1.keys())

    print(f"\n  Common: {len(common)}")
    print(f"  Only in reference: {len(only1)}")
    print(f"  Only in generated: {len(only2)}")

    if only1:
        print(f"\n  Missing from generated:")
        for name in sorted(only1)[:10]:
            print(f"    - {name}")
        if len(only1) > 10:
            print(f"    ... and {len(only1) - 10} more")

    if only2:
        print(f"\n  Extra in generated:")
        for name in sorted(only2)[:10]:
            print(f"    + {name}")
        if len(only2) > 10:
            print(f"    ... and {len(only2) - 10} more")

    # Compare common styles
    different = []
    for name in common:
        s1 = styles1[name].value
        s2 = styles2[name].value
        if s1 != s2:
            different.append(name)

    if different:
        print(f"\n  Styles with different properties: {len(different)}")
        for name in sorted(different)[:5]:
            print(f"\n    Style '{name}':")
            diffs = diff_values(styles1[name].value, styles2[name].value)
            for path, diff_type, v1, v2 in diffs[:5]:
                if diff_type == "added":
                    print(f"      + {path}: {v2}")
                elif diff_type == "removed":
                    print(f"      - {path}: {v1}")
                else:
                    print(f"      ~ {path}: {v1} -> {v2}")


def analyze_sections(frags1, frags2):
    """Analyze section structure differences."""
    print("\n" + "=" * 70)
    print(" SECTION STRUCTURE ANALYSIS ($260)")
    print("=" * 70)

    sections1 = frags1.get_all("$260")
    sections2 = frags2.get_all("$260")

    print(f"\n  Reference sections: {len(sections1)}")
    print(f"  Generated sections: {len(sections2)}")

    # Match by index (assuming same order)
    for i, (s1, s2) in enumerate(zip(sections1, sections2)):
        print(f"\n  Section {i}:")

        # Compare templates
        t1 = s1.value.get("$141", [])
        t2 = s2.value.get("$141", [])

        if len(t1) != len(t2):
            print(f"    Template count: {len(t1)} vs {len(t2)}")

        for j, (tpl1, tpl2) in enumerate(zip(t1, t2)):
            if tpl1 != tpl2:
                print(f"    Template {j} differs:")
                diffs = diff_values(tpl1, tpl2)
                for path, diff_type, v1, v2 in diffs[:3]:
                    print(f"      {path}: {v1} -> {v2}")


def analyze_storylines(frags1, frags2):
    """Analyze storyline/content block differences."""
    print("\n" + "=" * 70)
    print(" STORYLINE ANALYSIS ($259)")
    print("=" * 70)

    stories1 = frags1.get_all("$259")
    stories2 = frags2.get_all("$259")

    print(f"\n  Reference storylines: {len(stories1)}")
    print(f"  Generated storylines: {len(stories2)}")

    # Match by content structure
    matched, unmatched1, unmatched2 = match_fragments_by_content(frags1, frags2, "$259")

    print(f"\n  Matched: {len(matched)}")
    print(f"  Unmatched in reference: {len(unmatched1)}")
    print(f"  Unmatched in generated: {len(unmatched2)}")

    # Show matched pairs with differences
    for frag1, frag2, score in matched[:5]:
        content1 = frag1.value.get("$146", [])
        content2 = frag2.value.get("$146", [])

        if len(content1) != len(content2):
            print(f"\n  [{frag1.fid}] <-> [{frag2.fid}] (match: {score:.1%})")
            print(f"    Content items: {len(content1)} vs {len(content2)}")

            # Show content type breakdown
            types1 = defaultdict(int)
            types2 = defaultdict(int)
            for item in content1:
                types1[str(item.get("$159", "?"))] += 1
            for item in content2:
                types2[str(item.get("$159", "?"))] += 1

            all_types = set(types1.keys()) | set(types2.keys())
            for t in sorted(all_types):
                c1, c2 = types1.get(t, 0), types2.get(t, 0)
                if c1 != c2:
                    print(f"      {sym(t)}: {c1} vs {c2}")


def analyze_anchors(frags1, frags2):
    """Analyze anchor differences."""
    print("\n" + "=" * 70)
    print(" ANCHOR ANALYSIS ($266)")
    print("=" * 70)

    anchors1 = frags1.get_all("$266")
    anchors2 = frags2.get_all("$266")

    # Categorize anchors
    ext1, int1 = [], []
    ext2, int2 = [], []

    for frag in anchors1:
        if isinstance(frag.value, dict):
            if "$186" in frag.value:
                ext1.append(frag.value["$186"])
            elif "$183" in frag.value:
                int1.append(frag.value["$183"])

    for frag in anchors2:
        if isinstance(frag.value, dict):
            if "$186" in frag.value:
                ext2.append(frag.value["$186"])
            elif "$183" in frag.value:
                int2.append(frag.value["$183"])

    print(f"\n  Reference: {len(ext1)} external, {len(int1)} internal = {len(anchors1)} total")
    print(f"  Generated: {len(ext2)} external, {len(int2)} internal = {len(anchors2)} total")

    # Compare external URLs
    ext1_set = set(ext1)
    ext2_set = set(ext2)

    common_ext = ext1_set & ext2_set
    only_ref = ext1_set - ext2_set
    only_gen = ext2_set - ext1_set

    print(f"\n  External URLs:")
    print(f"    Common: {len(common_ext)}")
    print(f"    Only in reference: {len(only_ref)}")
    print(f"    Only in generated: {len(only_gen)}")

    if only_ref:
        print(f"\n    Missing external links:")
        for url in list(only_ref)[:5]:
            print(f"      - {url[:70]}...")

    if only_gen:
        print(f"\n    Extra external links:")
        for url in list(only_gen)[:5]:
            print(f"      + {url[:70]}...")

    # Difference in internal anchors
    print(f"\n  Internal anchors: {len(int1)} vs {len(int2)} ({len(int2) - len(int1):+d})")


def smart_diff(file1, file2, sections=None):
    """Perform smart diff between two KFX files."""
    print(f"Smart KFX Diff")
    print(f"  Reference: {file1}")
    print(f"  Generated: {file2}")
    print()

    frags1, method1 = load_kfx(file1)
    frags2, method2 = load_kfx(file2)

    print(f"  Loaded reference: {len(frags1.all_fragments)} fragments ({method1})")
    print(f"  Loaded generated: {len(frags2.all_fragments)} fragments ({method2})")

    # Fragment count summary
    print("\n" + "=" * 70)
    print(" FRAGMENT COUNT SUMMARY")
    print("=" * 70)

    all_types = sorted(set(frags1.types()) | set(frags2.types()))
    for ftype in all_types:
        c1 = frags1.count(ftype)
        c2 = frags2.count(ftype)
        diff = c2 - c1
        marker = " " if c1 == c2 else ("+" if diff > 0 else "-")
        diff_str = f"({diff:+d})" if diff != 0 else ""
        print(f"  {marker} {sym(ftype)}: {c1} vs {c2} {diff_str}")

    if sections is None or "all" in sections:
        sections = ["text", "styles", "sections", "storylines", "anchors"]

    if "text" in sections:
        analyze_text_content(frags1, frags2)

    if "styles" in sections:
        analyze_styles(frags1, frags2)

    if "sections" in sections:
        analyze_sections(frags1, frags2)

    if "storylines" in sections:
        analyze_storylines(frags1, frags2)

    if "anchors" in sections:
        analyze_anchors(frags1, frags2)

    print("\n" + "=" * 70)
    print(" ANALYSIS COMPLETE")
    print("=" * 70)


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Smart KFX diff with content matching")
    parser.add_argument("reference", help="Reference KFX file")
    parser.add_argument("generated", help="Generated KFX file to compare")
    parser.add_argument("--section", "-s", action="append",
                        choices=["text", "styles", "sections", "storylines", "anchors", "all"],
                        help="Section(s) to analyze (default: all)")

    args = parser.parse_args()

    if not os.path.exists(args.reference):
        print(f"Error: {args.reference} not found")
        sys.exit(1)
    if not os.path.exists(args.generated):
        print(f"Error: {args.generated} not found")
        sys.exit(1)

    smart_diff(args.reference, args.generated, args.section)


if __name__ == "__main__":
    main()
