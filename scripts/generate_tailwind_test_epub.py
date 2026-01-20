#!/usr/bin/env python3
"""
Generate a comprehensive test EPUB with Tailwind CSS and all HTML5 elements.
This helps discover KFX style and element ID mappings.
"""

import zipfile
import os
from pathlib import Path

OUTPUT_PATH = Path(__file__).parent.parent / "tests/fixtures/tailwind-test.epub"

# Tailwind CSS classes organized by category
# Using a representative subset of each category for manageable size
TAILWIND_CLASSES = {
    # Typography
    "font-family": ["font-sans", "font-serif", "font-mono"],
    "font-size": ["text-xs", "text-sm", "text-base", "text-lg", "text-xl", "text-2xl", "text-3xl", "text-4xl"],
    "font-weight": ["font-thin", "font-light", "font-normal", "font-medium", "font-semibold", "font-bold", "font-extrabold"],
    "font-style": ["italic", "not-italic"],
    "text-align": ["text-left", "text-center", "text-right", "text-justify"],
    "text-decoration": ["underline", "overline", "line-through", "no-underline"],
    "text-transform": ["uppercase", "lowercase", "capitalize", "normal-case"],
    "line-height": ["leading-none", "leading-tight", "leading-snug", "leading-normal", "leading-relaxed", "leading-loose"],
    "letter-spacing": ["tracking-tighter", "tracking-tight", "tracking-normal", "tracking-wide", "tracking-wider", "tracking-widest"],
    "text-color": ["text-black", "text-white", "text-gray-500", "text-red-500", "text-blue-500", "text-green-500"],
    "vertical-align": ["align-baseline", "align-top", "align-middle", "align-bottom", "align-text-top", "align-text-bottom", "align-sub", "align-super"],

    # Spacing
    "padding": ["p-0", "p-1", "p-2", "p-4", "p-8", "px-4", "py-2", "pt-4", "pb-4", "pl-4", "pr-4"],
    "margin": ["m-0", "m-1", "m-2", "m-4", "m-8", "mx-auto", "my-4", "mt-4", "mb-4", "ml-4", "mr-4", "-mt-4", "-mb-4"],

    # Sizing
    "width": ["w-0", "w-1", "w-4", "w-8", "w-16", "w-32", "w-64", "w-full", "w-screen", "w-auto", "w-1/2", "w-1/3", "w-1/4"],
    "height": ["h-0", "h-1", "h-4", "h-8", "h-16", "h-32", "h-64", "h-full", "h-screen", "h-auto"],
    "min-width": ["min-w-0", "min-w-full"],
    "max-width": ["max-w-xs", "max-w-sm", "max-w-md", "max-w-lg", "max-w-xl", "max-w-full", "max-w-prose"],

    # Layout
    "display": ["block", "inline-block", "inline", "flex", "inline-flex", "grid", "hidden"],
    "position": ["static", "fixed", "absolute", "relative", "sticky"],
    "float": ["float-left", "float-right", "float-none"],
    "clear": ["clear-left", "clear-right", "clear-both", "clear-none"],
    "overflow": ["overflow-auto", "overflow-hidden", "overflow-visible", "overflow-scroll"],

    # Flexbox
    "flex-direction": ["flex-row", "flex-row-reverse", "flex-col", "flex-col-reverse"],
    "flex-wrap": ["flex-wrap", "flex-wrap-reverse", "flex-nowrap"],
    "justify-content": ["justify-start", "justify-end", "justify-center", "justify-between", "justify-around", "justify-evenly"],
    "align-items": ["items-start", "items-end", "items-center", "items-baseline", "items-stretch"],
    "align-self": ["self-auto", "self-start", "self-end", "self-center", "self-stretch"],
    "flex": ["flex-1", "flex-auto", "flex-initial", "flex-none"],
    "flex-grow": ["grow", "grow-0"],
    "flex-shrink": ["shrink", "shrink-0"],

    # Grid
    "grid-cols": ["grid-cols-1", "grid-cols-2", "grid-cols-3", "grid-cols-4", "grid-cols-6", "grid-cols-12"],
    "grid-rows": ["grid-rows-1", "grid-rows-2", "grid-rows-3", "grid-rows-6"],
    "gap": ["gap-0", "gap-1", "gap-2", "gap-4", "gap-8", "gap-x-4", "gap-y-4"],

    # Backgrounds
    "bg-color": ["bg-transparent", "bg-black", "bg-white", "bg-gray-100", "bg-gray-500", "bg-red-500", "bg-blue-500", "bg-green-500"],

    # Borders
    "border-width": ["border-0", "border", "border-2", "border-4", "border-8", "border-t", "border-b", "border-l", "border-r"],
    "border-color": ["border-transparent", "border-black", "border-white", "border-gray-500", "border-red-500", "border-blue-500"],
    "border-style": ["border-solid", "border-dashed", "border-dotted", "border-double", "border-none"],
    "border-radius": ["rounded-none", "rounded-sm", "rounded", "rounded-md", "rounded-lg", "rounded-xl", "rounded-full"],

    # Effects
    "box-shadow": ["shadow-sm", "shadow", "shadow-md", "shadow-lg", "shadow-xl", "shadow-2xl", "shadow-none"],
    "opacity": ["opacity-0", "opacity-25", "opacity-50", "opacity-75", "opacity-100"],

    # Transforms
    "scale": ["scale-0", "scale-50", "scale-75", "scale-100", "scale-125", "scale-150"],
    "rotate": ["rotate-0", "rotate-45", "rotate-90", "rotate-180", "-rotate-45", "-rotate-90"],
    "translate": ["translate-x-0", "translate-x-4", "translate-y-0", "translate-y-4", "-translate-x-4", "-translate-y-4"],

    # Lists
    "list-style-type": ["list-none", "list-disc", "list-decimal"],
    "list-style-position": ["list-inside", "list-outside"],

    # Tables
    "border-collapse": ["border-collapse", "border-separate"],
    "table-layout": ["table-auto", "table-fixed"],
}

# HTML5 elements to test
HTML5_ELEMENTS = {
    "headings": ["h1", "h2", "h3", "h4", "h5", "h6"],
    "text": ["p", "span", "div", "pre", "code", "kbd", "samp", "var"],
    "inline-semantic": ["strong", "em", "b", "i", "u", "s", "mark", "small", "sub", "sup", "abbr", "cite", "q", "dfn"],
    "links": ["a"],
    "lists": ["ul", "ol", "li", "dl", "dt", "dd"],
    "quotes": ["blockquote"],
    "tables": ["table", "thead", "tbody", "tfoot", "tr", "th", "td", "caption"],
    "sections": ["article", "section", "nav", "aside", "header", "footer", "main", "figure", "figcaption"],
    "breaks": ["br", "hr"],
    "ruby": ["ruby", "rt", "rp"],
}


def generate_mimetype():
    return "application/epub+zip"


def generate_container_xml():
    return '''<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>'''


def generate_content_opf(chapters):
    manifest_items = ['<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>']
    manifest_items.append('<item id="style" href="tailwind.css" media-type="text/css"/>')

    spine_items = []

    for i, chapter in enumerate(chapters):
        item_id = f"ch{i}"
        manifest_items.append(f'<item id="{item_id}" href="{chapter}" media-type="application/xhtml+xml"/>')
        spine_items.append(f'<itemref idref="{item_id}"/>')

    return f'''<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">tailwind-css-test-epub</dc:identifier>
    <dc:title>Tailwind CSS Mapping Test</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-01-20T00:00:00Z</meta>
  </metadata>
  <manifest>
    {chr(10).join(f"    {item}" for item in manifest_items)}
  </manifest>
  <spine>
    {chr(10).join(f"    {item}" for item in spine_items)}
  </spine>
</package>'''


def generate_nav_xhtml(chapters):
    nav_items = []
    for i, (title, filename) in enumerate(chapters):
        nav_items.append(f'<li><a href="{filename}">{title}</a></li>')

    return f'''<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
  <title>Navigation</title>
</head>
<body>
  <nav epub:type="toc">
    <h1>Contents</h1>
    <ol>
      {chr(10).join(f"      {item}" for item in nav_items)}
    </ol>
  </nav>
</body>
</html>'''


def wrap_xhtml(title, body_content):
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <title>{title}</title>
  <link href="tailwind.css" rel="stylesheet" type="text/css"/>
</head>
<body>
{body_content}
</body>
</html>'''


def generate_html5_elements_chapter():
    """Generate chapter showing all HTML5 elements with no styling."""
    lines = ['<h1>HTML5 Elements Reference</h1>']
    lines.append('<p>Each element type shown with default styling.</p>')

    # Headings
    lines.append('<h2>Headings</h2>')
    for i in range(1, 7):
        lines.append(f'<h{i}>Heading {i} (h{i})</h{i}>')

    # Text elements
    lines.append('<h2>Text Elements</h2>')
    lines.append('<p>Paragraph (p) - Normal text content in a paragraph element.</p>')
    lines.append('<div>Division (div) - Generic block container.</div>')
    lines.append('<pre>Preformatted (pre) - Preserves whitespace and uses monospace.</pre>')
    lines.append('<p><code>Code (code)</code> - Inline code snippet.</p>')
    lines.append('<p><kbd>Keyboard (kbd)</kbd> - Keyboard input.</p>')
    lines.append('<p><samp>Sample (samp)</samp> - Sample output.</p>')
    lines.append('<p><var>Variable (var)</var> - Variable in math/programming.</p>')

    # Inline semantic
    lines.append('<h2>Inline Semantic Elements</h2>')
    lines.append('<p><strong>Strong (strong)</strong> - Important text.</p>')
    lines.append('<p><em>Emphasis (em)</em> - Emphasized text.</p>')
    lines.append('<p><b>Bold (b)</b> - Bold without importance.</p>')
    lines.append('<p><i>Italic (i)</i> - Alternate voice/mood.</p>')
    lines.append('<p><u>Underline (u)</u> - Unarticulated annotation.</p>')
    lines.append('<p><s>Strikethrough (s)</s> - No longer accurate.</p>')
    lines.append('<p><mark>Mark (mark)</mark> - Highlighted/marked text.</p>')
    lines.append('<p><small>Small (small)</small> - Side comments, fine print.</p>')
    lines.append('<p>H<sub>2</sub>O - Subscript (sub)</p>')
    lines.append('<p>E=mc<sup>2</sup> - Superscript (sup)</p>')
    lines.append('<p><abbr title="Abbreviation">ABBR</abbr> - Abbreviation.</p>')
    lines.append('<p><cite>Citation (cite)</cite> - Title of work.</p>')
    lines.append('<p><q>Inline quote (q)</q> - Short quotation.</p>')
    lines.append('<p><dfn>Definition (dfn)</dfn> - Term being defined.</p>')

    # Links
    lines.append('<h2>Links</h2>')
    lines.append('<p><a href="#test">Anchor link (a)</a> - Hyperlink.</p>')

    # Lists
    lines.append('<h2>Lists</h2>')
    lines.append('<h3>Unordered List (ul/li)</h3>')
    lines.append('<ul><li>Item 1</li><li>Item 2</li><li>Item 3</li></ul>')
    lines.append('<h3>Ordered List (ol/li)</h3>')
    lines.append('<ol><li>First</li><li>Second</li><li>Third</li></ol>')
    lines.append('<h3>Definition List (dl/dt/dd)</h3>')
    lines.append('<dl><dt>Term 1</dt><dd>Definition 1</dd><dt>Term 2</dt><dd>Definition 2</dd></dl>')

    # Blockquote
    lines.append('<h2>Blockquote</h2>')
    lines.append('<blockquote><p>This is a blockquote. It typically represents content quoted from another source.</p></blockquote>')

    # Tables
    lines.append('<h2>Tables</h2>')
    lines.append('''<table>
  <caption>Table Caption</caption>
  <thead><tr><th>Header 1</th><th>Header 2</th><th>Header 3</th></tr></thead>
  <tbody>
    <tr><td>Cell 1</td><td>Cell 2</td><td>Cell 3</td></tr>
    <tr><td>Cell 4</td><td>Cell 5</td><td>Cell 6</td></tr>
  </tbody>
  <tfoot><tr><td colspan="3">Table Footer</td></tr></tfoot>
</table>''')

    # Sections
    lines.append('<h2>Sectioning Elements</h2>')
    lines.append('<article><h3>Article</h3><p>Self-contained composition.</p></article>')
    lines.append('<section><h3>Section</h3><p>Generic section of document.</p></section>')
    lines.append('<nav><h3>Nav</h3><p>Navigation links.</p></nav>')
    lines.append('<aside><h3>Aside</h3><p>Tangentially related content.</p></aside>')
    lines.append('<header><h3>Header</h3><p>Introductory content.</p></header>')
    lines.append('<footer><h3>Footer</h3><p>Footer content.</p></footer>')
    lines.append('<figure><p>Figure content</p><figcaption>Figure caption</figcaption></figure>')

    # Breaks
    lines.append('<h2>Breaks</h2>')
    lines.append('<p>Line 1<br/>Line 2 after br</p>')
    lines.append('<p>Before hr</p><hr/><p>After hr</p>')

    # Ruby
    lines.append('<h2>Ruby Annotations</h2>')
    lines.append('<p><ruby>漢<rp>(</rp><rt>kan</rt><rp>)</rp>字<rp>(</rp><rt>ji</rt><rp>)</rp></ruby></p>')

    return wrap_xhtml("HTML5 Elements", '\n'.join(lines))


def generate_tailwind_chapter(category, classes):
    """Generate a chapter for a Tailwind category."""
    lines = [f'<h1>{category.replace("-", " ").title()}</h1>']

    for cls in classes:
        # Use appropriate element for the class
        if "text-" in cls or "font-" in cls or "leading-" in cls or "tracking-" in cls:
            lines.append(f'<p class="{cls}">{cls}: The quick brown fox jumps over the lazy dog.</p>')
        elif "list-" in cls:
            lines.append(f'<ul class="{cls}"><li>{cls} - Item 1</li><li>Item 2</li><li>Item 3</li></ul>')
        elif "border-" in cls or "rounded" in cls or "shadow" in cls:
            lines.append(f'<div class="{cls}" style="padding: 1em; margin: 0.5em 0;">{cls}</div>')
        elif "table-" in cls or "border-collapse" in cls or "border-separate" in cls:
            lines.append(f'<table class="{cls}"><tr><th>{cls}</th><th>Col 2</th></tr><tr><td>A</td><td>B</td></tr></table>')
        elif "flex" in cls or "grid" in cls or "justify-" in cls or "items-" in cls or "self-" in cls or "gap-" in cls:
            lines.append(f'<div class="{cls}" style="border: 1px solid #ccc; padding: 0.5em; margin: 0.5em 0;"><span>{cls}</span><span>Child 2</span><span>Child 3</span></div>')
        elif cls.startswith("w-") or cls.startswith("h-") or cls.startswith("min-") or cls.startswith("max-"):
            lines.append(f'<div class="{cls}" style="background: #eee; margin: 0.5em 0;">{cls}</div>')
        elif cls.startswith("p-") or cls.startswith("m-") or cls.startswith("px-") or cls.startswith("py-") or cls.startswith("pt-") or cls.startswith("pb-") or cls.startswith("pl-") or cls.startswith("pr-") or cls.startswith("mx-") or cls.startswith("my-") or cls.startswith("mt-") or cls.startswith("mb-") or cls.startswith("ml-") or cls.startswith("mr-") or cls.startswith("-m"):
            lines.append(f'<div class="{cls}" style="background: #ddd; border: 1px solid #999;">{cls}</div>')
        elif "bg-" in cls:
            lines.append(f'<div class="{cls}" style="padding: 1em; margin: 0.5em 0;">{cls}</div>')
        elif "opacity-" in cls:
            lines.append(f'<div class="{cls}" style="background: #333; color: white; padding: 1em;">{cls}</div>')
        elif "scale-" in cls or "rotate-" in cls or "translate-" in cls:
            lines.append(f'<div class="{cls}" style="display: inline-block; background: #eee; padding: 1em; margin: 1em;">{cls}</div>')
        elif cls in ["block", "inline-block", "inline", "flex", "inline-flex", "grid", "hidden"]:
            lines.append(f'<span class="{cls}" style="background: #eee; padding: 0.25em;">{cls}</span>')
        elif cls in ["static", "fixed", "absolute", "relative", "sticky"]:
            lines.append(f'<div class="{cls}" style="background: #eee; padding: 0.5em;">{cls} (position)</div>')
        elif "float-" in cls or "clear-" in cls:
            lines.append(f'<div class="{cls}" style="background: #eee; padding: 0.5em; margin: 0.5em;">{cls}</div>')
        elif "overflow-" in cls:
            lines.append(f'<div class="{cls}" style="width: 100px; height: 50px; background: #eee;">{cls} - Some content that might overflow the container.</div>')
        elif "align-" in cls:
            lines.append(f'<p>Baseline <span class="{cls}" style="background: #eee;">{cls}</span> text</p>')
        else:
            lines.append(f'<p class="{cls}">{cls}</p>')

    return wrap_xhtml(category.replace("-", " ").title(), '\n'.join(lines))


def get_tailwind_css():
    """Return a minimal Tailwind-like CSS for testing."""
    # We'll generate CSS rules for each class we use
    css_rules = []

    # Font family
    css_rules.append(".font-sans { font-family: ui-sans-serif, system-ui, sans-serif; }")
    css_rules.append(".font-serif { font-family: ui-serif, Georgia, serif; }")
    css_rules.append(".font-mono { font-family: ui-monospace, monospace; }")

    # Font size
    sizes = [("xs", "0.75rem"), ("sm", "0.875rem"), ("base", "1rem"), ("lg", "1.125rem"),
             ("xl", "1.25rem"), ("2xl", "1.5rem"), ("3xl", "1.875rem"), ("4xl", "2.25rem")]
    for name, size in sizes:
        css_rules.append(f".text-{name} {{ font-size: {size}; }}")

    # Font weight
    weights = [("thin", 100), ("light", 300), ("normal", 400), ("medium", 500),
               ("semibold", 600), ("bold", 700), ("extrabold", 800)]
    for name, weight in weights:
        css_rules.append(f".font-{name} {{ font-weight: {weight}; }}")

    # Font style
    css_rules.append(".italic { font-style: italic; }")
    css_rules.append(".not-italic { font-style: normal; }")

    # Text align
    for align in ["left", "center", "right", "justify"]:
        css_rules.append(f".text-{align} {{ text-align: {align}; }}")

    # Text decoration
    css_rules.append(".underline { text-decoration: underline; }")
    css_rules.append(".overline { text-decoration: overline; }")
    css_rules.append(".line-through { text-decoration: line-through; }")
    css_rules.append(".no-underline { text-decoration: none; }")

    # Text transform
    css_rules.append(".uppercase { text-transform: uppercase; }")
    css_rules.append(".lowercase { text-transform: lowercase; }")
    css_rules.append(".capitalize { text-transform: capitalize; }")
    css_rules.append(".normal-case { text-transform: none; }")

    # Line height
    line_heights = [("none", 1), ("tight", 1.25), ("snug", 1.375), ("normal", 1.5), ("relaxed", 1.625), ("loose", 2)]
    for name, val in line_heights:
        css_rules.append(f".leading-{name} {{ line-height: {val}; }}")

    # Letter spacing
    trackings = [("tighter", "-0.05em"), ("tight", "-0.025em"), ("normal", "0"),
                 ("wide", "0.025em"), ("wider", "0.05em"), ("widest", "0.1em")]
    for name, val in trackings:
        css_rules.append(f".tracking-{name} {{ letter-spacing: {val}; }}")

    # Text colors
    colors = {"black": "#000", "white": "#fff", "gray-500": "#6b7280",
              "red-500": "#ef4444", "blue-500": "#3b82f6", "green-500": "#22c55e"}
    for name, val in colors.items():
        css_rules.append(f".text-{name} {{ color: {val}; }}")
        css_rules.append(f".bg-{name.replace('500', '100') if '500' in name else name} {{ background-color: {val}20; }}")
        css_rules.append(f".bg-{name} {{ background-color: {val}; }}")
        css_rules.append(f".border-{name} {{ border-color: {val}; }}")
    css_rules.append(".bg-transparent { background-color: transparent; }")
    css_rules.append(".border-transparent { border-color: transparent; }")

    # Vertical align
    valigns = ["baseline", "top", "middle", "bottom", "text-top", "text-bottom", "sub", "super"]
    for va in valigns:
        css_rules.append(f".align-{va} {{ vertical-align: {va}; }}")

    # Spacing (padding/margin)
    spacing = [(0, "0"), (1, "0.25rem"), (2, "0.5rem"), (4, "1rem"), (8, "2rem")]
    for num, val in spacing:
        css_rules.append(f".p-{num} {{ padding: {val}; }}")
        css_rules.append(f".px-{num} {{ padding-left: {val}; padding-right: {val}; }}")
        css_rules.append(f".py-{num} {{ padding-top: {val}; padding-bottom: {val}; }}")
        css_rules.append(f".pt-{num} {{ padding-top: {val}; }}")
        css_rules.append(f".pb-{num} {{ padding-bottom: {val}; }}")
        css_rules.append(f".pl-{num} {{ padding-left: {val}; }}")
        css_rules.append(f".pr-{num} {{ padding-right: {val}; }}")
        css_rules.append(f".m-{num} {{ margin: {val}; }}")
        css_rules.append(f".mx-{num} {{ margin-left: {val}; margin-right: {val}; }}")
        css_rules.append(f".my-{num} {{ margin-top: {val}; margin-bottom: {val}; }}")
        css_rules.append(f".mt-{num} {{ margin-top: {val}; }}")
        css_rules.append(f".mb-{num} {{ margin-bottom: {val}; }}")
        css_rules.append(f".ml-{num} {{ margin-left: {val}; }}")
        css_rules.append(f".mr-{num} {{ margin-right: {val}; }}")
        if num > 0:
            css_rules.append(f".-mt-{num} {{ margin-top: -{val}; }}")
            css_rules.append(f".-mb-{num} {{ margin-bottom: -{val}; }}")
    css_rules.append(".mx-auto { margin-left: auto; margin-right: auto; }")

    # Width/Height
    for num, val in spacing:
        css_rules.append(f".w-{num} {{ width: {val}; }}")
        css_rules.append(f".h-{num} {{ height: {val}; }}")
    css_rules.extend([
        ".w-16 { width: 4rem; }", ".w-32 { width: 8rem; }", ".w-64 { width: 16rem; }",
        ".w-full { width: 100%; }", ".w-screen { width: 100vw; }", ".w-auto { width: auto; }",
        ".w-1\\/2 { width: 50%; }", ".w-1\\/3 { width: 33.333%; }", ".w-1\\/4 { width: 25%; }",
        ".h-16 { height: 4rem; }", ".h-32 { height: 8rem; }", ".h-64 { height: 16rem; }",
        ".h-full { height: 100%; }", ".h-screen { height: 100vh; }", ".h-auto { height: auto; }",
        ".min-w-0 { min-width: 0; }", ".min-w-full { min-width: 100%; }",
        ".max-w-xs { max-width: 20rem; }", ".max-w-sm { max-width: 24rem; }",
        ".max-w-md { max-width: 28rem; }", ".max-w-lg { max-width: 32rem; }",
        ".max-w-xl { max-width: 36rem; }", ".max-w-full { max-width: 100%; }",
        ".max-w-prose { max-width: 65ch; }",
    ])

    # Display
    css_rules.extend([
        ".block { display: block; }", ".inline-block { display: inline-block; }",
        ".inline { display: inline; }", ".flex { display: flex; }",
        ".inline-flex { display: inline-flex; }", ".grid { display: grid; }",
        ".hidden { display: none; }",
    ])

    # Position
    for pos in ["static", "fixed", "absolute", "relative", "sticky"]:
        css_rules.append(f".{pos} {{ position: {pos}; }}")

    # Float/Clear
    for f in ["left", "right", "none"]:
        css_rules.append(f".float-{f} {{ float: {f}; }}")
        css_rules.append(f".clear-{f} {{ clear: {f}; }}")
    css_rules.append(".clear-both { clear: both; }")

    # Overflow
    for o in ["auto", "hidden", "visible", "scroll"]:
        css_rules.append(f".overflow-{o} {{ overflow: {o}; }}")

    # Flexbox
    css_rules.extend([
        ".flex-row { flex-direction: row; }", ".flex-row-reverse { flex-direction: row-reverse; }",
        ".flex-col { flex-direction: column; }", ".flex-col-reverse { flex-direction: column-reverse; }",
        ".flex-wrap { flex-wrap: wrap; }", ".flex-wrap-reverse { flex-wrap: wrap-reverse; }",
        ".flex-nowrap { flex-wrap: nowrap; }",
        ".justify-start { justify-content: flex-start; }", ".justify-end { justify-content: flex-end; }",
        ".justify-center { justify-content: center; }", ".justify-between { justify-content: space-between; }",
        ".justify-around { justify-content: space-around; }", ".justify-evenly { justify-content: space-evenly; }",
        ".items-start { align-items: flex-start; }", ".items-end { align-items: flex-end; }",
        ".items-center { align-items: center; }", ".items-baseline { align-items: baseline; }",
        ".items-stretch { align-items: stretch; }",
        ".self-auto { align-self: auto; }", ".self-start { align-self: flex-start; }",
        ".self-end { align-self: flex-end; }", ".self-center { align-self: center; }",
        ".self-stretch { align-self: stretch; }",
        ".flex-1 { flex: 1 1 0%; }", ".flex-auto { flex: 1 1 auto; }",
        ".flex-initial { flex: 0 1 auto; }", ".flex-none { flex: none; }",
        ".grow { flex-grow: 1; }", ".grow-0 { flex-grow: 0; }",
        ".shrink { flex-shrink: 1; }", ".shrink-0 { flex-shrink: 0; }",
    ])

    # Grid
    for i in [1, 2, 3, 4, 6, 12]:
        css_rules.append(f".grid-cols-{i} {{ grid-template-columns: repeat({i}, minmax(0, 1fr)); }}")
    for i in [1, 2, 3, 6]:
        css_rules.append(f".grid-rows-{i} {{ grid-template-rows: repeat({i}, minmax(0, 1fr)); }}")
    for num, val in spacing:
        css_rules.append(f".gap-{num} {{ gap: {val}; }}")
        css_rules.append(f".gap-x-{num} {{ column-gap: {val}; }}")
        css_rules.append(f".gap-y-{num} {{ row-gap: {val}; }}")

    # Borders
    css_rules.extend([
        ".border-0 { border-width: 0; }", ".border { border-width: 1px; }",
        ".border-2 { border-width: 2px; }", ".border-4 { border-width: 4px; }",
        ".border-8 { border-width: 8px; }",
        ".border-t { border-top-width: 1px; }", ".border-b { border-bottom-width: 1px; }",
        ".border-l { border-left-width: 1px; }", ".border-r { border-right-width: 1px; }",
        ".border-solid { border-style: solid; }", ".border-dashed { border-style: dashed; }",
        ".border-dotted { border-style: dotted; }", ".border-double { border-style: double; }",
        ".border-none { border-style: none; }",
        ".rounded-none { border-radius: 0; }", ".rounded-sm { border-radius: 0.125rem; }",
        ".rounded { border-radius: 0.25rem; }", ".rounded-md { border-radius: 0.375rem; }",
        ".rounded-lg { border-radius: 0.5rem; }", ".rounded-xl { border-radius: 0.75rem; }",
        ".rounded-full { border-radius: 9999px; }",
    ])

    # Shadows
    css_rules.extend([
        ".shadow-sm { box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05); }",
        ".shadow { box-shadow: 0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1); }",
        ".shadow-md { box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1); }",
        ".shadow-lg { box-shadow: 0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1); }",
        ".shadow-xl { box-shadow: 0 20px 25px -5px rgb(0 0 0 / 0.1), 0 8px 10px -6px rgb(0 0 0 / 0.1); }",
        ".shadow-2xl { box-shadow: 0 25px 50px -12px rgb(0 0 0 / 0.25); }",
        ".shadow-none { box-shadow: none; }",
    ])

    # Opacity
    for op in [0, 25, 50, 75, 100]:
        css_rules.append(f".opacity-{op} {{ opacity: {op/100}; }}")

    # Transforms
    css_rules.extend([
        ".scale-0 { transform: scale(0); }", ".scale-50 { transform: scale(0.5); }",
        ".scale-75 { transform: scale(0.75); }", ".scale-100 { transform: scale(1); }",
        ".scale-125 { transform: scale(1.25); }", ".scale-150 { transform: scale(1.5); }",
        ".rotate-0 { transform: rotate(0deg); }", ".rotate-45 { transform: rotate(45deg); }",
        ".rotate-90 { transform: rotate(90deg); }", ".rotate-180 { transform: rotate(180deg); }",
        ".-rotate-45 { transform: rotate(-45deg); }", ".-rotate-90 { transform: rotate(-90deg); }",
        ".translate-x-0 { transform: translateX(0); }", ".translate-x-4 { transform: translateX(1rem); }",
        ".translate-y-0 { transform: translateY(0); }", ".translate-y-4 { transform: translateY(1rem); }",
        ".-translate-x-4 { transform: translateX(-1rem); }", ".-translate-y-4 { transform: translateY(-1rem); }",
    ])

    # Lists
    css_rules.extend([
        ".list-none { list-style-type: none; }", ".list-disc { list-style-type: disc; }",
        ".list-decimal { list-style-type: decimal; }",
        ".list-inside { list-style-position: inside; }", ".list-outside { list-style-position: outside; }",
    ])

    # Tables
    css_rules.extend([
        ".border-collapse { border-collapse: collapse; }", ".border-separate { border-collapse: separate; }",
        ".table-auto { table-layout: auto; }", ".table-fixed { table-layout: fixed; }",
    ])

    return "@charset \"utf-8\";\n\n" + "\n".join(css_rules)


def main():
    # Build list of chapters
    chapters = []
    chapter_files = []

    # First chapter: HTML5 elements
    chapters.append(("HTML5 Elements", "ch01-html5-elements.xhtml"))

    # Tailwind chapters by category
    for i, (category, classes) in enumerate(TAILWIND_CLASSES.items(), start=2):
        filename = f"ch{i:02d}-{category}.xhtml"
        chapters.append((category.replace("-", " ").title(), filename))

    # Create EPUB
    os.makedirs(OUTPUT_PATH.parent, exist_ok=True)

    with zipfile.ZipFile(OUTPUT_PATH, 'w', zipfile.ZIP_DEFLATED) as zf:
        # Mimetype must be first and uncompressed
        zf.writestr("mimetype", generate_mimetype(), compress_type=zipfile.ZIP_STORED)

        # Container
        zf.writestr("META-INF/container.xml", generate_container_xml())

        # Content OPF
        zf.writestr("OEBPS/content.opf", generate_content_opf([c[1] for c in chapters]))

        # Navigation
        zf.writestr("OEBPS/nav.xhtml", generate_nav_xhtml(chapters))

        # Tailwind CSS
        zf.writestr("OEBPS/tailwind.css", get_tailwind_css())

        # HTML5 elements chapter
        zf.writestr(f"OEBPS/{chapters[0][1]}", generate_html5_elements_chapter())

        # Tailwind chapters
        for i, (category, classes) in enumerate(TAILWIND_CLASSES.items(), start=2):
            filename = f"ch{i:02d}-{category}.xhtml"
            content = generate_tailwind_chapter(category, classes)
            zf.writestr(f"OEBPS/{filename}", content)

    print(f"Generated: {OUTPUT_PATH}")
    print(f"Total chapters: {len(chapters)}")
    total_classes = sum(len(classes) for classes in TAILWIND_CLASSES.values())
    print(f"Total Tailwind classes: {total_classes}")


if __name__ == "__main__":
    main()
