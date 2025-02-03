use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

mod chunker;
mod embedder;
mod indexer;
mod query;
mod store;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Directory path to index
    #[arg(short, long)]
    path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = PathBuf::from(cli.path);

    let indexer = indexer::Indexer::new();

    let files = indexer.index_directory(&path)?;
    let mut chunker = chunker::Chunker::new()?;

    #[derive(Serialize)]
    struct FileChunks {
        file_info: indexer::FileInfo,
        chunks: Vec<chunker::CodeChunk>,
    }

    let mut all_chunks: HashMap<String, FileChunks> = HashMap::new();

    for file in files {
        println!("Processing: {}", file.path.display());

        let content = std::fs::read_to_string(&file.path)
            .with_context(|| format!("Failed to read file: {}", file.path.display()))?;

        let chunks = chunker.chunk_file(&file.path, &content)?;

        if !chunks.is_empty() {
            let path_str = file.path.to_string_lossy().to_string();
            all_chunks.insert(
                path_str,
                FileChunks {
                    file_info: file,
                    chunks,
                },
            );
        }
    }

    // Create output directory if it doesn't exist
    fs::create_dir_all(".ragrep")?;

    // Write chunks to JSON file
    let json = serde_json::to_string_pretty(&all_chunks)?;
    fs::write(".ragrep/chunks.json", json)?;

    println!("\nChunks written to .ragrep/chunks.json");

    Ok(())
}
