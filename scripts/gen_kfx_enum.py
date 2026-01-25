#!/usr/bin/env python3
"""
Generate the KfxSymbol enum from KFX_SYMBOL_TABLE in symbols.rs.

Usage:
    python scripts/gen_kfx_enum.py
"""

import re

def to_camel_case(s: str) -> str:
    """Convert snake_case/kebab-case to CamelCase."""
    # Handle $ prefix
    s = s.lstrip('$')

    # Handle operators
    ops = {'==': 'Eq', '!=': 'Neq', '>': 'Gt', '>=': 'Gte', '<': 'Lt', '<=': 'Lte',
           '+': 'Plus', '-': 'Minus', '*': 'Mul', '/': 'Div'}
    if s in ops:
        return ops[s]

    # Split on underscores, dots, hyphens
    parts = re.split(r'[_.\-]', s)
    return ''.join(p.capitalize() for p in parts if p)

def main():
    # Read the symbols from the source file
    with open('src/kfx/symbols.rs', 'r') as f:
        content = f.read()

    # Extract the array entries
    match = re.search(r'pub static KFX_SYMBOL_TABLE: \[&str; \d+\] = \[(.*?)\];', content, re.DOTALL)
    if not match:
        print("Could not find KFX_SYMBOL_TABLE")
        return

    array_content = match.group(1)

    # Parse each entry
    entries = re.findall(r'"([^"]*)"', array_content)

    print("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]")
    print("#[repr(u16)]")
    print("#[allow(non_camel_case_types)]")
    print("pub enum KfxSymbol {")

    seen = {}
    for i, sym in enumerate(entries):
        name = to_camel_case(sym)

        # Handle duplicates
        if name in seen:
            seen[name] += 1
            name = f"{name}{seen[name]}"
        else:
            seen[name] = 1

        print(f"    {name} = {i},")

    print("}")

if __name__ == "__main__":
    main()
