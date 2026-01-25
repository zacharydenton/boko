use boko::kfx::symbols::KFX_SYMBOL_TABLE;
use ion_rs::{AnyEncoding, Decoder, ElementReader, IonResult, MapCatalog, Reader, SharedSymbolTable};
use std::{env, fs};

/// Ion 1.0 Binary Version Marker
const ION_BVM: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];

/// Build an Ion binary preamble that imports our KFX symbol table.
/// This allows parsing Ion data that uses KFX symbols without an embedded import.
fn build_symbol_table_preamble() -> Vec<u8> {
    build_symbol_table_preamble_with_max_id(848)
}

/// Build an Ion binary preamble with a custom max_id for extended symbols.
fn build_symbol_table_preamble_with_max_id(max_id: i64) -> Vec<u8> {
    use ion_rs::{ion_list, ion_struct, IntoAnnotatedElement, Element, Writer, WriteConfig, ElementWriter};
    use ion_rs::v1_0::Binary;

    // Build: $ion_symbol_table::{ imports: [{ name: "YJ_symbols", version: 10, max_id: N }] }
    // Amazon's KFX symbol table is named "YJ_symbols" (Yellow Jersey)
    let import = ion_struct! {
        "name": "YJ_symbols",
        "version": 10i64,
        "max_id": max_id,
    };

    let symbol_table: Element = ion_struct! {
        "imports": ion_list![import],
    }.with_annotations(["$ion_symbol_table"]);

    let buffer = Vec::new();
    let mut writer = Writer::new(WriteConfig::<Binary>::new(), buffer).unwrap();
    writer.write_element(&symbol_table).unwrap();
    writer.close().unwrap()
}

fn main() -> IonResult<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: kfx-dump <file>");
        eprintln!();
        eprintln!("Dumps KFX/KDF/Ion files. Supports:");
        eprintln!("  - KFX container files (.kfx) - CONT format");
        eprintln!("  - Raw Ion binary files (.kdf, .ion)");
        std::process::exit(1);
    }

    let path = &args[1];
    let data = fs::read(path).expect("Failed to read file");

    // Check for KFX container format (starts with "CONT")
    if data.len() >= 4 && &data[0..4] == b"CONT" {
        eprintln!("Detected KFX container format");
        dump_kfx_container(&data)?;
    } else if data.len() >= 4 && data[0..4] == ION_BVM {
        eprintln!("Detected raw Ion binary format");
        dump_ion_data(&data)?;
    } else {
        eprintln!("Unknown file format. First 16 bytes:");
        for byte in data.iter().take(16) {
            eprint!("{:02X} ", byte);
        }
        eprintln!();
        std::process::exit(1);
    }

    Ok(())
}

/// Read a little-endian u16 from a slice
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Read a little-endian u32 from a slice
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Read a little-endian u64 from a slice
fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

/// Parse and dump a KFX container file
fn dump_kfx_container(data: &[u8]) -> IonResult<()> {
    if data.len() < 18 {
        eprintln!("Container too short: {} bytes", data.len());
        return Ok(());
    }

    // Parse container header
    let version = read_u16_le(data, 4);
    let header_len = read_u32_le(data, 6) as usize;
    let container_info_offset = read_u32_le(data, 10) as usize;
    let container_info_length = read_u32_le(data, 14) as usize;

    eprintln!("Container version: {}", version);
    eprintln!("Header length: {}", header_len);
    eprintln!("Container info: offset={}, length={}", container_info_offset, container_info_length);
    eprintln!();

    // Extended symbols from doc symbol table
    let mut extended_symbols: Vec<String> = Vec::new();

    // Parse container info (Ion struct)
    if container_info_offset + container_info_length <= data.len() {
        let container_info_data = &data[container_info_offset..container_info_offset + container_info_length];
        eprintln!("=== Container Info ===");
        if let Err(e) = dump_ion_data(container_info_data) {
            eprintln!("Error parsing container info: {}", e);
        }
        eprintln!();

        // Extract doc symbols and add to extended symbol table
        if let Some((doc_sym_offset, doc_sym_length)) = parse_container_info_for_doc_symbols(container_info_data) {
            eprintln!("Document symbols: offset={}, length={}", doc_sym_offset, doc_sym_length);
            if doc_sym_offset + doc_sym_length <= data.len() {
                let doc_sym_data = &data[doc_sym_offset..doc_sym_offset + doc_sym_length];
                extended_symbols = extract_doc_symbols(doc_sym_data);
                eprintln!("Extracted {} document-specific symbols", extended_symbols.len());
                eprintln!();
            }
        }

        // Try to extract index table info from container info
        let index_info = parse_container_info_for_index(container_info_data);

        if let Some((index_offset, index_length)) = index_info {
            eprintln!("Index table: offset={}, length={}", index_offset, index_length);

            // Parse index table - each entry is 24 bytes
            let entry_size = 24;
            let num_entries = index_length / entry_size;
            eprintln!("Number of entities: {}", num_entries);
            eprintln!();

            for i in 0..num_entries {
                let entry_offset = index_offset + i * entry_size;
                if entry_offset + entry_size > data.len() {
                    break;
                }

                let id_idnum = read_u32_le(data, entry_offset);
                let type_idnum = read_u32_le(data, entry_offset + 4);
                let entity_offset = read_u64_le(data, entry_offset + 8) as usize;
                let entity_len = read_u64_le(data, entry_offset + 16) as usize;

                // Get symbol names if available
                let id_name = if (id_idnum as usize) < KFX_SYMBOL_TABLE.len() {
                    KFX_SYMBOL_TABLE[id_idnum as usize]
                } else {
                    "?"
                };
                let type_name = if (type_idnum as usize) < KFX_SYMBOL_TABLE.len() {
                    KFX_SYMBOL_TABLE[type_idnum as usize]
                } else {
                    "?"
                };

                eprintln!("=== Entity {} ===", i);
                eprintln!("  ID: ${} ({})", id_idnum, id_name);
                eprintln!("  Type: ${} ({})", type_idnum, type_name);
                eprintln!("  Offset: {} (absolute: {})", entity_offset, header_len + entity_offset);
                eprintln!("  Length: {}", entity_len);

                // Parse the entity
                let abs_offset = header_len + entity_offset;
                if abs_offset + entity_len <= data.len() {
                    let entity_data = &data[abs_offset..abs_offset + entity_len];
                    dump_entity(entity_data, &extended_symbols)?;
                }
                eprintln!();
            }
        }
    }

    Ok(())
}

/// Parse container info Ion struct to extract index table offset/length
fn parse_container_info_for_index(data: &[u8]) -> Option<(usize, usize)> {
    // Skip first 10 entries - see dump_ion_data_extended for explanation
    let mut catalog = MapCatalog::new();
    if let Ok(table) = SharedSymbolTable::new("YJ_symbols", 10, KFX_SYMBOL_TABLE[10..].iter().copied()) {
        catalog.insert_table(table);
    }

    // Prepend symbol table import
    let preamble = build_symbol_table_preamble();
    let mut full_data = preamble;
    if data.len() >= 4 && data[0..4] == ION_BVM {
        full_data.extend_from_slice(&data[4..]);
    } else {
        full_data.extend_from_slice(data);
    }

    let reader = Reader::new(AnyEncoding.with_catalog(catalog), &full_data[..]);
    if reader.is_err() {
        return None;
    }
    let mut reader = reader.unwrap();

    // Read the struct and extract key fields
    // $412 = bcIndexTabOffset, $413 = bcIndexTabLength
    let mut index_offset: Option<usize> = None;
    let mut index_length: Option<usize> = None;

    for element in reader.elements() {
        if let Ok(elem) = element
            && let Some(strukt) = elem.as_struct() {
                for field in strukt.iter() {
                    let (name, value) = field;
                    if let Some(field_name) = name.text() {
                        if field_name == "bcIndexTabOffset"
                            && let Some(i) = value.as_i64() {
                                index_offset = Some(i as usize);
                            }
                        if field_name == "bcIndexTabLength"
                            && let Some(i) = value.as_i64() {
                                index_length = Some(i as usize);
                            }
                    }
                }
            }
    }

    match (index_offset, index_length) {
        (Some(off), Some(len)) => Some((off, len)),
        _ => None,
    }
}

/// Parse container info to extract doc symbol table location
fn parse_container_info_for_doc_symbols(data: &[u8]) -> Option<(usize, usize)> {
    // Skip first 10 entries - see dump_ion_data_extended for explanation
    let mut catalog = MapCatalog::new();
    if let Ok(table) = SharedSymbolTable::new("YJ_symbols", 10, KFX_SYMBOL_TABLE[10..].iter().copied()) {
        catalog.insert_table(table);
    }

    let preamble = build_symbol_table_preamble();
    let mut full_data = preamble;
    if data.len() >= 4 && data[0..4] == ION_BVM {
        full_data.extend_from_slice(&data[4..]);
    } else {
        full_data.extend_from_slice(data);
    }

    let reader = Reader::new(AnyEncoding.with_catalog(catalog), &full_data[..]);
    if reader.is_err() {
        return None;
    }
    let mut reader = reader.unwrap();

    let mut doc_sym_offset: Option<usize> = None;
    let mut doc_sym_length: Option<usize> = None;

    for element in reader.elements() {
        if let Ok(elem) = element
            && let Some(strukt) = elem.as_struct() {
                for field in strukt.iter() {
                    let (name, value) = field;
                    if let Some(field_name) = name.text() {
                        if field_name == "bcDocSymbolOffset"
                            && let Some(i) = value.as_i64() {
                                doc_sym_offset = Some(i as usize);
                            }
                        if field_name == "bcDocSymbolLength"
                            && let Some(i) = value.as_i64() {
                                doc_sym_length = Some(i as usize);
                            }
                    }
                }
            }
    }

    match (doc_sym_offset, doc_sym_length) {
        (Some(off), Some(len)) if len > 0 => Some((off, len)),
        _ => None,
    }
}

/// Extract document-specific symbols from the doc symbols section
/// The doc symbols section is Ion binary containing a symbol table declaration.
/// We extract the "symbols" list which contains the new document-specific symbol names.
fn extract_doc_symbols(data: &[u8]) -> Vec<String> {

    // Simple approach: search for string sequences in the binary data
    // The symbols are stored as Ion strings in the "symbols" list
    let mut symbols = Vec::new();

    // Skip the Ion BVM if present
    let start = if data.len() >= 4 && data[0..4] == ION_BVM { 4 } else { 0 };

    // Look for strings in the data - Ion strings are prefixed with their type/length
    let mut i = start;
    while i < data.len() {
        let type_byte = data[i];
        let type_code = (type_byte >> 4) & 0x0F;

        // Type 8 = string
        if type_code == 8 {
            let len_nibble = type_byte & 0x0F;
            let (str_len, header_len) = if len_nibble == 14 {
                // VarUInt length follows
                if i + 1 < data.len() {
                    let len = data[i + 1] as usize;
                    if len & 0x80 == 0 {
                        // Single byte VarUInt
                        (len, 2)
                    } else {
                        // Multi-byte VarUInt - simplified handling
                        ((len & 0x7F), 2)
                    }
                } else {
                    break;
                }
            } else if len_nibble == 15 {
                // null.string
                i += 1;
                continue;
            } else {
                (len_nibble as usize, 1)
            };

            if i + header_len + str_len <= data.len() {
                let str_bytes = &data[i + header_len..i + header_len + str_len];
                if let Ok(s) = std::str::from_utf8(str_bytes) {
                    // Filter out non-symbol strings (imports, version info, etc.)
                    if !s.starts_with("YJ_symbols") && !s.is_empty() && !s.contains("version") {
                        symbols.push(s.to_string());
                    }
                }
                i += header_len + str_len;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    symbols.retain(|s| seen.insert(s.clone()));

    symbols
}

/// Parse and dump an entity (ENTY format)
fn dump_entity(data: &[u8], extended_symbols: &[String]) -> IonResult<()> {
    if data.len() < 10 {
        eprintln!("  Entity too short");
        return Ok(());
    }

    // Check for ENTY signature
    if &data[0..4] != b"ENTY" {
        eprintln!("  Not an ENTY (found: {:?})", &data[0..4]);
        // Maybe it's raw Ion?
        if data[0..4] == ION_BVM {
            eprintln!("  Raw Ion data:");
            return dump_ion_data_extended(data, extended_symbols);
        }
        return Ok(());
    }

    let version = read_u16_le(data, 4);
    let entity_header_len = read_u32_le(data, 6) as usize;

    eprintln!("  ENTY version: {}", version);
    eprintln!("  ENTY header length: {}", entity_header_len);

    // The entity info (compression, drm) is between offset 10 and entity_header_len
    // The actual Ion data starts at entity_header_len

    if entity_header_len < data.len() {
        let ion_data = &data[entity_header_len..];
        eprintln!("  Ion data ({} bytes):", ion_data.len());
        dump_ion_data_extended(ion_data, extended_symbols)?;
    }

    Ok(())
}

/// Dump Ion data using the KFX symbol table
fn dump_ion_data(data: &[u8]) -> IonResult<()> {
    dump_ion_data_extended(data, &[])
}

/// Dump Ion data using the KFX symbol table plus extended document symbols
fn dump_ion_data_extended(data: &[u8], extended_symbols: &[String]) -> IonResult<()> {
    // Build combined symbol table: base KFX symbols + extended doc symbols
    //
    // Our KFX_SYMBOL_TABLE includes Ion system symbols at indices 0-9 which Amazon's
    // YJ_symbols doesn't have. So our table[413] = Amazon's YJ[403] = "bcIndexTabLength".
    //
    // Ion-rs maps imported symbols: SST[N] → SID $(10+N).
    // For SID $413 to resolve to "bcIndexTabLength" (our table[413]):
    //   SID $413 → SST[403] → we need SST[403] = table[413]
    // So we skip our first 10 entries: SST[N] = table[N+10].
    let mut all_symbols: Vec<&str> = KFX_SYMBOL_TABLE[10..].to_vec();
    for sym in extended_symbols {
        all_symbols.push(sym.as_str());
    }

    // max_id for import: we have 839 base symbols mapping to $10-$848, plus extended
    let max_id = (848 + extended_symbols.len()) as i64;

    // Create catalog with extended symbol table
    let mut catalog = MapCatalog::new();
    catalog.insert_table(SharedSymbolTable::new(
        "YJ_symbols",
        10,
        all_symbols.iter().copied(),
    )?);

    // Build preamble with extended max_id
    let preamble = build_symbol_table_preamble_with_max_id(max_id);
    let mut full_data = preamble;

    if data.len() >= 4 && data[0..4] == ION_BVM {
        full_data.extend_from_slice(&data[4..]);
    } else {
        full_data.extend_from_slice(data);
    }

    // Create reader with catalog
    let mut reader = Reader::new(AnyEncoding.with_catalog(catalog), &full_data[..])?;

    // Read and print each element as Ion text
    let mut count = 0;
    for element in reader.elements() {
        match element {
            Ok(elem) => {
                // Convert to Ion text format
                let text = element_to_ion_text(&elem, &all_symbols);
                println!("{}", text);
                count += 1;
            }
            Err(e) => {
                if count == 0 {
                    eprintln!("  Error reading first element: {}", e);
                }
                break;
            }
        }
    }

    if count > 0 {
        eprintln!("  ({} top-level elements)", count);
    }
    Ok(())
}

/// Convert an Element to Ion text format, using $NNN for unknown symbols
fn element_to_ion_text(elem: &ion_rs::Element, _symbols: &[&str]) -> String {
    element_to_ion_text_indented(elem, _symbols, 0)
}

fn element_to_ion_text_indented(elem: &ion_rs::Element, _symbols: &[&str], indent: usize) -> String {
    use ion_rs::IonType;

    let indent_str = "  ".repeat(indent);
    let mut result = String::new();

    // Handle annotations
    let annotations: Vec<_> = elem.annotations().into_iter().collect();
    for ann in &annotations {
        if let Some(text) = ann.text() {
            result.push_str(text);
        } else {
            result.push_str("$0");  // Ion format for symbol with unknown text
        }
        result.push_str("::");
    }

    match elem.ion_type() {
        IonType::Null => result.push_str("null"),
        IonType::Bool => {
            if let Some(b) = elem.as_bool() {
                result.push_str(if b { "true" } else { "false" });
            } else {
                result.push_str("null.bool");
            }
        }
        IonType::Int => {
            if let Some(i) = elem.as_i64() {
                result.push_str(&i.to_string());
            } else if let Some(i) = elem.as_int() {
                result.push_str(&format!("{}", i));
            } else {
                result.push_str("null.int");
            }
        }
        IonType::Float => {
            if let Some(f) = elem.as_float() {
                result.push_str(&format!("{}", f));
            } else {
                result.push_str("null.float");
            }
        }
        IonType::Decimal => {
            if let Some(d) = elem.as_decimal() {
                result.push_str(&format!("{}", d));
            } else {
                result.push_str("null.decimal");
            }
        }
        IonType::Timestamp => {
            if let Some(t) = elem.as_timestamp() {
                result.push_str(&format!("{}", t));
            } else {
                result.push_str("null.timestamp");
            }
        }
        IonType::Symbol => {
            if let Some(sym) = elem.as_symbol() {
                if let Some(text) = sym.text() {
                    if text.chars().all(|c| c.is_alphanumeric() || c == '_') && !text.is_empty() {
                        result.push_str(text);
                    } else {
                        result.push('\'');
                        result.push_str(&text.replace('\'', "\\'"));
                        result.push('\'');
                    }
                } else {
                    result.push_str("$0");  // Ion format for symbol with unknown text
                }
            } else {
                result.push_str("null.symbol");
            }
        }
        IonType::String => {
            if let Some(s) = elem.as_string() {
                result.push('"');
                result.push_str(&s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"));
                result.push('"');
            } else {
                result.push_str("null.string");
            }
        }
        IonType::Clob => {
            result.push_str("{{/* clob */}}");
        }
        IonType::Blob => {
            if let Some(blob) = elem.as_blob() {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(blob);
                if b64.len() > 60 {
                    result.push_str(&format!("{{{{{}...}}}}", &b64[..60]));
                } else {
                    result.push_str(&format!("{{{{{}}}}}", b64));
                }
            } else {
                result.push_str("null.blob");
            }
        }
        IonType::List => {
            if let Some(list) = elem.as_list() {
                let items: Vec<_> = list.iter().collect();
                if items.is_empty() {
                    result.push_str("[]");
                } else if items.len() == 1 && !matches!(items[0].ion_type(), IonType::Struct | IonType::List) {
                    result.push('[');
                    result.push_str(&element_to_ion_text_indented(items[0], _symbols, 0));
                    result.push(']');
                } else {
                    result.push_str("[\n");
                    let inner_indent = "  ".repeat(indent + 1);
                    for (i, item) in items.iter().enumerate() {
                        result.push_str(&inner_indent);
                        result.push_str(&element_to_ion_text_indented(item, _symbols, indent + 1));
                        if i < items.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&indent_str);
                    result.push(']');
                }
            } else {
                result.push_str("null.list");
            }
        }
        IonType::SExp => {
            if let Some(sexp) = elem.as_sexp() {
                result.push('(');
                let items: Vec<_> = sexp.iter().map(|e| element_to_ion_text_indented(e, _symbols, 0)).collect();
                result.push_str(&items.join(" "));
                result.push(')');
            } else {
                result.push_str("null.sexp");
            }
        }
        IonType::Struct => {
            if let Some(strukt) = elem.as_struct() {
                let fields: Vec<_> = strukt.iter().collect();
                if fields.is_empty() {
                    result.push_str("{}");
                } else {
                    result.push_str("{\n");
                    let inner_indent = "  ".repeat(indent + 1);
                    for (i, (name, value)) in fields.iter().enumerate() {
                        let field_name = if let Some(text) = name.text() {
                            if text.chars().all(|c| c.is_alphanumeric() || c == '_') && !text.is_empty() {
                                text.to_string()
                            } else {
                                format!("'{}'", text.replace('\'', "\\'"))
                            }
                        } else {
                            "$0".to_string()  // Ion format for symbol with unknown text
                        };
                        result.push_str(&inner_indent);
                        result.push_str(&field_name);
                        result.push_str(": ");
                        result.push_str(&element_to_ion_text_indented(value, _symbols, indent + 1));
                        if i < fields.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&indent_str);
                    result.push('}');
                }
            } else {
                result.push_str("null.struct");
            }
        }
    }

    result
}

