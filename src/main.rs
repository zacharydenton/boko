//! boko - Fast ebook converter

use std::process::ExitCode;

use clap::Parser;

use boko::Book;

#[derive(Parser)]
#[command(name = "boko")]
#[command(version, about = "Fast ebook converter", long_about = None)]
#[command(after_help = "EXAMPLES:
    boko book.epub book.azw3    Convert EPUB to AZW3
    boko book.azw3 book.epub    Convert AZW3 to EPUB
    boko -i book.epub           Show book metadata")]
struct Cli {
    /// Input file (EPUB, AZW3, or MOBI)
    #[arg(value_name = "INPUT")]
    input: String,

    /// Output file (EPUB, AZW3)
    #[arg(value_name = "OUTPUT", required_unless_present = "info")]
    output: Option<String>,

    /// Show book metadata without converting
    #[arg(short, long)]
    info: bool,

    /// Suppress output messages
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.info {
        match show_info(&cli.input) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        let output = cli.output.expect("output required");
        match convert(&cli.input, &output, cli.quiet) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

fn show_info(path: &str) -> Result<(), String> {
    let book = Book::open(path).map_err(|e| e.to_string())?;

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
    print_toc(book.toc(), 1);

    // Assets
    let assets = book.list_assets();
    println!("\nAssets ({}):", assets.len());
    for asset in &assets {
        println!("  {}", asset.display());
    }

    Ok(())
}

fn print_toc(entries: &[boko::TocEntry], depth: usize) {
    for entry in entries {
        let indent = "  ".repeat(depth);
        println!("{}{} -> {}", indent, entry.title, entry.href);
        if !entry.children.is_empty() {
            print_toc(&entry.children, depth + 1);
        }
    }
}

fn convert(_input: &str, _output: &str, _quiet: bool) -> Result<(), String> {
    // TODO: Implement writers for new architecture
    Err("Conversion not yet implemented in new architecture".to_string())
}
