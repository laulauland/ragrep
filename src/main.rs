use anyhow::Result;
use clap::Parser;
use std::path::{Path, PathBuf};

mod cleaner;
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
    
    for file in files {
        println!("Found: {}", file.display());
    }

    Ok(())
}
