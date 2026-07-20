#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow", "lxml", "pypdf", "beautifulsoup4"]
# ///
"""Extract every math container from a KP gold KFX: alt_text + KVG -> SVG,
in fragment order, with a manifest for pairing against boko's typeset output."""
import json
import os
import sys

for cand in ["~/code/kfx/kfx_input", "~/code/kfx/kfx_output"]:
    p = os.path.expanduser(cand)
    if os.path.isdir(p):
        sys.path.insert(0, p)
        break

from kfxlib import yj_book  # noqa: E402

OPCODES = {0: ("M", 2), 1: ("L", 2), 2: ("Q", 4), 3: ("C", 6), 4: ("Z", 0)}


def path_to_d(path):
    p = list(path)
    d = []
    while p:
        name, nargs = OPCODES[int(p.pop(0))]
        d.append(name)
        for _ in range(nargs):
            d.append(f"{p.pop(0):g}")
    return " ".join(d)


def transform_to_svg(vals):
    vals = list(vals)
    vals[1], vals[2] = vals[2], vals[1]
    return "matrix(%s)" % " ".join(f"{v:g}" for v in vals)


def main(kfx_path, out_dir):
    book = yj_book.YJ_Book(kfx_path)
    book.decode_book()

    bundles = {}
    contents = {}  # content fragment name -> string list
    maths = []     # (alttext, kvg struct) in walk order

    for frag in book.fragments:
        if frag.ftype == "$692":
            bundles[str(frag.value.get("name"))] = frag.value["$693"]
        elif frag.ftype == "$145":
            v = frag.value
            contents[str(v.get("name"))] = v.get("$146", [])

    def content_text(ref):
        try:
            return str(contents[str(ref["name"])][ref["$403"]])
        except Exception:
            return ""

    def find_kvgs(data, out):
        if isinstance(data, (list, tuple)):
            for v in data:
                find_kvgs(v, out)
        elif hasattr(data, "items"):
            if data.get("$159") == "$272":
                out.append(data)
                return
            for v in data.values():
                find_kvgs(v, out)

    def walk(data):
        if isinstance(data, (list, tuple)):
            for v in data:
                walk(v)
        elif hasattr(data, "items"):
            if data.get("$615") == "$688":  # yj.classification: math
                alt = ""
                for ann in data.get("$683", []):
                    if ann.get("$687") == "$584":
                        alt = content_text(ann.get("$145", {}))
                kvgs = []
                find_kvgs(data.get("$146", []), kvgs)
                maths.append((alt, kvgs))
                return  # don't descend further into this container
            for v in data.values():
                walk(v)

    story_maths = []  # (fid, [(alt, kvg), ...])
    for frag in book.fragments:
        if frag.ftype == "$259":
            before = len(maths)
            walk(frag.value)
            story_maths.append((str(frag.fid), maths[before:]))
            del maths[before:]

    # True reading order: document_data ($538) reading_orders -> sections
    # ($260, in order) -> each section's story name ($176).
    def find_story(data):
        if isinstance(data, (list, tuple)):
            for v in data:
                r = find_story(v)
                if r is not None:
                    return r
        elif hasattr(data, "items"):
            if "$176" in data:
                return str(data["$176"])
            for v in data.values():
                r = find_story(v)
                if r is not None:
                    return r
        return None

    section_story = {}
    for frag in book.fragments:
        if frag.ftype == "$260":
            sn = find_story(frag.value)
            if sn is not None:
                section_story[str(frag.fid)] = sn
    story_order = []
    for frag in book.fragments:
        if frag.ftype == "$538":
            for ro in frag.value.get("$169", []):
                for sect in ro.get("$170", []):
                    st = section_story.get(str(sect))
                    if st and st not in story_order:
                        story_order.append(st)
    rank = {name: i for i, name in enumerate(story_order)}
    story_maths.sort(key=lambda item: rank.get(item[0], 10**9))
    for _, ms in story_maths:
        maths.extend(ms)

    os.makedirs(out_dir, exist_ok=True)
    man = open(os.path.join(out_dir, "manifest.jsonl"), "w")
    for i, (alt, kvgs) in enumerate(maths):
        if not kvgs:
            man.write(json.dumps({"i": i, "status": "no-kvg", "alttext": alt}) + "\n")
            continue
        w = max(k.get("$66") for k in kvgs)
        h = sum(k.get("$67") for k in kvgs)
        parts = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" '
                 f'width="{w}" height="{h}">']
        y_off = 0
        for k in kvgs:
            parts.append(f'<g transform="translate(0 {y_off})">')
            for shape in k.get("$250", []):
                if shape.get("$159") != "$273":
                    continue
                ref = shape["$249"]
                path = bundles[str(ref["name"])][ref["$403"]] if hasattr(ref, "items") else ref
                attrs = [f'd="{path_to_d(path)}"']
                if "$98" in shape:
                    attrs.append(f'transform="{transform_to_svg(shape["$98"])}"')
                parts.append(f"<path {' '.join(attrs)}/>")
            parts.append('</g>')
            y_off += k.get("$67")
        parts.append("</svg>")
        with open(os.path.join(out_dir, f"eq{i:04}.svg"), "w") as f:
            f.write("\n".join(parts))
        man.write(json.dumps({"i": i, "status": "ok", "alttext": alt,
                              "w": w, "h": h, "segments": len(kvgs)}) + "\n")
    man.close()
    print(f"gold equations: {len(maths)}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
