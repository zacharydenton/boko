#!/usr/bin/env python3
"""
KFX Diff Tool - Compare two KFX files section by section.

Usage:
    python scripts/kfx_diff.py file1.kfx file2.kfx [--section SECTION]

Sections: metadata, position, content, styles, navigation, anchors, resources, capabilities, all
"""

import sys
import os
import json
from pathlib import Path
from collections import defaultdict

# Add kfxlib to path
sys.path.insert(0, str(Path(__file__).parent))

from kfx_loader import load_kfx, FragmentStore
from kfx_symbols import symbol_name, format_symbol, get_symbol_names


def format_value(val, indent=0):
    """Format a value for readable output with symbol name resolution."""
    prefix = "  " * indent
    if isinstance(val, dict):
        if not val:
            return "{}"
        lines = ["{"]
        for k, v in sorted(val.items(), key=lambda x: str(x[0])):
            k_str = format_symbol(k) if str(k).startswith("$") else str(k)
            lines.append(f"{prefix}  {k_str}: {format_value(v, indent + 1)}")
        lines.append(f"{prefix}}}")
        return "\n".join(lines)
    elif isinstance(val, list):
        if not val:
            return "[]"
        if len(val) <= 3 and all(not isinstance(v, (dict, list)) for v in val):
            formatted = [format_symbol(v) if str(v).startswith("$") else str(v) for v in val]
            return f"[{', '.join(formatted)}]"
        lines = ["["]
        for i, v in enumerate(val):
            lines.append(f"{prefix}  [{i}]: {format_value(v, indent + 1)}")
        lines.append(f"{prefix}]")
        return "\n".join(lines)
    elif isinstance(val, bytes):
        return f"<bytes len={len(val)}>"
    elif isinstance(val, str) and len(val) > 100:
        return f'"{val[:100]}..."'
    elif str(val).startswith("$"):
        return format_symbol(val)
    else:
        return repr(val)


def get_fragments_by_type(book, ftype):
    """Get all fragments of a given type."""
    fragments = []
    for frag in book.get_all(ftype):
        fragments.append(frag)
    return fragments


def extract_metadata_490(book):
    """Extract $490 (kindle_metadata) as a structured dict."""
    frag = book.get("$490")
    if frag is None:
        return {}

    result = {}
    for category in frag.value.get("$491", []):
        cat_name = category.get("$495", "unknown")
        entries = {}
        for entry in category.get("$258", []):
            key = entry.get("$492", "")
            val = entry.get("$307", "")
            if key in entries:
                # Handle multiple values (e.g., multiple authors)
                if isinstance(entries[key], list):
                    entries[key].append(val)
                else:
                    entries[key] = [entries[key], val]
            else:
                entries[key] = val
        result[cat_name] = entries
    return result


def extract_metadata_258(book):
    """Extract $258 (metadata with reading orders)."""
    frag = book.get("$258")
    if frag is None:
        return {}

    result = {}
    for key, val in frag.value.items():
        if key == "$169":  # reading_orders
            orders = []
            for order in val:
                order_data = {
                    "name": str(order.get("$178", "")),
                    "sections": [str(s) for s in order.get("$170", [])]
                }
                orders.append(order_data)
            result["reading_orders"] = orders
        else:
            result[key] = val
    return result


def extract_styles(book):
    """Extract $157 (style) fragments."""
    styles = {}
    for frag in book.get_all("$157"):
        fid = str(frag.fid) if frag.fid else "singleton"
        style_data = {}
        for key, val in frag.value.items():
            style_data[key] = val
        styles[fid] = style_data
    return styles


def extract_content_summary(book):
    """Extract content block summaries ($259 storylines, $260 sections)."""
    result = {
        "storylines": {},
        "sections": {},
        "text_content": []
    }

    # $259 - storylines
    for frag in book.get_all("$259"):
        fid = str(frag.fid) if frag.fid else "singleton"
        content_items = frag.value.get("$146", [])
        result["storylines"][fid] = {
            "item_count": len(content_items),
            "types": [item.get("$159", "?") for item in content_items[:10]]
        }

    # $260 - sections
    for frag in book.get_all("$260"):
        fid = str(frag.fid) if frag.fid else "singleton"
        templates = frag.value.get("$141", [])
        result["sections"][fid] = {
            "template_count": len(templates),
            "position": frag.value.get("$155", None)
        }

    # $145 - text content
    for frag in book.get_all("$145"):
        fid = str(frag.fid) if frag.fid else "singleton"
        content = frag.value.get("$146", [])
        text_len = 0
        for item in content:
            if "$144" in item:
                text_len += len(item.get("$144", ""))
        result["text_content"].append({
            "id": fid,
            "items": len(content),
            "text_chars": text_len
        })

    return result


def extract_position_maps(book):
    """Extract position/location map data."""
    result = {
        "location_map": None,
        "position_map": None,
    }

    # $611 - location map
    frag = book.get("$611")
    if frag:
        entries = frag.value.get("$612", [])
        result["location_map"] = {
            "entry_count": len(entries),
            "sample_entries": entries[:5] if entries else []
        }

    # $609 - position map (dictionaries/prepub only)
    for frag in book.get_all("$609"):
        if frag.value:
            result["position_map"] = {
                "present": True,
                "keys": list(frag.value.keys())[:10]
            }

    return result


def extract_navigation(book):
    """Extract navigation data ($389, $391)."""
    result = {
        "book_navigation": [],
        "nav_containers": []
    }

    # $389 - book navigation
    frag = book.get("$389")
    if frag:
        for nav in frag.value:
            nav_info = {
                "type": nav.get("$235", "?"),
                "containers": len(nav.get("$392", []))
            }
            result["book_navigation"].append(nav_info)

    # $391 - nav containers (TOC entries)
    for frag in book.get_all("$391"):
        fid = str(frag.fid) if frag.fid else "singleton"
        entries = frag.value.get("$247", [])
        result["nav_containers"].append({
            "id": fid,
            "type": frag.value.get("$235", "?"),
            "entry_count": len(entries)
        })

    return result


def extract_anchors(book):
    """Extract anchor fragments ($266)."""
    anchors = {
        "internal": [],
        "external": []
    }

    for frag in book.get_all("$266"):
        anchor_id = str(frag.value.get("$180", "?"))

        if "$186" in frag.value:
            # External URL
            anchors["external"].append({
                "id": anchor_id,
                "url": frag.value["$186"][:50] + "..." if len(str(frag.value["$186"])) > 50 else frag.value["$186"]
            })
        elif "$183" in frag.value:
            # Internal position
            pos = frag.value["$183"]
            anchors["internal"].append({
                "id": anchor_id,
                "eid": pos.get("$155", "?"),
                "offset": pos.get("$143", None)
            })

    return anchors


def extract_resources(book):
    """Extract resource data ($164, $417)."""
    resources = []

    for frag in book.get_all("$164"):
        fid = str(frag.fid) if frag.fid else "singleton"
        res = {
            "id": fid,
            "format": str(frag.value.get("$161", "?")),
            "mime": frag.value.get("$162", "?"),
            "width": frag.value.get("$422", None),
            "height": frag.value.get("$423", None),
            "location": str(frag.value.get("$165", "?"))
        }
        resources.append(res)

    # Count raw media
    raw_media_count = len(list(book.get_all("$417")))

    return {
        "resources": resources,
        "raw_media_count": raw_media_count
    }


def extract_capabilities(book):
    """Extract format capabilities ($585, $593)."""
    result = {
        "content_features": [],
        "format_capabilities": []
    }

    # $585 - content features
    frag = book.get("$585")
    if frag:
        for feature in frag.value.get("$590", []):
            result["content_features"].append({
                "name": feature.get("$492", "?"),
                "namespace": feature.get("$586", "?"),
                "version": feature.get("$589", {}).get("version", {})
            })

    # $593 - format capabilities
    for frag in book.get_all("$593"):
        for cap in frag.value:
            result["format_capabilities"].append({
                "name": cap.get("$492", "?"),
                "version": cap.get("$5", "?")
            })

    return result


def diff_dicts(d1, d2, path=""):
    """Compare two dicts and return differences."""
    diffs = []

    all_keys = set(d1.keys()) | set(d2.keys())
    for key in sorted(all_keys, key=str):
        key_path = f"{path}.{key}" if path else str(key)

        if key not in d1:
            diffs.append(f"  + {key_path}: {format_value(d2[key])}")
        elif key not in d2:
            diffs.append(f"  - {key_path}: {format_value(d1[key])}")
        elif d1[key] != d2[key]:
            if isinstance(d1[key], dict) and isinstance(d2[key], dict):
                diffs.extend(diff_dicts(d1[key], d2[key], key_path))
            elif isinstance(d1[key], list) and isinstance(d2[key], list):
                diffs.append(f"  ~ {key_path}:")
                diffs.append(f"      file1: {len(d1[key])} items")
                diffs.append(f"      file2: {len(d2[key])} items")
                # Show first difference
                for i, (v1, v2) in enumerate(zip(d1[key], d2[key])):
                    if v1 != v2:
                        diffs.append(f"      first diff at [{i}]:")
                        diffs.append(f"        file1: {format_value(v1)}")
                        diffs.append(f"        file2: {format_value(v2)}")
                        break
            else:
                diffs.append(f"  ~ {key_path}:")
                diffs.append(f"      file1: {format_value(d1[key])}")
                diffs.append(f"      file2: {format_value(d2[key])}")

    return diffs


def print_section(title, data1, data2):
    """Print a section comparison."""
    print(f"\n{'=' * 60}")
    print(f" {title}")
    print('=' * 60)

    if data1 == data2:
        print("  [IDENTICAL]")
        return

    diffs = diff_dicts(data1, data2)
    if diffs:
        for diff in diffs:
            print(diff)
    else:
        print("  [IDENTICAL]")


def print_summary(title, data1, data2):
    """Print a summary comparison (counts only)."""
    print(f"\n{title}:")
    print(f"  file1: {format_value(data1)}")
    print(f"  file2: {format_value(data2)}")


def compare_kfx(file1, file2, sections=None):
    """Compare two KFX files."""
    print(f"Comparing KFX files:")
    print(f"  file1: {file1}")
    print(f"  file2: {file2}")

    frags1, method1 = load_kfx(file1)
    frags2, method2 = load_kfx(file2)
    print(f"  Loaded file1: {len(frags1.all_fragments)} fragments ({method1})")
    print(f"  Loaded file2: {len(frags2.all_fragments)} fragments ({method2})")

    if sections is None or "all" in sections:
        sections = ["metadata", "capabilities", "position", "content", "styles", "navigation", "anchors", "resources"]

    # Fragment count summary
    print(f"\n{'=' * 60}")
    print(" FRAGMENT COUNTS")
    print('=' * 60)

    types1 = defaultdict(int)
    types2 = defaultdict(int)
    for frag in frags1.all_fragments:
        types1[str(frag.ftype)] += 1
    for frag in frags2.all_fragments:
        types2[str(frag.ftype)] += 1

    all_types = sorted(set(types1.keys()) | set(types2.keys()))
    for ftype in all_types:
        c1, c2 = types1.get(ftype, 0), types2.get(ftype, 0)
        marker = " " if c1 == c2 else "~"
        ftype_name = format_symbol(ftype)
        print(f"  {marker} {ftype_name}: {c1} vs {c2}")

    # Section comparisons
    if "metadata" in sections:
        print_section("METADATA ($490)",
                      extract_metadata_490(frags1),
                      extract_metadata_490(frags2))
        print_section("METADATA ($258)",
                      extract_metadata_258(frags1),
                      extract_metadata_258(frags2))

    if "capabilities" in sections:
        print_section("CAPABILITIES ($585, $593)",
                      extract_capabilities(frags1),
                      extract_capabilities(frags2))

    if "position" in sections:
        print_section("POSITION/LOCATION MAPS",
                      extract_position_maps(frags1),
                      extract_position_maps(frags2))

    if "content" in sections:
        print_section("CONTENT SUMMARY",
                      extract_content_summary(frags1),
                      extract_content_summary(frags2))

    if "styles" in sections:
        styles1 = extract_styles(frags1)
        styles2 = extract_styles(frags2)
        print(f"\n{'=' * 60}")
        print(" STYLES ($157)")
        print('=' * 60)
        print(f"  file1: {len(styles1)} styles")
        print(f"  file2: {len(styles2)} styles")

        # Find common style names by looking at $173 (style name)
        names1 = {s.get("$173", fid): fid for fid, s in styles1.items()}
        names2 = {s.get("$173", fid): fid for fid, s in styles2.items()}

        common = set(names1.keys()) & set(names2.keys())
        only1 = set(names1.keys()) - set(names2.keys())
        only2 = set(names2.keys()) - set(names1.keys())

        if only1:
            print(f"  Only in file1: {list(only1)[:10]}")
        if only2:
            print(f"  Only in file2: {list(only2)[:10]}")

        # Compare common styles
        diffs_found = 0
        for name in sorted(common, key=str):
            s1 = styles1[names1[name]]
            s2 = styles2[names2[name]]
            if s1 != s2:
                diffs_found += 1
                if diffs_found <= 5:
                    print(f"\n  Style '{name}' differs:")
                    diffs = diff_dicts(s1, s2, "    ")
                    for d in diffs[:10]:
                        print(f"  {d}")

        if diffs_found > 5:
            print(f"\n  ... and {diffs_found - 5} more style differences")

    if "navigation" in sections:
        print_section("NAVIGATION ($389, $391)",
                      extract_navigation(frags1),
                      extract_navigation(frags2))

    if "anchors" in sections:
        anchors1 = extract_anchors(frags1)
        anchors2 = extract_anchors(frags2)
        print(f"\n{'=' * 60}")
        print(" ANCHORS ($266)")
        print('=' * 60)
        print(f"  Internal anchors: {len(anchors1['internal'])} vs {len(anchors2['internal'])}")
        print(f"  External anchors: {len(anchors1['external'])} vs {len(anchors2['external'])}")

    if "resources" in sections:
        print_section("RESOURCES ($164, $417)",
                      extract_resources(frags1),
                      extract_resources(frags2))

    print(f"\n{'=' * 60}")
    print(" COMPARISON COMPLETE")
    print('=' * 60)


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Compare two KFX files section by section")
    parser.add_argument("file1", help="First KFX file")
    parser.add_argument("file2", help="Second KFX file")
    parser.add_argument("--section", "-s", action="append",
                        choices=["metadata", "position", "content", "styles",
                                 "navigation", "anchors", "resources", "capabilities", "all"],
                        help="Section(s) to compare (default: all)")

    args = parser.parse_args()

    if not os.path.exists(args.file1):
        print(f"Error: {args.file1} not found")
        sys.exit(1)
    if not os.path.exists(args.file2):
        print(f"Error: {args.file2} not found")
        sys.exit(1)

    compare_kfx(args.file1, args.file2, args.section)


if __name__ == "__main__":
    main()
