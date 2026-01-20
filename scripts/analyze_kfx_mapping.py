#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "amazon-ion",
#     "pillow",
#     "pypdf",
#     "lxml",
#     "beautifulsoup4",
# ]
# ///
"""
Analyze KFX files to extract CSS-to-KFX symbol mappings.

Finds markers like __CSS_font_bold__ in the KFX text content and shows
what KFX properties are applied to each marker.

Usage:
    uv run scripts/analyze_kfx_mapping.py <kfx_file>

Output format:
  === CSS PROPERTIES WITH EFFECT ===
  font-bold    -> FONT_WEIGHT=FONT_WEIGHT_BOLD

  === CSS PROPERTIES WITHOUT EFFECT ===
  (grouped by category)
"""

import sys
import re
from pathlib import Path
from collections import defaultdict

sys.path.insert(0, str(Path(__file__).parent))

from kfx_dump import load_kfx
from kfx_symbols import format_symbol, get_symbol_names


def process_content_block(val, styles, text_to_style, text_to_inline_styles, parent_style=None):
    """Recursively process a content block to extract text-to-style mappings."""
    if not isinstance(val, dict):
        return

    # Get style at this level (if any), otherwise inherit from parent
    current_style = val.get('$157')
    if current_style:
        current_style = str(current_style)
    else:
        current_style = parent_style

    # Check if this item has a text reference
    text_ref = val.get('$145', {})
    if isinstance(text_ref, dict) and '$403' in text_ref:
        text_idx = text_ref.get('$403')
        frag_version = str(text_ref.get('version', ''))
        key = (frag_version, int(text_idx))

        if current_style:
            text_to_style[key] = current_style

        # Check for inline style runs
        inline_runs = val.get('$142', [])
        if inline_runs:
            runs = []
            for run in inline_runs:
                if isinstance(run, dict):
                    offset = run.get('$143', 0)
                    length = run.get('$144', 0)
                    run_style = run.get('$157')
                    if run_style:
                        runs.append((offset, length, str(run_style)))
            if runs:
                text_to_inline_styles[key] = runs

    # Recursively process child content, passing current style down
    for child in val.get('$146', []):
        process_content_block(child, styles, text_to_style, text_to_inline_styles, current_style)


def ion_to_python(ion_val):
    """Convert Ion values to plain Python types."""
    if hasattr(ion_val, 'items'):
        return {str(k): ion_to_python(v) for k, v in ion_val.items()}
    elif hasattr(ion_val, '__iter__') and not isinstance(ion_val, (str, bytes)):
        return [ion_to_python(v) for v in ion_val]
    elif hasattr(ion_val, 'text'):
        return ion_val.text
    else:
        return ion_val


def format_value(v, known_symbols, show_ids=True):
    """Format a KFX value with symbol names and IDs."""
    if isinstance(v, str) and v.startswith('$') and v[1:].isdigit():
        name = known_symbols.get(v)
        if show_ids:
            return f"{v} ({name})" if name else v
        return name if name else v
    elif isinstance(v, dict):
        # Format unit/value pairs nicely
        if '$307' in v and '$306' in v:
            val = v['$307']
            unit = v['$306']
            unit_name = known_symbols.get(unit, unit)
            if show_ids:
                return f"{val} {unit} ({unit_name})" if unit_name != unit else f"{val} {unit}"
            return f"{val} [{unit_name}]"
        parts = [f"{format_value(k, known_symbols, show_ids)}={format_value(dv, known_symbols, show_ids)}"
                 for k, dv in v.items()]
        return "{" + ", ".join(parts) + "}"
    elif isinstance(v, list):
        return "[" + ", ".join(format_value(x, known_symbols, show_ids) for x in v) + "]"
    elif isinstance(v, int) and v > 0xFF000000:
        # Likely an ARGB color
        return f"0x{v:08X}"
    else:
        return str(v)


def get_css_category(css_class: str) -> str:
    """Categorize a CSS class by its property type."""
    # Common Tailwind prefixes and their categories
    prefixes = [
        # Typography
        ('font-', 'font'),
        ('text-', 'text'),
        ('tracking-', 'letter-spacing'),
        ('leading-', 'line-height'),
        ('decoration-', 'text-decoration'),
        ('underline', 'text-decoration'),
        ('overline', 'text-decoration'),
        ('line-through', 'text-decoration'),
        ('no-underline', 'text-decoration'),
        ('uppercase', 'text-transform'),
        ('lowercase', 'text-transform'),
        ('capitalize', 'text-transform'),
        ('normal-case', 'text-transform'),
        ('italic', 'font-style'),
        ('not-italic', 'font-style'),
        ('antialiased', 'font-smoothing'),
        ('subpixel-antialiased', 'font-smoothing'),
        ('truncate', 'text-overflow'),
        ('indent-', 'text-indent'),
        ('align-', 'vertical-align'),
        ('whitespace-', 'white-space'),
        ('break-', 'word-break'),
        ('hyphens-', 'hyphens'),
        ('content-', 'content'),

        # Colors
        ('bg-', 'background'),
        ('from-', 'gradient'),
        ('via-', 'gradient'),
        ('to-', 'gradient'),
        ('border-', 'border'),
        ('outline-', 'outline'),
        ('ring-', 'ring'),
        ('shadow-', 'shadow'),
        ('opacity-', 'opacity'),
        ('mix-blend-', 'mix-blend'),
        ('bg-blend-', 'bg-blend'),

        # Spacing
        ('p-', 'padding'),
        ('px-', 'padding'),
        ('py-', 'padding'),
        ('pt-', 'padding'),
        ('pr-', 'padding'),
        ('pb-', 'padding'),
        ('pl-', 'padding'),
        ('ps-', 'padding'),
        ('pe-', 'padding'),
        ('m-', 'margin'),
        ('mx-', 'margin'),
        ('my-', 'margin'),
        ('mt-', 'margin'),
        ('mr-', 'margin'),
        ('mb-', 'margin'),
        ('ml-', 'margin'),
        ('ms-', 'margin'),
        ('me-', 'margin'),
        ('space-', 'space'),
        ('gap-', 'gap'),

        # Sizing
        ('w-', 'width'),
        ('min-w-', 'min-width'),
        ('max-w-', 'max-width'),
        ('h-', 'height'),
        ('min-h-', 'min-height'),
        ('max-h-', 'max-height'),
        ('size-', 'size'),
        ('aspect-', 'aspect-ratio'),

        # Layout
        ('container', 'container'),
        ('columns-', 'columns'),
        ('break-', 'break'),
        ('box-', 'box'),
        ('block', 'display'),
        ('inline-block', 'display'),
        ('inline', 'display'),
        ('flex', 'display'),
        ('inline-flex', 'display'),
        ('grid', 'display'),
        ('inline-grid', 'display'),
        ('contents', 'display'),
        ('flow-root', 'display'),
        ('hidden', 'display'),
        ('float-', 'float'),
        ('clear-', 'clear'),
        ('isolate', 'isolation'),
        ('isolation-', 'isolation'),
        ('object-', 'object'),
        ('overflow-', 'overflow'),
        ('overscroll-', 'overscroll'),
        ('static', 'position'),
        ('fixed', 'position'),
        ('absolute', 'position'),
        ('relative', 'position'),
        ('sticky', 'position'),
        ('inset-', 'inset'),
        ('top-', 'top'),
        ('right-', 'right'),
        ('bottom-', 'bottom'),
        ('left-', 'left'),
        ('start-', 'start'),
        ('end-', 'end'),
        ('visible', 'visibility'),
        ('invisible', 'visibility'),
        ('collapse', 'visibility'),
        ('z-', 'z-index'),

        # Flexbox/Grid
        ('basis-', 'flex-basis'),
        ('flex-', 'flex'),
        ('grow', 'flex-grow'),
        ('shrink', 'flex-shrink'),
        ('order-', 'order'),
        ('grid-', 'grid'),
        ('col-', 'grid-column'),
        ('row-', 'grid-row'),
        ('auto-', 'grid-auto'),
        ('justify-', 'justify'),
        ('items-', 'align-items'),
        ('self-', 'align-self'),
        ('place-', 'place'),

        # Tables
        ('table-', 'table'),
        ('border-collapse', 'border-collapse'),
        ('border-separate', 'border-collapse'),
        ('border-spacing-', 'border-spacing'),
        ('caption-', 'caption'),

        # Lists
        ('list-', 'list'),

        # Transforms
        ('scale-', 'transform'),
        ('rotate-', 'transform'),
        ('translate-', 'transform'),
        ('skew-', 'transform'),
        ('origin-', 'transform-origin'),

        # Transitions/Animation
        ('transition-', 'transition'),
        ('duration-', 'transition'),
        ('ease-', 'transition'),
        ('delay-', 'transition'),
        ('animate-', 'animation'),

        # Filters
        ('blur-', 'filter'),
        ('brightness-', 'filter'),
        ('contrast-', 'filter'),
        ('drop-shadow-', 'filter'),
        ('grayscale-', 'filter'),
        ('hue-rotate-', 'filter'),
        ('invert-', 'filter'),
        ('saturate-', 'filter'),
        ('sepia-', 'filter'),
        ('backdrop-', 'backdrop-filter'),

        # Interactivity
        ('accent-', 'accent-color'),
        ('appearance-', 'appearance'),
        ('cursor-', 'cursor'),
        ('caret-', 'caret'),
        ('pointer-events-', 'pointer-events'),
        ('resize-', 'resize'),
        ('scroll-', 'scroll'),
        ('snap-', 'scroll-snap'),
        ('touch-', 'touch-action'),
        ('select-', 'user-select'),
        ('will-change-', 'will-change'),

        # SVG
        ('fill-', 'fill'),
        ('stroke-', 'stroke'),

        # Accessibility
        ('sr-only', 'accessibility'),
        ('not-sr-only', 'accessibility'),
        ('forced-color-adjust-', 'accessibility'),
    ]

    for prefix, category in prefixes:
        if css_class.startswith(prefix) or css_class == prefix.rstrip('-'):
            return category

    return 'other'


def main():
    if len(sys.argv) < 2:
        print("Usage: analyze_kfx_mapping.py <kfx_file>")
        sys.exit(1)

    kfx_path = sys.argv[1]
    fragments, method = load_kfx(kfx_path)
    known_symbols = get_symbol_names()

    print(f"Loaded {len(fragments)} fragments using {method}")
    print()

    # Build indices
    styles = {}  # style_id -> properties dict
    text_fragments = {}  # fragment_id -> list of text strings
    text_to_style = {}  # (fragment_id, index) -> style_id
    text_to_inline_styles = {}  # (fragment_id, index) -> [(offset, length, style_id), ...]

    for frag in fragments:
        ftype = str(frag.ftype)

        # Collect styles ($157)
        if ftype == '$157':
            style_id = str(frag.fid)
            styles[style_id] = ion_to_python(frag.value)

        # Collect text content ($145) - per fragment
        elif ftype == '$145':
            frag_id = str(frag.fid)
            val = ion_to_python(frag.value)
            text_fragments[frag_id] = val.get('$146', [])

        # Collect paragraph structures ($259)
        elif ftype == '$259':
            val = ion_to_python(frag.value)
            process_content_block(val, styles, text_to_style, text_to_inline_styles)

    # Flatten text content for display
    text_content = []
    text_key_to_global = {}  # (frag_id, idx) -> global_idx
    for frag_id, texts in text_fragments.items():
        for idx, text in enumerate(texts):
            key = (frag_id, idx)
            text_key_to_global[key] = len(text_content)
            text_content.append((key, text))

    total_text = sum(len(t) for t in text_fragments.values())
    print(f"Found {total_text} text entries across {len(text_fragments)} fragments")
    print(f"Found {len(styles)} styles")
    print(f"Found {len(text_to_style)} text-to-style mappings")
    print()

    # Find baseline style
    baseline_style = None
    baseline_style_id = None
    for key, text in text_content:
        if text == '__BASELINE__':
            baseline_style_id = text_to_style.get(key)
            baseline_style = styles.get(baseline_style_id, {})
            break

    if baseline_style:
        print("=" * 70)
        print("BASELINE STYLE")
        print("=" * 70)
        for k, v in sorted(baseline_style.items()):
            if k != '$173':  # Skip style name
                print(f"  {k}: {format_value(v, known_symbols, show_ids=True)}")
        print()

    # Find all markers and their styles
    marker_pattern = re.compile(r'^__(.+)__$')

    # Results storage
    html_results = []
    css_with_effect = defaultdict(list)  # category -> [(class, diffs)]
    css_without_effect = defaultdict(list)  # category -> [class, ...]

    for key, text in text_content:
        match = marker_pattern.match(str(text))
        if not match:
            continue

        marker_name = match.group(1)

        # Get the style for this text
        style_id = text_to_style.get(key)
        style = styles.get(style_id, {}) if style_id else {}

        # Check for inline styles (for HTML elements wrapped in spans)
        inline_runs = text_to_inline_styles.get(key, [])
        if inline_runs:
            # Use the first inline style that covers the marker
            for offset, length, run_style_id in inline_runs:
                run_style = styles.get(run_style_id, {})
                if run_style:
                    style = run_style
                    style_id = run_style_id
                    break

        # Calculate diff from baseline
        diffs = {}
        if baseline_style:
            for k, v in style.items():
                if k == '$173':  # Skip style name
                    continue
                if k not in baseline_style or baseline_style[k] != v:
                    diffs[k] = v
            # Note removed properties
            for k in baseline_style:
                if k != '$173' and k not in style:
                    diffs[k] = "(removed)"
        else:
            diffs = {k: v for k, v in style.items() if k != '$173'}

        # Categorize result
        if marker_name.startswith('CSS_'):
            css_class = marker_name[4:].replace('_', '-')
            category = get_css_category(css_class)

            if diffs:
                css_with_effect[category].append((css_class, diffs))
            else:
                css_without_effect[category].append(css_class)
        elif marker_name.startswith('inline_') or marker_name.startswith('ELEM_'):
            # HTML element marker
            elem_name = marker_name.replace('inline_', '').replace('ELEM_', '')
            html_results.append((elem_name, diffs))
        else:
            # Structure marker (like "ul_li_1", "blockquote_p1", etc.)
            html_results.append((marker_name, diffs))

    # Output HTML structures
    if html_results:
        print("=" * 70)
        print("HTML STRUCTURES")
        print("=" * 70)
        for name, diffs in html_results:
            if diffs:
                props = ", ".join(f"{k}={format_value(v, known_symbols, show_ids=True)}"
                                  for k, v in sorted(diffs.items()))
                print(f"  {name:25} -> {props}")
            else:
                print(f"  {name:25} -> (baseline)")
        print()

    # Output CSS properties WITH effect, grouped by category
    if css_with_effect:
        print("=" * 70)
        print("CSS PROPERTIES WITH EFFECT")
        print("=" * 70)

        for category in sorted(css_with_effect.keys()):
            results = css_with_effect[category]
            print(f"\n  --- {category} ({len(results)} classes) ---")
            for css_class, diffs in sorted(results):
                props = ", ".join(f"{k}={format_value(v, known_symbols, show_ids=True)}"
                                  for k, v in sorted(diffs.items()))
                print(f"  {css_class:30} -> {props}")
        print()

    # Output summary of CSS properties WITHOUT effect
    if css_without_effect:
        print("=" * 70)
        print("CSS PROPERTIES WITHOUT EFFECT")
        print("=" * 70)
        total_no_effect = sum(len(v) for v in css_without_effect.values())
        print(f"\n  Total: {total_no_effect} classes had no KFX style difference\n")

        for category in sorted(css_without_effect.keys()):
            classes = css_without_effect[category]
            print(f"  --- {category} ({len(classes)} classes) ---")
            # Show first few classes as examples
            if len(classes) <= 5:
                print(f"      {', '.join(sorted(classes))}")
            else:
                examples = sorted(classes)[:3]
                print(f"      {', '.join(examples)}, ... and {len(classes) - 3} more")
        print()

    # Summary statistics
    total_with = sum(len(v) for v in css_with_effect.values())
    total_without = sum(len(v) for v in css_without_effect.values())
    total = total_with + total_without

    print("=" * 70)
    print("SUMMARY")
    print("=" * 70)
    print(f"  Total CSS classes tested: {total}")
    print(f"  With KFX effect:          {total_with} ({100*total_with/total:.1f}%)" if total else "")
    print(f"  Without effect:           {total_without} ({100*total_without/total:.1f}%)" if total else "")
    print()

    # Summary of unknown symbols used
    print("=" * 70)
    print("SYMBOL SUMMARY")
    print("=" * 70)

    all_symbols = set()
    for style in styles.values():
        collect_symbols(style, all_symbols)

    unknown = sorted([s for s in all_symbols if s not in known_symbols],
                     key=lambda x: int(x[1:]) if x[1:].isdigit() else 0)
    known_used = sorted([s for s in all_symbols if s in known_symbols],
                        key=lambda x: int(x[1:]) if x[1:].isdigit() else 0)

    print(f"\nKnown symbols used ({len(known_used)}):")
    for sym in known_used:
        print(f"  {format_symbol(sym)}")

    if unknown:
        # Filter out style name IDs (high numbers)
        real_unknown = [s for s in unknown if int(s[1:]) < 800]
        if real_unknown:
            print(f"\nUnknown property symbols ({len(real_unknown)}):")
            for sym in real_unknown:
                print(f"  {sym}")


def collect_symbols(obj, symbols_set):
    """Recursively collect all symbols from a value."""
    if isinstance(obj, str) and obj.startswith('$') and obj[1:].isdigit():
        symbols_set.add(obj)
    elif isinstance(obj, dict):
        for k, v in obj.items():
            if isinstance(k, str) and k.startswith('$') and k[1:].isdigit():
                symbols_set.add(k)
            collect_symbols(v, symbols_set)
    elif isinstance(obj, list):
        for item in obj:
            collect_symbols(item, symbols_set)


if __name__ == "__main__":
    main()
