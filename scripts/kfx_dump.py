#!/usr/bin/env python3
"""
Dump the full structure of a KFX file using kfxlib.

Usage:
    python scripts/kfx_dump.py <kfx_file> [--filter TYPE] [--compare <other_kfx>]

Examples:
    python scripts/kfx_dump.py book.kfx
    python scripts/kfx_dump.py book.kfx --filter '$157'  # Only show style fragments
    python scripts/kfx_dump.py generated.kfx --compare reference.kfx
"""

import sys
import os
import argparse
from pathlib import Path

# Add kfxlib to path (use local copy in scripts/ directory)
SCRIPT_DIR = Path(__file__).parent
KFXLIB_PATH = os.environ.get('KFXLIB_PATH', str(SCRIPT_DIR))
sys.path.insert(0, KFXLIB_PATH)

try:
    from kfxlib import YJ_Book
    from kfxlib.yj_container import YJContainer, YJFragmentList
except ImportError as e:
    print(f"Error: Could not import kfxlib from {KFXLIB_PATH}")
    print(f"Set KFXLIB_PATH environment variable to the kfxlib directory")
    print(f"Import error: {e}")
    sys.exit(1)


def format_value(val, indent=0):
    """Format a value for display with proper indentation."""
    prefix = "  " * indent

    if isinstance(val, dict):
        if not val:
            return "{}"
        lines = ["{"]
        for k, v in sorted(val.items(), key=lambda x: str(x[0])):
            formatted_v = format_value(v, indent + 1)
            lines.append(f"{prefix}  {k}: {formatted_v}")
        lines.append(f"{prefix}}}")
        return "\n".join(lines)
    elif isinstance(val, (list, tuple)):
        if not val:
            return "[]"
        if len(val) <= 3 and all(not isinstance(v, (dict, list, tuple)) for v in val):
            # Short list, show inline
            return "[" + ", ".join(format_value(v, 0) for v in val) + "]"
        lines = ["["]
        for i, v in enumerate(val):
            formatted_v = format_value(v, indent + 1)
            lines.append(f"{prefix}  [{i}]: {formatted_v}")
        lines.append(f"{prefix}]")
        return "\n".join(lines)
    elif isinstance(val, bytes):
        if len(val) <= 32:
            return f"bytes({len(val)}): {val.hex()}"
        return f"bytes({len(val)}): {val[:32].hex()}..."
    elif isinstance(val, str):
        if len(val) > 100:
            return repr(val[:100] + "...")
        return repr(val)
    else:
        return repr(val)


def load_kfx(filepath):
    """Load a KFX file and return its fragments."""
    # Try YJ_Book first
    try:
        book = YJ_Book(filepath)
        fragments = list(book.fragments.get_all())
        if fragments:
            return fragments, "YJ_Book"
    except Exception as e:
        pass

    # Try YJContainer directly
    try:
        container = YJContainer(filepath)
        fragments = list(container.fragments.get_all())
        if fragments:
            return fragments, "YJContainer"
    except Exception as e:
        pass

    # Manual parsing fallback
    return load_kfx_manual(filepath), "manual"


def load_kfx_manual(filepath):
    """Manually parse KFX container to extract fragments."""
    from kfxlib.ion_binary import IonBinary
    from kfxlib.ion import IonSymbol
    from kfxlib.yj_symbol_catalog import YJ_SYMBOLS

    with open(filepath, 'rb') as f:
        data = f.read()

    # Check for CONT header
    if data[:4] != b'CONT':
        raise ValueError(f"Not a valid KFX container (expected CONT, got {data[:4]})")

    # Parse header
    version = int.from_bytes(data[4:6], 'little')
    header_len = int.from_bytes(data[6:10], 'little')
    ci_offset = int.from_bytes(data[10:14], 'little')
    ci_len = int.from_bytes(data[14:18], 'little')

    # Create symbol table that wraps YJ_SYMBOLS
    # YJ_SYMBOLS starts at symbol ID 10 (index 0 = "$10")
    class SymbolTableWrapper:
        # Ion system symbol table (IDs 0-8)
        ION_SYSTEM_SYMBOLS = [
            "$ion",              # 0
            "$ion_1_0",          # 1
            "$ion_symbol_table", # 2
            "name",              # 3
            "version",           # 4
            "imports",           # 5
            "symbols",           # 6
            "max_id",            # 7
            "$ion_shared_symbol_table",  # 8
        ]

        def __init__(self, shared):
            self.shared = shared
            self.local_symbols = {}
            # Symbol IDs in YJ_SYMBOLS start at 10
            self.shared_base = 10

        def get_symbol(self, sid):
            # Check local symbols first
            if sid in self.local_symbols:
                return IonSymbol(self.local_symbols[sid])
            # Ion system symbols (0-8)
            if 0 <= sid < len(self.ION_SYSTEM_SYMBOLS):
                return IonSymbol(self.ION_SYSTEM_SYMBOLS[sid])
            # YJ_SYMBOLS: symbol ID 10 is at index 0, ID 11 at index 1, etc.
            idx = sid - self.shared_base
            if 0 <= idx < len(self.shared.symbols):
                sym = self.shared.symbols[idx]
                return IonSymbol(f"${sid}" if sym is None else sym)
            return IonSymbol(f"${sid}")

        def add_symbol(self, sid, name):
            self.local_symbols[sid] = name

    symtab = SymbolTableWrapper(YJ_SYMBOLS)

    # Parse container info to get index table location
    ion = IonBinary()
    ion.symtab = symtab

    ci_data = data[ci_offset:ci_offset+ci_len]
    ci = ion.deserialize_single_value(ci_data, 0)

    # Debug: print container info keys
    if isinstance(ci, dict):
        print(f"Container info keys: {list(ci.keys())}", file=sys.stderr)

    # Get index table offset and length
    # Container info keys (from debug): $409-$416, $594-$595
    # $413 = index_table_offset, $414 = index_table_length (based on typical structure)
    index_offset = ci.get('$413')
    index_length = ci.get('$414')

    # Get symbol table offset and length for local symbols
    # $415 = symtab_offset, $416 = symtab_length (typical)
    symtab_offset = ci.get('$415?') or ci.get('$415')
    symtab_length = ci.get('$416')

    if symtab_offset and symtab_length:
        # Parse local symbol table
        st_data = data[symtab_offset:symtab_offset+symtab_length]
        try:
            ion2 = IonBinary()
            ion2.symtab = symtab
            local_syms = ion2.deserialize_single_value(st_data, 0)
            if isinstance(local_syms, dict) and '$7' in local_syms:
                # $7 is 'symbols' - list of local symbol names
                symbols_list = local_syms.get('$7', [])
                # Local symbols start after shared symbols
                base_sid = len(YJ_SYMBOLS.symbols)
                for i, sym_name in enumerate(symbols_list):
                    symtab.add_symbol(base_sid + i, sym_name)
        except Exception as e:
            print(f"Warning: Could not parse local symbol table: {e}", file=sys.stderr)

    if index_offset is None or index_length is None:
        raise ValueError("Could not find entity index table in container info")

    # Parse entity index table
    entry_size = 24  # id(4) + type(4) + offset(8) + length(8)
    num_entries = index_length // entry_size

    class Fragment:
        def __init__(self, fid, ftype, value):
            self.fid = fid
            self.ftype = ftype
            self.value = value

    fragments = []
    payload_start = header_len

    for i in range(num_entries):
        idx_pos = index_offset + i * entry_size
        eid = int.from_bytes(data[idx_pos:idx_pos+4], 'little')
        etype = int.from_bytes(data[idx_pos+4:idx_pos+8], 'little')
        eoffset = int.from_bytes(data[idx_pos+8:idx_pos+16], 'little')
        elength = int.from_bytes(data[idx_pos+16:idx_pos+24], 'little')

        # Read entity payload
        entity_data = data[payload_start + eoffset : payload_start + eoffset + elength]

        # Skip ENTY header if present
        # Structure: magic(4) + version(2) + header_len(4) + header_ion + content_ion
        # header_len = 10 + len(header_ion), content_ion starts after header_len bytes
        if entity_data[:4] == b'ENTY':
            header_len = int.from_bytes(entity_data[6:10], 'little')
            # Content Ion starts after header_len bytes from the start of ENTY
            entity_data = entity_data[header_len:]

        try:
            ion = IonBinary()
            ion.symtab = symtab
            value = ion.deserialize_single_value(entity_data, 0)

            ftype = f"${etype}"
            fid = symtab.get_symbol(eid) if eid < 10000 else f"${eid}"

            fragments.append(Fragment(fid, ftype, value))
        except Exception as e:
            fragments.append(Fragment(f"${eid}", f"${etype}", f"<parse error: {e}>"))

    return fragments


def dump_fragments(fragments, filter_type=None, index=None, frag_id=None, text_search=None):
    """Print all fragments."""
    # Filter fragments first
    filtered = []
    for frag in fragments:
        ftype = frag.ftype
        fid = frag.fid

        if filter_type and ftype != filter_type:
            continue
        if frag_id and fid != frag_id:
            continue
        if text_search and not search_value_for_text(frag.value, text_search):
            continue

        filtered.append(frag)

    # Show count info
    if index is not None or frag_id is not None or text_search is not None:
        print(f"Matched {len(filtered)} fragments")

    # If index specified, show only that one
    if index is not None:
        if index < 0 or index >= len(filtered):
            print(f"Error: Index {index} out of range (0-{len(filtered)-1})")
            return
        filtered = [filtered[index]]

    for i, frag in enumerate(filtered):
        print(f"\n{'='*60}")
        if index is None and frag_id is None:
            print(f"[{i}] Fragment: {frag.fid}")
        else:
            print(f"Fragment: {frag.fid}")
        print(f"Type: {frag.ftype}")
        print(f"Value:")
        print(format_value(frag.value, indent=1))


def compare_fragments(frags1, frags2, filter_type=None):
    """Compare fragments from two KFX files."""
    # Build lookup by (ftype, fid)
    def build_lookup(frags):
        lookup = {}
        for f in frags:
            key = (f.ftype, f.fid)
            lookup[key] = f.value
        return lookup

    lookup1 = build_lookup(frags1)
    lookup2 = build_lookup(frags2)

    all_keys = set(lookup1.keys()) | set(lookup2.keys())

    only_in_1 = []
    only_in_2 = []
    different = []
    same = []

    for key in sorted(all_keys):
        ftype, fid = key
        if filter_type and ftype != filter_type:
            continue

        in_1 = key in lookup1
        in_2 = key in lookup2

        if in_1 and not in_2:
            only_in_1.append(key)
        elif in_2 and not in_1:
            only_in_2.append(key)
        elif lookup1[key] != lookup2[key]:
            different.append(key)
        else:
            same.append(key)

    print(f"\n{'='*60}")
    print(f"COMPARISON SUMMARY")
    print(f"{'='*60}")
    print(f"Only in file 1: {len(only_in_1)}")
    print(f"Only in file 2: {len(only_in_2)}")
    print(f"Different: {len(different)}")
    print(f"Same: {len(same)}")

    if only_in_1:
        print(f"\n--- Only in file 1 ---")
        for ftype, fid in only_in_1:
            print(f"  {ftype} / {fid}")

    if only_in_2:
        print(f"\n--- Only in file 2 ---")
        for ftype, fid in only_in_2:
            print(f"  {ftype} / {fid}")

    if different:
        print(f"\n--- Different ---")
        for ftype, fid in different:
            print(f"\n{ftype} / {fid}:")
            print(f"  File 1:")
            print(format_value(lookup1[(ftype, fid)], indent=2))
            print(f"  File 2:")
            print(format_value(lookup2[(ftype, fid)], indent=2))


def search_value_for_text(val, text):
    """Recursively search a value for text content."""
    if isinstance(val, str):
        return text.lower() in val.lower()
    elif isinstance(val, dict):
        return any(search_value_for_text(v, text) for v in val.values())
    elif isinstance(val, (list, tuple)):
        return any(search_value_for_text(v, text) for v in val)
    return False


def main():
    parser = argparse.ArgumentParser(description="Dump KFX file structure using kfxlib")
    parser.add_argument("kfx_file", help="KFX file to analyze")
    parser.add_argument("--filter", "-f", help="Filter by fragment type (e.g., '$157' for styles)")
    parser.add_argument("--index", "-i", type=int, help="Show only the fragment at this index (0-based, within filtered results)")
    parser.add_argument("--id", help="Show only the fragment with this ID (e.g., '$907')")
    parser.add_argument("--text", "-t", help="Search for fragments containing this text (case-insensitive)")
    parser.add_argument("--compare", "-c", help="Compare with another KFX file")
    parser.add_argument("--summary", "-s", action="store_true", help="Only show summary, not full content")
    parser.add_argument("--order", "-o", type=int, default=0, help="Show first N fragments in file order (e.g., --order 30)")
    args = parser.parse_args()

    if not Path(args.kfx_file).exists():
        print(f"Error: File not found: {args.kfx_file}")
        sys.exit(1)

    print(f"Loading {args.kfx_file}...")
    frags1, method1 = load_kfx(args.kfx_file)
    print(f"Loaded {len(frags1)} fragments using {method1}")

    # Count by type
    types = {}
    for f in frags1:
        types[f.ftype] = types.get(f.ftype, 0) + 1

    print(f"\nFragment types:")
    for t, c in sorted(types.items()):
        print(f"  {t}: {c}")

    if args.order > 0:
        print(f"\nFragment order (first {args.order}):")
        for i, frag in enumerate(frags1[:args.order]):
            print(f"  {i:3}: {frag.ftype} ({frag.fid})")

    if args.compare:
        if not Path(args.compare).exists():
            print(f"Error: Comparison file not found: {args.compare}")
            sys.exit(1)

        print(f"\nLoading {args.compare}...")
        frags2, method2 = load_kfx(args.compare)
        print(f"Loaded {len(frags2)} fragments using {method2}")

        compare_fragments(frags1, frags2, filter_type=args.filter)
    elif not args.summary:
        dump_fragments(frags1, filter_type=args.filter, index=args.index, frag_id=args.id, text_search=args.text)


if __name__ == "__main__":
    main()
