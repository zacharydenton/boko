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
    if let Some(ref publisher) = meta.publisher {
        println!("Publisher: {publisher}");
    }
    if let Some(ref desc) = meta.description {
        let desc = desc.trim();
        if desc.len() > 200 {
            println!("Description: {}...", &desc[..200]);
        } else {
            println!("Description: {desc}");
        }
    }
    println!("Chapters: {}", book.spine().len());
    println!("TOC entries: {}", book.toc().len());
    println!("Assets: {}", book.list_assets().len());

    Ok(())
}

fn convert(_input: &str, _output: &str, _quiet: bool) -> Result<(), String> {
    // TODO: Implement writers for new architecture
    Err("Conversion not yet implemented in new architecture".to_string())
}
