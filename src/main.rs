//! boko - Fast ebook converter

use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use boko::{read_epub, read_mobi, write_epub, write_mobi};

#[derive(Parser)]
#[command(name = "boko")]
#[command(version, about = "Fast ebook converter", long_about = None)]
#[command(after_help = "EXAMPLES:
    boko book.epub book.azw3    Convert EPUB to AZW3
    boko book.azw3 book.epub    Convert AZW3 to EPUB
    boko -i book.epub           Show book metadata")]
struct Cli {
    /// Input file (EPUB, AZW3, MOBI, or KFX)
    #[arg(value_name = "INPUT")]
    input: String,

    /// Output file (EPUB, AZW3, or KFX)
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

fn get_format(path: &str) -> Option<&'static str> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .and_then(|ext| match ext.as_str() {
            "epub" => Some("epub"),
            "azw3" | "mobi" => Some("mobi"),
            _ => None,
        })
}

fn show_info(path: &str) -> Result<(), String> {
    let format = get_format(path).ok_or_else(|| format!("unsupported format: {path}"))?;

    let book = match format {
        "epub" => read_epub(path).map_err(|e| e.to_string())?,
        "mobi" => read_mobi(path).map_err(|e| e.to_string())?,
        _ => unreachable!(),
    };

    println!("File: {path}");
    println!("Title: {}", book.metadata.title);
    if !book.metadata.authors.is_empty() {
        println!("Authors: {}", book.metadata.authors.join(", "));
    }
    if !book.metadata.language.is_empty() {
        println!("Language: {}", book.metadata.language);
    }
    if let Some(ref publisher) = book.metadata.publisher {
        println!("Publisher: {publisher}");
    }
    if let Some(ref desc) = book.metadata.description {
        let desc = desc.trim();
        if desc.len() > 200 {
            println!("Description: {}...", &desc[..200]);
        } else {
            println!("Description: {desc}");
        }
    }
    println!("Chapters: {}", book.spine.len());
    println!("TOC entries: {}", book.toc.len());
    println!("Resources: {}", book.resources.len());

    Ok(())
}

fn convert(input: &str, output: &str, quiet: bool) -> Result<(), String> {
    let input_format =
        get_format(input).ok_or_else(|| format!("unsupported input format: {input}"))?;
    let output_format =
        get_format(output).ok_or_else(|| format!("unsupported output format: {output}"))?;

    if !quiet {
        eprintln!("Reading {input}...");
    }

    let book = match input_format {
        "epub" => read_epub(input).map_err(|e| e.to_string())?,
        "mobi" => read_mobi(input).map_err(|e| e.to_string())?,
        _ => unreachable!(),
    };

    if !quiet {
        eprintln!("Writing {output}...");
    }

    match output_format {
        "epub" => write_epub(&book, output).map_err(|e| e.to_string())?,
        "mobi" => write_mobi(&book, output).map_err(|e| e.to_string())?,
        _ => unreachable!(),
    }

    if !quiet {
        eprintln!("Done.");
    }

    Ok(())
}
