use clap::Parser;
use anyhow::Result;

mod indexer;
mod cleaner;
mod embedder;
mod store;
mod query;

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
    println!("Indexing codebase at: {}", cli.path);
    Ok(())
}
