#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "tinycss2",
#     "requests",
# ]
# ///
"""
Generate a test EPUB with unique markers for CSS-to-KFX mapping analysis.

Downloads Tailwind CSS utilities from CDN and generates one test case per CSS class.
Also includes every valid HTML5 element.

Usage:
    uv run scripts/generate_css_test_epub.py
"""

import re
import zipfile
from pathlib import Path

import requests
import tinycss2

OUTPUT_PATH = Path(__file__).parent.parent / "tests" / "fixtures" / "css-mapping-test.epub"

# Tailwind utilities CSS from CDN
TAILWIND_CDN_URL = "https://raw.githubusercontent.com/fondoger/tailwindcss-utilities/refs/heads/main/dist/tailwind-utilities.css"

# Complete list of HTML5 elements (from WHATWG HTML Living Standard)
# Grouped by category for reference
HTML5_ELEMENTS = {
    # Document metadata
    "metadata": ["base", "head", "link", "meta", "style", "title"],

    # Sectioning root
    "sectioning_root": ["body"],

    # Content sectioning
    "content_sectioning": [
        "address", "article", "aside", "footer", "header", "h1", "h2", "h3",
        "h4", "h5", "h6", "hgroup", "main", "nav", "section", "search",
    ],

    # Text content
    "text_content": [
        "blockquote", "dd", "div", "dl", "dt", "figcaption", "figure", "hr",
        "li", "menu", "ol", "p", "pre", "ul",
    ],

    # Inline text semantics
    "inline_text": [
        "a", "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "dfn",
        "em", "i", "kbd", "mark", "q", "rp", "rt", "ruby", "s", "samp",
        "small", "span", "strong", "sub", "sup", "time", "u", "var", "wbr",
    ],

    # Image and multimedia
    "media": ["area", "audio", "img", "map", "track", "video"],

    # Embedded content
    "embedded": ["embed", "iframe", "object", "picture", "portal", "source"],

    # SVG and MathML
    "svg_math": ["svg", "math"],

    # Scripting
    "scripting": ["canvas", "noscript", "script"],

    # Demarcating edits
    "edits": ["del", "ins"],

    # Table content
    "table": [
        "caption", "col", "colgroup", "table", "tbody", "td", "tfoot", "th",
        "thead", "tr",
    ],

    # Forms
    "forms": [
        "button", "datalist", "fieldset", "form", "input", "label", "legend",
        "meter", "optgroup", "option", "output", "progress", "select", "textarea",
    ],

    # Interactive elements
    "interactive": ["details", "dialog", "summary"],

    # Web components
    "web_components": ["slot", "template"],
}

# Inline elements for testing
INLINE_ELEMENTS = [
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "dfn", "em",
    "i", "kbd", "mark", "q", "s", "samp", "small", "span", "strong",
    "sub", "sup", "time", "u", "var", "del", "ins",
]


def marker(name: str) -> str:
    """Generate a unique marker for a test case."""
    return f"__{name}__"


def download_tailwind_css() -> str:
    """Download Tailwind CSS utilities from CDN."""
    print(f"Downloading Tailwind CSS from {TAILWIND_CDN_URL}...")
    response = requests.get(TAILWIND_CDN_URL, timeout=30)
    response.raise_for_status()
    print(f"Downloaded {len(response.text)} bytes")
    return response.text


def parse_css_classes(css_content: str) -> list[tuple[str, str]]:
    """
    Parse CSS content and extract class selectors with their rules.

    Returns list of (class_name, css_rule_text) tuples.
    """
    classes = []

    # CSS properties/values that Kindle doesn't support (cause errors or warnings)
    UNSUPPORTED_PATTERNS = [
        'visibility: collapse',
        'visibility:collapse',
        '-webkit-box',
        'currentcolor',
        'currentColor',
        'caption-side',
    ]

    # Parse with tinycss2
    rules = tinycss2.parse_stylesheet(css_content, skip_whitespace=True, skip_comments=True)

    for rule in rules:
        if rule.type == 'qualified-rule':
            # Get the selector (prelude)
            selector = tinycss2.serialize(rule.prelude).strip()

            # Get the declarations
            declarations = tinycss2.serialize(rule.content).strip()

            # Skip rules with unsupported CSS
            if any(pattern in declarations for pattern in UNSUPPORTED_PATTERNS):
                continue

            # Check if it's a simple class selector (starts with . and no other selectors)
            # Skip pseudo-classes, combinators, etc.
            if selector.startswith('.') and not any(c in selector for c in [' ', '>', '+', '~', ':', '[', ',']):
                class_name = selector[1:]  # Remove the leading dot

                # Skip classes with special characters that are hard to use as markers
                if class_name and not any(c in class_name for c in ['\\', '@', '%']):
                    classes.append((class_name, f".{class_name} {{ {declarations} }}"))

    return classes


def generate_content(css_classes: list[tuple[str, str]]) -> str:
    """Generate the XHTML content with test cases."""
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<!DOCTYPE html>',
        '<html xmlns="http://www.w3.org/1999/xhtml">',
        '<head>',
        '  <title>CSS Mapping Test</title>',
        '  <link href="styles.css" rel="stylesheet" type="text/css"/>',
        '</head>',
        '<body>',
        '',
        '<!-- ============================================ -->',
        '<!-- BASELINE -->',
        '<!-- ============================================ -->',
        '<h1>Baseline</h1>',
        f'<p>{marker("BASELINE")}</p>',
        '',
    ]

    # ==========================================
    # HTML5 STRUCTURES
    # ==========================================
    lines.append('<!-- ============================================ -->')
    lines.append('<!-- HTML5 STRUCTURES -->')
    lines.append('<!-- ============================================ -->')
    lines.append('')

    # --- Headings ---
    lines.append('<h1>Heading Level 1</h1>')
    lines.append(f'<p>{marker("after_h1")}</p>')
    lines.append('<h2>Heading Level 2</h2>')
    lines.append(f'<p>{marker("after_h2")}</p>')
    lines.append('<h3>Heading Level 3</h3>')
    lines.append(f'<p>{marker("after_h3")}</p>')
    lines.append('<h4>Heading Level 4</h4>')
    lines.append(f'<p>{marker("after_h4")}</p>')
    lines.append('<h5>Heading Level 5</h5>')
    lines.append(f'<p>{marker("after_h5")}</p>')
    lines.append('<h6>Heading Level 6</h6>')
    lines.append(f'<p>{marker("after_h6")}</p>')
    lines.append('')

    # --- Inline Elements in a paragraph ---
    lines.append('<h2>Inline Elements</h2>')
    lines.append('<p>')
    for elem in INLINE_ELEMENTS:
        m = marker(f"inline_{elem}")
        if elem == "a":
            lines.append(f'  <{elem} href="#">{m}</{elem}>')
        elif elem == "data":
            lines.append(f'  <{elem} value="42">{m}</{elem}>')
        elif elem == "time":
            lines.append(f'  <{elem} datetime="2024-01-15">{m}</{elem}>')
        elif elem == "bdo":
            lines.append(f'  <{elem} dir="ltr">{m}</{elem}>')
        else:
            lines.append(f'  <{elem}>{m}</{elem}>')
    lines.append('</p>')
    lines.append('')

    # --- Paragraphs ---
    lines.append('<h2>Paragraphs</h2>')
    lines.append(f'<p>{marker("p_first")}</p>')
    lines.append(f'<p>{marker("p_second")}</p>')
    lines.append(f'<p>{marker("p_third")}</p>')
    lines.append('')

    # --- Blockquote ---
    lines.append('<h2>Blockquote</h2>')
    lines.append('<blockquote>')
    lines.append(f'  <p>{marker("blockquote_p1")}</p>')
    lines.append(f'  <p>{marker("blockquote_p2")}</p>')
    lines.append(f'  <footer>— <cite>{marker("blockquote_cite")}</cite></footer>')
    lines.append('</blockquote>')
    lines.append('')

    # --- Preformatted text ---
    lines.append('<h2>Preformatted Text</h2>')
    lines.append(f'<pre>{marker("pre_content")}</pre>')
    lines.append(f'<pre><code>{marker("pre_code")}</code></pre>')
    lines.append('')

    # --- Unordered List ---
    lines.append('<h2>Unordered List</h2>')
    lines.append('<ul>')
    lines.append(f'  <li>{marker("ul_li_1")}</li>')
    lines.append(f'  <li>{marker("ul_li_2")}</li>')
    lines.append(f'  <li>{marker("ul_li_3")}</li>')
    lines.append('</ul>')
    lines.append('')

    # --- Ordered List ---
    lines.append('<h2>Ordered List</h2>')
    lines.append('<ol>')
    lines.append(f'  <li>{marker("ol_li_1")}</li>')
    lines.append(f'  <li>{marker("ol_li_2")}</li>')
    lines.append(f'  <li>{marker("ol_li_3")}</li>')
    lines.append('</ol>')
    lines.append('')

    # --- Nested List ---
    lines.append('<h2>Nested List</h2>')
    lines.append('<ul>')
    lines.append(f'  <li>{marker("nested_ul_li_1")}')
    lines.append('    <ul>')
    lines.append(f'      <li>{marker("nested_ul_li_1a")}</li>')
    lines.append(f'      <li>{marker("nested_ul_li_1b")}</li>')
    lines.append('    </ul>')
    lines.append('  </li>')
    lines.append(f'  <li>{marker("nested_ul_li_2")}</li>')
    lines.append('</ul>')
    lines.append('')

    # --- Definition List ---
    lines.append('<h2>Definition List</h2>')
    lines.append('<dl>')
    lines.append(f'  <dt>{marker("dl_dt_1")}</dt>')
    lines.append(f'  <dd>{marker("dl_dd_1")}</dd>')
    lines.append(f'  <dt>{marker("dl_dt_2")}</dt>')
    lines.append(f'  <dd>{marker("dl_dd_2a")}</dd>')
    lines.append(f'  <dd>{marker("dl_dd_2b")}</dd>')
    lines.append('</dl>')
    lines.append('')

    # --- Simple Table ---
    lines.append('<h2>Simple Table</h2>')
    lines.append('<table>')
    lines.append(f'  <caption>{marker("table_caption")}</caption>')
    lines.append('  <thead>')
    lines.append('    <tr>')
    lines.append(f'      <th>{marker("th_col1")}</th>')
    lines.append(f'      <th>{marker("th_col2")}</th>')
    lines.append(f'      <th>{marker("th_col3")}</th>')
    lines.append('    </tr>')
    lines.append('  </thead>')
    lines.append('  <tbody>')
    lines.append('    <tr>')
    lines.append(f'      <td>{marker("td_r1c1")}</td>')
    lines.append(f'      <td>{marker("td_r1c2")}</td>')
    lines.append(f'      <td>{marker("td_r1c3")}</td>')
    lines.append('    </tr>')
    lines.append('    <tr>')
    lines.append(f'      <td>{marker("td_r2c1")}</td>')
    lines.append(f'      <td>{marker("td_r2c2")}</td>')
    lines.append(f'      <td>{marker("td_r2c3")}</td>')
    lines.append('    </tr>')
    lines.append('  </tbody>')
    lines.append('  <tfoot>')
    lines.append('    <tr>')
    lines.append(f'      <td>{marker("tfoot_c1")}</td>')
    lines.append(f'      <td>{marker("tfoot_c2")}</td>')
    lines.append(f'      <td>{marker("tfoot_c3")}</td>')
    lines.append('    </tr>')
    lines.append('  </tfoot>')
    lines.append('</table>')
    lines.append('')

    # --- Table with row headers ---
    lines.append('<h2>Table with Row Headers</h2>')
    lines.append('<table>')
    lines.append('  <tbody>')
    lines.append('    <tr>')
    lines.append(f'      <th scope="row">{marker("row_th_1")}</th>')
    lines.append(f'      <td>{marker("row_td_1a")}</td>')
    lines.append(f'      <td>{marker("row_td_1b")}</td>')
    lines.append('    </tr>')
    lines.append('    <tr>')
    lines.append(f'      <th scope="row">{marker("row_th_2")}</th>')
    lines.append(f'      <td>{marker("row_td_2a")}</td>')
    lines.append(f'      <td>{marker("row_td_2b")}</td>')
    lines.append('    </tr>')
    lines.append('  </tbody>')
    lines.append('</table>')
    lines.append('')

    # --- Figure with caption ---
    lines.append('<h2>Figure</h2>')
    lines.append('<figure>')
    lines.append(f'  <p>{marker("figure_content")}</p>')
    lines.append(f'  <figcaption>{marker("figcaption")}</figcaption>')
    lines.append('</figure>')
    lines.append('')

    # --- Article structure ---
    lines.append('<h2>Article Structure</h2>')
    lines.append('<article>')
    lines.append(f'  <header><h3>{marker("article_title")}</h3></header>')
    lines.append(f'  <p>{marker("article_p1")}</p>')
    lines.append(f'  <p>{marker("article_p2")}</p>')
    lines.append(f'  <footer>{marker("article_footer")}</footer>')
    lines.append('</article>')
    lines.append('')

    # --- Section/Aside ---
    lines.append('<h2>Section and Aside</h2>')
    lines.append('<section>')
    lines.append(f'  <h3>{marker("section_title")}</h3>')
    lines.append(f'  <p>{marker("section_p")}</p>')
    lines.append('</section>')
    lines.append('<aside>')
    lines.append(f'  <p>{marker("aside_p")}</p>')
    lines.append('</aside>')
    lines.append('')

    # --- Address ---
    lines.append('<h2>Address</h2>')
    lines.append('<address>')
    lines.append(f'  {marker("address_line1")}<br/>')
    lines.append(f'  {marker("address_line2")}')
    lines.append('</address>')
    lines.append('')

    # --- Ruby annotation ---
    lines.append('<h2>Ruby Annotation</h2>')
    lines.append(f'<p><ruby>{marker("ruby_base")}<rp>(</rp><rt>{marker("ruby_rt")}</rt><rp>)</rp></ruby></p>')
    lines.append('')

    # --- Details/Summary ---
    lines.append('<h2>Details and Summary</h2>')
    lines.append('<details>')
    lines.append(f'  <summary>{marker("summary")}</summary>')
    lines.append(f'  <p>{marker("details_content")}</p>')
    lines.append('</details>')
    lines.append('')

    # --- Horizontal rule ---
    lines.append('<h2>Horizontal Rule</h2>')
    lines.append(f'<p>{marker("before_hr")}</p>')
    lines.append('<hr/>')
    lines.append(f'<p>{marker("after_hr")}</p>')

    # ==========================================
    # CSS CLASSES
    # ==========================================
    lines.append('')
    lines.append('<!-- ============================================ -->')
    lines.append('<!-- CSS CLASSES (Tailwind Utilities) -->')
    lines.append('<!-- ============================================ -->')
    lines.append('')
    lines.append('<h1>CSS Classes</h1>')

    for css_class, _ in css_classes:
        # Convert class name to valid marker (replace special chars)
        marker_name = css_class.replace('-', '_').replace('/', '_').replace('.', '_')
        m = marker(f"CSS_{marker_name}")

        # Determine appropriate element for the class
        if css_class.startswith(('list-',)):
            # List classes - apply to ul/ol
            if 'decimal' in css_class or 'roman' in css_class:
                lines.append(f'<ol class="{css_class}"><li>{m}</li></ol>')
            else:
                lines.append(f'<ul class="{css_class}"><li>{m}</li></ul>')

        elif css_class.startswith(('border-collapse', 'border-separate', 'table-')):
            # Table classes
            lines.append(f'<table class="{css_class}"><tr><td>{m}</td></tr></table>')

        elif css_class.startswith(('bg-', 'text-')) and css_class not in ('text-left', 'text-center', 'text-right', 'text-justify', 'text-start', 'text-end'):
            # Color classes - use span
            lines.append(f'<p><span class="{css_class}">{m}</span></p>')

        elif css_class.startswith(('align-',)):
            # Vertical align - use span
            lines.append(f'<p><span class="{css_class}">{m}</span></p>')

        elif css_class.startswith(('m-', 'mx-', 'my-', 'mt-', 'mb-', 'ml-', 'mr-', 'ms-', 'me-',
                                   'p-', 'px-', 'py-', 'pt-', 'pb-', 'pl-', 'pr-', 'ps-', 'pe-',
                                   'w-', 'max-w-', 'min-w-', 'h-', 'max-h-', 'min-h-',
                                   'gap-', 'space-')):
            # Box model classes - use div
            lines.append(f'<div class="{css_class}">{m}</div>')

        elif css_class.startswith(('border',)) and css_class not in ('border-collapse', 'border-separate'):
            # Border classes - use div
            lines.append(f'<div class="{css_class}">{m}</div>')

        elif css_class in ('block', 'inline', 'inline-block', 'hidden', 'flex', 'grid', 'contents', 'flow-root'):
            # Display classes - use span to see transformation
            lines.append(f'<p><span class="{css_class}">{m}</span></p>')

        else:
            # Default - use p with class
            lines.append(f'<p class="{css_class}">{m}</p>')

    lines.extend([
        '',
        '</body>',
        '</html>',
    ])

    return '\n'.join(lines)


def generate_nav() -> str:
    """Generate navigation document."""
    return '''<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
  <title>Navigation</title>
</head>
<body>
<nav epub:type="toc">
  <h1>Contents</h1>
  <ol>
    <li><a href="content.xhtml">CSS Mapping Test</a></li>
  </ol>
</nav>
</body>
</html>
'''


def generate_opf() -> str:
    """Generate package document."""
    return '''<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">css-mapping-test</dc:identifier>
    <dc:title>CSS Mapping Test</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2024-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="content" href="content.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="styles.css" media-type="text/css"/>
  </manifest>
  <spine>
    <itemref idref="content"/>
  </spine>
</package>
'''


def generate_container() -> str:
    """Generate container.xml."""
    return '''<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
'''


def create_epub() -> None:
    """Create the test EPUB file."""
    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)

    # Download and parse Tailwind CSS
    css_content = download_tailwind_css()
    css_classes = parse_css_classes(css_content)

    print(f"Parsed {len(css_classes)} CSS classes")

    with zipfile.ZipFile(OUTPUT_PATH, 'w', zipfile.ZIP_DEFLATED) as zf:
        # Mimetype must be first and uncompressed
        zf.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)

        # Container
        zf.writestr('META-INF/container.xml', generate_container())

        # Package document
        zf.writestr('OEBPS/content.opf', generate_opf())

        # Navigation
        zf.writestr('OEBPS/nav.xhtml', generate_nav())

        # Stylesheet (the original Tailwind CSS)
        zf.writestr('OEBPS/styles.css', css_content)

        # Content
        zf.writestr('OEBPS/content.xhtml', generate_content(css_classes))

    print(f"Created: {OUTPUT_PATH}")
    print(f"Size: {OUTPUT_PATH.stat().st_size} bytes")

    # Print summary
    print(f"\nTest cases:")
    print(f"  CSS classes: {len(css_classes)}")


if __name__ == "__main__":
    create_epub()
