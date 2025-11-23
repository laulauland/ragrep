use crate::context::AppContext;
use crate::embedder::Embedding;
use crate::git_watcher::GitIndexWatcher;
use crate::protocol::{Message, SearchRequest, SearchResponse, SearchResult, SearchStats};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use log::{debug, error, info, warn};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Receiver;
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
    pub async fn serve(&mut self) -> Result<()> {
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
        std::fs::write(&self.pid_path, pid.to_string()).context("Failed to write PID file")?;

        info!("Server PID: {}", pid);

        // Remove old socket if it exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).context("Failed to remove old socket")?;
        }

        // Create the listener
        let listener =
            UnixListener::bind(&self.socket_path).context("Failed to bind Unix socket")?;

        // Start git watcher if enabled and in a git repo
        let git_watcher_rx = self.start_git_watcher().await?;

        info!("Server listening on {}", self.socket_path.display());

        // Convert blocking receiver to async if watcher exists
        let mut git_rx_async = if let Some(blocking_rx) = git_watcher_rx {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<PathBuf>>();
            let tx_clone = tx.clone();
            // Spawn task to bridge blocking receiver to async channel
            tokio::spawn(async move {
                // Run the blocking receiver in a blocking task
                tokio::task::spawn_blocking(move || {
                    loop {
                        match blocking_rx.recv() {
                            Ok(files) => {
                                if tx_clone.send(files).is_err() {
                                    break; // Receiver dropped
                                }
                            }
                            Err(_) => {
                                break; // Channel closed or error
                            }
                        }
                    }
                })
                .await
                .ok();
            });
            Some(rx)
        } else {
            None
        };

        // Accept connections and handle git changes in a loop
        loop {
            tokio::select! {
                // Handle client connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            let context = Arc::clone(&self.context);
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

                // Handle git changes
                changed_files_result = async {
                    if let Some(ref mut rx) = git_rx_async {
                        rx.recv().await
                    } else {
                        // Wait forever if no watcher (this branch will never be selected)
                        std::future::pending::<Option<Vec<PathBuf>>>().await
                    }
                } => {
                    if let Some(changed_files) = changed_files_result {
                        self.handle_git_changes(changed_files).await;
                    }
                }
            }
        }
    }

    async fn start_git_watcher(&self) -> Result<Option<Receiver<Vec<PathBuf>>>> {
        // Check config
        let config_enabled = {
            let context = self.context.lock().await;
            context.config_manager.config().git_watch.enabled
        };

        if !config_enabled {
            info!("Git watching disabled in config");
            return Ok(None);
        }

        // Check if in git repo
        // Get base path from socket path (go up from .ragrep/ragrep.sock)
        let base_path = self
            .socket_path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("Invalid socket path"))?;

        if !GitIndexWatcher::is_git_repo(base_path) {
            warn!("Not in a git repository, git watching disabled");
            return Ok(None);
        }

        // Start watcher
        let watcher = GitIndexWatcher::new(base_path)?;
        let debounce = {
            let context = self.context.lock().await;
            context.config_manager.config().git_watch.debounce_ms
        };
        let rx = watcher.watch_debounced(debounce)?;

        info!("Git watcher started (debounce: {}ms)", debounce);

        Ok(Some(rx))
    }

    async fn handle_git_changes(&mut self, changed_files: Vec<PathBuf>) {
        info!(
            "Detected {} changed files, reindexing...",
            changed_files.len()
        );

        for file in &changed_files {
            debug!("  - {}", file.display());
        }

        let mut context = self.context.lock().await;
        match context.reindex_files(changed_files).await {
            Ok(()) => {
                info!("Reindex complete");
            }
            Err(e) => {
                error!("Reindex failed: {}", e);
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
