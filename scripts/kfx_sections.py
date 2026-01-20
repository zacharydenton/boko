#!/usr/bin/env python3
"""
KFX Section Dumper - Dump specific sections of a KFX file in readable format.

Usage:
    python scripts/kfx_sections.py file.kfx SECTION [--limit N]

Sections:
    metadata    - $490 and $258 metadata
    styles      - $157 style definitions
    content     - $145, $259, $260 content blocks
    position    - $611, $612 location/position maps
    navigation  - $389, $391 TOC and navigation
    anchors     - $266 anchor definitions
    resources   - $164, $417 resource descriptors
    capabilities - $585, $593 format capabilities
    all         - Everything
"""

import sys
import os
from pathlib import Path
from collections import defaultdict

sys.path.insert(0, str(Path(__file__).parent))

from kfx_loader import load_kfx
from kfx_symbols import format_symbol


def sym(s):
    """Format a symbol with readable name."""
    return format_symbol(s)


def dump_metadata(frags, limit=None):
    """Dump metadata sections."""
    print("\n" + "=" * 70)
    print(" METADATA ($490 KINDLE_METADATA)")
    print("=" * 70)

    frag = frags.get("$490")
    if frag:
        for category in frag.value.get("$491", []):
            cat_name = category.get("$495", "unknown")
            print(f"\n  [{cat_name}]")
            for entry in category.get("$258", []):
                key = entry.get("$492", "")
                val = entry.get("$307", "")
                if isinstance(val, str) and len(val) > 60:
                    val = val[:60] + "..."
                print(f"    {key}: {val}")
    else:
        print("  (not present)")

    print("\n" + "=" * 70)
    print(" METADATA ($258 METADATA - Reading Orders)")
    print("=" * 70)

    frag = frags.get("$258")
    if frag:
        for key, val in frag.value.items():
            key_str = str(key)
            if key_str == "$169":  # reading_orders
                print(f"\n  Reading Orders ({len(val)} total):")
                for i, order in enumerate(val):
                    name = order.get("$178", "unnamed")
                    sections = order.get("$170", [])
                    print(f"    [{i}] {name}: {len(sections)} sections")
                    if limit and i >= limit:
                        print(f"    ... (showing first {limit})")
                        break
            else:
                print(f"  {sym(key)}: {val}")
    else:
        print("  (not present)")


def dump_styles(frags, limit=None):
    """Dump style definitions."""
    print("\n" + "=" * 70)
    print(" STYLES ($157 STYLE)")
    print("=" * 70)

    styles = frags.get_all("$157")
    print(f"\n  Total styles: {len(styles)}")

    for i, frag in enumerate(styles):
        if limit and i >= limit:
            print(f"\n  ... (showing first {limit} of {len(styles)})")
            break

        fid = str(frag.fid) if frag.fid else "singleton"
        style_name = frag.value.get("$173", fid)
        print(f"\n  [{i}] Style: {style_name}")

        # Group properties by category
        layout_props = {}
        text_props = {}
        other_props = {}

        layout_keys = {"$56", "$57", "$66", "$67", "$42", "$43", "$44", "$45", "$16", "$18", "$19"}
        text_keys = {"$47", "$48", "$49", "$50", "$51", "$52", "$53", "$54", "$55"}

        for k, v in frag.value.items():
            k_str = str(k)
            if k_str in layout_keys:
                layout_props[k] = v
            elif k_str in text_keys:
                text_props[k] = v
            elif k_str != "$173":  # Skip name
                other_props[k] = v

        if layout_props:
            print("    Layout:")
            for k, v in layout_props.items():
                print(f"      {sym(k)}: {format_value(v)}")

        if text_props:
            print("    Text:")
            for k, v in text_props.items():
                print(f"      {sym(k)}: {format_value(v)}")

        if other_props:
            print("    Other:")
            for k, v in other_props.items():
                print(f"      {sym(k)}: {format_value(v)}")


def dump_content(frags, limit=None):
    """Dump content blocks."""
    print("\n" + "=" * 70)
    print(" SECTIONS ($260 SECTION)")
    print("=" * 70)

    sections = frags.get_all("$260")
    print(f"\n  Total sections: {len(sections)}")

    for i, frag in enumerate(sections):
        if limit and i >= limit:
            print(f"\n  ... (showing first {limit})")
            break

        fid = str(frag.fid) if frag.fid else "singleton"
        pos = frag.value.get("$155", "?")
        templates = frag.value.get("$141", [])
        print(f"\n  [{i}] Section: {fid}")
        print(f"      Position (EID): {pos}")
        print(f"      Templates: {len(templates)}")

        for j, tpl in enumerate(templates[:3]):
            ttype = sym(tpl.get("$159", "?"))
            layout = sym(tpl.get("$156", "?"))
            print(f"        [{j}] type={ttype} layout={layout}")

    print("\n" + "=" * 70)
    print(" STORYLINES ($259 CONTENT_BLOCK)")
    print("=" * 70)

    storylines = frags.get_all("$259")
    print(f"\n  Total storylines: {len(storylines)}")

    for i, frag in enumerate(storylines):
        if limit and i >= limit:
            print(f"\n  ... (showing first {limit})")
            break

        fid = str(frag.fid) if frag.fid else "singleton"
        content = frag.value.get("$146", [])
        print(f"\n  [{i}] Storyline: {fid}")
        print(f"      Content items: {len(content)}")

        # Summarize content types
        types = defaultdict(int)
        for item in content:
            types[str(item.get("$159", "?"))] += 1
        type_summary = {sym(k): v for k, v in types.items()}
        print(f"      Types: {type_summary}")

    print("\n" + "=" * 70)
    print(" TEXT CONTENT ($145 TEXT_CONTENT)")
    print("=" * 70)

    text_frags = frags.get_all("$145")
    print(f"\n  Total text blocks: {len(text_frags)}")

    total_chars = 0
    for i, frag in enumerate(text_frags):
        if limit and i >= limit:
            print(f"\n  ... (showing first {limit})")
            break

        fid = str(frag.fid) if frag.fid else "singleton"
        content = frag.value.get("$146", [])

        # Count text characters
        chars = 0
        for item in content:
            if "$144" in item:
                chars += len(item.get("$144", ""))
        total_chars += chars

        print(f"\n  [{i}] Text Block: {fid}")
        print(f"      Items: {len(content)}, Characters: {chars}")

        # Show first text snippet
        for item in content[:1]:
            if "$144" in item:
                text = item["$144"]
                if len(text) > 80:
                    text = text[:80] + "..."
                print(f"      Preview: \"{text}\"")

    print(f"\n  Total text characters: {total_chars}")


def dump_position(frags, limit=None):
    """Dump position/location maps."""
    print("\n" + "=" * 70)
    print(" LOCATION MAP ($611)")
    print("=" * 70)

    frag = frags.get("$611")
    if frag:
        entries = frag.value.get("$612", [])
        print(f"\n  Total entries: {len(entries)}")

        show = limit or 20
        for i, entry in enumerate(entries[:show]):
            eid = entry.get("$155", "?")
            offset = entry.get("$143", 0)
            length = entry.get("$614", 0)
            print(f"    [{i}] EID={eid} offset={offset} length={length}")

        if len(entries) > show:
            print(f"    ... ({len(entries) - show} more entries)")
    else:
        print("  (not present)")

    print("\n" + "=" * 70)
    print(" POSITION MAP ($609)")
    print("=" * 70)

    pos_maps = frags.get_all("$609")
    if pos_maps:
        for frag in pos_maps:
            print(f"\n  Fragment: {frag.fid}")
            print(f"  Keys: {list(frag.value.keys())[:10]}")
    else:
        print("  (not present - normal for regular ebooks)")


def dump_navigation(frags, limit=None):
    """Dump navigation structures."""
    print("\n" + "=" * 70)
    print(" BOOK NAVIGATION ($389 BOOK_NAVIGATION)")
    print("=" * 70)

    frag = frags.get("$389")
    if frag:
        for i, nav in enumerate(frag.value):
            nav_type = sym(nav.get("$235", "?"))
            containers = nav.get("$392", [])
            print(f"\n  [{i}] Type: {nav_type}")
            print(f"      Containers: {len(containers)}")
            for j, cont in enumerate(containers[:5]):
                print(f"        [{j}] {cont}")
    else:
        print("  (not present)")

    print("\n" + "=" * 70)
    print(" NAV CONTAINERS ($391 NAV_CONTAINER_TYPE) - TOC Entries")
    print("=" * 70)

    containers = frags.get_all("$391")
    print(f"\n  Total containers: {len(containers)}")

    for i, frag in enumerate(containers):
        if limit and i >= limit:
            print(f"\n  ... (showing first {limit})")
            break

        fid = str(frag.fid) if frag.fid else "singleton"
        nav_type = sym(frag.value.get("$235", "?"))
        entries = frag.value.get("$247", [])

        print(f"\n  [{i}] Container: {fid}")
        print(f"      Type: {nav_type}")
        print(f"      Entries: {len(entries)}")

        for j, entry in enumerate(entries[:5]):
            label = entry.get("$241", "?")
            if isinstance(label, str) and len(label) > 40:
                label = label[:40] + "..."
            pos = entry.get("$246", entry.get("$155", "?"))
            print(f"        [{j}] \"{label}\" -> {pos}")

        if len(entries) > 5:
            print(f"        ... ({len(entries) - 5} more)")


def dump_anchors(frags, limit=None):
    """Dump anchor definitions."""
    print("\n" + "=" * 70)
    print(" ANCHORS ($266 PAGE_TEMPLATE/ANCHOR)")
    print("=" * 70)

    anchors = frags.get_all("$266")
    print(f"\n  Total anchors: {len(anchors)}")

    internal = []
    external = []

    for frag in anchors:
        anchor_id = str(frag.value.get("$180", "?"))
        if "$186" in frag.value:
            external.append((anchor_id, frag.value["$186"]))
        elif "$183" in frag.value:
            pos = frag.value["$183"]
            internal.append((anchor_id, pos.get("$155", "?"), pos.get("$143")))

    print(f"\n  Internal anchors: {len(internal)}")
    show = limit or 20
    for i, (aid, eid, offset) in enumerate(internal[:show]):
        off_str = f" +{offset}" if offset else ""
        print(f"    {aid} -> EID {eid}{off_str}")
    if len(internal) > show:
        print(f"    ... ({len(internal) - show} more)")

    print(f"\n  External anchors: {len(external)}")
    for i, (aid, url) in enumerate(external[:show]):
        if len(str(url)) > 60:
            url = str(url)[:60] + "..."
        print(f"    {aid} -> {url}")
    if len(external) > show:
        print(f"    ... ({len(external) - show} more)")


def dump_resources(frags, limit=None):
    """Dump resource descriptors."""
    print("\n" + "=" * 70)
    print(" RESOURCES ($164 RESOURCE)")
    print("=" * 70)

    resources = frags.get_all("$164")
    print(f"\n  Total resources: {len(resources)}")

    for i, frag in enumerate(resources):
        if limit and i >= limit:
            print(f"\n  ... (showing first {limit})")
            break

        fid = str(frag.fid) if frag.fid else "singleton"
        fmt = frag.value.get("$161", "?")
        mime = frag.value.get("$162", "?")
        width = frag.value.get("$422", "?")
        height = frag.value.get("$423", "?")
        location = frag.value.get("$165", "?")

        print(f"\n  [{i}] Resource: {fid}")
        print(f"      Format: {sym(fmt)}")
        print(f"      MIME: {mime}")
        print(f"      Size: {width}x{height}")
        print(f"      Location: {location}")

    print("\n" + "=" * 70)
    print(" RAW MEDIA ($417 RAW_MEDIA)")
    print("=" * 70)

    media = frags.get_all("$417")
    print(f"\n  Total raw media: {len(media)}")

    total_bytes = 0
    for i, frag in enumerate(media):
        fid = str(frag.fid) if frag.fid else "singleton"
        data = frag.value
        size = len(data) if isinstance(data, (bytes, bytearray)) else 0
        total_bytes += size

        if limit and i >= limit:
            continue

        print(f"    [{i}] {fid}: {size:,} bytes")

    if limit and len(media) > limit:
        print(f"    ... ({len(media) - limit} more)")

    print(f"\n  Total media size: {total_bytes:,} bytes")


def dump_capabilities(frags, limit=None):
    """Dump format capabilities."""
    print("\n" + "=" * 70)
    print(" CONTENT FEATURES ($585 FORMAT_CAPABILITIES_OLD)")
    print("=" * 70)

    frag = frags.get("$585")
    if frag:
        features = frag.value.get("$590", [])
        print(f"\n  Total features: {len(features)}")

        for i, feat in enumerate(features):
            name = feat.get("$492", "?")
            namespace = feat.get("$586", "?")
            version_info = feat.get("$589", {})
            version = version_info.get("version", version_info.get("imports", {}))
            print(f"    [{i}] {name}")
            print(f"        Namespace: {namespace}")
            print(f"        Version: {version}")
    else:
        print("  (not present)")

    print("\n" + "=" * 70)
    print(" FORMAT CAPABILITIES ($593 FORMAT_CAPABILITIES)")
    print("=" * 70)

    caps = frags.get_all("$593")
    if caps:
        for frag in caps:
            print(f"\n  Capabilities:")
            for cap in frag.value:
                name = cap.get("$492", "?")
                version = cap.get("$5", "?")
                print(f"    {name} v{version}")
    else:
        print("  (not present)")


def format_value(val, indent=0):
    """Format a value for display."""
    if isinstance(val, dict):
        if not val:
            return "{}"
        items = [f"{sym(k)}: {format_value(v)}" for k, v in val.items()]
        return "{ " + ", ".join(items) + " }"
    elif isinstance(val, list):
        if len(val) <= 3:
            return "[" + ", ".join(format_value(v) for v in val) + "]"
        return f"[{len(val)} items]"
    elif isinstance(val, bytes):
        return f"<{len(val)} bytes>"
    elif isinstance(val, str) and len(val) > 50:
        return f'"{val[:50]}..."'
    elif str(val).startswith("$"):
        return sym(val)
    else:
        return repr(val)


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Dump specific sections of a KFX file")
    parser.add_argument("file", help="KFX file to dump")
    parser.add_argument("section", nargs="?", default="all",
                        choices=["metadata", "styles", "content", "position",
                                 "navigation", "anchors", "resources", "capabilities", "all"],
                        help="Section to dump (default: all)")
    parser.add_argument("--limit", "-l", type=int, default=None,
                        help="Limit number of items shown per section")

    args = parser.parse_args()

    if not os.path.exists(args.file):
        print(f"Error: {args.file} not found")
        sys.exit(1)

    print(f"Loading {args.file}...")
    frags, method = load_kfx(args.file)
    print(f"Loaded {len(frags.all_fragments)} fragments using {method}")

    # Count fragments
    print(f"\nFragment summary:")
    for ftype in sorted(frags.types()):
        print(f"  {sym(ftype)}: {frags.count(ftype)}")

    section = args.section
    limit = args.limit

    if section in ("all", "metadata"):
        dump_metadata(frags, limit)
    if section in ("all", "capabilities"):
        dump_capabilities(frags, limit)
    if section in ("all", "styles"):
        dump_styles(frags, limit)
    if section in ("all", "content"):
        dump_content(frags, limit)
    if section in ("all", "position"):
        dump_position(frags, limit)
    if section in ("all", "navigation"):
        dump_navigation(frags, limit)
    if section in ("all", "anchors"):
        dump_anchors(frags, limit)
    if section in ("all", "resources"):
        dump_resources(frags, limit)


if __name__ == "__main__":
    main()
