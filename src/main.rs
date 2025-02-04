use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

mod chunker;
mod context;
mod embedder;
mod indexer;
mod query;
mod store;

use context::AppContext;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Index the current directory or specified path
    Index {
        /// Directory path to index (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Query the indexed codebase (default command)
    Query {
        /// Search query
        query: String,
    },
}

#[derive(Serialize)]
struct FileChunks {
    file_info: indexer::FileInfo,
    chunks: Vec<chunker::CodeChunk>,
}

async fn index_codebase(ctx: Arc<AppContext>, path: PathBuf) -> Result<()> {
    println!("Indexing codebase at: {}", path.display());

    let indexer = indexer::Indexer::new();
    let mut chunker = chunker::Chunker::new()?;
    let files = indexer.index_directory(&path)?;
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

async fn query_codebase(ctx: Arc<AppContext>, query: String) -> Result<()> {
    println!("Searching for: {}", query);
    // TODO: Implement actual query functionality
    println!("Query mode not yet implemented");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let context = Arc::new(AppContext::new()?);

    match cli.command {
        Some(Commands::Index { path }) => {
            let index_path = path
                .map(PathBuf::from)
                .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));
            index_codebase(Arc::clone(&context), index_path).await?;
        }
        Some(Commands::Query { query }) => {
            query_codebase(Arc::clone(&context), query).await?;
        }
        None => {
            // Default to query mode if no subcommand is provided
            println!("No command specified. Use --help to see available commands.");
            println!("Example usage:");
            println!("  Index: ragrep index [--path <dir>]");
            println!("  Query: ragrep query <search-term>");
        }
    }

    Ok(())
}
