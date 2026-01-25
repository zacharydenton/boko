//! boko - Fast ebook converter

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use boko::{Book, ChapterId, Format, IRChapter, NodeId, Role, ToCss, TocEntry};

#[derive(Parser)]
#[command(name = "boko")]
#[command(version, about = "Fast ebook converter", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show book metadata and structure
    Info {
        /// Input file (EPUB, AZW3, or MOBI)
        file: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Convert between ebook formats
    Convert {
        /// Input file
        input: String,

        /// Output file
        output: String,

        /// Suppress output messages
        #[arg(short, long)]
        quiet: bool,
    },

    /// Dump the IR (Intermediate Representation) for a book
    Dump {
        /// Input file (EPUB, AZW3, or MOBI)
        file: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Show structure only (hide text content)
        #[arg(short, long)]
        structure: bool,

        /// Hide style information
        #[arg(long)]
        no_styles: bool,

        /// Only dump a specific chapter by ID
        #[arg(short, long)]
        chapter: Option<u32>,

        /// Only dump the style pool
        #[arg(long)]
        styles_only: bool,

        /// Limit tree traversal depth
        #[arg(short, long)]
        depth: Option<usize>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Info { file, json } => show_info(&file, json),
        Command::Convert { input, output, quiet } => convert(&input, &output, quiet),
        Command::Dump {
            file,
            json,
            structure,
            no_styles,
            chapter,
            styles_only,
            depth,
        } => dump_ir(
            &file,
            DumpOptions {
                json,
                structure,
                no_styles,
                chapter,
                styles_only,
                depth,
            },
        ),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// JSON output structures
#[derive(Serialize)]
struct BookInfo {
    file: String,
    metadata: MetadataInfo,
    spine: Vec<SpineInfo>,
    toc: Vec<TocInfo>,
    assets: Vec<String>,
}

#[derive(Serialize)]
struct MetadataInfo {
    title: String,
    authors: Vec<String>,
    language: String,
    identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subjects: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rights: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Serialize)]
struct SpineInfo {
    id: u32,
    path: String,
    size: usize,
}

#[derive(Serialize)]
struct TocInfo {
    title: String,
    href: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<TocInfo>,
}

fn show_info(path: &str, json: bool) -> Result<(), String> {
    let book = Book::open(path).map_err(|e| e.to_string())?;

    if json {
        print_json(&book, path)
    } else {
        print_human(&book, path)
    }
}

fn print_json(book: &Book, path: &str) -> Result<(), String> {
    let meta = book.metadata();

    let info = BookInfo {
        file: path.to_string(),
        metadata: MetadataInfo {
            title: meta.title.clone(),
            authors: meta.authors.clone(),
            language: meta.language.clone(),
            identifier: meta.identifier.clone(),
            publisher: meta.publisher.clone(),
            date: meta.date.clone(),
            subjects: meta.subjects.clone(),
            rights: meta.rights.clone(),
            cover_image: meta.cover_image.clone(),
            description: meta.description.clone(),
        },
        spine: book
            .spine()
            .iter()
            .map(|e| SpineInfo {
                id: e.id.0,
                path: book.source_id(e.id).unwrap_or("").to_string(),
                size: e.size_estimate,
            })
            .collect(),
        toc: book.toc().iter().map(toc_to_info).collect(),
        assets: book
            .list_assets()
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    };

    let json = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn toc_to_info(entry: &TocEntry) -> TocInfo {
    TocInfo {
        title: entry.title.clone(),
        href: entry.href.clone(),
        children: entry.children.iter().map(toc_to_info).collect(),
    }
}

fn print_human(book: &Book, path: &str) -> Result<(), String> {
    let meta = book.metadata();
    println!("File: {path}");
    println!("Title: {}", meta.title);
    if !meta.authors.is_empty() {
        println!("Authors: {}", meta.authors.join(", "));
    }
    if !meta.language.is_empty() {
        println!("Language: {}", meta.language);
    }
    if !meta.identifier.is_empty() {
        println!("Identifier: {}", meta.identifier);
    }
    if let Some(ref publisher) = meta.publisher {
        println!("Publisher: {publisher}");
    }
    if let Some(ref date) = meta.date {
        println!("Date: {date}");
    }
    if !meta.subjects.is_empty() {
        println!("Subjects: {}", meta.subjects.join(", "));
    }
    if let Some(ref rights) = meta.rights {
        println!("Rights: {rights}");
    }
    if let Some(ref cover) = meta.cover_image {
        println!("Cover: {cover}");
    }
    if let Some(ref desc) = meta.description {
        let desc = desc.trim();
        if desc.len() > 200 {
            println!("Description: {}...", &desc[..200]);
        } else {
            println!("Description: {desc}");
        }
    }

    // Spine (chapters)
    println!("\nSpine ({} chapters):", book.spine().len());
    for entry in book.spine() {
        let source = book.source_id(entry.id).unwrap_or("?");
        println!("  [{}] {} ({} bytes)", entry.id.0, source, entry.size_estimate);
    }

    // Table of contents
    println!("\nTable of Contents ({} entries):", book.toc().len());
    print_toc_human(book.toc(), 1);

    // Assets
    let assets = book.list_assets();
    println!("\nAssets ({}):", assets.len());
    for asset in &assets {
        println!("  {}", asset.display());
    }

    Ok(())
}

fn print_toc_human(entries: &[TocEntry], depth: usize) {
    for entry in entries {
        let indent = "  ".repeat(depth);
        println!("{}{} -> {}", indent, entry.title, entry.href);
        if !entry.children.is_empty() {
            print_toc_human(&entry.children, depth + 1);
        }
    }
}

fn convert(input: &str, output: &str, quiet: bool) -> Result<(), String> {
    let output_format = Format::from_path(output).ok_or_else(|| {
        format!(
            "Unknown output format: {}. Supported: .epub, .azw3",
            output
        )
    })?;

    if output_format == Format::Mobi {
        return Err("MOBI output is not supported; use .azw3 instead".to_string());
    }

    if !quiet {
        eprintln!("Converting {} -> {}", input, output);
    }

    let mut book = Book::open(input).map_err(|e| format!("Failed to open input: {e}"))?;

    let mut file =
        std::fs::File::create(output).map_err(|e| format!("Failed to create output: {e}"))?;

    book.export(output_format, &mut file)
        .map_err(|e| format!("Conversion failed: {e}"))?;

    if !quiet {
        eprintln!("Done.");
    }

    Ok(())
}

// ----------------------------------------------------------------------------
// Dump command
// ----------------------------------------------------------------------------

struct DumpOptions {
    json: bool,
    structure: bool,
    no_styles: bool,
    chapter: Option<u32>,
    styles_only: bool,
    depth: Option<usize>,
}

fn dump_ir(path: &str, opts: DumpOptions) -> Result<(), String> {
    let mut book = Book::open(path).map_err(|e| e.to_string())?;

    if opts.json {
        dump_ir_json(&mut book, path, &opts)
    } else {
        dump_ir_tree(&mut book, path, &opts)
    }
}

// JSON output structures for dump command
#[derive(Serialize)]
struct DumpInfo {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<Vec<StyleInfo>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    chapters: Vec<ChapterDump>,
}

#[derive(Serialize)]
struct StyleInfo {
    id: u32,
    css: String,
}

#[derive(Serialize)]
struct ChapterDump {
    id: u32,
    path: String,
    node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tree: Option<NodeDump>,
}

#[derive(Serialize)]
struct NodeDump {
    id: u32,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<NodeDump>,
}

fn dump_ir_json(book: &mut Book, path: &str, opts: &DumpOptions) -> Result<(), String> {
    let mut info = DumpInfo {
        file: path.to_string(),
        styles: None,
        chapters: Vec::new(),
    };

    // If styles_only, just dump the style pool from the first chapter
    if opts.styles_only {
        let chapter_id = opts.chapter.unwrap_or(0);
        let chapter = book
            .load_chapter(ChapterId(chapter_id))
            .map_err(|e| e.to_string())?;
        info.styles = Some(collect_styles(&chapter));
        let json = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
        println!("{json}");
        return Ok(());
    }

    // Collect chapters to dump
    let chapter_ids: Vec<(ChapterId, String)> = if let Some(id) = opts.chapter {
        let source = book.source_id(ChapterId(id)).unwrap_or("").to_string();
        vec![(ChapterId(id), source)]
    } else {
        book.spine()
            .iter()
            .map(|e| {
                let source = book.source_id(e.id).unwrap_or("").to_string();
                (e.id, source)
            })
            .collect()
    };

    for (id, source_path) in chapter_ids {
        let chapter = book.load_chapter(id).map_err(|e| e.to_string())?;

        let tree = if !opts.styles_only {
            Some(dump_node_json(&chapter, NodeId::ROOT, opts, 0))
        } else {
            None
        };

        info.chapters.push(ChapterDump {
            id: id.0,
            path: source_path,
            node_count: chapter.node_count(),
            tree,
        });
    }

    let json = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn dump_node_json(chapter: &IRChapter, id: NodeId, opts: &DumpOptions, depth: usize) -> NodeDump {
    let node = chapter.node(id).unwrap();

    let text = if !opts.structure && node.role == Role::Text && !node.text.is_empty() {
        let content = chapter.text(node.text);
        Some(truncate_text(content, 100))
    } else {
        None
    };

    let style_id = if !opts.no_styles && node.style.0 != 0 {
        Some(node.style.0)
    } else {
        None
    };

    // Collect children
    let children: Vec<NodeDump> = if opts.depth.is_none() || depth < opts.depth.unwrap() {
        chapter
            .children(id)
            .map(|child_id| dump_node_json(chapter, child_id, opts, depth + 1))
            .collect()
    } else {
        Vec::new()
    };

    NodeDump {
        id: id.0,
        role: role_to_string(node.role),
        text,
        style_id,
        href: chapter.semantics.href(id).map(String::from),
        src: chapter.semantics.src(id).map(String::from),
        alt: chapter.semantics.alt(id).map(String::from),
        anchor_id: chapter.semantics.id(id).map(String::from),
        children,
    }
}

fn dump_ir_tree(book: &mut Book, path: &str, opts: &DumpOptions) -> Result<(), String> {
    println!("File: {path}");
    println!();

    // If styles_only, just dump the style pool
    if opts.styles_only {
        let chapter_id = opts.chapter.unwrap_or(0);
        let chapter = book
            .load_chapter(ChapterId(chapter_id))
            .map_err(|e| e.to_string())?;
        println!("Style Pool ({} styles):", chapter.styles.len());
        for (id, style) in chapter.styles.iter() {
            let css = style.to_css_string();
            if css.is_empty() {
                println!("  [{}] (default)", id.0);
            } else {
                println!("  [{}] {}", id.0, css);
            }
        }
        return Ok(());
    }

    // Collect chapters to dump
    let chapter_ids: Vec<(ChapterId, String)> = if let Some(id) = opts.chapter {
        let source = book.source_id(ChapterId(id)).unwrap_or("").to_string();
        vec![(ChapterId(id), source)]
    } else {
        book.spine()
            .iter()
            .map(|e| {
                let source = book.source_id(e.id).unwrap_or("").to_string();
                (e.id, source)
            })
            .collect()
    };

    for (idx, (id, source_path)) in chapter_ids.iter().enumerate() {
        let chapter = book.load_chapter(*id).map_err(|e| e.to_string())?;

        if idx > 0 {
            println!();
        }
        println!(
            "Chapter {} [{}] ({} nodes)",
            id.0,
            source_path,
            chapter.node_count()
        );

        if !opts.no_styles {
            println!("  Styles: {} unique", chapter.styles.len());
        }

        println!();
        dump_node_tree(&chapter, NodeId::ROOT, opts, 0);
    }

    Ok(())
}

fn dump_node_tree(chapter: &IRChapter, id: NodeId, opts: &DumpOptions, depth: usize) {
    // Check depth limit
    if let Some(max_depth) = opts.depth {
        if depth > max_depth {
            return;
        }
    }

    let node = chapter.node(id).unwrap();
    let indent = "  ".repeat(depth);

    // Build the node display line
    let mut line = format!("{}{}", indent, role_to_string(node.role));

    // Add style if not hidden and not default
    if !opts.no_styles && node.style.0 != 0 {
        line.push_str(&format!(" [s{}]", node.style.0));
    }

    // Add semantic attributes
    if let Some(href) = chapter.semantics.href(id) {
        line.push_str(&format!(" href=\"{}\"", truncate_text(href, 40)));
    }
    if let Some(src) = chapter.semantics.src(id) {
        line.push_str(&format!(" src=\"{}\"", truncate_text(src, 40)));
    }
    if let Some(alt) = chapter.semantics.alt(id) {
        line.push_str(&format!(" alt=\"{}\"", truncate_text(alt, 30)));
    }
    if let Some(anchor_id) = chapter.semantics.id(id) {
        line.push_str(&format!(" id=\"{}\"", anchor_id));
    }

    // Add text content for text nodes
    if !opts.structure && node.role == Role::Text && !node.text.is_empty() {
        let text = chapter.text(node.text);
        line.push_str(&format!(": \"{}\"", truncate_text(text, 60)));
    }

    println!("{line}");

    // Recurse into children
    for child_id in chapter.children(id) {
        dump_node_tree(chapter, child_id, opts, depth + 1);
    }
}

fn collect_styles(chapter: &IRChapter) -> Vec<StyleInfo> {
    chapter
        .styles
        .iter()
        .map(|(id, style)| StyleInfo {
            id: id.0,
            css: style.to_css_string(),
        })
        .collect()
}

fn role_to_string(role: Role) -> String {
    match role {
        Role::Block => "Block".to_string(),
        Role::Paragraph => "Paragraph".to_string(),
        Role::Heading(level) => format!("Heading({})", level),
        Role::Span => "Span".to_string(),
        Role::Link => "Link".to_string(),
        Role::Image => "Image".to_string(),
        Role::Emphasis => "Emphasis".to_string(),
        Role::Strong => "Strong".to_string(),
        Role::Code => "Code".to_string(),
        Role::BlockQuote => "BlockQuote".to_string(),
        Role::List { ordered: true } => "List(ordered)".to_string(),
        Role::List { ordered: false } => "List(unordered)".to_string(),
        Role::ListItem => "ListItem".to_string(),
        Role::Table => "Table".to_string(),
        Role::TableRow => "TableRow".to_string(),
        Role::TableCell { header: true } => "TableCell(header)".to_string(),
        Role::TableCell { header: false } => "TableCell".to_string(),
        Role::Preformatted => "Preformatted".to_string(),
        Role::LineBreak => "LineBreak".to_string(),
        Role::HorizontalRule => "HorizontalRule".to_string(),
        Role::Text => "Text".to_string(),
        Role::Root => "Root".to_string(),
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    // Normalize whitespace
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");

    // Count characters (not bytes) to handle multi-byte UTF-8 correctly
    let char_count = normalized.chars().count();
    if char_count <= max_chars {
        normalized
    } else {
        let truncated: String = normalized.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}
