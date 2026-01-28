use boko::kfx::symbols::KFX_SYMBOL_TABLE;
use clap::Parser;
use ion_rs::{
    AnyEncoding, Decoder, ElementReader, IonResult, MapCatalog, Reader, SharedSymbolTable,
};
use std::collections::HashMap;
use std::fs;

/// Ion 1.0 Binary Version Marker
const ION_BVM: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];

/// Dump KFX/KDF/Ion files for debugging
#[derive(Parser, Debug)]
#[command(name = "kfx-dump")]
#[command(
    about = "Dumps KFX/KDF/Ion files. Supports KFX container files (.kfx) and raw Ion binary files (.kdf, .ion)"
)]
struct Args {
    /// KFX file to dump
    file: String,

    /// Resolve entity ID references to show names
    #[arg(short, long)]
    resolve: bool,

    /// Show statistics (entity counts by type)
    #[arg(short, long)]
    stat: bool,

    /// Print detailed report for specified field/fragment (can be specified multiple times)
    /// Supported: anchors, toc
    #[arg(short = 'f', long = "field")]
    field: Vec<String>,
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
    use ion_rs::v1_0::Binary;
    use ion_rs::{
        Element, ElementWriter, IntoAnnotatedElement, WriteConfig, Writer, ion_list, ion_struct,
    };

    // Build: $ion_symbol_table::{ imports: [{ name: "YJ_symbols", version: 10, max_id: N }] }
    // Amazon's KFX symbol table is named "YJ_symbols" (Yellow Jersey)
    let import = ion_struct! {
        "name": "YJ_symbols",
        "version": 10i64,
        "max_id": max_id,
    };

    let symbol_table: Element = ion_struct! {
        "imports": ion_list![import],
    }
    .with_annotations(["$ion_symbol_table"]);

    let buffer = Vec::new();
    let mut writer = Writer::new(WriteConfig::<Binary>::new(), buffer).unwrap();
    writer.write_element(&symbol_table).unwrap();
    writer.close().unwrap()
}

fn main() -> IonResult<()> {
    let args = Args::parse();

    let data = fs::read(&args.file).expect("Failed to read file");

    // Handle field reports
    if !args.field.is_empty() {
        if data.len() < 4 || &data[0..4] != b"CONT" {
            eprintln!("Field reports require a KFX container file");
            std::process::exit(1);
        }

        for field in &args.field {
            match field.as_str() {
                "anchors" => report_anchors(&data)?,
                "container" => report_container(&data)?,
                "features" => report_features(&data)?,
                "document" => report_document(&data)?,
                "metadata" => report_metadata(&data)?,
                "navigation" => report_navigation(&data)?,
                "reading_orders" => report_reading_orders(&data)?,
                "resources" => report_resources(&data)?,
                "sections" => report_sections(&data)?,
                other => {
                    eprintln!(
                        "Unknown field report: {}. Supported: anchors, container, document, features, metadata, navigation, reading_orders, resources, sections",
                        other
                    );
                    std::process::exit(1);
                }
            }
        }
        return Ok(());
    }

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
    eprintln!(
        "Container info: offset={}, length={}",
        container_info_offset, container_info_length
    );
    eprintln!();

    // Extended symbols from doc symbol table
    let mut extended_symbols: Vec<String> = Vec::new();

    // Parse container info (Ion struct)
    if container_info_offset + container_info_length <= data.len() {
        let container_info_data =
            &data[container_info_offset..container_info_offset + container_info_length];
        eprintln!("=== Container Info ===");
        if let Err(e) = dump_ion_data(container_info_data) {
            eprintln!("Error parsing container info: {}", e);
        }
        eprintln!();

        // Extract doc symbols and add to extended symbol table
        if let Some((doc_sym_offset, doc_sym_length)) =
            parse_container_info_for_doc_symbols(container_info_data)
        {
            eprintln!(
                "Document symbols: offset={}, length={}",
                doc_sym_offset, doc_sym_length
            );
            if doc_sym_offset + doc_sym_length <= data.len() {
                let doc_sym_data = &data[doc_sym_offset..doc_sym_offset + doc_sym_length];
                extended_symbols = extract_doc_symbols(doc_sym_data);
                eprintln!(
                    "Extracted {} document-specific symbols",
                    extended_symbols.len()
                );
                eprintln!();
            }
        }

        // Try to extract index table info from container info
        let index_info = parse_container_info_for_index(container_info_data);

        if let Some((index_offset, index_length)) = index_info {
            eprintln!(
                "Index table: offset={}, length={}",
                index_offset, index_length
            );

            // Parse index table - each entry is 24 bytes
            let entry_size = 24;
            let num_entries = index_length / entry_size;
            eprintln!("Number of entities: {}", num_entries);
            eprintln!();

            // First pass: build resolution maps if resolving
            let maps = if resolve {
                build_maps(
                    data,
                    header_len,
                    index_offset,
                    num_entries,
                    &extended_symbols,
                )
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
                let Some(id_idnum) = read_u32_le(data, entry_offset) else {
                    continue;
                };
                let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
                    continue;
                };
                let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize)
                else {
                    continue;
                };
                let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize)
                else {
                    continue;
                };

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
                        eprintln!(
                            "  ID: ${} ({}) [{}:{}]",
                            id_idnum, id_name, info.entity_type, name
                        );
                    } else {
                        eprintln!("  ID: ${} ({}) [{}]", id_idnum, id_name, info.entity_type);
                    }
                } else {
                    eprintln!("  ID: ${} ({})", id_idnum, id_name);
                }
                eprintln!("  Type: ${} ({})", type_idnum, type_name);
                eprintln!(
                    "  Offset: {} (absolute: {})",
                    entity_offset,
                    header_len + entity_offset
                );
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

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
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

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

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
    println!("{:<25} {:>8} {:>12}  Details", "Type", "Count", "Size");
    println!("{}", "-".repeat(70));

    // Print each type
    for (type_name, count) in sorted {
        let size = total_size_by_type.get(type_name).unwrap_or(&0);
        let details = singleton_details
            .get(type_name)
            .map(|s| s.as_str())
            .unwrap_or("");
        println!(
            "{:<25} {:>8} {:>12}  {}",
            type_name,
            count,
            format_size(*size),
            details
        );
    }

    // Print totals
    println!("{}", "-".repeat(70));
    println!(
        "{:<25} {:>8} {:>12}",
        "TOTAL",
        total_entities,
        format_size(total_size)
    );
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
            if let IonValue::List(items) = inner
                && let Some(IonValue::Struct(fields)) = items.first()
            {
                for (fid, fval) in fields {
                    if *fid == KfxSymbol::Locations as u64
                        && let IonValue::List(locations) = fval
                    {
                        return Some(format!("{} locations", locations.len()));
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
                            if *fid == KfxSymbol::Contains as u64
                                && let IonValue::List(contains) = fval
                            {
                                total_contains += contains.len();
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
                IonValue::List(items) if !items.is_empty() => match &items[0] {
                    IonValue::Annotated(_, inner) => inner.as_ref(),
                    other => other,
                },
                _ => inner,
            };
            if let IonValue::Struct(fields) = nav_struct {
                for (fid, fval) in fields {
                    // NavContainers = 392
                    if *fid == 392
                        && let IonValue::List(containers) = fval
                    {
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
                                    if *cfid == KfxSymbol::NavType as u64
                                        && let IonValue::Symbol(sym) = cfval
                                    {
                                        nav_type = Some(match *sym as u32 {
                                            s if s == KfxSymbol::Toc as u32 => "toc",
                                            s if s == KfxSymbol::Landmarks as u32 => "landmarks",
                                            s if s == KfxSymbol::PageList as u32 => "pagelist",
                                            s if s == KfxSymbol::Headings as u32 => "headings",
                                            _ => "other",
                                        });
                                    }
                                    if *cfid == KfxSymbol::Entries as u64
                                        && let IonValue::List(entries) = cfval
                                    {
                                        entry_count = count_nav_entries(entries);
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
        "container_entity_map" => {
            // container_list with entity name lists (uses Contains field)
            if let IonValue::Struct(fields) = inner {
                for (fid, fval) in fields {
                    if *fid == KfxSymbol::ContainerList as u64
                        && let IonValue::List(containers) = fval
                    {
                        let mut total_entities = 0;
                        for container in containers {
                            if let IonValue::Struct(cfields) = container {
                                for (cfid, cfval) in cfields {
                                    if *cfid == KfxSymbol::Contains as u64
                                        && let IonValue::List(names) = cfval
                                    {
                                        total_entities += names.len();
                                    }
                                }
                            }
                        }
                        return Some(format!("{} entity refs", total_entities));
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
                if *fid == KfxSymbol::Entries as u64
                    && let IonValue::List(children) = fval
                {
                    count += count_nav_entries(children);
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

/// Collected anchor information for reporting
#[derive(Debug)]
struct AnchorInfo {
    name: String,
    source_text: Option<String>, // Text of the link that references this anchor
    destination: AnchorDestination,
}

#[derive(Debug)]
enum AnchorDestination {
    Internal {
        id: i64,
        offset: Option<i64>,
        text: Option<String>,
    },
    External {
        uri: String,
    },
    Target, // This anchor is a target (no position/uri - it's pointed TO)
}

/// Info about a link_to reference in a storyline
#[derive(Debug, Clone)]
struct LinkToRef {
    anchor_name: String,
    content_name: String, // Content entity name (e.g., "content_1")
    content_index: i64,   // Index within content_list array
    offset: i64,
    length: i64,
}

/// Report all anchors from a KFX container
fn report_anchors(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len();

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // First pass: collect content entities (content_name → list of strings)
    let mut content_map: HashMap<String, Vec<String>> = HashMap::new();
    // For destination text lookup by fragment ID
    let mut content_by_id: HashMap<i64, String> = HashMap::new();
    // Map fragment ID → (content_name, content_index) for resolving anchor destinations
    let mut fragment_content_map: HashMap<i64, (String, i64)> = HashMap::new();
    // Second pass: collect link_to references from storylines
    let mut link_to_refs: Vec<LinkToRef> = Vec::new();
    // Third pass: collect anchors

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(id_idnum) = read_u32_le(data, entry_offset) else {
            continue;
        };
        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];

        // Parse ENTY format
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);
        let value = match parser.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Collect content entities
        if type_idnum == KfxSymbol::Content as u32
            && let Some((name, texts)) =
                extract_content_texts(&value, &extended_symbols, base_symbol_count)
        {
            // Also build concatenated text for destination lookup
            let full_text = texts.join("");
            content_by_id.insert(id_idnum as i64, full_text);
            content_map.insert(name, texts);
        }

        // Collect link_to references and fragment content mappings from storylines
        if type_idnum == KfxSymbol::Storyline as u32 {
            extract_link_to_refs(
                &value,
                &extended_symbols,
                base_symbol_count,
                &mut link_to_refs,
            );
            let mut _unused_types = HashMap::new();
            extract_fragment_content_refs(
                &value,
                &extended_symbols,
                base_symbol_count,
                &mut fragment_content_map,
                &mut _unused_types,
            );
        }
    }

    // Build anchor_name → source text mapping
    let mut anchor_source_text: HashMap<String, String> = HashMap::new();
    for link_ref in &link_to_refs {
        if let Some(content_texts) = content_map.get(&link_ref.content_name)
            && let Some(content_text) = content_texts.get(link_ref.content_index as usize)
        {
            let start = link_ref.offset as usize;
            // offset and length are in characters
            let text: String = content_text
                .chars()
                .skip(start)
                .take(link_ref.length as usize)
                .collect();
            anchor_source_text.insert(link_ref.anchor_name.clone(), text);
        }
    }

    // Now collect anchors with full info
    let mut anchors: Vec<AnchorInfo> = Vec::new();

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        // Check if this is an anchor entity (type = 266)
        if type_idnum != KfxSymbol::Anchor as u32 {
            continue;
        }

        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];

        // Parse ENTY format
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);
        let value = match parser.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Extract anchor info
        if let Some(anchor) = extract_anchor_info(
            &value,
            &extended_symbols,
            base_symbol_count,
            &content_map,
            &fragment_content_map,
            &anchor_source_text,
        ) {
            anchors.push(anchor);
        }
    }

    // Print report
    println!("=== Anchors ({} total) ===\n", anchors.len());

    for anchor in &anchors {
        let source = anchor.source_text.as_deref().unwrap_or("-");
        match &anchor.destination {
            AnchorDestination::Internal { id, offset, text } => {
                let position = match offset {
                    Some(off) => format!("{}:{}", id, off),
                    None => format!("{}", id),
                };
                if let Some(dest_text) = text {
                    let dest_preview: String = dest_text.chars().take(40).collect();
                    let ellipsis = if dest_text.len() > 40 { "..." } else { "" };
                    println!(
                        "{:<30} {:>10} → {} \"{}{}\"",
                        anchor.name,
                        format!("\"{}\"", source),
                        position,
                        dest_preview,
                        ellipsis
                    );
                } else {
                    println!(
                        "{:<30} {:>10} → {}",
                        anchor.name,
                        format!("\"{}\"", source),
                        position
                    );
                }
            }
            AnchorDestination::External { uri } => {
                println!(
                    "{:<30} {:>10} → {}",
                    anchor.name,
                    format!("\"{}\"", source),
                    uri
                );
            }
            AnchorDestination::Target => {
                println!(
                    "{:<30} {:>10} (target)",
                    anchor.name,
                    format!("\"{}\"", source)
                );
            }
        }
    }

    Ok(())
}

/// TOC entry for reporting
#[derive(Debug)]
struct NavEntryInfo {
    label: String,
    landmark_type: Option<String>, // h2, h3, cover_page, srl, etc.
    target_id: Option<i64>,
    target_offset: Option<i64>,
    target_text: Option<String>, // Preview of text at target
    target_type: Option<String>, // Entity type name (e.g., "storyline", "section")
    depth: usize,
}

/// Report navigation (headings, toc, landmarks) from a KFX container
fn report_navigation(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len();

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Collect content entities for text preview lookup
    let mut content_map: HashMap<String, Vec<String>> = HashMap::new();
    // Map fragment ID → (content_name, content_index) for resolving target positions
    let mut fragment_content_map: HashMap<i64, (String, i64)> = HashMap::new();
    // Map container ID → container type (from storyline content_list)
    let mut container_type_map: HashMap<i64, String> = HashMap::new();

    // First pass: collect content, storyline data, and entity types
    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];

        // Parse ENTY format
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);
        let value = match parser.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Collect content entities
        if type_idnum == KfxSymbol::Content as u32
            && let Some((name, texts)) =
                extract_content_texts(&value, &extended_symbols, base_symbol_count)
        {
            content_map.insert(name, texts);
        }

        // Collect fragment content mappings and container types from storylines
        if type_idnum == KfxSymbol::Storyline as u32 {
            extract_fragment_content_refs(
                &value,
                &extended_symbols,
                base_symbol_count,
                &mut fragment_content_map,
                &mut container_type_map,
            );
        }
    }

    // Second pass: find book_navigation entity and extract TOC
    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };

        // Look for BookNavigation entity (type = 389)
        if type_idnum != KfxSymbol::BookNavigation as u32 {
            continue;
        }

        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];

        // Parse ENTY format
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);
        let value = match parser.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Extract and print TOC
        extract_and_print_navigation(
            &value,
            &extended_symbols,
            base_symbol_count,
            &content_map,
            &fragment_content_map,
            &container_type_map,
        );
    }

    Ok(())
}

/// Extract navigation entries from book_navigation and print them
fn extract_and_print_navigation(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    content_map: &HashMap<String, Vec<String>>,
    fragment_content_map: &HashMap<i64, (String, i64)>,
    container_type_map: &HashMap<i64, String>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    // Unwrap list if present (book_navigation contains a list with one struct)
    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        IonValue::List(items) if !items.is_empty() => &items[0],
        _ => value,
    };

    let inner = match inner {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => inner,
    };

    let fields = match inner {
        IonValue::Struct(f) => f,
        _ => return,
    };

    // Find nav_containers field (392)
    for (field_id, field_value) in fields {
        if *field_id != KfxSymbol::NavContainers as u64 {
            continue;
        }

        let containers = match field_value {
            IonValue::List(items) => items,
            _ => continue,
        };

        for container in containers {
            // Each container is an annotated struct with annotation nav_container (391)
            let container_inner = match container {
                IonValue::Annotated(_, inner) => inner.as_ref(),
                _ => container,
            };

            let container_fields = match container_inner {
                IonValue::Struct(f) => f,
                _ => continue,
            };

            // Extract nav_type, container_name, and entries
            let mut nav_type = String::new();
            let mut container_name = String::new();
            let mut entries: Option<&Vec<IonValue>> = None;

            for (cfield_id, cfield_value) in container_fields {
                match *cfield_id as u32 {
                    id if id == KfxSymbol::NavType as u32 => {
                        if let IonValue::Symbol(sym_id) = cfield_value {
                            nav_type =
                                resolve_symbol(*sym_id, extended_symbols, base_symbol_count);
                        }
                    }
                    id if id == KfxSymbol::NavContainerName as u32 => {
                        if let IonValue::Symbol(sym_id) = cfield_value {
                            container_name =
                                resolve_symbol(*sym_id, extended_symbols, base_symbol_count);
                        }
                    }
                    id if id == KfxSymbol::Entries as u32 => {
                        if let IonValue::List(items) = cfield_value {
                            entries = Some(items);
                        }
                    }
                    _ => {}
                }
            }

            // Print all nav containers
            if !nav_type.is_empty() {
                let header = match nav_type.as_str() {
                    "toc" => format!("Table of Contents ({})", container_name),
                    "headings" => format!("Headings ({})", container_name),
                    "landmarks" => format!("Landmarks ({})", container_name),
                    _ => format!("{} ({})", nav_type, container_name),
                };
                println!("=== {} ===\n", header);

                if let Some(entry_list) = entries {
                    let mut nav_entries = Vec::new();
                    extract_nav_entries(
                        entry_list,
                        extended_symbols,
                        base_symbol_count,
                        content_map,
                        fragment_content_map,
                        container_type_map,
                        0,
                        &mut nav_entries,
                    );

                    for entry in &nav_entries {
                        let indent = "  ".repeat(entry.depth);
                        let position = match (entry.target_id, entry.target_offset) {
                            (Some(id), Some(off)) if off != 0 => format!("→ {}:{}", id, off),
                            (Some(id), _) => format!("→ {}", id),
                            _ => String::new(),
                        };

                        // Show landmark type if present (for headings: h2, h3; for landmarks: cover_page, etc.)
                        let landmark_info = entry
                            .landmark_type
                            .as_ref()
                            .map(|t| format!("[{}] ", t))
                            .unwrap_or_default();

                        // Show entity type if known
                        let type_info = if let Some(t) = &entry.target_type {
                            format!(" ({})", t)
                        } else if entry.target_text.is_some() {
                            String::new()
                        } else {
                            " (no content)".to_string()
                        };

                        // Build display label - use landmark_type if label is generic
                        let display_label = if entry.label == "heading-nav-unit"
                            || entry.label == "cover-nav-unit"
                        {
                            entry
                                .landmark_type
                                .clone()
                                .unwrap_or_else(|| entry.label.clone())
                        } else {
                            entry.label.clone()
                        };

                        if let Some(text) = &entry.target_text {
                            let preview: String = text.chars().take(50).collect();
                            let ellipsis = if text.chars().count() > 50 { "..." } else { "" };
                            println!(
                                "{}{}{:<35} {:>12}{}  \"{}{}\"",
                                indent,
                                landmark_info,
                                display_label,
                                position,
                                type_info,
                                preview,
                                ellipsis
                            );
                        } else {
                            println!(
                                "{}{}{:<35} {:>12}{}",
                                indent, landmark_info, display_label, position, type_info
                            );
                        }
                    }

                    println!("\nTotal entries: {}", nav_entries.len());
                }
                println!();
            }
        }
    }
}

/// Recursively extract navigation entries from nav_unit list
#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn extract_nav_entries(
    entries: &[boko::kfx::ion::IonValue],
    extended_symbols: &[String],
    base_symbol_count: usize,
    content_map: &HashMap<String, Vec<String>>,
    fragment_content_map: &HashMap<i64, (String, i64)>,
    container_type_map: &HashMap<i64, String>,
    depth: usize,
    result: &mut Vec<NavEntryInfo>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    for entry in entries {
        // Each entry is nav_unit (393) annotated struct
        let entry_inner = match entry {
            IonValue::Annotated(_, inner) => inner.as_ref(),
            _ => entry,
        };

        let fields = match entry_inner {
            IonValue::Struct(f) => f,
            _ => continue,
        };

        let mut label = String::new();
        let mut landmark_type: Option<String> = None;
        let mut target_id: Option<i64> = None;
        let mut target_offset: Option<i64> = None;
        let mut children: Option<&Vec<IonValue>> = None;

        for (field_id, field_value) in fields {
            match *field_id as u32 {
                id if id == KfxSymbol::LandmarkType as u32 => {
                    // landmark_type is a symbol (h2, h3, cover_page, srl, etc.)
                    if let IonValue::Symbol(sym_id) = field_value {
                        landmark_type = Some(resolve_symbol(
                            *sym_id,
                            extended_symbols,
                            base_symbol_count,
                        ));
                    }
                }
                id if id == KfxSymbol::Representation as u32 => {
                    // representation is a struct with label field
                    if let IonValue::Struct(rep_fields) = field_value {
                        for (rep_field_id, rep_field_value) in rep_fields {
                            if *rep_field_id as u32 == KfxSymbol::Label as u32
                                && let IonValue::String(s) = rep_field_value
                            {
                                label = s.clone();
                            }
                        }
                    }
                }
                id if id == KfxSymbol::TargetPosition as u32 => {
                    // target_position is a struct with id and offset
                    if let IonValue::Struct(pos_fields) = field_value {
                        for (pos_field_id, pos_field_value) in pos_fields {
                            match *pos_field_id as u32 {
                                pid if pid == KfxSymbol::Id as u32 => {
                                    if let IonValue::Int(i) = pos_field_value {
                                        target_id = Some(*i);
                                    }
                                }
                                pid if pid == KfxSymbol::Offset as u32 => {
                                    if let IonValue::Int(i) = pos_field_value {
                                        target_offset = Some(*i);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                id if id == KfxSymbol::Entries as u32 => {
                    if let IonValue::List(items) = field_value {
                        children = Some(items);
                    }
                }
                _ => {}
            }
        }

        // Get text preview at target position
        let target_text = target_id.and_then(|id| {
            fragment_content_map
                .get(&id)
                .and_then(|(content_name, content_index)| {
                    content_map.get(content_name).and_then(|texts| {
                        texts
                            .get(*content_index as usize)
                            .map(|text| {
                                let start = target_offset.unwrap_or(0) as usize;
                                text.chars().skip(start).take(60).collect::<String>()
                            })
                            .filter(|s| !s.is_empty())
                    })
                })
        });

        // Get container type for target ID (from storyline content_list)
        let target_type = target_id.and_then(|id| container_type_map.get(&id).cloned());

        if !label.is_empty() || target_id.is_some() || landmark_type.is_some() {
            result.push(NavEntryInfo {
                label: if label.is_empty() {
                    "(untitled)".to_string()
                } else {
                    label
                },
                landmark_type,
                target_id,
                target_offset,
                target_text,
                target_type,
                depth,
            });
        }

        // Recurse into children
        if let Some(child_entries) = children {
            extract_nav_entries(
                child_entries,
                extended_symbols,
                base_symbol_count,
                content_map,
                fragment_content_map,
                container_type_map,
                depth + 1,
                result,
            );
        }
    }
}

/// Resolve a symbol ID to its text name
fn resolve_symbol(sym_id: u64, extended_symbols: &[String], base_symbol_count: usize) -> String {
    let idx = sym_id as usize;
    if idx < base_symbol_count {
        KFX_SYMBOL_TABLE
            .get(idx)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("${}", sym_id))
    } else {
        let ext_idx = idx - base_symbol_count;
        extended_symbols
            .get(ext_idx)
            .cloned()
            .unwrap_or_else(|| format!("${}", sym_id))
    }
}

/// Extract name and content_list texts from a content entity
fn extract_content_texts(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
) -> Option<(String, Vec<String>)> {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    let fields = match inner {
        IonValue::Struct(f) => f,
        _ => return None,
    };

    let mut name: Option<String> = None;
    let mut texts: Vec<String> = Vec::new();

    for (field_id, field_value) in fields {
        if *field_id == KfxSymbol::Name as u64 {
            name = ion_value_to_string(field_value, extended_symbols, base_symbol_count);
        }
        if *field_id == KfxSymbol::ContentList as u64
            && let IonValue::List(items) = field_value
        {
            for item in items {
                if let IonValue::String(s) = item {
                    texts.push(s.clone());
                }
            }
        }
    }

    name.map(|n| (n, texts))
}

/// Extract fragment ID → content references from a storyline
fn extract_fragment_content_refs(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    refs: &mut HashMap<i64, (String, i64)>,
    container_types: &mut HashMap<i64, String>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    let fields = match inner {
        IonValue::Struct(f) => f,
        _ => return,
    };

    for (field_id, field_value) in fields {
        if *field_id == KfxSymbol::ContentList as u64 {
            extract_fragment_content_from_list(
                field_value,
                extended_symbols,
                base_symbol_count,
                refs,
                container_types,
            );
        }
    }
}

/// Recursively extract fragment ID → content refs and container types from content_list
fn extract_fragment_content_from_list(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    refs: &mut HashMap<i64, (String, i64)>,
    container_types: &mut HashMap<i64, String>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    if let IonValue::List(items) = value {
        for item in items {
            let inner = match item {
                IonValue::Annotated(_, inner) => inner.as_ref(),
                _ => item,
            };

            if let IonValue::Struct(fields) = inner {
                let mut fragment_id: Option<i64> = None;
                let mut content_name: Option<String> = None;
                let mut content_index: Option<i64> = None;
                let mut container_type: Option<String> = None;

                for (field_id, field_value) in fields {
                    // Get fragment ID
                    if *field_id == KfxSymbol::Id as u64
                        && let IonValue::Int(i) = field_value
                    {
                        fragment_id = Some(*i);
                    }

                    // Get container type
                    if *field_id == KfxSymbol::Type as u64
                        && let IonValue::Symbol(sym_id) = field_value
                    {
                        container_type =
                            Some(resolve_symbol(*sym_id, extended_symbols, base_symbol_count));
                    }

                    // Get content reference
                    if *field_id == KfxSymbol::Content as u64
                        && let IonValue::Struct(content_fields) = field_value
                    {
                        for (cf_id, cf_value) in content_fields {
                            if *cf_id == KfxSymbol::Name as u64 {
                                content_name = ion_value_to_string(
                                    cf_value,
                                    extended_symbols,
                                    base_symbol_count,
                                );
                            }
                            if *cf_id == KfxSymbol::Index as u64
                                && let IonValue::Int(i) = cf_value
                            {
                                content_index = Some(*i);
                            }
                        }
                    }

                    // Recurse into nested content_list
                    if *field_id == KfxSymbol::ContentList as u64 {
                        extract_fragment_content_from_list(
                            field_value,
                            extended_symbols,
                            base_symbol_count,
                            refs,
                            container_types,
                        );
                    }
                }

                // Record container type if we have an ID
                if let (Some(fid), Some(ctype)) = (fragment_id, container_type) {
                    container_types.insert(fid, ctype);
                }

                // If we found all content info, add to map
                if let (Some(fid), Some(cname), Some(cindex)) =
                    (fragment_id, content_name, content_index)
                {
                    refs.insert(fid, (cname, cindex));
                }
            }
        }
    }
}

/// Extract link_to references from a storyline
fn extract_link_to_refs(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    refs: &mut Vec<LinkToRef>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    let fields = match inner {
        IonValue::Struct(f) => f,
        _ => return,
    };

    for (field_id, field_value) in fields {
        if *field_id == KfxSymbol::ContentList as u64 {
            extract_link_to_from_content_list(
                field_value,
                extended_symbols,
                base_symbol_count,
                refs,
            );
        }
    }
}

/// Recursively extract link_to refs from storyline content_list
fn extract_link_to_from_content_list(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    refs: &mut Vec<LinkToRef>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    if let IonValue::List(items) = value {
        for item in items {
            let inner = match item {
                IonValue::Annotated(_, inner) => inner.as_ref(),
                _ => item,
            };

            if let IonValue::Struct(fields) = inner {
                let mut content_name: Option<String> = None;
                let mut content_index: Option<i64> = None;
                let mut inline_refs: Vec<(String, i64, i64)> = Vec::new(); // (anchor_name, offset, length)

                for (field_id, field_value) in fields {
                    // Look for content: { name: ..., index: ... } - the text reference
                    if *field_id == KfxSymbol::Content as u64
                        && let IonValue::Struct(content_fields) = field_value
                    {
                        for (cf_id, cf_value) in content_fields {
                            if *cf_id == KfxSymbol::Name as u64 {
                                content_name = ion_value_to_string(
                                    cf_value,
                                    extended_symbols,
                                    base_symbol_count,
                                );
                            }
                            if *cf_id == KfxSymbol::Index as u64
                                && let IonValue::Int(i) = cf_value
                            {
                                content_index = Some(*i);
                            }
                        }
                    }

                    // Look for style_events with inline elements (link_to, offset, length)
                    if *field_id == KfxSymbol::StyleEvents as u64 {
                        extract_inline_link_to(
                            field_value,
                            extended_symbols,
                            base_symbol_count,
                            &mut inline_refs,
                        );
                    }

                    // Recurse for nested content_list (nested storyline fragments)
                    if *field_id == KfxSymbol::ContentList as u64 {
                        extract_link_to_from_content_list(
                            field_value,
                            extended_symbols,
                            base_symbol_count,
                            refs,
                        );
                    }
                }

                // If we found content info and inline refs, add them
                if let (Some(cname), Some(cindex)) = (content_name, content_index) {
                    for (anchor_name, offset, length) in inline_refs {
                        refs.push(LinkToRef {
                            anchor_name,
                            content_name: cname.clone(),
                            content_index: cindex,
                            offset,
                            length,
                        });
                    }
                }
            }
        }
    }
}

/// Extract inline link_to references (offset, length, link_to)
fn extract_inline_link_to(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    refs: &mut Vec<(String, i64, i64)>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    if let IonValue::List(items) = value {
        for item in items {
            let inner = match item {
                IonValue::Annotated(_, inner) => inner.as_ref(),
                _ => item,
            };

            if let IonValue::Struct(fields) = inner {
                let mut link_to: Option<String> = None;
                let mut offset: Option<i64> = None;
                let mut length: Option<i64> = None;

                for (field_id, field_value) in fields {
                    if *field_id == KfxSymbol::LinkTo as u64 {
                        link_to =
                            ion_value_to_string(field_value, extended_symbols, base_symbol_count);
                    }
                    if *field_id == KfxSymbol::Offset as u64
                        && let IonValue::Int(i) = field_value
                    {
                        offset = Some(*i);
                    }
                    if *field_id == KfxSymbol::Length as u64
                        && let IonValue::Int(i) = field_value
                    {
                        length = Some(*i);
                    }
                }

                if let (Some(anchor_name), Some(off), Some(len)) = (link_to, offset, length) {
                    refs.push((anchor_name, off, len));
                }
            }
        }
    }
}

/// Extract anchor info from an Ion value
fn extract_anchor_info(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
    content_map: &HashMap<String, Vec<String>>,
    fragment_content_map: &HashMap<i64, (String, i64)>,
    anchor_source_text: &HashMap<String, String>,
) -> Option<AnchorInfo> {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    let fields = match inner {
        IonValue::Struct(f) => f,
        _ => return None,
    };

    let mut anchor_name: Option<String> = None;
    let mut position_id: Option<i64> = None;
    let mut position_offset: Option<i64> = None;
    let mut uri: Option<String> = None;

    for (field_id, field_value) in fields {
        match *field_id as u32 {
            id if id == KfxSymbol::AnchorName as u32 => {
                anchor_name = ion_value_to_string(field_value, extended_symbols, base_symbol_count);
            }
            id if id == KfxSymbol::Position as u32 => {
                // position is a struct with id and offset
                if let IonValue::Struct(pos_fields) = field_value {
                    for (pos_field_id, pos_field_value) in pos_fields {
                        match *pos_field_id as u32 {
                            pid if pid == KfxSymbol::Id as u32 => {
                                if let IonValue::Int(i) = pos_field_value {
                                    position_id = Some(*i);
                                }
                            }
                            pid if pid == KfxSymbol::Offset as u32 => {
                                if let IonValue::Int(i) = pos_field_value {
                                    position_offset = Some(*i);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            id if id == KfxSymbol::Uri as u32 => {
                if let IonValue::String(s) = field_value {
                    uri = Some(s.clone());
                }
            }
            _ => {}
        }
    }

    let name = anchor_name.unwrap_or_else(|| "(unnamed)".to_string());
    let source_text = anchor_source_text.get(&name).cloned();

    let destination = if let Some(u) = uri {
        AnchorDestination::External { uri: u }
    } else if let Some(id) = position_id {
        // Try to get destination text by looking up fragment → content mapping
        let text = fragment_content_map
            .get(&id)
            .and_then(|(content_name, content_index)| {
                content_map.get(content_name).and_then(|texts| {
                    texts
                        .get(*content_index as usize)
                        .map(|text| {
                            let start = position_offset.unwrap_or(0) as usize;
                            let preview: String = text.chars().skip(start).take(60).collect();
                            preview
                        })
                        .filter(|s| !s.is_empty())
                })
            });
        AnchorDestination::Internal {
            id,
            offset: position_offset,
            text,
        }
    } else {
        AnchorDestination::Target
    };

    Some(AnchorInfo {
        name,
        source_text,
        destination,
    })
}

/// Parse container info Ion struct to extract index table offset/length
fn parse_container_info_for_index(data: &[u8]) -> Option<(usize, usize)> {
    // Skip first 10 entries - see dump_ion_data_extended for explanation
    let mut catalog = MapCatalog::new();
    if let Ok(table) =
        SharedSymbolTable::new("YJ_symbols", 10, KFX_SYMBOL_TABLE[10..].iter().copied())
    {
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
            && let Some(strukt) = elem.as_struct()
        {
            for field in strukt.iter() {
                let (name, value) = field;
                if let Some(field_name) = name.text() {
                    if field_name == "bcIndexTabOffset"
                        && let Some(i) = value.as_i64()
                    {
                        index_offset = Some(i as usize);
                    }
                    if field_name == "bcIndexTabLength"
                        && let Some(i) = value.as_i64()
                    {
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
    if let Ok(table) =
        SharedSymbolTable::new("YJ_symbols", 10, KFX_SYMBOL_TABLE[10..].iter().copied())
    {
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
            && let Some(strukt) = elem.as_struct()
        {
            for field in strukt.iter() {
                let (name, value) = field;
                if let Some(field_name) = name.text() {
                    if field_name == "bcDocSymbolOffset"
                        && let Some(i) = value.as_i64()
                    {
                        doc_sym_offset = Some(i as usize);
                    }
                    if field_name == "bcDocSymbolLength"
                        && let Some(i) = value.as_i64()
                    {
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

        let Some(id_idnum) = read_u32_le(data, entry_offset) else {
            continue;
        };
        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u64_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

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
            if entity_data.len() >= 10
                && &entity_data[0..4] == b"ENTY"
                && let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize)
                && entity_header_len < entity_data.len()
            {
                let ion_data = &entity_data[entity_header_len..];

                let mut parser = IonParser::new(ion_data);
                if let Ok(value) = parser.parse() {
                    name = extract_name_from_ion(&value, extended_symbols, base_symbol_count);

                    // For storyline entities, extract fragment IDs from content_list
                    if type_idnum == KfxSymbol::Storyline as u32
                        && let Some(story_name) = &name
                    {
                        extract_fragment_ids(&value, story_name, &mut fragment_map);
                    }
                }
            }
        }

        entity_map.insert(id_idnum as u64, EntityInfo { entity_type, name });
    }

    ResolutionMaps {
        entity_map,
        fragment_map,
    }
}

/// Extract fragment IDs from storyline content_list and map them to the story name
fn extract_fragment_ids(
    value: &boko::kfx::ion::IonValue,
    story_name: &str,
    fragment_map: &mut HashMap<u64, String>,
) {
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
fn extract_fragment_ids_from_list(
    value: &boko::kfx::ion::IonValue,
    story_name: &str,
    fragment_map: &mut HashMap<u64, String>,
) {
    use boko::kfx::ion::IonValue;

    if let IonValue::List(items) = value {
        for item in items {
            extract_id_from_content_item(item, story_name, fragment_map);
        }
    }
}

/// Extract id field from a content_list item struct, and recurse into nested content_lists
fn extract_id_from_content_item(
    value: &boko::kfx::ion::IonValue,
    story_name: &str,
    fragment_map: &mut HashMap<u64, String>,
) {
    use boko::kfx::ion::IonValue;
    use boko::kfx::symbols::KfxSymbol;

    let inner = match value {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        _ => value,
    };

    if let IonValue::Struct(fields) = inner {
        for (field_id, field_value) in fields {
            if *field_id == KfxSymbol::Id as u64
                && let IonValue::Int(id) = field_value
            {
                fragment_map.insert(*id as u64, story_name.to_string());
            }
            // Recurse into nested content_lists
            if *field_id == KfxSymbol::ContentList as u64 {
                extract_fragment_ids_from_list(field_value, story_name, fragment_map);
            }
        }
    }
}

/// Extract a name field from an Ion value for entity identification
fn extract_name_from_ion(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
) -> Option<String> {
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
fn ion_value_to_string(
    value: &boko::kfx::ion::IonValue,
    extended_symbols: &[String],
    base_symbol_count: usize,
) -> Option<String> {
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
            if *field_id == 7
                && let IonValue::List(items) = field_value
            {
                return items
                    .iter()
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
        Self {
            field_name: None,
            in_target_position: false,
        }
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
            result.push_str("$0"); // Ion format for symbol with unknown text
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
                    else if is_int_entity_ref_field(field)
                        && let Some(info) = maps.entity_map.get(&(i as u64))
                    {
                        if let Some(name) = &info.name {
                            result.push_str(&format!(" /* {}:{} */", info.entity_type, name));
                        } else {
                            result.push_str(&format!(" /* {} */", info.entity_type));
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
                    result.push_str("$0"); // Ion format for symbol with unknown text
                }
            } else {
                result.push_str("null.symbol");
            }
        }
        IonType::String => {
            if let Some(s) = elem.as_string() {
                result.push('"');
                result.push_str(
                    &s.replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\n', "\\n"),
                );
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
                } else if items.len() == 1
                    && !matches!(items[0].ion_type(), IonType::Struct | IonType::List)
                {
                    result.push('[');
                    result.push_str(&element_to_ion_text_inner(
                        items[0],
                        maps,
                        resolve,
                        0,
                        Some(ctx),
                    ));
                    result.push(']');
                } else {
                    result.push_str("[\n");
                    let inner_indent = "  ".repeat(indent + 1);
                    for (i, item) in items.iter().enumerate() {
                        result.push_str(&inner_indent);
                        result.push_str(&element_to_ion_text_inner(
                            item,
                            maps,
                            resolve,
                            indent + 1,
                            Some(ctx),
                        ));
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
                let items: Vec<_> = sexp
                    .iter()
                    .map(|e| element_to_ion_text_inner(e, maps, resolve, 0, None))
                    .collect();
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
                            if text.chars().all(|c| c.is_alphanumeric() || c == '_')
                                && !text.is_empty()
                            {
                                text.to_string()
                            } else {
                                format!("'{}'", text.replace('\'', "\\'"))
                            }
                        } else {
                            "$0".to_string() // Ion format for symbol with unknown text
                        };
                        result.push_str(&inner_indent);
                        result.push_str(&field_name_str);
                        result.push_str(": ");
                        // Pass field context for resolution (tracks target_position ancestry)
                        let field_ctx = ctx.with_field(name.text());
                        result.push_str(&element_to_ion_text_inner(
                            value,
                            maps,
                            resolve,
                            indent + 1,
                            Some(field_ctx),
                        ));
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

/// Report container header and info from a KFX file
fn report_container(data: &[u8]) -> IonResult<()> {
    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
        return Ok(());
    }

    // Parse binary header
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

    println!("=== Container Header (Binary) ===\n");
    println!("magic:                 CONT");
    println!("version:               {}", version);
    println!("header_len:            {}", header_len);
    println!("container_info_offset: {}", container_info_offset);
    println!("container_info_length: {}", container_info_length);
    println!();

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Parse container info Ion struct
    let mut catalog = MapCatalog::new();
    if let Ok(table) =
        SharedSymbolTable::new("YJ_symbols", 10, KFX_SYMBOL_TABLE[10..].iter().copied())
    {
        catalog.insert_table(table);
    }

    let preamble = build_symbol_table_preamble();
    let mut full_data = preamble;
    if container_info_data.len() >= 4 && container_info_data[0..4] == ION_BVM {
        full_data.extend_from_slice(&container_info_data[4..]);
    } else {
        full_data.extend_from_slice(container_info_data);
    }

    let reader = Reader::new(AnyEncoding.with_catalog(catalog), &full_data[..]);
    if reader.is_err() {
        eprintln!("Failed to parse container info Ion");
        return Ok(());
    }
    let mut reader = reader.unwrap();

    println!("=== Container Info (Ion) ===\n");

    for element in reader.elements() {
        if let Ok(elem) = element
            && let Some(strukt) = elem.as_struct()
        {
            for field in strukt.iter() {
                let (name, value) = field;
                let field_name = name.text().unwrap_or("?");

                // Format value based on type
                let value_str = if let Some(i) = value.as_i64() {
                    format!("{}", i)
                } else if let Some(s) = value.as_string() {
                    format!("\"{}\"", s)
                } else if let Some(b) = value.as_blob() {
                    format!("<blob {} bytes>", b.len())
                } else if value.is_null() {
                    "null".to_string()
                } else {
                    format!("{:?}", value.ion_type())
                };

                // Pad field name for alignment
                println!("{:<25} {}", format!("{}:", field_name), value_str);
            }
        }
    }

    Ok(())
}

/// Report features (content_features entity) from a KFX file
fn report_features(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len() as u64;
    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Find content_features entity (type 585)
    let content_features_type = KfxSymbol::ContentFeatures as u32;

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u32_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        if type_idnum != content_features_type {
            continue;
        }

        // Found content_features entity
        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);

        if let Ok(value) = parser.parse() {
            println!("=== Content Features ===\n");

            // Helper to resolve symbol name
            let resolve_sym = |id: u64| -> &str {
                if id < base_symbol_count {
                    KFX_SYMBOL_TABLE.get(id as usize).copied().unwrap_or("?")
                } else {
                    let ext_idx = (id as usize) - (base_symbol_count as usize);
                    extended_symbols
                        .get(ext_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("?")
                }
            };

            // Extract and display features list
            if let boko::kfx::ion::IonValue::Struct(fields) = &value {
                for (field_id, field_value) in fields {
                    let field_name = resolve_sym(*field_id);

                    if field_name == "features" {
                        if let boko::kfx::ion::IonValue::List(features) = field_value {
                            for (idx, feature) in features.iter().enumerate() {
                                if let boko::kfx::ion::IonValue::Struct(ffields) = feature {
                                    let mut namespace = String::new();
                                    let mut key = String::new();
                                    let mut major = 0i64;
                                    let mut minor = 0i64;

                                    for (fid, fval) in ffields {
                                        let fname = resolve_sym(*fid);

                                        match fname {
                                            "namespace" => {
                                                if let boko::kfx::ion::IonValue::String(s) = fval {
                                                    namespace = s.clone();
                                                }
                                            }
                                            "key" => {
                                                if let boko::kfx::ion::IonValue::String(s) = fval {
                                                    key = s.clone();
                                                }
                                            }
                                            "version_info" => {
                                                // Extract version from nested struct
                                                if let boko::kfx::ion::IonValue::Struct(vi) = fval {
                                                    for (vid, vval) in vi {
                                                        let vname = resolve_sym(*vid);
                                                        if vname == "version" {
                                                            if let boko::kfx::ion::IonValue::Struct(
                                                                ver,
                                                            ) = vval
                                                            {
                                                                for (verid, verval) in ver {
                                                                    let vername =
                                                                        resolve_sym(*verid);
                                                                    if vername == "major_version" {
                                                                        if let boko::kfx::ion::IonValue::Int(v) = verval {
                                                                            major = *v;
                                                                        }
                                                                    }
                                                                    if vername == "minor_version" {
                                                                        if let boko::kfx::ion::IonValue::Int(v) = verval {
                                                                            minor = *v;
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    println!(
                                        "{:2}. {}.{} v{}.{}",
                                        idx + 1,
                                        namespace,
                                        key,
                                        major,
                                        minor
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        return Ok(());
    }

    eprintln!("No content_features entity found");
    Ok(())
}

/// Report metadata (book_metadata entity) from a KFX file
fn report_metadata(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len() as u64;
    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Find book_metadata entity (type 490)
    let book_metadata_type = KfxSymbol::BookMetadata as u32;

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u32_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        if type_idnum != book_metadata_type {
            continue;
        }

        // Found book_metadata entity
        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);

        if let Ok(value) = parser.parse() {
            // Helper to resolve symbol name
            let resolve_sym = |id: u64| -> &str {
                if id < base_symbol_count {
                    KFX_SYMBOL_TABLE.get(id as usize).copied().unwrap_or("?")
                } else {
                    let ext_idx = (id as usize) - (base_symbol_count as usize);
                    extended_symbols
                        .get(ext_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("?")
                }
            };

            // Extract and display categorised_metadata
            if let boko::kfx::ion::IonValue::Struct(fields) = &value {
                for (field_id, field_value) in fields {
                    let field_name = resolve_sym(*field_id);

                    if field_name == "categorised_metadata" {
                        if let boko::kfx::ion::IonValue::List(categories) = field_value {
                            for category in categories {
                                if let boko::kfx::ion::IonValue::Struct(cat_fields) = category {
                                    let mut cat_name = String::new();
                                    let mut metadata_list = Vec::new();

                                    for (cid, cval) in cat_fields {
                                        let cname = resolve_sym(*cid);
                                        match cname {
                                            "category" => {
                                                if let boko::kfx::ion::IonValue::String(s) = cval {
                                                    cat_name = s.clone();
                                                }
                                            }
                                            "metadata" => {
                                                if let boko::kfx::ion::IonValue::List(items) = cval
                                                {
                                                    for item in items {
                                                        if let boko::kfx::ion::IonValue::Struct(
                                                            item_fields,
                                                        ) = item
                                                        {
                                                            let mut key = String::new();
                                                            let mut val = String::new();

                                                            for (iid, ival) in item_fields {
                                                                let iname = resolve_sym(*iid);
                                                                match iname {
                                                                    "key" => {
                                                                        if let boko::kfx::ion::IonValue::String(s) = ival {
                                                                            key = s.clone();
                                                                        }
                                                                    }
                                                                    "value" => {
                                                                        val = match ival {
                                                                            boko::kfx::ion::IonValue::String(s) => s.clone(),
                                                                            boko::kfx::ion::IonValue::Int(i) => i.to_string(),
                                                                            boko::kfx::ion::IonValue::Bool(b) => b.to_string(),
                                                                            _ => format!("{:?}", ival),
                                                                        };
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }

                                                            if !key.is_empty() {
                                                                metadata_list.push((key, val));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    // Print category header and metadata
                                    if !cat_name.is_empty() {
                                        println!("=== {} ===\n", cat_name);
                                        for (key, val) in &metadata_list {
                                            // Truncate long values
                                            let display_val = if val.len() > 60 {
                                                format!("{}...", &val[..60])
                                            } else {
                                                val.clone()
                                            };
                                            println!("{:<25} {}", format!("{}:", key), display_val);
                                        }
                                        println!();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        return Ok(());
    }

    eprintln!("No book_metadata entity found");
    Ok(())
}

/// Report reading orders from a KFX file
fn report_reading_orders(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len() as u64;
    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Find metadata entity (type 258) which contains reading_orders
    let metadata_type = KfxSymbol::Metadata as u32;

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u32_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        if type_idnum != metadata_type {
            continue;
        }

        // Found metadata entity
        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);

        if let Ok(value) = parser.parse() {
            // Helper to resolve symbol name
            let resolve_sym = |id: u64| -> &str {
                if id < base_symbol_count {
                    KFX_SYMBOL_TABLE.get(id as usize).copied().unwrap_or("?")
                } else {
                    let ext_idx = (id as usize) - (base_symbol_count as usize);
                    extended_symbols
                        .get(ext_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("?")
                }
            };

            println!("=== Reading Orders ===\n");

            // Extract reading_orders field
            if let boko::kfx::ion::IonValue::Struct(fields) = &value {
                for (field_id, field_value) in fields {
                    let field_name = resolve_sym(*field_id);

                    if field_name == "reading_orders" {
                        if let boko::kfx::ion::IonValue::List(orders) = field_value {
                            for (idx, order) in orders.iter().enumerate() {
                                if let boko::kfx::ion::IonValue::Struct(order_fields) = order {
                                    let mut order_name = String::new();
                                    let mut sections: Vec<String> = Vec::new();

                                    for (oid, oval) in order_fields {
                                        let oname = resolve_sym(*oid);
                                        match oname {
                                            "reading_order_name" => {
                                                if let boko::kfx::ion::IonValue::Symbol(s) = oval {
                                                    order_name = resolve_sym(*s).to_string();
                                                } else if let boko::kfx::ion::IonValue::String(s) =
                                                    oval
                                                {
                                                    order_name = s.clone();
                                                }
                                            }
                                            "sections" => {
                                                if let boko::kfx::ion::IonValue::List(secs) = oval {
                                                    for sec in secs {
                                                        if let boko::kfx::ion::IonValue::Symbol(s) =
                                                            sec
                                                        {
                                                            sections
                                                                .push(resolve_sym(*s).to_string());
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    println!(
                                        "{}. {} ({} sections)",
                                        idx + 1,
                                        order_name,
                                        sections.len()
                                    );
                                    for (sidx, sec) in sections.iter().enumerate() {
                                        println!("   {:3}. {}", sidx + 1, sec);
                                    }
                                    println!();
                                }
                            }
                        }
                    }
                }
            }
        }

        return Ok(());
    }

    eprintln!("No metadata entity found");
    Ok(())
}

/// Report document data from a KFX file
fn report_document(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len() as u64;
    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Find document_data entity (type 538)
    let document_data_type = KfxSymbol::DocumentData as u32;

    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u32_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        if type_idnum != document_data_type {
            continue;
        }

        // Found document_data entity
        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);

        if let Ok(value) = parser.parse() {
            // Helper to resolve symbol name
            let resolve_sym = |id: u64| -> String {
                if id < base_symbol_count {
                    KFX_SYMBOL_TABLE
                        .get(id as usize)
                        .copied()
                        .unwrap_or("?")
                        .to_string()
                } else {
                    let ext_idx = (id as usize) - (base_symbol_count as usize);
                    extended_symbols
                        .get(ext_idx)
                        .cloned()
                        .unwrap_or_else(|| "?".to_string())
                }
            };

            println!("=== Document Data ===\n");

            // Extract and display document properties
            if let boko::kfx::ion::IonValue::Struct(fields) = &value {
                for (field_id, field_value) in fields {
                    let field_name = resolve_sym(*field_id);

                    // Skip reading_orders (already has its own report)
                    if field_name == "reading_orders" {
                        continue;
                    }

                    let value_str = format_ion_value_simple(field_value, &resolve_sym);
                    println!("{:<25} {}", format!("{}:", field_name), value_str);
                }
            }
        }

        return Ok(());
    }

    eprintln!("No document_data entity found");
    Ok(())
}

/// Format an IonValue simply for display
fn format_ion_value_simple<F>(value: &boko::kfx::ion::IonValue, resolve_sym: &F) -> String
where
    F: Fn(u64) -> String,
{
    use boko::kfx::ion::IonValue;
    match value {
        IonValue::Null => "null".to_string(),
        IonValue::Bool(b) => b.to_string(),
        IonValue::Int(i) => i.to_string(),
        IonValue::Float(f) => format!("{}", f),
        IonValue::Decimal(d) => d.clone(),
        IonValue::String(s) => format!("\"{}\"", s),
        IonValue::Symbol(s) => resolve_sym(*s).to_string(),
        IonValue::Blob(b) => format!("<blob {} bytes>", b.len()),
        IonValue::List(items) => {
            let parts: Vec<String> = items
                .iter()
                .take(5)
                .map(|v| format_ion_value_simple(v, resolve_sym))
                .collect();
            if items.len() > 5 {
                format!("[{}, ... ({} more)]", parts.join(", "), items.len() - 5)
            } else {
                format!("[{}]", parts.join(", "))
            }
        }
        IonValue::Struct(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .take(3)
                .map(|(k, v)| {
                    format!("{}: {}", resolve_sym(*k), format_ion_value_simple(v, resolve_sym))
                })
                .collect();
            if fields.len() > 3 {
                format!("{{ {}, ... }}", parts.join(", "))
            } else {
                format!("{{ {} }}", parts.join(", "))
            }
        }
        IonValue::Annotated(_, inner) => format_ion_value_simple(inner, resolve_sym),
    }
}

/// Report sections from a KFX file
fn report_sections(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len() as u64;
    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Find all section entities (type 260)
    let section_type = KfxSymbol::Section as u32;

    println!("=== Sections ===\n");

    let mut section_count = 0;
    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u32_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        if type_idnum != section_type {
            continue;
        }

        // Found section entity
        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);

        if let Ok(value) = parser.parse() {
            let resolve_sym = |id: u64| -> String {
                if id < base_symbol_count {
                    KFX_SYMBOL_TABLE
                        .get(id as usize)
                        .copied()
                        .unwrap_or("?")
                        .to_string()
                } else {
                    let ext_idx = (id as usize) - (base_symbol_count as usize);
                    extended_symbols
                        .get(ext_idx)
                        .cloned()
                        .unwrap_or_else(|| "?".to_string())
                }
            };

            // Extract section info
            if let boko::kfx::ion::IonValue::Struct(fields) = &value {
                let mut section_name = String::new();
                let mut templates: Vec<String> = Vec::new();

                for (field_id, field_value) in fields {
                    let field_name = resolve_sym(*field_id);
                    match field_name.as_str() {
                        "section_name" => {
                            if let boko::kfx::ion::IonValue::Symbol(s) = field_value {
                                section_name = resolve_sym(*s);
                            }
                        }
                        "page_templates" => {
                            if let boko::kfx::ion::IonValue::List(tpls) = field_value {
                                for tpl in tpls {
                                    if let boko::kfx::ion::IonValue::Struct(tpl_fields) = tpl {
                                        let mut tpl_type = String::new();
                                        let mut story_name = String::new();
                                        let mut dims = String::new();

                                        for (tid, tval) in tpl_fields {
                                            let tname = resolve_sym(*tid);
                                            match tname.as_str() {
                                                "type" => {
                                                    if let boko::kfx::ion::IonValue::Symbol(s) =
                                                        tval
                                                    {
                                                        tpl_type = resolve_sym(*s);
                                                    }
                                                }
                                                "story_name" => {
                                                    if let boko::kfx::ion::IonValue::Symbol(s) =
                                                        tval
                                                    {
                                                        story_name = resolve_sym(*s);
                                                    }
                                                }
                                                "fixed_width" => {
                                                    if let boko::kfx::ion::IonValue::Int(w) = tval {
                                                        dims = format!("{}x", w);
                                                    }
                                                }
                                                "fixed_height" => {
                                                    if let boko::kfx::ion::IonValue::Int(h) = tval {
                                                        dims = format!("{}{}", dims, h);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }

                                        let tpl_desc = if !dims.is_empty() {
                                            format!("{} ({}, {})", story_name, tpl_type, dims)
                                        } else {
                                            format!("{} ({})", story_name, tpl_type)
                                        };
                                        templates.push(tpl_desc);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if !section_name.is_empty() {
                    section_count += 1;
                    let templates_str = if templates.is_empty() {
                        String::new()
                    } else {
                        format!(" → {}", templates.join(", "))
                    };
                    println!("{:<15}{}", section_name, templates_str);
                }
            }
        }
    }

    println!("\nTotal sections: {}", section_count);
    Ok(())
}

/// Report external resources from a KFX file
fn report_resources(data: &[u8]) -> IonResult<()> {
    use boko::kfx::ion::IonParser;
    use boko::kfx::symbols::KfxSymbol;

    if data.len() < 18 || &data[0..4] != b"CONT" {
        eprintln!("Not a KFX container");
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

    if container_info_offset + container_info_length > data.len() {
        eprintln!("Container info out of bounds");
        return Ok(());
    }

    let container_info_data =
        &data[container_info_offset..container_info_offset + container_info_length];

    // Get index table location
    let Some((index_offset, index_length)) = parse_container_info_for_index(container_info_data)
    else {
        eprintln!("Could not find index table");
        return Ok(());
    };

    // Extract extended symbols
    let extended_symbols = if let Some((doc_sym_offset, doc_sym_length)) =
        parse_container_info_for_doc_symbols(container_info_data)
    {
        if doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let base_symbol_count = KFX_SYMBOL_TABLE.len() as u64;
    let entry_size = 24;
    let num_entries = index_length / entry_size;

    // Find all external_resource entities (type 164)
    let resource_type = KfxSymbol::ExternalResource as u32;

    println!("=== External Resources ===\n");

    let mut resource_count = 0;
    for i in 0..num_entries {
        let entry_offset = index_offset + i * entry_size;
        if entry_offset + entry_size > data.len() {
            break;
        }

        let Some(type_idnum) = read_u32_le(data, entry_offset + 4) else {
            continue;
        };
        let Some(entity_offset) = read_u64_le(data, entry_offset + 8).map(|v| v as usize) else {
            continue;
        };
        let Some(entity_len) = read_u32_le(data, entry_offset + 16).map(|v| v as usize) else {
            continue;
        };

        if type_idnum != resource_type {
            continue;
        }

        // Found external_resource entity
        let abs_offset = header_len + entity_offset;
        if abs_offset + entity_len > data.len() {
            continue;
        }

        let entity_data = &data[abs_offset..abs_offset + entity_len];
        if entity_data.len() < 10 || &entity_data[0..4] != b"ENTY" {
            continue;
        }

        let Some(entity_header_len) = read_u32_le(entity_data, 6).map(|v| v as usize) else {
            continue;
        };
        if entity_header_len >= entity_data.len() {
            continue;
        }

        let ion_data = &entity_data[entity_header_len..];
        let mut parser = IonParser::new(ion_data);

        if let Ok(value) = parser.parse() {
            let resolve_sym = |id: u64| -> String {
                if id < base_symbol_count {
                    KFX_SYMBOL_TABLE
                        .get(id as usize)
                        .copied()
                        .unwrap_or("?")
                        .to_string()
                } else {
                    let ext_idx = (id as usize) - (base_symbol_count as usize);
                    extended_symbols
                        .get(ext_idx)
                        .cloned()
                        .unwrap_or_else(|| "?".to_string())
                }
            };

            // Extract resource info
            if let boko::kfx::ion::IonValue::Struct(fields) = &value {
                let mut resource_name = String::new();
                let mut format = String::new();
                let mut location = String::new();
                let mut width: Option<i64> = None;
                let mut height: Option<i64> = None;

                for (field_id, field_value) in fields {
                    let field_name = resolve_sym(*field_id);
                    match field_name.as_str() {
                        "resource_name" => {
                            if let boko::kfx::ion::IonValue::Symbol(s) = field_value {
                                resource_name = resolve_sym(*s);
                            }
                        }
                        "format" => {
                            if let boko::kfx::ion::IonValue::Symbol(s) = field_value {
                                format = resolve_sym(*s);
                            }
                        }
                        "location" => {
                            if let boko::kfx::ion::IonValue::String(s) = field_value {
                                location = s.clone();
                            }
                        }
                        "resource_width" => {
                            if let boko::kfx::ion::IonValue::Int(w) = field_value {
                                width = Some(*w);
                            }
                        }
                        "resource_height" => {
                            if let boko::kfx::ion::IonValue::Int(h) = field_value {
                                height = Some(*h);
                            }
                        }
                        _ => {}
                    }
                }

                if !resource_name.is_empty() {
                    resource_count += 1;
                    let dims = match (width, height) {
                        (Some(w), Some(h)) => format!(" {}x{}", w, h),
                        _ => String::new(),
                    };
                    println!(
                        "{:<10} {:<6}{} → {}",
                        resource_name, format, dims, location
                    );
                }
            }
        }
    }

    println!("\nTotal resources: {}", resource_count);
    Ok(())
}
