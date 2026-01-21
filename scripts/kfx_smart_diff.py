#!/usr/bin/env python3
"""
KFX Smart Diff - Deep semantic comparison of KFX files.

Compares KFX files by matching fragments based on content rather than IDs,
since symbol IDs are arbitrary and differ between generators. Shows ALL
differences with full deep diffs.

Usage:
    python scripts/kfx_smart_diff.py file1.kfx file2.kfx [--section SECTION]
    python scripts/kfx_smart_diff.py file1.kfx file2.kfx -s styles  # only styles
    python scripts/kfx_smart_diff.py file1.kfx file2.kfx --content endnotes  # focus on endnotes
"""

import sys
import os
import zipfile
import re
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
    DIM = "\033[2m"
    RESET = "\033[0m"
else:
    GREEN = RED = YELLOW = CYAN = BOLD = DIM = RESET = ""


def sym(s):
    """Format symbol with readable name."""
    return format_symbol(s)


def unwrap_annotation(val):
    """Unwrap IonAnnotation to get inner value."""
    if hasattr(val, 'value') and hasattr(val, 'annotations'):
        return val.value
    return val


def normalize_value(val, depth=0):
    """
    Recursively normalize an Ion value for comparison.
    - Unwraps annotations
    - Converts symbol IDs to readable names
    - Sorts dict keys for consistent comparison
    """
    val = unwrap_annotation(val)

    if hasattr(val, 'keys'):  # dict-like
        result = {}
        for k, v in val.items():
            k_str = sym(str(k)) if str(k).startswith('$') else str(k)
            result[k_str] = normalize_value(v, depth + 1)
        return result

    elif isinstance(val, (list, tuple)):
        return [normalize_value(v, depth + 1) for v in val]

    elif isinstance(val, str):
        if val.startswith('$'):
            return sym(val)
        return val

    elif isinstance(val, bytes):
        return f"<{len(val)} bytes>"

    else:
        s = str(val)
        if s.startswith('$'):
            return sym(s)
        return val


def format_value(val, indent=0, max_width=100):
    """Format a normalized value for display."""
    prefix = "  " * indent

    if isinstance(val, dict):
        if not val:
            return "{}"
        lines = ["{"]
        for k, v in sorted(val.items()):
            v_str = format_value(v, indent + 1, max_width - 4)
            if '\n' in v_str:
                lines.append(f"{prefix}  {k}:")
                lines.append(v_str)
            else:
                lines.append(f"{prefix}  {k}: {v_str}")
        lines.append(f"{prefix}}}")
        return '\n'.join(lines)

    elif isinstance(val, list):
        if not val:
            return "[]"
        if len(val) <= 3 and all(not isinstance(v, (dict, list)) for v in val):
            items = [format_value(v, 0, 20) for v in val]
            short = "[" + ", ".join(items) + "]"
            if len(short) < max_width:
                return short
        lines = ["["]
        for v in val:
            v_str = format_value(v, indent + 1, max_width - 4)
            lines.append(f"{prefix}  {v_str}")
        lines.append(f"{prefix}]")
        return '\n'.join(lines)

    elif isinstance(val, str):
        if len(val) > 60:
            return f'"{val[:57]}..."'
        return f'"{val}"'

    else:
        return str(val)


def deep_diff(val1, val2, path=""):
    """
    Deep diff two normalized values.
    Returns list of (path, val1, val2) differences.
    """
    diffs = []

    if type(val1) != type(val2):
        diffs.append((path, val1, val2))
        return diffs

    if isinstance(val1, dict):
        all_keys = set(val1.keys()) | set(val2.keys())
        for k in sorted(all_keys):
            child_path = f"{path}.{k}" if path else k
            if k not in val1:
                diffs.append((child_path, None, val2[k]))
            elif k not in val2:
                diffs.append((child_path, val1[k], None))
            else:
                diffs.extend(deep_diff(val1[k], val2[k], child_path))

    elif isinstance(val1, list):
        # For lists, compare element by element
        max_len = max(len(val1), len(val2))
        for i in range(max_len):
            child_path = f"{path}[{i}]"
            if i >= len(val1):
                diffs.append((child_path, None, val2[i]))
            elif i >= len(val2):
                diffs.append((child_path, val1[i], None))
            else:
                diffs.extend(deep_diff(val1[i], val2[i], child_path))

    elif val1 != val2:
        diffs.append((path, val1, val2))

    return diffs


def similarity_score(val1, val2):
    """Calculate similarity score between two normalized values (0-1)."""
    if type(val1) != type(val2):
        return 0.0

    if isinstance(val1, dict):
        if not val1 and not val2:
            return 1.0
        all_keys = set(val1.keys()) | set(val2.keys())
        if not all_keys:
            return 1.0
        common = set(val1.keys()) & set(val2.keys())
        # Score based on key overlap and value similarity
        key_score = len(common) / len(all_keys)
        if not common:
            return key_score * 0.5
        value_scores = [similarity_score(val1[k], val2[k]) for k in common]
        value_score = sum(value_scores) / len(value_scores)
        return (key_score + value_score) / 2

    elif isinstance(val1, list):
        if not val1 and not val2:
            return 1.0
        if not val1 or not val2:
            return 0.0
        # Simple length-based similarity for lists
        return 1.0 - abs(len(val1) - len(val2)) / max(len(val1), len(val2))

    elif isinstance(val1, str):
        return SequenceMatcher(None, val1, val2).ratio()

    else:
        return 1.0 if val1 == val2 else 0.0


def get_fragment_signature(frag, ftype):
    """Get a signature for matching fragments across files."""
    val = normalize_value(frag.value)

    if ftype == "$145":  # TEXT_CONTENT
        # Use actual text content as signature
        texts = []
        def extract_text(v):
            if isinstance(v, str) and not v.startswith('$') and len(v) > 5:
                texts.append(v)
            elif isinstance(v, dict):
                for child in v.values():
                    extract_text(child)
            elif isinstance(v, list):
                for child in v:
                    extract_text(child)
        extract_text(val)
        return " ".join(texts)[:500]

    elif ftype == "$157":  # STYLE
        # Use style properties (excluding name) as signature
        if isinstance(val, dict):
            props = {k: v for k, v in val.items()
                    if k not in ('style_name', 'content_name', '$173', '$176')}
            return str(sorted(props.items()))
        return str(val)

    elif ftype == "$259":  # STORYLINE
        # Match by content structure
        if isinstance(val, dict):
            content = val.get('content_list', val.get('$146', []))
            return f"storyline:{len(content)}"
        return str(frag.fid)

    elif ftype == "$260":  # SECTION
        # Match by template count
        if isinstance(val, dict):
            templates = val.get('$141', [])
            return f"section:{len(templates)}"
        return str(frag.fid)

    else:
        # Default: use normalized value as signature
        return str(val)[:500]


def match_fragments(frags1, frags2, ftype):
    """
    Match fragments of a given type between two files.
    Returns: (matched_pairs, only_in_1, only_in_2)
    """
    list1 = [(f, normalize_value(f.value), get_fragment_signature(f, ftype))
             for f in frags1.get_all(ftype)]
    list2 = [(f, normalize_value(f.value), get_fragment_signature(f, ftype))
             for f in frags2.get_all(ftype)]

    matched = []
    used2 = set()

    for i, (f1, v1, sig1) in enumerate(list1):
        best_match = None
        best_score = 0.3  # Minimum threshold

        for j, (f2, v2, sig2) in enumerate(list2):
            if j in used2:
                continue

            # Try signature match first (fast)
            if sig1 == sig2:
                best_match = j
                best_score = 1.0
                break

            # Fall back to similarity score
            score = similarity_score(v1, v2)
            if score > best_score:
                best_score = score
                best_match = j

        if best_match is not None:
            matched.append((f1, list1[i][1], list2[best_match][0], list2[best_match][1], best_score))
            used2.add(best_match)

    only1 = [(f, v) for i, (f, v, _) in enumerate(list1)
             if not any(m[0] == f for m in matched)]
    only2 = [(f, v) for j, (f, v, _) in enumerate(list2) if j not in used2]

    return matched, only1, only2


def print_diff(path, val1, val2, indent=4):
    """Print a single diff entry."""
    prefix = " " * indent
    if val1 is None:
        print(f"{prefix}{GREEN}+ {path}: {format_value(val2, 0, 60)}{RESET}")
    elif val2 is None:
        print(f"{prefix}{RED}- {path}: {format_value(val1, 0, 60)}{RESET}")
    else:
        v1_str = format_value(val1, 0, 40)
        v2_str = format_value(val2, 0, 40)
        print(f"{prefix}{YELLOW}~ {path}:{RESET}")
        print(f"{prefix}    {RED}- {v1_str}{RESET}")
        print(f"{prefix}    {GREEN}+ {v2_str}{RESET}")


def analyze_fragment_type(frags1, frags2, ftype, name):
    """Analyze a fragment type with deep diff."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" {name} ({ftype})")
    print(f"{'=' * 70}{RESET}")

    n1 = frags1.count(ftype)
    n2 = frags2.count(ftype)

    print(f"\n  Count: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({n2-n1:+d}){RESET}")

    if n1 == 0 and n2 == 0:
        return

    matched, only1, only2 = match_fragments(frags1, frags2, ftype)

    print(f"  Matched: {len(matched)}/{max(n1, n2)}")

    # Show diffs for matched pairs
    different_count = 0
    for f1, v1, f2, v2, score in matched:
        diffs = deep_diff(v1, v2)
        if diffs:
            different_count += 1
            print(f"\n  {CYAN}Match ({score:.0%}): {f1.fid} <-> {f2.fid}{RESET}")
            for path, d1, d2 in diffs:
                print_diff(path, d1, d2)

    if different_count == 0 and matched:
        print(f"  {GREEN}All matched fragments are identical{RESET}")

    # Show unmatched
    if only1:
        print(f"\n  {RED}Only in file1: {len(only1)}{RESET}")
        for f, v in only1:
            print(f"    - {f.fid}")
            if ftype == "$157":  # Show style details
                print(f"      {format_value(v, 3, 70)}")

    if only2:
        print(f"\n  {GREEN}Only in file2: {len(only2)}{RESET}")
        for f, v in only2:
            print(f"    + {f.fid}")
            if ftype == "$157":  # Show style details
                print(f"      {format_value(v, 3, 70)}")


def analyze_text_content(frags1, frags2):
    """Special analysis for text content - compare paragraphs."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" TEXT CONTENT ($145)")
    print(f"{'=' * 70}{RESET}")

    def get_paragraphs(frags):
        paragraphs = []
        for frag in frags.get_all("$145"):
            val = normalize_value(frag.value)
            def extract(v):
                if isinstance(v, str) and not v.startswith('$') and len(v) > 5:
                    paragraphs.append(v.strip())
                elif isinstance(v, dict):
                    for child in v.values():
                        extract(child)
                elif isinstance(v, list):
                    for child in v:
                        extract(child)
            extract(val)
        return paragraphs

    n1 = frags1.count("$145")
    n2 = frags2.count("$145")
    paras1 = get_paragraphs(frags1)
    paras2 = get_paragraphs(frags2)

    print(f"\n  Text blocks: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({n2-n1:+d}){RESET}")

    print(f"  Paragraphs: {len(paras1)} vs {len(paras2)}", end="")
    if len(paras1) == len(paras2):
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({len(paras2)-len(paras1):+d}){RESET}")

    total1 = sum(len(p) for p in paras1)
    total2 = sum(len(p) for p in paras2)
    print(f"  Total characters: {total1:,} vs {total2:,}", end="")
    if abs(total1 - total2) < 100:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({total2-total1:+d}){RESET}")

    # Match paragraphs
    def normalize(s):
        return ' '.join(s.lower().split())

    norm1 = {normalize(p): p for p in paras1}
    norm2 = {normalize(p): p for p in paras2}

    common = set(norm1.keys()) & set(norm2.keys())
    only1_set = set(norm1.keys()) - set(norm2.keys())
    only2_set = set(norm2.keys()) - set(norm1.keys())

    # Fuzzy match remaining
    fuzzy_matched = 0
    truly_only1 = []
    truly_only2 = list(only2_set)

    for norm_p1 in only1_set:
        p1 = norm1[norm_p1]
        best_score = 0.85
        best_idx = None

        for i, norm_p2 in enumerate(truly_only2):
            if abs(len(norm_p1) - len(norm_p2)) > len(norm_p1) * 0.3:
                continue
            score = SequenceMatcher(None, norm_p1, norm_p2).ratio()
            if score > best_score:
                best_score = score
                best_idx = i

        if best_idx is not None:
            fuzzy_matched += 1
            truly_only2.pop(best_idx)
        else:
            truly_only1.append(p1)

    total_matched = len(common) + fuzzy_matched
    coverage = total_matched / max(len(paras1), len(paras2)) * 100 if paras1 or paras2 else 100

    print(f"\n  Paragraph matching:")
    print(f"    Exact: {len(common)}")
    if fuzzy_matched:
        print(f"    Fuzzy (>85%): {fuzzy_matched}")
    print(f"    Coverage: {coverage:.1f}%", end="")
    if coverage >= 99:
        print(f" {GREEN}✓{RESET}")
    elif coverage >= 90:
        print(f" {YELLOW}{RESET}")
    else:
        print(f" {RED}{RESET}")

    if truly_only1:
        print(f"\n  {RED}Paragraphs only in file1: {len(truly_only1)}{RESET}")
        for p in truly_only1:
            preview = p[:120].replace('\n', ' ')
            print(f"    - \"{preview}{'...' if len(p) > 120 else ''}\"")

    if truly_only2:
        print(f"\n  {GREEN}Paragraphs only in file2: {len(truly_only2)}{RESET}")
        for norm_p in truly_only2:
            p = norm2[norm_p]
            preview = p[:120].replace('\n', ' ')
            print(f"    + \"{preview}{'...' if len(p) > 120 else ''}\"")



def analyze_styles(frags1, frags2):
    """Deep analysis of style differences."""
    print(f"\n{BOLD}{'=' * 70}")
    print(f" STYLES ($157)")
    print(f"{'=' * 70}{RESET}")

    n1 = frags1.count("$157")
    n2 = frags2.count("$157")

    print(f"\n  Count: {n1} vs {n2}", end="")
    if n1 == n2:
        print(f" {GREEN}✓{RESET}")
    else:
        print(f" {YELLOW}({n2-n1:+d}){RESET}")

    # Normalize styles (remove name fields)
    def get_style_props(frag):
        val = normalize_value(frag.value)
        if isinstance(val, dict):
            return {k: v for k, v in val.items()
                   if k not in ('style_name', 'content_name', '$173', '$176', 'STYLE_NAME', 'CONTENT_NAME')}
        return val

    styles1 = [(f, get_style_props(f)) for f in frags1.get_all("$157")]
    styles2 = [(f, get_style_props(f)) for f in frags2.get_all("$157")]

    # Match by property similarity
    matched = []
    used2 = set()

    for f1, p1 in styles1:
        best_match = None
        best_score = 0.5

        for j, (f2, p2) in enumerate(styles2):
            if j in used2:
                continue
            score = similarity_score(p1, p2)
            if score > best_score:
                best_score = score
                best_match = (j, f2, p2)

        if best_match:
            j, f2, p2 = best_match
            matched.append((f1, p1, f2, p2, best_score))
            used2.add(j)

    only1 = [(f, p) for f, p in styles1 if not any(m[0] == f for m in matched)]
    only2 = [(f, p) for j, (f, p) in enumerate(styles2) if j not in used2]

    print(f"  Matched: {len(matched)}/{max(n1, n2)}")

    # Count exact vs different matches
    exact = sum(1 for _, p1, _, p2, _ in matched if p1 == p2)
    different = len(matched) - exact

    print(f"    Identical: {exact}")
    print(f"    Different: {different}")

    # Show differences in matched styles
    for f1, p1, f2, p2, score in matched:
        if p1 == p2:
            continue
        diffs = deep_diff(p1, p2)
        if diffs:
            print(f"\n  {CYAN}Style match ({score:.0%}):{RESET}")
            for path, d1, d2 in diffs:
                print_diff(path, d1, d2)

    # Show unmatched styles
    if only1:
        print(f"\n  {RED}Styles only in file1: {len(only1)}{RESET}")
        for f, p in only1:
            print(f"    - {format_value(p, 3, 80)}")

    if only2:
        print(f"\n  {GREEN}Styles only in file2: {len(only2)}{RESET}")
        for f, p in only2:
            print(f"    + {format_value(p, 3, 80)}")


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
        analyze_fragment_type(frags1, frags2, "$260", "SECTIONS")

    if "storylines" in sections:
        analyze_fragment_type(frags1, frags2, "$259", "STORYLINES")

    if "anchors" in sections:
        analyze_fragment_type(frags1, frags2, "$266", "ANCHORS")

    print(f"\n{BOLD}{'=' * 70}")
    print(f" COMPLETE")
    print(f"{'=' * 70}{RESET}")


def show_epub_section(epub_path, section_name):
    """Extract and display EPUB content for a section with matching CSS rules."""
    if not epub_path or not os.path.exists(epub_path):
        return

    print(f"\n  {BOLD}Source EPUB content:{RESET}")

    try:
        with zipfile.ZipFile(epub_path, 'r') as z:
            # First, collect all CSS files
            css_content = {}
            for name in z.namelist():
                if name.endswith('.css'):
                    css_content[name] = z.read(name).decode('utf-8')

            # Find the XHTML file for this section
            xhtml_file = None
            xhtml_content = None
            for name in z.namelist():
                if section_name.lower() in name.lower() and name.endswith(('.xhtml', '.html')):
                    xhtml_file = name
                    xhtml_content = z.read(name).decode('utf-8')
                    break

            if not xhtml_content:
                print(f"    {YELLOW}No XHTML file found matching '{section_name}'{RESET}")
                return

            print(f"    {CYAN}XHTML file: {xhtml_file}{RESET}")

            # Extract classes and IDs used in the document
            classes_used = set(re.findall(r'class="([^"]+)"', xhtml_content))
            ids_used = set(re.findall(r'id="([^"]+)"', xhtml_content))
            elements_used = set(re.findall(r'<(\w+)[>\s]', xhtml_content))

            # Show relevant HTML structure
            body_match = re.search(r'<body[^>]*>(.*?)</body>', xhtml_content, re.DOTALL)
            if body_match:
                body = body_match.group(1)
                print(f"\n    {CYAN}HTML structure:{RESET}")
                lines = body.strip().split('\n')
                for line in lines[:25]:
                    line = line.rstrip()
                    if line.strip():
                        # Truncate long lines
                        if len(line) > 120:
                            line = line[:117] + '...'
                        print(f"      {line}")
                if len(lines) > 25:
                    print(f"      ... ({len(lines) - 25} more lines)")

            # Show matching CSS rules
            print(f"\n    {CYAN}Matching CSS rules:{RESET}")

            # Flatten all classes
            all_classes = set()
            for class_str in classes_used:
                all_classes.update(class_str.split())

            for css_file, css in css_content.items():
                relevant_rules = []

                # Find rules that match elements, classes, or IDs in this section
                # Split CSS into rules (simplified)
                rules = re.findall(r'([^{}]+)\{([^{}]+)\}', css)

                for selector, properties in rules:
                    selector = selector.strip()
                    properties = properties.strip()

                    # Check if selector matches anything in this section
                    matched = False

                    # Check element selectors (ol, li, etc.)
                    for elem in ['ol', 'li', 'ul', 'section', 'p', 'a', 'h2']:
                        if elem in elements_used and re.search(rf'\b{elem}\b', selector):
                            matched = True
                            break

                    # Check class selectors
                    for cls in all_classes:
                        if f'.{cls}' in selector or cls in selector:
                            matched = True
                            break

                    # Check ID selectors
                    for id_ in ids_used:
                        if f'#{id_}' in selector:
                            matched = True
                            break

                    # Highlight list-related rules
                    if 'list' in selector.lower() or 'list' in properties.lower():
                        matched = True

                    if matched:
                        # Clean up the rule for display
                        props_clean = '; '.join(p.strip() for p in properties.split(';') if p.strip())
                        if len(props_clean) > 80:
                            props_clean = props_clean[:77] + '...'
                        relevant_rules.append((selector, props_clean))

                if relevant_rules:
                    print(f"\n      From {css_file}:")
                    for selector, props in relevant_rules[:15]:
                        # Highlight list-style properties
                        if 'list' in props.lower():
                            print(f"        {YELLOW}{selector} {{ {props} }}{RESET}")
                        else:
                            print(f"        {selector} {{ {props} }}")
                    if len(relevant_rules) > 15:
                        print(f"        ... ({len(relevant_rules) - 15} more rules)")

    except Exception as e:
        print(f"    {RED}Error reading EPUB: {e}{RESET}")


def analyze_content_section(frags1, frags2, section_name, epub_path=None):
    """
    Analyze a specific content section by name (e.g., 'endnotes', 'colophon').
    Finds TEXT_CONTENT containing the section and shows detailed structure comparison.
    """
    # Show source EPUB content first if provided
    if epub_path:
        show_epub_section(epub_path, section_name)

    print(f"\n{BOLD}{'=' * 70}")
    print(f" CONTENT SECTION: {section_name}")
    print(f"{'=' * 70}{RESET}")

    def find_section_text(frags, name):
        """Find TEXT_CONTENT fragments containing the section name."""
        results = []
        name_lower = name.lower()
        for frag in frags.get_all("$145"):
            val = frag.value
            # Check both $145 and $146 keys for text arrays
            texts = val.get('$145', []) or val.get('$146', [])
            for i, t in enumerate(texts):
                if isinstance(t, str) and name_lower in t.lower():
                    results.append((frag, i, t))
                    break
        return results

    def get_all_texts(frags, frag_id):
        """Get all texts from a TEXT_CONTENT fragment."""
        for frag in frags.get_all("$145"):
            if str(frag.fid) == str(frag_id):
                val = frag.value
                return val.get('$145', []) or val.get('$146', [])
        return []

    def find_content_items(frags, text_frag_id):
        """Find CONTENT_BLOCK items that reference a TEXT_CONTENT fragment."""
        items = []
        for frag in frags.get_all("$259"):
            content_array = frag.value.get('$146', [])

            def search(item, depth=0, path=""):
                if isinstance(item, dict):
                    text_ref = item.get('$145')
                    if text_ref and isinstance(text_ref, dict):
                        version = text_ref.get('version')
                        if str(version) == str(text_frag_id):
                            items.append({
                                'depth': depth,
                                'path': path,
                                'offset': text_ref.get('$403', 0),
                                'style': item.get('$157'),
                                'keys': list(item.keys()),
                                'item': item
                            })

                    nested = item.get('$146', [])
                    for i, child in enumerate(nested):
                        search(child, depth + 1, f"{path}[{i}]")

            for i, item in enumerate(content_array):
                search(item, 0, f"[{i}]")

        return sorted(items, key=lambda x: x['offset'])

    # Find sections in both files
    sections1 = find_section_text(frags1, section_name)
    sections2 = find_section_text(frags2, section_name)

    print(f"\n  Found in file1: {len(sections1)} TEXT_CONTENT fragment(s)")
    print(f"  Found in file2: {len(sections2)} TEXT_CONTENT fragment(s)")

    if not sections1 and not sections2:
        print(f"\n  {YELLOW}Section '{section_name}' not found in either file{RESET}")
        return

    # Show text content comparison
    for frag1, idx1, text1 in sections1:
        print(f"\n  {CYAN}File1 TEXT_CONTENT: {frag1.fid}{RESET}")
        texts1 = get_all_texts(frags1, frag1.fid)
        print(f"    Paragraphs: {len(texts1)}")
        for i, t in enumerate(texts1[:10]):
            preview = str(t)[:80].replace('\n', '\\n')
            print(f"      [{i}]: {preview}{'...' if len(str(t)) > 80 else ''}")
        if len(texts1) > 10:
            print(f"      ... and {len(texts1) - 10} more")

        # Find content items for this text
        items1 = find_content_items(frags1, frag1.fid)
        print(f"\n    Content items referencing this text: {len(items1)}")

        # Show structure of first few items
        for ci in items1[:5]:
            style_str = sym(str(ci['style'])) if ci['style'] else 'None'
            keys = [sym(str(k)) for k in ci['keys'] if str(k) not in ['$145', '$146', '$155', '$157', '$159']]
            extra = f", extra: {keys}" if keys else ""
            print(f"      offset={ci['offset']}, depth={ci['depth']}, style={style_str}{extra}")

            # Check for list-related properties
            item = ci['item']
            for k in ['$100', '$761', '$762', '$763']:
                if k in item or any(str(key) == k for key in item.keys()):
                    v = item.get(k) or item.get(int(k[1:]))
                    print(f"        {YELLOW}{sym(k)}: {v}{RESET}")

    for frag2, idx2, text2 in sections2:
        print(f"\n  {CYAN}File2 TEXT_CONTENT: {frag2.fid}{RESET}")
        texts2 = get_all_texts(frags2, frag2.fid)
        print(f"    Paragraphs: {len(texts2)}")
        for i, t in enumerate(texts2[:10]):
            preview = str(t)[:80].replace('\n', '\\n')
            print(f"      [{i}]: {preview}{'...' if len(str(t)) > 80 else ''}")
        if len(texts2) > 10:
            print(f"      ... and {len(texts2) - 10} more")

        # Find content items for this text
        items2 = find_content_items(frags2, frag2.fid)
        print(f"\n    Content items referencing this text: {len(items2)}")

        for ci in items2[:5]:
            style_str = sym(str(ci['style'])) if ci['style'] else 'None'
            keys = [sym(str(k)) for k in ci['keys'] if str(k) not in ['$145', '$146', '$155', '$157', '$159']]
            extra = f", extra: {keys}" if keys else ""
            print(f"      offset={ci['offset']}, depth={ci['depth']}, style={style_str}{extra}")

            item = ci['item']
            for k in ['$100', '$761', '$762', '$763']:
                if k in item or any(str(key) == k for key in item.keys()):
                    v = item.get(k) or item.get(int(k[1:]))
                    print(f"        {YELLOW}{sym(k)}: {v}{RESET}")

    # Collect ALL styles used in this section from both files
    all_items1 = []
    all_items2 = []

    for frag1, _, _ in sections1:
        all_items1.extend(find_content_items(frags1, frag1.fid))
    for frag2, _, _ in sections2:
        all_items2.extend(find_content_items(frags2, frag2.fid))

    styles1 = set(str(ci['style']) for ci in all_items1 if ci['style'])
    styles2 = set(str(ci['style']) for ci in all_items2 if ci['style'])

    print(f"\n  {BOLD}All styles used in this section:{RESET}")
    print(f"    File1: {len(styles1)} unique styles")
    print(f"    File2: {len(styles2)} unique styles")

    # Show ALL styles from file1
    if styles1:
        print(f"\n  {CYAN}File1 styles:{RESET}")
        for style_id in sorted(styles1):
            for sf in frags1.get_all('$157'):
                if str(sf.fid) == style_id:
                    print(f"\n    {style_id}:")
                    for k, v in sf.value.items():
                        k_str = sym(str(k))
                        v_str = sym(str(v)) if str(v).startswith('$') else str(v)
                        # Highlight list-related properties
                        if '76' in str(k) or '100' in str(k):
                            print(f"      {YELLOW}{k_str}: {v_str}{RESET}")
                        else:
                            print(f"      {k_str}: {v_str}")
                    break

    # Show ALL styles from file2
    if styles2:
        print(f"\n  {CYAN}File2 styles:{RESET}")
        for style_id in sorted(styles2):
            for sf in frags2.get_all('$157'):
                if str(sf.fid) == style_id:
                    print(f"\n    {style_id}:")
                    for k, v in sf.value.items():
                        k_str = sym(str(k))
                        v_str = sym(str(v)) if str(v).startswith('$') else str(v)
                        if '76' in str(k) or '100' in str(k):
                            print(f"      {YELLOW}{k_str}: {v_str}{RESET}")
                        else:
                            print(f"      {k_str}: {v_str}")
                    break


def full_fragment_diff(file1, file2):
    """
    Comprehensive fragment-by-fragment diff.
    Goes through every fragment in file1, finds matching fragment in file2,
    and shows detailed diff or full fragment if unmatched.
    """
    print(f"{BOLD}KFX Full Fragment Diff{RESET}")
    print(f"  File 1 (source): {file1}")
    print(f"  File 2 (target): {file2}")
    print()

    frags1, _ = load_kfx(file1)
    frags2, _ = load_kfx(file2)

    print(f"  Loaded: {len(frags1.all_fragments)} vs {len(frags2.all_fragments)} fragments")

    # Group fragments by type
    all_types = sorted(set(frags1.types()) | set(frags2.types()))

    total_identical = 0
    total_different = 0
    total_only1 = 0
    total_only2 = 0

    for ftype in all_types:
        list1 = list(frags1.get_all(ftype))
        list2 = list(frags2.get_all(ftype))

        if not list1 and not list2:
            continue

        print(f"\n{BOLD}{'=' * 70}")
        print(f" {sym(ftype)} ({ftype}): {len(list1)} vs {len(list2)}")
        print(f"{'=' * 70}{RESET}")

        matched, only1, only2 = match_fragments(frags1, frags2, ftype)

        # Count identical vs different
        identical = 0
        different = 0

        for f1, v1, f2, v2, score in matched:
            diffs = deep_diff(v1, v2)
            if not diffs:
                identical += 1
            else:
                different += 1
                print(f"\n  {CYAN}Fragment {f1.fid} <-> {f2.fid} (match: {score:.0%}){RESET}")
                for path, d1, d2 in diffs[:20]:  # Limit to first 20 diffs
                    print_diff(path, d1, d2)
                if len(diffs) > 20:
                    print(f"    {DIM}... and {len(diffs) - 20} more differences{RESET}")

        # Show unmatched from file1 (missing in file2)
        for f, v in only1:
            print(f"\n  {RED}ONLY IN FILE1: {f.fid}{RESET}")
            print(f"    {format_value(v, 2, 100)}")

        # Show unmatched from file2 (missing in file1)
        for f, v in only2:
            print(f"\n  {GREEN}ONLY IN FILE2: {f.fid}{RESET}")
            print(f"    {format_value(v, 2, 100)}")

        if identical == len(matched) and not only1 and not only2:
            print(f"  {GREEN}All {identical} fragments identical{RESET}")
        else:
            print(f"\n  Summary: {identical} identical, {different} different, "
                  f"{len(only1)} only in file1, {len(only2)} only in file2")

        total_identical += identical
        total_different += different
        total_only1 += len(only1)
        total_only2 += len(only2)

    print(f"\n{BOLD}{'=' * 70}")
    print(f" SUMMARY")
    print(f"{'=' * 70}{RESET}")
    print(f"  Identical fragments: {total_identical}")
    print(f"  Different fragments: {total_different}")
    print(f"  Only in file1: {total_only1}")
    print(f"  Only in file2: {total_only2}")


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Smart KFX diff with semantic matching and deep diffs")
    parser.add_argument("file1", help="First KFX file")
    parser.add_argument("file2", help="Second KFX file to compare")
    parser.add_argument("--section", "-s", action="append",
                        choices=["text", "styles", "sections", "storylines", "anchors", "all"],
                        help="Section(s) to analyze (default: all)")
    parser.add_argument("--content", "-c", type=str,
                        help="Focus on specific content section by name (e.g., 'endnotes', 'colophon')")
    parser.add_argument("--epub", "-e", type=str,
                        help="Source EPUB file to show original XHTML and CSS (use with --content)")
    parser.add_argument("--full", "-f", action="store_true",
                        help="Full fragment-by-fragment diff (comprehensive)")

    args = parser.parse_args()

    if not os.path.exists(args.file1):
        print(f"Error: {args.file1} not found")
        sys.exit(1)
    if not os.path.exists(args.file2):
        print(f"Error: {args.file2} not found")
        sys.exit(1)

    if args.content:
        # Focus on specific content section
        print(f"{BOLD}KFX Content Section Diff{RESET}")
        print(f"  File 1: {args.file1}")
        print(f"  File 2: {args.file2}")
        print(f"  Content filter: {args.content}")
        if args.epub:
            print(f"  Source EPUB: {args.epub}")
        print()

        frags1, _ = load_kfx(args.file1)
        frags2, _ = load_kfx(args.file2)

        print(f"  Loaded: {len(frags1.all_fragments)} vs {len(frags2.all_fragments)} fragments")

        analyze_content_section(frags1, frags2, args.content, args.epub)
    elif args.full:
        full_fragment_diff(args.file1, args.file2)
    else:
        smart_diff(args.file1, args.file2, args.section)


if __name__ == "__main__":
    main()
