use anyhow::{Context, Result};
use clap::Parser;
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
    
    for file in files {
        println!("\nProcessing: {}", file.path.display());
        
        // Read file content
        let content = std::fs::read_to_string(&file.path)
            .with_context(|| format!("Failed to read file: {}", file.path.display()))?;
            
        // Chunk the file
        let chunks = chunker.chunk_file(&file.path, &content)?;
        
        // Print chunks for debugging
        for chunk in chunks {
            println!("\n--- {} ---", chunk.kind);
            if !chunk.leading_comments.is_empty() {
                println!("Comments:\n{}", chunk.leading_comments);
            }
            println!("Content:\n{}", chunk.content);
        }
    }

    Ok(())
}
