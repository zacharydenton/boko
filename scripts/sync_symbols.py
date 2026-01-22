#!/usr/bin/env python3
"""
Synchronize KFX symbol definitions from kfxlib's yj_to_epub_properties.py to Rust.

This script parses the authoritative symbol mappings from kfxlib and compares/updates
the Rust symbol definitions in src/kfx/writer/symbols.rs.

Usage:
    python scripts/sync_symbols.py                    # Show diff only
    python scripts/sync_symbols.py --update           # Update symbols.rs
    python scripts/sync_symbols.py --generate-report  # Generate markdown report
"""

import argparse
import ast
import re
import sys
from pathlib import Path
from collections import defaultdict
from typing import Dict, List, Tuple, Optional, Set


def extract_dict_block(content: str, var_name: str) -> str:
    """Extract a dictionary block from Python source, handling nested braces."""
    match = re.search(rf'{var_name}\s*=\s*\{{', content)
    if not match:
        return ""

    start = match.end() - 1  # Include the opening brace
    depth = 1
    pos = start + 1

    while depth > 0 and pos < len(content):
        if content[pos] == '{':
            depth += 1
        elif content[pos] == '}':
            depth -= 1
        pos += 1

    return content[start:pos]


def parse_yj_properties(source_path: Path) -> Tuple[Dict, Dict, Dict]:
    """Parse YJ_PROPERTY_INFO and YJ_LENGTH_UNITS from yj_to_epub_properties.py."""

    with open(source_path, 'r') as f:
        content = f.read()

    # Extract YJ_PROPERTY_INFO dictionary
    property_info = {}
    value_mappings = {}  # symbol -> {value_symbol: value_name}

    # Find YJ_PROPERTY_INFO block
    prop_block = extract_dict_block(content, 'YJ_PROPERTY_INFO')
    if prop_block:
        # Parse each entry - handle both simple and complex Prop() calls
        # Pattern: "$123": Prop("css-name"),  or "$123": Prop("css-name", {...}),
        # Need to handle nested braces in the value dict

        # First find all "$NNN": Prop( patterns
        prop_start_pattern = re.compile(r'"\$(\d+)":\s*Prop\("([^"]+)"')

        pos = 0
        while pos < len(prop_block):
            match = prop_start_pattern.search(prop_block, pos)
            if not match:
                break

            symbol_id = int(match.group(1))
            css_name = match.group(2)
            property_info[symbol_id] = css_name

            # Check if there's a value dict after the css name
            end_pos = match.end()
            # Skip whitespace
            while end_pos < len(prop_block) and prop_block[end_pos] in ' \t\n':
                end_pos += 1

            if end_pos < len(prop_block) and prop_block[end_pos] == ',':
                end_pos += 1
                while end_pos < len(prop_block) and prop_block[end_pos] in ' \t\n':
                    end_pos += 1

                if end_pos < len(prop_block) and prop_block[end_pos] == '{':
                    # Extract value dict
                    depth = 1
                    dict_start = end_pos
                    end_pos += 1
                    while depth > 0 and end_pos < len(prop_block):
                        if prop_block[end_pos] == '{':
                            depth += 1
                        elif prop_block[end_pos] == '}':
                            depth -= 1
                        end_pos += 1

                    values_str = prop_block[dict_start:end_pos]

                    # Parse value mappings: "$123": "value" or "$123": None
                    value_map = {}
                    value_pattern = re.compile(r'"\$(\d+)":\s*(?:"([^"]*)"|None)')
                    for val_match in value_pattern.finditer(values_str):
                        val_symbol = int(val_match.group(1))
                        val_name = val_match.group(2)  # None if Python None
                        value_map[val_symbol] = val_name

                    if value_map:
                        value_mappings[symbol_id] = value_map

            pos = max(match.end(), end_pos)

    # Find YJ_LENGTH_UNITS block
    length_units = {}
    units_block = extract_dict_block(content, 'YJ_LENGTH_UNITS')
    if units_block:
        unit_pattern = re.compile(r'"\$(\d+)":\s*"([^"]+)"')
        for match in unit_pattern.finditer(units_block):
            symbol_id = int(match.group(1))
            unit_name = match.group(2)
            length_units[symbol_id] = unit_name

    # Find BORDER_STYLES
    border_styles = {}
    border_block = extract_dict_block(content, 'BORDER_STYLES')
    if border_block:
        border_pattern = re.compile(r'"\$(\d+)":\s*"([^"]+)"')
        for match in border_pattern.finditer(border_block):
            symbol_id = int(match.group(1))
            style_name = match.group(2)
            border_styles[symbol_id] = style_name

    # Merge border styles into value mappings
    for prop_id, prop_name in property_info.items():
        if 'border' in prop_name and 'style' in prop_name:
            if prop_id not in value_mappings:
                value_mappings[prop_id] = border_styles

    return property_info, length_units, value_mappings


def parse_rust_symbols(symbols_path: Path) -> Dict[str, int]:
    """Parse existing symbol definitions from symbols.rs."""

    with open(symbols_path, 'r') as f:
        content = f.read()

    symbols = {}
    # Pattern: pub const NAME: u64 = 123;
    pattern = re.compile(r'pub const (\w+):\s*u64\s*=\s*(\d+);')

    for match in pattern.finditer(content):
        name = match.group(1)
        value = int(match.group(2))
        symbols[name] = value

    return symbols


def css_name_to_rust_const(css_name: str) -> str:
    """Convert CSS property name to Rust constant name."""
    # Remove -kfx- and -webkit- prefixes
    name = css_name.lstrip('-')
    if name.startswith('kfx-'):
        name = name[4:]
    if name.startswith('webkit-'):
        name = name[7:]
    if name.startswith('amzn-'):
        name = name[5:]

    # Convert to UPPER_SNAKE_CASE
    name = name.replace('-', '_').upper()

    return name


def generate_symbol_report(
    property_info: Dict[int, str],
    length_units: Dict[int, str],
    value_mappings: Dict[int, Dict[int, str]],
    rust_symbols: Dict[str, int]
) -> str:
    """Generate a markdown report of symbol mappings."""

    lines = [
        "# KFX Symbol Mapping Report",
        "",
        "Generated from kfxlib's yj_to_epub_properties.py",
        "",
        "## CSS Property Symbols",
        "",
        "| Symbol | CSS Property | Rust Const |",
        "|--------|--------------|------------|",
    ]

    # Reverse lookup for Rust symbols
    rust_by_value = {v: k for k, v in rust_symbols.items()}

    for symbol_id in sorted(property_info.keys()):
        css_name = property_info[symbol_id]
        rust_name = rust_by_value.get(symbol_id, "❌ MISSING")
        lines.append(f"| ${symbol_id} | {css_name} | {rust_name} |")

    lines.extend([
        "",
        "## Length Unit Symbols",
        "",
        "| Symbol | Unit | Rust Const |",
        "|--------|------|------------|",
    ])

    for symbol_id in sorted(length_units.keys()):
        unit_name = length_units[symbol_id]
        rust_name = rust_by_value.get(symbol_id, "❌ MISSING")
        lines.append(f"| ${symbol_id} | {unit_name} | {rust_name} |")

    # Collect all value symbols
    all_values = {}
    for prop_id, values in value_mappings.items():
        for val_id, val_name in values.items():
            if val_id not in all_values:
                all_values[val_id] = []
            all_values[val_id].append((prop_id, val_name))

    lines.extend([
        "",
        "## Value Symbols (Enum Values)",
        "",
        "| Symbol | Values | Used By |",
        "|--------|--------|---------|",
    ])

    for symbol_id in sorted(all_values.keys()):
        usages = all_values[symbol_id]
        values = set(v for _, v in usages if v)
        props = set(property_info.get(p, f"${p}") for p, _ in usages)
        rust_name = rust_by_value.get(symbol_id, "❌ MISSING")
        lines.append(f"| ${symbol_id} | {', '.join(values) or 'None'} | {', '.join(props)} |")

    return '\n'.join(lines)


def find_symbol_issues(
    property_info: Dict[int, str],
    length_units: Dict[int, str],
    value_mappings: Dict[int, Dict[int, str]],
    rust_symbols: Dict[str, int]
) -> Tuple[List[Tuple[int, str]], List[Tuple[int, str, str, str]]]:
    """Find symbols missing from Rust and potentially misnamed ones."""

    rust_by_value = {v: k for k, v in rust_symbols.items()}

    missing = []
    potential_misnames = []  # (symbol_id, rust_name, expected_css, actual_purpose)

    # Check property symbols
    for symbol_id, css_name in property_info.items():
        if symbol_id not in rust_by_value:
            missing.append((symbol_id, css_name))
        else:
            # Check if the name makes sense
            rust_name = rust_by_value[symbol_id]
            expected = css_name_to_rust_const(css_name)
            # If rust name doesn't contain expected parts, might be misnamed
            css_parts = css_name.replace('-', '_').upper().split('_')
            rust_parts = rust_name.split('_')
            # Simple heuristic: if less than half the parts match, flag it
            matches = sum(1 for p in css_parts if p in rust_parts)
            if matches < len(css_parts) // 2 and css_name not in ['-kfx-attrib-xml-lang']:
                potential_misnames.append((symbol_id, rust_name, expected, css_name))

    # Check unit symbols
    for symbol_id, unit_name in length_units.items():
        if symbol_id not in rust_by_value:
            missing.append((symbol_id, f"unit: {unit_name}"))

    # Check value symbols
    all_values = set()
    for values in value_mappings.values():
        all_values.update(values.keys())

    for symbol_id in all_values:
        if symbol_id not in rust_by_value:
            # Find what it's used for
            for prop_id, values in value_mappings.items():
                if symbol_id in values:
                    val_name = values[symbol_id]
                    prop_name = property_info.get(prop_id, f"${prop_id}")
                    missing.append((symbol_id, f"{prop_name}: {val_name}"))
                    break

    return missing, potential_misnames


# Keep the old function name as an alias for compatibility
def find_missing_symbols(
    property_info: Dict[int, str],
    length_units: Dict[int, str],
    value_mappings: Dict[int, Dict[int, str]],
    rust_symbols: Dict[str, int]
) -> Tuple[List[Tuple[int, str]], List[Tuple[int, str, int]]]:
    missing, _ = find_symbol_issues(property_info, length_units, value_mappings, rust_symbols)
    return missing, []


def generate_rust_additions(missing: List[Tuple[int, str]]) -> str:
    """Generate Rust constant definitions for missing symbols."""

    lines = ["// Generated symbol additions from yj_to_epub_properties.py", ""]

    # Group by category
    by_category = defaultdict(list)
    for symbol_id, description in missing:
        if 'unit:' in description:
            by_category['units'].append((symbol_id, description))
        elif ':' in description:
            # Value symbol
            prop_name = description.split(':')[0].strip()
            by_category[f'values_{prop_name}'].append((symbol_id, description))
        else:
            by_category['properties'].append((symbol_id, description))

    for category, items in sorted(by_category.items()):
        lines.append(f"// {category.replace('_', ' ').title()}")
        for symbol_id, description in sorted(items):
            const_name = css_name_to_rust_const(description.split(':')[0] if ':' in description else description)
            lines.append(f"pub const {const_name}: u64 = {symbol_id}; // ${symbol_id} - {description}")
        lines.append("")

    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description='Sync KFX symbols from kfxlib to Rust')
    parser.add_argument('--update', action='store_true', help='Update symbols.rs with missing symbols')
    parser.add_argument('--generate-report', action='store_true', help='Generate markdown report')
    parser.add_argument('--verbose', '-v', action='store_true', help='Verbose output')
    args = parser.parse_args()

    # Paths
    project_root = Path(__file__).parent.parent
    yj_path = project_root / 'kfxinput' / 'kfxlib' / 'yj_to_epub_properties.py'
    symbols_path = project_root / 'src' / 'kfx' / 'writer' / 'symbols.rs'

    if not yj_path.exists():
        print(f"Error: Source file not found: {yj_path}")
        sys.exit(1)

    if not symbols_path.exists():
        print(f"Error: Symbols file not found: {symbols_path}")
        sys.exit(1)

    print(f"Parsing {yj_path.name}...")
    property_info, length_units, value_mappings = parse_yj_properties(yj_path)

    print(f"Parsing {symbols_path.name}...")
    rust_symbols = parse_rust_symbols(symbols_path)

    print(f"\nFound {len(property_info)} CSS property mappings")
    print(f"Found {len(length_units)} length unit mappings")
    print(f"Found {len(rust_symbols)} existing Rust symbols")

    # Find differences
    missing, potential_misnames = find_symbol_issues(
        property_info, length_units, value_mappings, rust_symbols
    )

    # Remove duplicates from missing (same symbol can be used by multiple properties)
    seen_ids = set()
    unique_missing = []
    for symbol_id, desc in missing:
        if symbol_id not in seen_ids:
            seen_ids.add(symbol_id)
            unique_missing.append((symbol_id, desc))

    if potential_misnames:
        print(f"\n⚠️  Found {len(potential_misnames)} potentially misnamed symbols:")
        for symbol_id, rust_name, expected, css_name in sorted(potential_misnames)[:15]:
            print(f"  ${symbol_id}: {rust_name} (CSS: {css_name}, expected: {expected})")
        if len(potential_misnames) > 15:
            print(f"  ... and {len(potential_misnames) - 15} more")

    if unique_missing:
        print(f"\n⚠️  Found {len(unique_missing)} missing symbols:")
        for symbol_id, desc in sorted(unique_missing)[:20]:
            print(f"  ${symbol_id}: {desc}")
        if len(unique_missing) > 20:
            print(f"  ... and {len(unique_missing) - 20} more")
    else:
        print("\n✅ All symbols are present in Rust!")

    if args.generate_report:
        report = generate_symbol_report(
            property_info, length_units, value_mappings, rust_symbols
        )
        report_path = project_root / 'docs' / 'kfx-symbol-report.md'
        report_path.parent.mkdir(exist_ok=True)
        with open(report_path, 'w') as f:
            f.write(report)
        print(f"\nReport written to {report_path}")

    if args.update and unique_missing:
        additions = generate_rust_additions(unique_missing)
        print(f"\n--- Suggested additions to symbols.rs ---\n")
        print(additions)
        print("--- End suggestions ---")

        # Could auto-insert into symbols.rs here
        # For now, just print suggestions

    # Print summary of what we have
    if args.verbose:
        print("\n=== Detailed Symbol Coverage ===")

        rust_by_value = {v: k for k, v in rust_symbols.items()}

        print("\nCSS Properties:")
        for symbol_id in sorted(property_info.keys()):
            css_name = property_info[symbol_id]
            rust_name = rust_by_value.get(symbol_id, "MISSING")
            status = "✅" if symbol_id in rust_by_value else "❌"
            print(f"  {status} ${symbol_id}: {css_name} -> {rust_name}")

        print("\nLength Units:")
        for symbol_id in sorted(length_units.keys()):
            unit_name = length_units[symbol_id]
            rust_name = rust_by_value.get(symbol_id, "MISSING")
            status = "✅" if symbol_id in rust_by_value else "❌"
            print(f"  {status} ${symbol_id}: {unit_name} -> {rust_name}")

    return 0 if not unique_missing else 1


if __name__ == '__main__':
    sys.exit(main())
