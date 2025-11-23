use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use env_logger::Env;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::{debug, info, warn};
use std::io::Write;
use std::path::PathBuf;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

mod chunker;
mod client;
mod config;
mod context;
mod db;
mod embedder;
mod git_watcher;
mod indexer;
mod protocol;
mod reranker;
mod server;

use context::AppContext;
use embedder::Embedding;
use protocol::{SearchRequest, SearchResponse};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Search query (default command)
    query: Option<String>,

    /// Display only filenames and line numbers without code content
    #[arg(short = 'l', long = "compact")]
    files_only: bool,

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
    /// Start the ragrep server
    Serve {},
}

async fn index_codebase(ctx: &mut AppContext, path: PathBuf) -> Result<()> {
    info!("Initializing ragrep...");
    debug!(
        "Global config: {}",
        ctx.config_manager.global_config_path.display()
    );
    if let Some(local_path) = &ctx.config_manager.local_config_path {
        debug!("Local config: {}", local_path.display());
    }
    debug!("Database: {}", ctx.ragrep_dir.join("ragrep.db").display());
    let model_cache_dir = ctx.config_manager.get_model_cache_dir()?;
    debug!("Model cache: {}", model_cache_dir.display());
    info!("Indexing codebase at: {}", path.display());

    let indexer = indexer::Indexer::new();
    let mut chunker = chunker::Chunker::new()?;
    let files = indexer.index_directory(&path)?;
    let total_files = files.len();
    let mut total_chunks = 0;
    let mut processed_chunks = 0;

    // Set up progress bars
    let multi = MultiProgress::new();

    let files_pb = multi.add(ProgressBar::new(total_files as u64));
    files_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    files_pb.set_message("Processing files");

    let chunks_pb = multi.add(ProgressBar::new_spinner());
    chunks_pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap(),
    );
    chunks_pb.set_message("Processing chunks");

    for file in files {
        debug!("Processing: {}", file.path.display());
        files_pb.set_message(format!("Processing {}", file.path.display()));

        let content = std::fs::read_to_string(&file.path)
            .with_context(|| format!("Failed to read file: {}", file.path.display()))?;

        let chunks = chunker.chunk_file(&file.path, &content)?;
        total_chunks += chunks.len();
        chunks_pb.set_length(total_chunks as u64);
        chunks_pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} chunks ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        if !chunks.is_empty() {
            let file_path = file.path.to_string_lossy().to_string();

            // Process chunks and store in database
            for (chunk_index, chunk) in chunks.iter().enumerate() {
                // Generate embedding for the chunk
                let Embedding(embedding) =
                    ctx.embedder.embed_text(&chunk.content, &file_path).await?;

                // Create longer-lived bindings for the values
                let chunk_idx = chunk_index as i32;

                // Store chunk and embedding in database
                ctx.db.save_chunk(
                    &file_path,
                    chunk_idx,
                    &chunk.kind,
                    chunk.parent_name.as_deref(),
                    chunk.start_line,
                    chunk.end_line,
                    &chunk.content,
                    chunk.hash(),
                    &embedding,
                )?;

                processed_chunks += 1;
                chunks_pb.set_position(processed_chunks as u64);
            }
        }

        files_pb.inc(1);
    }

    files_pb.finish_with_message("Files processing complete!");
    chunks_pb.finish_with_message("Chunks processing complete!");

    info!("Indexing complete! {} chunks processed", processed_chunks);
    debug!("Database: {}", ctx.ragrep_dir.join("ragrep.db").display());

    Ok(())
}

fn display_search_results(response: &SearchResponse, files_only: bool) -> Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    for result in &response.results {
        // Print file path in purple with line range
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Magenta)).set_bold(true))?;
        write!(stdout, "{}:", result.file_path)?;
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        writeln!(stdout, "{}:{}", result.start_line, result.end_line)?;
        stdout.reset()?;

        debug!(
            "Match found in {} (lines {}-{}) with relevance score: {:.4}",
            result.file_path, result.start_line, result.end_line, result.score
        );

        // Print content with line numbers only if not in files-only mode
        if !files_only && !result.text.is_empty() {
            for (i, line) in result.text.lines().enumerate() {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true))?;
                write!(stdout, "{}:", result.start_line + i as i32)?;
                stdout.reset()?;
                writeln!(stdout, " {}", line)?;
            }
            writeln!(stdout)?;
        }
    }

    // Print stats
    info!(
        "Found {} results in {}ms (from {} candidates)",
        response.stats.num_results, response.stats.total_time_ms, response.stats.num_candidates
    );

    Ok(())
}

async fn query_codebase(ctx: &mut AppContext, query: String, files_only: bool) -> Result<()> {
    debug!("Searching for: {}", query);

    let request = SearchRequest {
        query,
        top_n: 10,
        files_only,
    };

    let response = server::execute_search(ctx, request).await?;

    if response.results.is_empty() {
        info!("No similar code found");
        return Ok(());
    }

    display_search_results(&response, files_only)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up logging with indicatif bridge
    let logger = env_logger::Builder::from_env(Env::default().default_filter_or("info")).build();
    let level = logger.filter();
    let multi = MultiProgress::new();

    LogWrapper::new(multi.clone(), logger).try_init().unwrap();
    log::set_max_level(level);

    let cli = Cli::parse();
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    match (&cli.query, &cli.command) {
        (Some(query), None) => {
            // Try to use server first
            if client::RagrepClient::is_server_available(&current_dir) {
                info!("Server detected, using fast mode");

                let client = client::RagrepClient::new(&current_dir)?;
                info!("Connected to server at {}", client.socket_path().display());

                let request = protocol::SearchRequest {
                    query: query.clone(),
                    top_n: 10,
                    files_only: cli.files_only,
                };

                match client.search(request).await {
                    Ok(response) => {
                        display_search_results(&response, cli.files_only)?;
                    }
                    Err(e) => {
                        warn!("Server query failed: {}, falling back to standalone", e);
                        warn!("Running in standalone mode (slower, loads models for each query)");
                        // Fall back to standalone
                        let mut context = AppContext::new(&current_dir).await?;
                        query_codebase(&mut context, query.clone(), cli.files_only).await?;
                    }
                }
            } else {
                // No server found, run standalone
                warn!("No server detected. Start one with: ragrep serve");
                info!("Running in standalone mode...");
                let mut context = AppContext::new(&current_dir).await?;
                query_codebase(&mut context, query.clone(), cli.files_only).await?;
            }
        }
        (None, Some(Commands::Index { path })) => {
            let index_path = path
                .clone()
                .map(PathBuf::from)
                .unwrap_or(current_dir.clone());
            let mut context = AppContext::new(&current_dir).await?;
            index_codebase(&mut context, index_path).await?;
        }
        (None, Some(Commands::Serve {})) => {
            // Create AppContext (loads models)
            let context = AppContext::new(&current_dir).await?;

            // Create server
            let mut server = server::RagrepServer::new(context, &current_dir);
            let pid_path = server.pid_path().clone();
            let socket_path = server.socket_path().clone();

            // Handle Ctrl+C gracefully
            let server_task = tokio::spawn(async move { server.serve().await });

            tokio::select! {
                result = server_task => {
                    result??;
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received Ctrl+C, shutting down...");
                }
            }

            // Clean up PID file and socket
            let _ = std::fs::remove_file(&pid_path);
            let _ = std::fs::remove_file(&socket_path);
            info!("Server stopped");
        }
        (None, None) => {
            info!("No command or query specified. Use --help to see available commands.");
            info!("Example usage:");
            info!("  Index: ragrep index [--path <dir>]");
            info!("  Query: ragrep \"your search term\"");
            info!("  Server: ragrep serve");
        }
        (Some(_), Some(_)) => {
            warn!("Cannot specify both a query and a command. Use either:");
            info!("  ragrep index [--path <dir>]");
            info!("  ragrep \"your search term\"");
            info!("  ragrep serve");
        }
    }

    Ok(())
}
