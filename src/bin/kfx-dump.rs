use boko::kfx::symbols::KFX_SYMBOL_TABLE;
use clap::Parser;
use ion_rs::{AnyEncoding, Decoder, ElementReader, IonResult, MapCatalog, Reader, SharedSymbolTable};
use std::collections::HashMap;
use std::fs;

/// Ion 1.0 Binary Version Marker
const ION_BVM: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];

/// Dump KFX/KDF/Ion files for debugging
#[derive(Parser, Debug)]
#[command(name = "kfx-dump")]
#[command(about = "Dumps KFX/KDF/Ion files. Supports KFX container files (.kfx) and raw Ion binary files (.kdf, .ion)")]
struct Args {
    /// KFX file to dump
    file: String,

    /// Resolve entity ID references to show names
    #[arg(short, long)]
    resolve: bool,

    /// Show statistics (entity counts by type)
    #[arg(short, long)]
    stat: bool,
}

/// Resolved entity information for better output
#[derive(Debug, Clone)]
struct EntityInfo {
    entity_type: String,
    name: Option<String>,
}

/// Build an Ion binary preamble that imports our KFX symbol table.
/// This allows parsing Ion data that uses KFX symbols without an embedded import.
fn build_symbol_table_preamble() -> Vec<u8> {
    use boko::kfx::symbols::KFX_MAX_SYMBOL_ID;
    build_symbol_table_preamble_with_max_id(KFX_MAX_SYMBOL_ID as i64)
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
    let args = Args::parse();

    let data = fs::read(&args.file).expect("Failed to read file");

    // Check for KFX container format (starts with "CONT")
    if data.len() >= 4 && &data[0..4] == b"CONT" {
        if args.stat {
            dump_kfx_stats(&data)?;
        } else {
            eprintln!("Detected KFX container format");
            dump_kfx_container(&data, args.resolve)?;
        }
    } else if data.len() >= 4 && data[0..4] == ION_BVM {
        if args.stat {
            eprintln!("Stats not supported for raw Ion files");
            std::process::exit(1);
        }
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

/// Read a little-endian u16 from a slice with bounds checking
fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

/// Read a little-endian u32 from a slice with bounds checking
fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

/// Read a little-endian u64 from a slice with bounds checking
fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    data.get(offset..offset + 8)?
        .try_into()
        .ok()
        .map(u64::from_le_bytes)
}

/// Parse and dump a KFX container file
fn dump_kfx_container(data: &[u8], resolve: bool) -> IonResult<()> {
    if data.len() < 18 {
        eprintln!("Container too short: {} bytes", data.len());
        return Ok(());
    }

    // Parse container header with bounds checking
    let Some(version) = read_u16_le(data, 4) else {
        eprintln!("Failed to read container version");
        return Ok(());
    };
    let Some(header_len) = read_u32_le(data, 6).map(|v| v as usize) else {
        eprintln!("Failed to read header length");
        return Ok(());
    };
    let Some(container_info_offset) = read_u32_le(data, 10).map(|v| v as usize) else {
        eprintln!("Failed to read container info offset");
        return Ok(());
    };
    let Some(container_info_length) = read_u32_le(data, 14).map(|v| v as usize) else {
        eprintln!("Failed to read container info length");
        return Ok(());
    };

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

            // First pass: build resolution maps if resolving
            let maps = if resolve {
                build_maps(data, header_len, index_offset, num_entries, &extended_symbols)
            } else {
                ResolutionMaps {
                    entity_map: HashMap::new(),
                    fragment_map: HashMap::new(),
                }
            };

            // Second pass: dump entities
            for i in 0..num_entries {
                let entry_offset = index_offset + i * entry_size;
                if entry_offset + entry_size > data.len() {
                    break;
                }

                // Read entity index entry with bounds checking
                let Some(id_idnum) = read_u32_le(data, entry_offset) else { continue };
                let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else { continue };
                let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else { continue };
                let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else { continue };

                // Get symbol names if available
                // Note: These are raw symbol IDs from the container. The display
                // is for debugging purposes only.
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
                // Show resolved info if available
                if let Some(info) = maps.entity_map.get(&(id_idnum as u64)) {
                    if let Some(name) = &info.name {
                        eprintln!("  ID: ${} ({}) [{}:{}]", id_idnum, id_name, info.entity_type, name);
                    } else {
                        eprintln!("  ID: ${} ({}) [{}]", id_idnum, id_name, info.entity_type);
                    }
                } else {
                    eprintln!("  ID: ${} ({})", id_idnum, id_name);
                }
                eprintln!("  Type: ${} ({})", type_idnum, type_name);
                eprintln!("  Offset: {} (absolute: {})", entity_offset, header_len + entity_offset);
                eprintln!("  Length: {}", entity_len);

                // Parse the entity
                let abs_offset = header_len + entity_offset;
                if abs_offset + entity_len <= data.len() {
                    let entity_data = &data[abs_offset..abs_offset + entity_len];
                    dump_entity(entity_data, &extended_symbols, &maps, resolve)?;
                }
                eprintln!();
            }
        }
    }

    Ok(())
}

/// Dump statistics for a KFX container (entity counts by type)
fn dump_kfx_stats(data: &[u8]) -> IonResult<()> {
    if data.len() < 18 {
        eprintln!("Container too short: {} bytes", data.len());
        return Ok(());
    }

    // Parse container header
    let Some(header_len) = read_u32_le(data, 6).map(|v| v as usize) else {
        eprintln!("Failed to read header length");
        return Ok(());
    };
    let Some(container_info_offset) = read_u32_le(data, 10).map(|v| v as usize) else {
        eprintln!("Failed to read container info offset");
        return Ok(());
    };
    let Some(container_info_length) = read_u32_le(data, 14).map(|v| v as usize) else {
        eprintln!("Failed to read container info length");
        return Ok(());
    };

    // Parse container info to get index table location
    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data = &data[container_info_offset..container_info_offset + container_info_length];
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data) else {
        eprintln!("Could not find index table in container info");
        return Ok(());
    };

    // Count entities by type and track singleton entity data for detailed stats
    let entry_size = 24;
    let num_entries = index_length / entry_size;
    let mut type_counts: HashMap<String, usize> = HashMap::new();
    let mut total_size_by_type: HashMap<String, usize> = HashMap::new();
    // For singletons: type_name -> (abs_offset, entity_len)
    let mut singleton_data: HashMap<String, (usize, usize)> = HashMap::new();

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else { continue };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else { continue };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else { continue };

        // Get type name
        let type_name = if (type_idnum as usize) < KFX_SYMBOL_TABLE.len() {
            KFX_SYMBOL_TABLE[type_idnum as usize].to_string()
        } else {
            format!("${}", type_idnum)
        };

        let count = type_counts.entry(type_name.clone()).or_insert(0);
        *count += 1;
        *total_size_by_type.entry(type_name.clone()).or_insert(0) += entity_len;

        // Track singleton entity locations for detailed parsing
        if *count == 1 {
            singleton_data.insert(type_name, (header_len + entity_offset, entity_len));
        } else {
            // No longer a singleton
            singleton_data.remove(&type_name.clone());
        }
    }

    // Parse singleton entities to extract list counts
    let mut singleton_details: HashMap<String, String> = HashMap::new();
    for (type_name, (abs_offset, entity_len)) in &singleton_data {
        if abs_offset + entity_len <= data.len() {
            let entity_data = &data[*abs_offset..*abs_offset + *entity_len];
            if let Some(detail) = extract_singleton_details(entity_data, type_name) {
                singleton_details.insert(type_name.clone(), detail);
            }
        }
    }

    // Sort by count (descending), then by name
    let mut sorted: Vec<_> = type_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

    // Calculate totals
    let total_entities: usize = type_counts.values().sum();
    let total_size: usize = total_size_by_type.values().sum();

    // Print header
    println!("{:<25} {:>8} {:>12}  {}", "Type", "Count", "Size", "Details");
    println!("{}", "-".repeat(70));

    // Print each type
    for (type_name, count) in sorted {
        let size = total_size_by_type.get(type_name).unwrap_or(&0);
        let details = singleton_details.get(type_name).map(|s| s.as_str()).unwrap_or("");
        println!("{:<25} {:>8} {:>12}  {}", type_name, count, format_size(*size), details);
    }

    // Print totals
    println!("{}", "-".repeat(70));
    println!("{:<25} {:>8} {:>12}", "TOTAL", total_entities, format_size(total_size));
    println!();
    println!("Container size: {}", format_size(data.len()));

    Ok(())
}

/// Extract details from a singleton entity (list counts, etc.)
fn extract_singleton_details(entity_data: &[u8], type_name: &str) -> Option<String> {
    use boko::kfx::ion::{IonParser, IonValue};
    use boko::kfx::symbols::KfxSymbol;

    // Check for ENTY format
    if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
        return None;
    }

    let entity_header_len = read_u32_le(entity_data, 6)? as usize;
    if entity_header_len >= entity_data.len() {
        return None;
    }

    let ion_data = &entity_data[entity_header_len..];
    let mut parser = IonParser::new(ion_data);
    let value = parser.parse().ok()?;

    // Unwrap annotations
    let inner = match &value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => &value,
    };

    match type_name {
        "location_map" => {
            // locations list inside a wrapper struct
            if let IonValue::List(items) = inner {
                if let Some(IonValue::Struct(fields)) = items.first() {
                    for (fid, fval) in fields {
                        if *fid == KfxSymbol::Locations as u64 {
                            if let IonValue::List(locations) = fval {
                                return Some(format!("{} locations", locations.len()));
                            }
                        }
                    }
                }
            }
        }
        "position_id_map" => {
            // List of {pid, eid} structs
            if let IonValue::List(items) = inner {
                return Some(format!("{} entries", items.len()));
            }
        }
        "position_map" => {
            // List of section entries with contains arrays
            if let IonValue::List(items) = inner {
                let mut total_contains = 0;
                for item in items {
                    if let IonValue::Struct(fields) = item {
                        for (fid, fval) in fields {
                            if *fid == KfxSymbol::Contains as u64 {
                                if let IonValue::List(contains) = fval {
                                    total_contains += contains.len();
                                }
                            }
                        }
                    }
                }
                return Some(format!("{} sections, {} refs", items.len(), total_contains));
            }
        }
        "book_navigation" => {
            // book_navigation is a list containing a struct
            // First unwrap the list to get the struct
            let nav_struct = match inner {
                IonValue::List(items) if !items.is_empty() => {
                    match &items[0] {
                        IonValue::Annotated(_, inner) => inner.as_ref(),
                        other => other,
                    }
                }
                _ => inner,
            };
            if let IonValue::Struct(fields) = nav_struct {
                for (fid, fval) in fields {
                    // NavContainers = 392
                    if *fid == 392 {
                        if let IonValue::List(containers) = fval {
                            let mut details = Vec::new();
                            for container in containers {
                                // Unwrap annotation (nav_container::)
                                let container_inner = match container {
                                    IonValue::Annotated(_, inner) => inner.as_ref(),
                                    _ => container,
                                };
                                if let IonValue::Struct(cfields) = container_inner {
                                    let mut nav_type = None;
                                    let mut entry_count = 0;
                                    for (cfid, cfval) in cfields {
                                        if *cfid == KfxSymbol::NavType as u64 {
                                            if let IonValue::Symbol(sym) = cfval {
                                                nav_type = Some(match *sym as u32 {
                                                    s if s == KfxSymbol::Toc as u32 => "toc",
                                                    s if s == KfxSymbol::Landmarks as u32 => "landmarks",
                                                    s if s == KfxSymbol::PageList as u32 => "pagelist",
                                                    s if s == KfxSymbol::Headings as u32 => "headings",
                                                    _ => "other",
                                                });
                                            }
                                        }
                                        if *cfid == KfxSymbol::Entries as u64 {
                                            if let IonValue::List(entries) = cfval {
                                                entry_count = count_nav_entries(entries);
                                            }
                                        }
                                    }
                                    if let Some(nt) = nav_type {
                                        details.push(format!("{}:{}", nt, entry_count));
                                    }
                                }
                            }
                            return Some(details.join(", "));
                        }
                    }
                }
            }
        }
        "container_entity_map" => {
            // container_list with entity name lists (uses Contains field)
            if let IonValue::Struct(fields) = inner {
                for (fid, fval) in fields {
                    if *fid == KfxSymbol::ContainerList as u64 {
                        if let IonValue::List(containers) = fval {
                            let mut total_entities = 0;
                            for container in containers {
                                if let IonValue::Struct(cfields) = container {
                                    for (cfid, cfval) in cfields {
                                        if *cfid == KfxSymbol::Contains as u64 {
                                            if let IonValue::List(names) = cfval {
                                                total_entities += names.len();
                                            }
                                        }
                                    }
                                }
                            }
                            return Some(format!("{} entity refs", total_entities));
                        }
                    }
                }
            }
        }
        _ => {}
    }

    None
}

/// Count navigation entries recursively (including nested children)
fn count_nav_entries(entries: &[boko::kfx::ion::IonValue]) -> usize {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let mut count = entries.len();
    for entry in entries {
        if let IonValue::Struct(fields) = entry {
            for (fid, fval) in fields {
                if *fid == KfxSymbol::Entries as u64 {
                    if let IonValue::List(children) = fval {
                        count += count_nav_entries(children);
                    }
                }
            }
        }
    }
    count
}

/// Format a size in bytes with appropriate units
fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
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
    // $413 = bcIndexTabOffset, $414 = bcIndexTabLength
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

/// Build entity map for resolving entity ID references.
/// First pass through all entities to extract type and name info.
/// Maps for resolving references in KFX files
struct ResolutionMaps {
    /// Entity symbol ID → EntityInfo (for entity header display)
    entity_map: HashMap<u64, EntityInfo>,
    /// Content fragment ID → storyline name (for target_position resolution)
    fragment_map: HashMap<u64, String>,
}

fn build_maps(
    data: &[u8],
    header_len: usize,
    index_offset: usize,
    num_entries: usize,
    extended_symbols: &[String],
) -> ResolutionMaps {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    let mut entity_map = HashMap::new();
    let mut fragment_map = HashMap::new();
    let entry_size = 24;
    let base_symbol_count = KFX_SYMBOL_TABLE.len();

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(id_idnum) = read_u32_le(data, entry_offset) else { continue };
        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else { continue };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else { continue };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else { continue };

        // Get entity type name
        let entity_type = if (type_idnum as usize) < KFX_SYMBOL_TABLE.len() {
            KFX_SYMBOL_TABLE[type_idnum as usize].to_string()
        } else {
            format!("${}", type_idnum)
        };

        // Parse entity to extract name field and fragment IDs
        let abs_offset = header_len + entity_offset;
        let mut name: Option<String> = None;

        if abs_offset + entity_len <= data.len() {
            let entity_data = &data[abs_offset..abs_offset + entity_len];

            // Check for ENTY format
            if entity_data.len() >= 10 && &entity_data[0..4] == b"ENTY" {
                if let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) {
                    if entity_header_len < entity_data.len() {
                        let ion_data = &entity_data[entity_header_len..];

                        let mut parser = IonParser::new(ion_data);
                        if let Ok(value) = parser.parse() {
                            name = extract_name_from_ion(&value, extended_symbols, base_symbol_count);

                            // For storyline entities, extract fragment IDs from content_list
                            if type_idnum == KfxSymbol::Storyline as u32 {
                                if let Some(story_name) = &name {
                                    extract_fragment_ids(&value, story_name, &mut fragment_map);
                                }
                            }
                        }
                    }
                }
            }
        }

        entity_map.insert(id_idnum as u64, EntityInfo { entity_type, name });
    }

    ResolutionMaps { entity_map, fragment_map }
}

/// Extract fragment IDs from storyline content_list and map them to the story name
fn extract_fragment_ids(value: &boko::kfx::ion::IonValue, story_name: &str, fragment_map: &mut HashMap<u64, String>) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    if let IonValue::Struct(fields) = inner {
        // Look for content_list field (KfxSymbol::ContentList = 146)
        for (field_id, field_value) in fields {
            if *field_id == KfxSymbol::ContentList as u64 {
                extract_fragment_ids_from_list(field_value, story_name, fragment_map);
            }
        }
    }
}

/// Recursively extract fragment IDs from a content_list
fn extract_fragment_ids_from_list(value: &boko::kfx::ion::IonValue, story_name: &str, fragment_map: &mut HashMap<u64, String>) {
    use boko::kfx::ion::IonValue;

    if let IonValue::List(items) = value {
        for item in items {
            extract_id_from_content_item(item, story_name, fragment_map);
        }
    }
}

/// Extract id field from a content_list item struct, and recurse into nested content_lists
fn extract_id_from_content_item(value: &boko::kfx::ion::IonValue, story_name: &str, fragment_map: &mut HashMap<u64, String>) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    if let IonValue::Struct(fields) = inner {
        for (field_id, field_value) in fields {
            if *field_id == KfxSymbol::Id as u64 {
                if let IonValue::Int(id) = field_value {
                    fragment_map.insert(*id as u64, story_name.to_string());
                }
            }
            // Recurse into nested content_lists
            if *field_id == KfxSymbol::ContentList as u64 {
                extract_fragment_ids_from_list(field_value, story_name, fragment_map);
            }
        }
    }
}

/// Extract a name field from an Ion value for entity identification
fn extract_name_from_ion(value: &boko::kfx::ion::IonValue, extended_symbols: &[String], base_symbol_count: usize) -> Option<String> {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    if let IonValue::Struct(fields) = inner {
        // Look for common name fields
        for (field_id, field_value) in fields {
            let field_id = *field_id;
            if field_id == KfxSymbol::Id as u64
                || field_id == KfxSymbol::SectionName as u64
                || field_id == KfxSymbol::StoryName as u64
                || field_id == KfxSymbol::AnchorName as u64
            {
                return ion_value_to_string(field_value, extended_symbols, base_symbol_count);
            }
        }
    }

    None
}

/// Convert an Ion value to a string representation for display
fn ion_value_to_string(value: &boko::kfx::ion::IonValue, extended_symbols: &[String], base_symbol_count: usize) -> Option<String> {
    use boko::kfx::ion::IonValue;

    match value {
        IonValue::String(s) => Some(s.clone()),
        IonValue::Symbol(id) => {
            let id = *id as usize;
            if id < KFX_SYMBOL_TABLE.len() {
                Some(KFX_SYMBOL_TABLE[id].to_string())
            } else if id >= base_symbol_count && id - base_symbol_count < extended_symbols.len() {
                Some(extended_symbols[id - base_symbol_count].clone())
            } else {
                Some(format!("${}", id))
            }
        }
        IonValue::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

/// Extract document-specific symbols from the doc symbols section.
/// The doc symbols section is Ion binary containing a $ion_symbol_table struct.
/// We extract the "symbols" list which contains the local symbol names.
fn extract_doc_symbols(data: &[u8]) -> Vec<String> {
    use boko::kfx::ion::{IonParser, IonValue};

    // Use our own IonParser which doesn't do special symbol table handling
    let mut parser = IonParser::new(data);
    let value = match parser.parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("DEBUG: Failed to parse Ion: {}", e);
            return Vec::new();
        }
    };

    // The value should be an annotated struct: $3::{ imports: [...], symbols: [...] }
    // We need to find the "symbols" field (symbol ID 7)
    let inner = match &value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => &value,
    };

    if let IonValue::Struct(fields) = inner {
        for (field_id, field_value) in fields {
            // Symbol ID 7 = "symbols"
            if *field_id == 7 {
                if let IonValue::List(items) = field_value {
                    return items.iter()
                        .filter_map(|item| {
                            if let IonValue::String(s) = item {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                }
            }
        }
    }

    Vec::new()
}

/// Parse and dump an entity (ENTY format)
fn dump_entity(
    data: &[u8],
    extended_symbols: &[String],
    maps: &ResolutionMaps,
    resolve: bool,
) -> IonResult<()> {
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
            return dump_ion_data_extended(data, extended_symbols, maps, resolve);
        }
        return Ok(());
    }

    let Some(version) = read_u16_le(data, 4) else {
        eprintln!("  Failed to read ENTY version");
        return Ok(());
    };
    let Some(entity_header_len) = read_u32_le(data, 6).map(|v| v as usize) else {
        eprintln!("  Failed to read ENTY header length");
        return Ok(());
    };

    eprintln!("  ENTY version: {}", version);
    eprintln!("  ENTY header length: {}", entity_header_len);

    // The entity info (compression, drm) is between offset 10 and entity_header_len
    // The actual Ion data starts at entity_header_len

    if entity_header_len < data.len() {
        let ion_data = &data[entity_header_len..];
        eprintln!("  Ion data ({} bytes):", ion_data.len());
        dump_ion_data_extended(ion_data, extended_symbols, maps, resolve)?;
    }

    Ok(())
}

/// Dump Ion data using the KFX symbol table
fn dump_ion_data(data: &[u8]) -> IonResult<()> {
    let empty_maps = ResolutionMaps {
        entity_map: HashMap::new(),
        fragment_map: HashMap::new(),
    };
    dump_ion_data_extended(data, &[], &empty_maps, false)
}

/// Dump Ion data using the KFX symbol table plus extended document symbols
fn dump_ion_data_extended(
    data: &[u8],
    extended_symbols: &[String],
    maps: &ResolutionMaps,
    resolve: bool,
) -> IonResult<()> {
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

    // max_id for import: base symbols (0-851) plus extended document symbols
    use boko::kfx::symbols::KFX_MAX_SYMBOL_ID;
    let max_id = (KFX_MAX_SYMBOL_ID + extended_symbols.len()) as i64;

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
                let text = element_to_ion_text(&elem, &all_symbols, maps, resolve);
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
fn element_to_ion_text(
    elem: &ion_rs::Element,
    _symbols: &[&str], // Unused: symbol resolution handled by ion_rs catalog
    maps: &ResolutionMaps,
    resolve: bool,
) -> String {
    element_to_ion_text_inner(elem, maps, resolve, 0, None)
}

/// Check if this is an integer entity reference field (for reading_order bounds)
/// Note: story_name/section_name are typically symbols, not integers
fn is_int_entity_ref_field(field_name: &str) -> bool {
    matches!(field_name, "reading_order_start" | "reading_order_end")
}

/// Context for tracking parent struct when resolving references
#[derive(Clone, Copy)]
struct FieldContext<'a> {
    /// Current field name (e.g., "id")
    field_name: Option<&'a str>,
    /// Whether we're inside a target_position struct
    in_target_position: bool,
}

impl<'a> FieldContext<'a> {
    fn new() -> Self {
        Self { field_name: None, in_target_position: false }
    }

    fn with_field(self, name: Option<&'a str>) -> Self {
        Self {
            field_name: name,
            in_target_position: self.in_target_position || name == Some("target_position"),
        }
    }
}

fn element_to_ion_text_inner(
    elem: &ion_rs::Element,
    maps: &ResolutionMaps,
    resolve: bool,
    indent: usize,
    ctx: Option<FieldContext<'_>>,
) -> String {
    use ion_rs::IonType;

    let ctx = ctx.unwrap_or_else(FieldContext::new);
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
                // Try to resolve references
                if resolve {
                    let field = ctx.field_name.unwrap_or("");
                    // Only resolve "id" fields inside target_position structs
                    if field == "id" && ctx.in_target_position {
                        if let Some(story_name) = maps.fragment_map.get(&(i as u64)) {
                            result.push_str(&format!(" /* {} */", story_name));
                        }
                    }
                    // Resolve integer entity references (reading_order bounds)
                    else if is_int_entity_ref_field(field) {
                        if let Some(info) = maps.entity_map.get(&(i as u64)) {
                            if let Some(name) = &info.name {
                                result.push_str(&format!(" /* {}:{} */", info.entity_type, name));
                            } else {
                                result.push_str(&format!(" /* {} */", info.entity_type));
                            }
                        }
                    }
                }
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
                    result.push_str(&element_to_ion_text_inner(items[0], maps, resolve, 0, Some(ctx)));
                    result.push(']');
                } else {
                    result.push_str("[\n");
                    let inner_indent = "  ".repeat(indent + 1);
                    for (i, item) in items.iter().enumerate() {
                        result.push_str(&inner_indent);
                        result.push_str(&element_to_ion_text_inner(item, maps, resolve, indent + 1, Some(ctx)));
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
                let items: Vec<_> = sexp.iter().map(|e| element_to_ion_text_inner(e, maps, resolve, 0, None)).collect();
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
                        let field_name_str = if let Some(text) = name.text() {
                            if text.chars().all(|c| c.is_alphanumeric() || c == '_') && !text.is_empty() {
                                text.to_string()
                            } else {
                                format!("'{}'", text.replace('\'', "\\'"))
                            }
                        } else {
                            "$0".to_string()  // Ion format for symbol with unknown text
                        };
                        result.push_str(&inner_indent);
                        result.push_str(&field_name_str);
                        result.push_str(": ");
                        // Pass field context for resolution (tracks target_position ancestry)
                        let field_ctx = ctx.with_field(name.text());
                        result.push_str(&element_to_ion_text_inner(value, maps, resolve, indent + 1, Some(field_ctx)));
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

