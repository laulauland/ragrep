use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use colored::*;
use serde::Serialize;
use std::path::PathBuf;

mod chunker;
mod config;
mod context;
mod db;
mod embedder;
mod indexer;

use context::AppContext;
use embedder::Embedding;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Search query (default command)
    query: Option<String>,

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
}

#[derive(Serialize)]
struct FileChunks {
    file_info: indexer::FileInfo,
    chunks: Vec<chunker::CodeChunk>,
}

async fn index_codebase(ctx: &mut AppContext, path: PathBuf) -> Result<()> {
    println!("Initializing ragrep...");
    println!(
        "Global config: {}",
        ctx.config_manager.global_config_path.display()
    );
    if let Some(local_path) = &ctx.config_manager.local_config_path {
        println!("Local config: {}", local_path.display());
    }
    println!("Database: {}", ctx.ragrep_dir.join("ragrep.db").display());
    let model_cache_dir = ctx.config_manager.get_model_cache_dir()?;
    println!("Model cache: {}", model_cache_dir.display());
    println!("\nIndexing codebase at: {}", path.display());

    let indexer = indexer::Indexer::new();
    let mut chunker = chunker::Chunker::new()?;
    let files = indexer.index_directory(&path)?;
    let mut total_chunks = 0;
    let mut processed_chunks = 0;

    for file in files {
        println!("Processing: {}", file.path.display());

        let content = std::fs::read_to_string(&file.path)
            .with_context(|| format!("Failed to read file: {}", file.path.display()))?;

        let chunks = chunker.chunk_file(&file.path, &content)?;
        total_chunks += chunks.len();

        if !chunks.is_empty() {
            let file_path = file.path.to_string_lossy().to_string();

            // Process chunks and store in database
            for (chunk_index, chunk) in chunks.iter().enumerate() {
                // Generate embedding for the chunk
                let Embedding(embedding) =
                    ctx.embedder.embed_text(&chunk.content, &file_path).await?;

                // Create longer-lived bindings for the values
                let chunk_idx = chunk_index as i32;
                let start_line = chunk.start_byte as i32;
                let end_line = chunk.end_byte as i32;

                // Store chunk and embedding in database
                ctx.db.save_chunk(
                    &file_path,
                    chunk_idx,
                    &chunk.kind,
                    chunk.parent_name.as_deref(),
                    start_line,
                    end_line,
                    &chunk.content,
                    &embedding,
                )?;

                processed_chunks += 1;
                print!(
                    "\rProcessed {}/{} chunks...",
                    processed_chunks, total_chunks
                );
            }
        }
    }

    println!("\nIndexing complete! {} chunks processed", processed_chunks);
    println!("Database: {}", ctx.ragrep_dir.join("ragrep.db").display());

    Ok(())
}

async fn query_codebase(ctx: &mut AppContext, query: String) -> Result<()> {
    println!("Searching for: {}", query.bright_green().bold());

    let Embedding(query_embedding) = ctx.embedder.embed_query(&query).await?;
    let results = ctx.db.find_similar_chunks(&query_embedding, 10)?;

    if results.is_empty() {
        println!("No similar code found");
    } else {
        for (text, file_path, start_line, end_line, _node_type, distance) in results {
            // Print filename and location in ripgrep style (colored)
            print!("{}", file_path.bright_purple().bold());
            print!(":");
            print!("{}", start_line.to_string().cyan());
            print!(":");
            println!("{}", end_line.to_string().cyan());

            // Print the actual code content
            for (i, line) in text.lines().enumerate() {
                println!(
                    "{:>4}│ {}",
                    (start_line + i as i32).to_string().bright_green(),
                    line
                );
            }

            // Print debug similarity score if RUST_LOG=debug
            if std::env::var("RUST_LOG")
                .unwrap_or_default()
                .contains("debug")
            {
                println!("Similarity: {:.2}%", (1.0 - distance) * 100.0);
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;
    let mut context = AppContext::new(&current_dir).await?;

    match (&cli.query, &cli.command) {
        (Some(query), None) => {
            query_codebase(&mut context, query.clone()).await?;
        }
        (None, Some(Commands::Index { path })) => {
            let index_path = path.clone().map(PathBuf::from).unwrap_or(current_dir);
            index_codebase(&mut context, index_path).await?;
        }
        (None, None) => {
            println!("No command or query specified. Use --help to see available commands.");
            println!("Example usage:");
            println!("  Index: ragrep index [--path <dir>]");
            println!("  Query: ragrep \"your search term\"");
        }
        (Some(_), Some(_)) => {
            println!("Cannot specify both a query and a command. Use either:");
            println!("  ragrep index [--path <dir>]");
            println!("  ragrep \"your search term\"");
        }
    }

    Ok(())
}
