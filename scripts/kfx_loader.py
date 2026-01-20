#!/usr/bin/env python3
"""
KFX Loader - Load KFX files into a fragment store.

This module handles loading KFX files using multiple methods:
1. YJ_Book (kfxlib's high-level API)
2. YJContainer (kfxlib's container API)
3. Manual parsing (fallback that always works)
"""

import sys
from pathlib import Path
from collections import defaultdict

sys.path.insert(0, str(Path(__file__).parent))

from kfxlib import YJ_Book
from kfxlib.yj_container import YJContainer
from kfxlib.ion_binary import IonBinary
from kfxlib.ion import IonSymbol
from kfxlib.yj_symbol_catalog import YJ_SYMBOLS


class Fragment:
    """Simple fragment container."""
    def __init__(self, fid, ftype, value):
        self.fid = fid
        self.ftype = ftype
        self.value = value


class FragmentStore:
    """Helper class for accessing fragments by type."""

    def __init__(self, fragments):
        self.by_type = defaultdict(list)
        self.by_fid = {}
        self.all_fragments = fragments

        for frag in fragments:
            ftype = str(frag.ftype)
            self.by_type[ftype].append(frag)
            if frag.fid:
                self.by_fid[(ftype, str(frag.fid))] = frag

    def get(self, ftype):
        """Get first fragment of given type."""
        frags = self.by_type.get(ftype, [])
        return frags[0] if frags else None

    def get_all(self, ftype):
        """Get all fragments of given type."""
        return self.by_type.get(ftype, [])

    def types(self):
        """Get all fragment types."""
        return self.by_type.keys()

    def count(self, ftype):
        """Count fragments of given type."""
        return len(self.by_type.get(ftype, []))

    def __iter__(self):
        return iter(self.all_fragments)


def load_kfx_manual(filepath):
    """Manually parse KFX container to extract fragments."""

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
    class SymbolTableWrapper:
        ION_SYSTEM_SYMBOLS = [
            "$ion", "$ion_1_0", "$ion_symbol_table", "name", "version",
            "imports", "symbols", "max_id", "$ion_shared_symbol_table",
        ]

        def __init__(self, shared):
            self.shared = shared
            self.local_symbols = {}
            self.shared_base = 10

        def get_symbol(self, sid):
            if sid in self.local_symbols:
                return IonSymbol(self.local_symbols[sid])
            if 0 <= sid < len(self.ION_SYSTEM_SYMBOLS):
                return IonSymbol(self.ION_SYSTEM_SYMBOLS[sid])
            idx = sid - self.shared_base
            if 0 <= idx < len(self.shared.symbols):
                sym = self.shared.symbols[idx]
                return IonSymbol(f"${sid}" if sym is None else sym)
            return IonSymbol(f"${sid}")

        def add_symbol(self, sid, name):
            self.local_symbols[sid] = name

    symtab = SymbolTableWrapper(YJ_SYMBOLS)

    # Parse container info
    ion = IonBinary()
    ion.symtab = symtab
    ci_data = data[ci_offset:ci_offset+ci_len]
    ci = ion.deserialize_single_value(ci_data, 0)

    # Get index and symbol table locations
    index_offset = ci.get('$413')
    index_length = ci.get('$414')
    symtab_offset = ci.get('$415?') or ci.get('$415')
    symtab_length = ci.get('$416')

    if symtab_offset and symtab_length:
        st_data = data[symtab_offset:symtab_offset+symtab_length]
        try:
            ion2 = IonBinary()
            ion2.symtab = symtab
            local_syms = ion2.deserialize_single_value(st_data, 0)
            if isinstance(local_syms, dict) and '$7' in local_syms:
                symbols_list = local_syms.get('$7', [])
                base_sid = len(YJ_SYMBOLS.symbols)
                for i, sym_name in enumerate(symbols_list):
                    symtab.add_symbol(base_sid + i, sym_name)
        except Exception:
            pass

    if index_offset is None or index_length is None:
        raise ValueError("Could not find entity index table")

    # Parse entity index table
    entry_size = 24
    num_entries = index_length // entry_size
    fragments = []
    payload_start = header_len

    for i in range(num_entries):
        idx_pos = index_offset + i * entry_size
        eid = int.from_bytes(data[idx_pos:idx_pos+4], 'little')
        etype = int.from_bytes(data[idx_pos+4:idx_pos+8], 'little')
        eoffset = int.from_bytes(data[idx_pos+8:idx_pos+16], 'little')
        elength = int.from_bytes(data[idx_pos+16:idx_pos+24], 'little')

        entity_data = data[payload_start + eoffset : payload_start + eoffset + elength]

        if entity_data[:4] == b'ENTY':
            enty_header_len = int.from_bytes(entity_data[6:10], 'little')
            entity_data = entity_data[enty_header_len:]

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


def load_kfx(filepath):
    """Load a KFX file and return a FragmentStore."""
    fragments = None
    method = None

    # Try YJ_Book first
    try:
        book = YJ_Book(filepath)
        frags = list(book.fragments.get_all())
        if frags:
            fragments = frags
            method = "YJ_Book"
    except Exception:
        pass

    # Try YJContainer
    if fragments is None:
        try:
            container = YJContainer(filepath)
            frags = list(container.fragments.get_all())
            if frags:
                fragments = frags
                method = "YJContainer"
        except Exception:
            pass

    # Fall back to manual parsing
    if fragments is None:
        fragments = load_kfx_manual(filepath)
        method = "manual"

    return FragmentStore(fragments), method


if __name__ == "__main__":
    import sys
    if len(sys.argv) < 2:
        print("Usage: python kfx_loader.py <kfx_file>")
        sys.exit(1)

    store, method = load_kfx(sys.argv[1])
    print(f"Loaded {len(store.all_fragments)} fragments using {method}")
    print(f"\nFragment types:")
    for ftype in sorted(store.types()):
        print(f"  {ftype}: {store.count(ftype)}")
