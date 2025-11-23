use crate::context::AppContext;
use crate::embedder::Embedding;
use crate::protocol::{Message, SearchRequest, SearchResponse, SearchResult, SearchStats};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use log::{debug, error, info, warn};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

pub struct RagrepServer {
    context: Arc<Mutex<AppContext>>,
    socket_path: PathBuf,
    pid_path: PathBuf,
}

impl RagrepServer {
    /// Create a new server instance
    pub fn new(context: AppContext, base_path: &std::path::Path) -> Self {
        let ragrep_dir = base_path.join(".ragrep");
        let socket_path = ragrep_dir.join("ragrep.sock");
        let pid_path = ragrep_dir.join("server.pid");

        Self {
            context: Arc::new(Mutex::new(context)),
            socket_path,
            pid_path,
        }
    }

    /// Start the server and listen for connections
    pub async fn serve(&self) -> Result<()> {
        // Check for existing server
        if let Ok(old_pid_str) = std::fs::read_to_string(&self.pid_path) {
            let pid: u32 = old_pid_str
                .trim()
                .parse()
                .context("Failed to parse PID file")?;

            // Check if process is still running
            if is_process_running(pid) {
                return Err(anyhow!("Server already running (PID: {})", pid));
            } else {
                warn!("Found stale PID file, cleaning up");
                let _ = std::fs::remove_file(&self.pid_path);
                let _ = std::fs::remove_file(&self.socket_path);
            }
        }

        // Write our PID
        let pid = std::process::id();
        std::fs::write(&self.pid_path, pid.to_string())
            .context("Failed to write PID file")?;

        info!("Server PID: {}", pid);

        // Remove old socket if it exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).context("Failed to remove old socket")?;
        }

        // Create the listener
        let listener =
            UnixListener::bind(&self.socket_path).context("Failed to bind Unix socket")?;

        info!("Server listening on {}", self.socket_path.display());

        // Accept connections in a loop
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let context = Arc::clone(&self.context);

                    // Spawn a task to handle this connection
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, context).await {
                            error!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                }
            }
        }
    }

    /// Get the PID file path
    pub fn pid_path(&self) -> &PathBuf {
        &self.pid_path
    }

    /// Get the socket file path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }
}

/// Execute a search query and return results (shared implementation)
pub async fn execute_search(
    context: &mut AppContext,
    request: SearchRequest,
) -> Result<SearchResponse> {
    let start = Instant::now();

    debug!("Executing search: {}", request.query);

    // Step 1: Generate embedding for the query
    let Embedding(query_embedding) = context.embedder.embed_query(&request.query).await?;

    // Step 2: Search the database
    let initial_results = context
        .db
        .find_similar_chunks(&query_embedding, request.top_n)?;

    if initial_results.is_empty() {
        return Ok(SearchResponse {
            results: vec![],
            stats: SearchStats {
                total_time_ms: start.elapsed().as_millis() as u64,
                num_candidates: 0,
                num_results: 0,
            },
        });
    }

    // Step 3: Rerank results
    let documents: Vec<String> = initial_results
        .iter()
        .map(|(text, _, _, _, _, _)| text.clone())
        .collect();

    let reranked_indices =
        context
            .reranker
            .rerank(&request.query, &documents, Some(request.top_n))?;

    // Step 4: Convert to SearchResult format
    let results: Vec<SearchResult> = reranked_indices
        .iter()
        .map(|(idx, score)| {
            let (text, file_path, start_line, end_line, _node_type, _distance) =
                &initial_results[*idx];
            SearchResult {
                file_path: file_path.clone(),
                start_line: *start_line,
                end_line: *end_line,
                text: if request.files_only {
                    String::new()
                } else {
                    text.clone()
                },
                score: *score,
            }
        })
        .collect();

    let elapsed = start.elapsed();
    let num_results = results.len();

    Ok(SearchResponse {
        results,
        stats: SearchStats {
            total_time_ms: elapsed.as_millis() as u64,
            num_candidates: initial_results.len(),
            num_results,
        },
    })
}

/// Execute a search query and return results (server version with Arc<Mutex>)
async fn handle_search(
    context: Arc<Mutex<AppContext>>,
    request: SearchRequest,
) -> Result<SearchResponse> {
    let mut context_guard = context.lock().await;
    execute_search(&mut *context_guard, request).await
}

/// Handle a single client connection
async fn handle_connection(stream: UnixStream, context: Arc<Mutex<AppContext>>) -> Result<()> {
    debug!("New connection");

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        // Parse the message
        let message: Message = serde_json::from_str(&line).context("Failed to parse message")?;

        debug!("Received message: {:?}", message);

        let response = match message {
            Message::Request { id, request } => {
                match handle_search(Arc::clone(&context), request).await {
                    Ok(search_response) => Message::Response {
                        id,
                        response: search_response,
                    },
                    Err(e) => Message::Error {
                        id,
                        message: format!("Search failed: {}", e),
                    },
                }
            }
            _ => {
                warn!("Unexpected message type");
                continue;
            }
        };

        // Send response
        let response_json = serde_json::to_string(&response)?;
        writer.write_all(response_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;

        line.clear();
    }

    debug!("Connection closed");
    Ok(())
}

/// Check if a process with the given PID is still running
fn is_process_running(pid: u32) -> bool {
    // Use `kill -0` which is portable across Unix systems (Linux, macOS, etc.)
    // It sends signal 0 which doesn't kill the process, just checks if it exists
    Command::new("kill")
        .args(&["-0", &pid.to_string()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
