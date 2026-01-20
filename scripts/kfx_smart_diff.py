#!/usr/bin/env python3
"""
KFX Smart Diff - Compare KFX files with semantic understanding.

Compares KFX files by matching fragments based on content rather than IDs,
since symbol IDs are arbitrary and differ between generators.

Usage:
    python scripts/kfx_smart_diff.py file1.kfx file2.kfx [--section SECTION]
"""

import sys
import os
from pathlib import Path
from collections import defaultdict
from difflib import SequenceMatcher

sys.path.insert(0, str(Path(__file__).parent))

from kfx_loader import load_kfx
from kfx_symbols import format_symbol


# ANSI colors (disabled if not a tty)
if sys.stdout.isatty():
    GREEN = "\033[32m"
    RED = "\033[31m"
    YELLOW = "\033[33m"
    CYAN = "\033[36m"
    BOLD = "\033[1m"
    RESET = "\033[0m"
else:
    GREEN = RED = YELLOW = CYAN = BOLD = RESET = ""


def sym(s):
    """Format symbol with readable name."""
    return format_symbol(s)


def unwrap_annotation(val):
    """Unwrap IonAnnotation to get inner value."""
    if hasattr(val, 'value') and hasattr(val, 'annotations'):
        return val.value
    return val


def extract_pure_text(frag):
    """Extract actual text strings from a fragment, ignoring symbol IDs."""
    texts = []

    def extract(val):
        val = unwrap_annotation(val)
        if isinstance(val, str):
            # Skip symbol-like strings
            if not val.startswith('$') and len(val) > 1:
                texts.append(val)
        elif hasattr(val, 'keys'):  # dict-like
            for v in val.values():
                extract(v)
        elif isinstance(val, (list, tuple)):
            for item in val:
                extract(item)

    extract(frag.value)
    return texts


def get_style_properties(frag):
    """Get style properties as a normalized dict for comparison."""
    val = unwrap_annotation(frag.value)
    if not hasattr(val, 'keys'):
        return {}

    props = {}
    # Skip name/id fields, keep actual style properties
    skip_keys = {'$173', '$176', 'version'}  # style name, content name

    for k, v in val.items():
        k_str = str(k)
        if k_str not in skip_keys:
            props[k_str] = v

    return props


def props_signature(props):
    """Create a signature from style properties for matching."""
    # Sort keys and create a hashable representation
    items = []
    for k in sorted(props.keys()):
        v = props[k]
        if hasattr(v, 'keys'):
            # For nested dicts, just use the keys
            items.append((k, tuple(sorted(str(x) for x in v.keys()))))
        else:
            items.append((k, str(v)[:50]))
    return tuple(items)


def similarity(s1, s2):
    """Calculate string similarity ratio."""
    if not s1 and not s2:
        return 1.0
    if not s1 or not s2:
        return 0.0
    return SequenceMatcher(None, s1, s2).ratio()


def format_value_compact(val, max_depth=2, max_len=80):
    """Format value compactly for display."""
    val = unwrap_annotation(val)

    if max_depth <= 0:
        return "..."

    if hasattr(val, 'keys'):
        if not val:
            return "{}"
        items = []
        for k, v in list(val.items())[:4]:
            k_str = sym(k) if str(k).startswith("$") else str(k)
            v_str = format_value_compact(v, max_depth - 1, max_len // 2)
            items.append(f"{k_str}: {v_str}")
        result = "{ " + ", ".join(items)
        if len(val) > 4:
            result += f", ...+{len(val)-4}"
        result += " }"
        return result[:max_len] + "..." if len(result) > max_len else result

    elif isinstance(val, (list, tuple)):
        if not val:
            return "[]"
        if len(val) <= 2:
            items = [format_value_compact(v, max_depth - 1, max_len // 2) for v in val]
            return "[" + ", ".join(items) + "]"
        return f"[{len(val)} items]"

    elif isinstance(val, bytes):
        return f"<{len(val)} bytes>"

    elif isinstance(val, str):
        if val.startswith('$'):
            return sym(val)
        if len(val) > 40:
            return f'"{val[:40]}..."'
        return f'"{val}"'

    else:
        s = str(val)
        if s.startswith('$'):
            return sym(s)
        return s[:max_len] + "..." if len(s) > max_len else s


def analyze_text_content(frags1, frags2):
    """Analyze text content differences."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" TEXT CONTENT ($145)")
    print(f"{'=' * 70}{RESET}")

    # Extract pure text from each file
    texts1 = []
    texts2 = []

    for frag in frags1.get_all("$145"):
        texts1.extend(extract_pure_text(frag))

    for frag in frags2.get_all("$145"):
        texts2.extend(extract_pure_text(frag))

    n1, n2 = len(frags1.get_all("$145")), len(frags2.get_all("$145"))
    print(f"\n  Text blocks: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({n2-n1:+d}){RESET}")

    # Total character count
    total1 = sum(len(t) for t in texts1)
    total2 = sum(len(t) for t in texts2)
    print(f"  Total text: {total1:,} vs {total2:,} chars", end="")
    if abs(total1 - total2) < 100:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({total2-total1:+d}){RESET}")

    # Compare actual text content
    full1 = " ".join(texts1)
    full2 = " ".join(texts2)

    if full1 == full2:
        print(f"\n  {GREEN}Text content is IDENTICAL{RESET}")
    else:
        sim = similarity(full1, full2)
        color = GREEN if sim > 0.99 else YELLOW if sim > 0.9 else RED
        print(f"\n  Text similarity: {color}{sim:.1%}{RESET}")

        # Find first difference
        for i, (c1, c2) in enumerate(zip(full1, full2)):
            if c1 != c2:
                ctx1 = full1[max(0, i-30):i+30]
                ctx2 = full2[max(0, i-30):i+30]
                print(f"\n  First diff at char {i}:")
                print(f"    file1: ...{ctx1}...")
                print(f"    file2: ...{ctx2}...")
                break


def analyze_styles(frags1, frags2):
    """Analyze style differences by property content, not symbol IDs."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" STYLES ($157)")
    print(f"{'=' * 70}{RESET}")

    styles1 = [(frag, get_style_properties(frag)) for frag in frags1.get_all("$157")]
    styles2 = [(frag, get_style_properties(frag)) for frag in frags2.get_all("$157")]

    n1, n2 = len(styles1), len(styles2)
    print(f"\n  Style count: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({n2-n1:+d}){RESET}")

    # Group styles by their property signature
    sigs1 = defaultdict(list)
    sigs2 = defaultdict(list)

    for frag, props in styles1:
        sig = props_signature(props)
        sigs1[sig].append((frag, props))

    for frag, props in styles2:
        sig = props_signature(props)
        sigs2[sig].append((frag, props))

    common_sigs = set(sigs1.keys()) & set(sigs2.keys())
    only1 = set(sigs1.keys()) - set(sigs2.keys())
    only2 = set(sigs2.keys()) - set(sigs1.keys())

    # Count styles by signature match
    matched1 = sum(len(sigs1[sig]) for sig in common_sigs)
    matched2 = sum(len(sigs2[sig]) for sig in common_sigs)

    print(f"  Matching style signatures: {len(common_sigs)}")
    print(f"  Unique to file1: {len(only1)} ({sum(len(sigs1[s]) for s in only1)} styles)")
    print(f"  Unique to file2: {len(only2)} ({sum(len(sigs2[s]) for s in only2)} styles)")

    if only1:
        print(f"\n  {YELLOW}Styles only in file1:{RESET}")
        shown = 0
        for sig in list(only1)[:3]:
            for frag, props in sigs1[sig][:1]:
                print(f"    - {format_value_compact(props)}")
                shown += 1
        remaining = sum(len(sigs1[s]) for s in only1) - shown
        if remaining > 0:
            print(f"    ... and {remaining} more")

    if only2:
        print(f"\n  {YELLOW}Styles only in file2:{RESET}")
        shown = 0
        for sig in list(only2)[:3]:
            for frag, props in sigs2[sig][:1]:
                print(f"    + {format_value_compact(props)}")
                shown += 1
        remaining = sum(len(sigs2[s]) for s in only2) - shown
        if remaining > 0:
            print(f"    ... and {remaining} more")


def analyze_sections(frags1, frags2):
    """Analyze section structure."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" SECTIONS ($260)")
    print(f"{'=' * 70}{RESET}")

    sections1 = frags1.get_all("$260")
    sections2 = frags2.get_all("$260")

    n1, n2 = len(sections1), len(sections2)
    print(f"\n  Section count: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {RED}({n2-n1:+d}){RESET}")

    # Compare section by section
    for i, (s1, s2) in enumerate(zip(sections1, sections2)):
        v1 = unwrap_annotation(s1.value)
        v2 = unwrap_annotation(s2.value)

        # Compare template counts
        t1 = v1.get("$141", []) if hasattr(v1, 'get') else []
        t2 = v2.get("$141", []) if hasattr(v2, 'get') else []

        if len(t1) != len(t2):
            print(f"\n  Section {i}: template count {len(t1)} vs {len(t2)}")


def analyze_storylines(frags1, frags2):
    """Analyze storyline content."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" STORYLINES ($259)")
    print(f"{'=' * 70}{RESET}")

    stories1 = frags1.get_all("$259")
    stories2 = frags2.get_all("$259")

    n1, n2 = len(stories1), len(stories2)
    print(f"\n  Storyline count: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {RED}({n2-n1:+d}){RESET}")

    # Compare content item counts
    def get_content_count(frag):
        val = unwrap_annotation(frag.value)
        if hasattr(val, 'get'):
            content = val.get("$146", [])
            return len(content)
        return 0

    total1 = sum(get_content_count(s) for s in stories1)
    total2 = sum(get_content_count(s) for s in stories2)

    print(f"  Total content items: {total1} vs {total2}", end="")
    if total1 == total2:
        print(f" {GREEN}✓{RESET}")
    else:
        pct = abs(total2 - total1) / max(total1, 1) * 100
        color = YELLOW if pct < 10 else RED
        print(f" {color}({total2-total1:+d}, {pct:.1f}%){RESET}")


def analyze_anchors(frags1, frags2):
    """Analyze anchor differences."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" ANCHORS ($266)")
    print(f"{'=' * 70}{RESET}")

    anchors1 = frags1.get_all("$266")
    anchors2 = frags2.get_all("$266")

    # Categorize anchors
    def categorize(anchors):
        ext, internal = [], []
        for frag in anchors:
            val = unwrap_annotation(frag.value)
            if hasattr(val, 'get'):
                if "$186" in val:
                    ext.append(val["$186"])
                elif "$183" in val:
                    internal.append(val["$183"])
        return ext, internal

    ext1, int1 = categorize(anchors1)
    ext2, int2 = categorize(anchors2)

    print(f"\n  External anchors: {len(ext1)} vs {len(ext2)}", end="")
    if len(ext1) == len(ext2):
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({len(ext2)-len(ext1):+d}){RESET}")

    print(f"  Internal anchors: {len(int1)} vs {len(int2)}", end="")
    if len(int1) == len(int2):
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({len(int2)-len(int1):+d}){RESET}")

    # Check external URL coverage
    ext1_set = set(ext1)
    ext2_set = set(ext2)

    missing = ext1_set - ext2_set
    extra = ext2_set - ext1_set

    if missing:
        print(f"\n  {RED}External URLs only in file1:{RESET}")
        for url in list(missing)[:3]:
            print(f"    - {url[:60]}...")

    if extra:
        print(f"\n  {YELLOW}External URLs only in file2:{RESET}")
        for url in list(extra)[:3]:
            print(f"    + {url[:60]}...")


def smart_diff(file1, file2, sections=None):
    """Perform smart diff between two KFX files."""
    print(f"{BOLD}KFX Smart Diff{RESET}")
    print(f"  File 1: {file1}")
    print(f"  File 2: {file2}")
    print()

    frags1, method1 = load_kfx(file1)
    frags2, method2 = load_kfx(file2)

    print(f"  Loaded: {len(frags1.all_fragments)} vs {len(frags2.all_fragments)} fragments")

    # Fragment count summary
    print(f"\n{BOLD}{'=' * 70}")
    print(f" FRAGMENT SUMMARY")
    print(f"{'=' * 70}{RESET}")

    all_types = sorted(set(frags1.types()) | set(frags2.types()))
    matches = 0
    for ftype in all_types:
        c1 = frags1.count(ftype)
        c2 = frags2.count(ftype)
        diff = c2 - c1

        if c1 == c2:
            marker = f"{GREEN}✓{RESET}"
            matches += 1
        else:
            marker = f"{YELLOW}{diff:+d}{RESET}" if abs(diff) < 10 else f"{RED}{diff:+d}{RESET}"

        print(f"  {sym(ftype):.<30} {c1:>4} vs {c2:<4} {marker}")

    print(f"\n  {matches}/{len(all_types)} fragment types match exactly")

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

    print(f"\n{BOLD}{'=' * 70}")
    print(f" COMPLETE")
    print(f"{'=' * 70}{RESET}")


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Smart KFX diff with semantic matching")
    parser.add_argument("file1", help="First KFX file")
    parser.add_argument("file2", help="Second KFX file to compare")
    parser.add_argument("--section", "-s", action="append",
                        choices=["text", "styles", "sections", "storylines", "anchors", "all"],
                        help="Section(s) to analyze (default: all)")

    args = parser.parse_args()

    if not os.path.exists(args.file1):
        print(f"Error: {args.file1} not found")
        sys.exit(1)
    if not os.path.exists(args.file2):
        print(f"Error: {args.file2} not found")
        sys.exit(1)

    smart_diff(args.file1, args.file2, args.section)


if __name__ == "__main__":
    main()
