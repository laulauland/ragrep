# Phase 1 Implementation Guide: Server/Client Architecture

**Goal**: Transform ragrep from a slow, model-reloading CLI into a fast client/server system where models stay loaded in memory.

**Time Estimate**: 1 week  
**Lines of Code**: ~400 lines  
**Difficulty**: Intermediate

---

## 📚 Background: Why Are We Doing This?

Right now, every time you run `ragrep "query"`, the program:
1. Loads the embedding model from disk (1.5 seconds)
2. Loads the reranker model from disk (3.1 seconds)
3. Runs your query (2.7 seconds)
4. Exits and throws away the loaded models

**Total: 7.4 seconds per query** - and you pay the 4.6s model loading cost EVERY TIME.

With a server/client architecture:
- **Server**: Runs once, keeps models in memory
- **Client**: Connects to server, sends query, gets results
- **Result**: Queries drop from 7.4s → 2.7s (63% faster!)

---

## 🎯 What We're Building

```
┌─────────────────────────────────────────────────────────────┐
│ BEFORE (Current)                                            │
│                                                             │
│  $ ragrep "error handling"                                 │
│    ├─ Load embedder (1.5s)                                 │
│    ├─ Load reranker (3.1s)                                 │
│    ├─ Run query (2.7s)                                     │
│    └─ Exit                                                 │
│    Total: 7.4s                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ AFTER (Phase 1)                                            │
│                                                             │
│  Terminal 1:                                               │
│  $ ragrep serve                                            │
│    ├─ Load embedder (1.5s) ← ONE TIME                     │
│    ├─ Load reranker (3.1s) ← ONE TIME                     │
│    └─ Listen on .ragrep/ragrep.sock                       │
│                                                             │
│  Terminal 2:                                               │
│  $ ragrep "error handling"                                 │
│    ├─ Connect to server                                   │
│    ├─ Send query                                          │
│    ├─ Get results (2.7s)                                  │
│    └─ Display                                             │
│    Total: 2.7s ⚡                                          │
└─────────────────────────────────────────────────────────────┘
```

---

## 📋 Implementation Milestones

We'll build this in 7 milestones, each with verifiable behavior:

1. **Define Protocol Types** - Create message format
2. **Build Server Skeleton** - Server that starts and listens
3. **Add Query Handling** - Server processes search queries
4. **Build Client** - Client that connects and queries
5. **Add Fallback Logic** - Client works without server
6. **Process Management** - PID files and clean shutdown
7. **Integration Testing** - Verify everything works together

---

## Milestone 1: Define Protocol Types

**Goal**: Create the message format for client ↔ server communication.

**Why First**: We need to agree on the "language" before building the talkers.

### Step 1.1: Create `src/protocol.rs`

Create a new file that defines our communication protocol:

```rust
use serde::{Deserialize, Serialize};

/// Request from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// The search query string
    pub query: String,
    /// Maximum number of results to return
    pub top_n: usize,
    /// If true, only return file paths (no content)
    pub files_only: bool,
}

/// Single search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub start_line: i32,
    pub end_line: i32,
    pub text: String,
    pub score: f32,
}

/// Response from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub stats: SearchStats,
}

/// Statistics about the search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchStats {
    pub total_time_ms: u64,
    pub num_candidates: usize,
    pub num_results: usize,
}

/// Wrapper for JSON-RPC style communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    Request { id: u64, request: SearchRequest },
    Response { id: u64, response: SearchResponse },
    Error { id: u64, message: String },
}
```

**Why This Design**:
- `SearchRequest` matches what our CLI currently accepts
- `SearchResult` matches what we currently return
- `Message` enum uses JSON-RPC style (id for request/response matching)
- `#[serde(tag = "type")]` makes JSON cleaner (`{"type": "Request", ...}`)

### Step 1.2: Add to `src/main.rs`

At the top of `src/main.rs`, add:

```rust
mod protocol;
```

### Step 1.3: Verify It Compiles

```bash
cargo check
```

**Expected Output**:
```
   Compiling ragrep v0.1.0 (/Users/you/ragrep)
warning: unused imports ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.34s
```

You'll see warnings about unused code - that's fine! We'll use it soon.

### Step 1.4: Write a Unit Test

Add this at the bottom of `src/protocol.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let request = Message::Request {
            id: 1,
            request: SearchRequest {
                query: "test".to_string(),
                top_n: 10,
                files_only: false,
            },
        };

        // Serialize to JSON
        let json = serde_json::to_string(&request).unwrap();
        
        // Deserialize back
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        
        // Verify it round-trips correctly
        match deserialized {
            Message::Request { id, request } => {
                assert_eq!(id, 1);
                assert_eq!(request.query, "test");
                assert_eq!(request.top_n, 10);
            }
            _ => panic!("Wrong message type"),
        }
    }
}
```

### Step 1.5: Run the Test

```bash
cargo test test_message_serialization
```

**Expected Output**:
```
running 1 test
test protocol::tests::test_message_serialization ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

✅ **Milestone 1 Complete**: You can serialize/deserialize messages!

---

## Milestone 2: Build Server Skeleton

**Goal**: Create a server that starts, listens on a Unix socket, and accepts connections.

### Step 2.1: Create `src/server.rs`

Create a new file with the basic server structure:

```rust
use crate::context::AppContext;
use crate::protocol::{Message, SearchRequest, SearchResponse, SearchResult, SearchStats};
use anyhow::{Context as AnyhowContext, Result};
use log::{debug, info, warn, error};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

pub struct RagrepServer {
    context: Arc<AppContext>,
    socket_path: PathBuf,
}

impl RagrepServer {
    /// Create a new server instance
    pub fn new(context: AppContext, base_path: &std::path::Path) -> Self {
        let socket_path = base_path.join(".ragrep").join("ragrep.sock");
        
        Self {
            context: Arc::new(context),
            socket_path,
        }
    }

    /// Start the server and listen for connections
    pub async fn serve(&self) -> Result<()> {
        // Remove old socket if it exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)
                .context("Failed to remove old socket")?;
        }

        // Create the listener
        let listener = UnixListener::bind(&self.socket_path)
            .context("Failed to bind Unix socket")?;

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
}

/// Handle a single client connection
async fn handle_connection(stream: UnixStream, context: Arc<AppContext>) -> Result<()> {
    debug!("New connection");
    
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        // Parse the message
        let message: Message = serde_json::from_str(&line)
            .context("Failed to parse message")?;

        debug!("Received message: {:?}", message);

        // For now, just echo back an empty response
        let response = match message {
            Message::Request { id, request } => {
                // We'll implement this properly in the next milestone
                Message::Response {
                    id,
                    response: SearchResponse {
                        results: vec![],
                        stats: SearchStats {
                            total_time_ms: 0,
                            num_candidates: 0,
                            num_results: 0,
                        },
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
```

**Key Concepts**:
- `Arc<AppContext>`: Shared reference to models (safe for concurrent access)
- `UnixListener`: Tokio's async Unix socket listener
- `tokio::spawn`: Each connection runs in its own task (concurrent!)
- Line-delimited JSON: Each message is one line (simple framing)

### Step 2.2: Add to `src/main.rs`

Add at the top:
```rust
mod server;
```

And add a new command variant:

```rust
#[derive(Subcommand)]
enum Commands {
    Index { 
        #[arg(short, long)] 
        path: Option<String> 
    },
    Serve {},  // ← Add this
}
```

Then in the `main()` function's match statement, add:

```rust
(None, Some(Commands::Serve {})) => {
    // Create AppContext (loads models)
    let mut context = AppContext::new(&current_dir).await?;
    
    // Create and start server
    let server = server::RagrepServer::new(context, &current_dir);
    server.serve().await?;
}
```

### Step 2.3: Test the Server Starts

```bash
cargo build
./target/debug/rag serve
```

**Expected Output**:
```
[INFO] Loading models...
[TIMING] Embedder model loading: 1.465s
[TIMING] Reranker model loading: 3.144s
[INFO] Server listening on /path/to/project/.ragrep/ragrep.sock
```

Leave it running!

### Step 2.4: Verify the Socket Exists

In another terminal:

```bash
ls -la .ragrep/ragrep.sock
```

**Expected Output**:
```
srwxr-xr-x  1 you  staff  0 Nov 23 15:30 .ragrep/ragrep.sock
```

Notice the `s` at the start - that's a socket file!

### Step 2.5: Test Connection with `nc`

While server is running, in another terminal:

```bash
echo '{"type":"Request","id":1,"request":{"query":"test","top_n":10,"files_only":false}}' | nc -U .ragrep/ragrep.sock
```

**Expected Output**:
```json
{"type":"Response","id":1,"response":{"results":[],"stats":{"total_time_ms":0,"num_candidates":0,"num_results":0}}}
```

Great! The server receives messages and responds (even if with empty results for now).

### Step 2.6: Stop the Server

In the server terminal, press `Ctrl+C`.

**Expected**: Server stops, socket file is NOT cleaned up (we'll fix this later).

✅ **Milestone 2 Complete**: Server starts, listens, accepts connections, and responds!

---

## Milestone 3: Add Query Handling

**Goal**: Make the server actually execute searches using the existing AppContext.

### Step 3.1: Implement `handle_search` Function

Add this function to `src/server.rs`:

```rust
use crate::embedder::Embedding;
use std::time::Instant;

/// Execute a search query and return results
async fn handle_search(
    context: Arc<AppContext>,
    request: SearchRequest,
) -> Result<SearchResponse> {
    let start = Instant::now();
    
    debug!("Executing search: {}", request.query);

    // Step 1: Generate embedding for the query
    let Embedding(query_embedding) = context.embedder.embed_query(&request.query).await?;
    
    // Step 2: Search the database
    let initial_results = context.db.find_similar_chunks(&query_embedding, request.top_n)?;
    
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
    
    let reranked_indices = context.reranker.rerank(
        &request.query,
        &documents,
        Some(request.top_n)
    )?;
    
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
                text: if request.files_only { String::new() } else { text.clone() },
                score: *score,
            }
        })
        .collect();
    
    let elapsed = start.elapsed();
    
    Ok(SearchResponse {
        results,
        stats: SearchStats {
            total_time_ms: elapsed.as_millis() as u64,
            num_candidates: initial_results.len(),
            num_results: results.len(),
        },
    })
}
```

### Step 3.2: Update `handle_connection` to Use It

Replace the placeholder in `handle_connection`:

```rust
// Replace this section:
let response = match message {
    Message::Request { id, request } => {
        // OLD: Empty response
        Message::Response {
            id,
            response: SearchResponse {
                results: vec![],
                stats: SearchStats { ... },
            },
        }
    }
    ...
};

// With this:
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
```

### Step 3.3: Test Real Queries

First, make sure you have an indexed database:

```bash
cargo build
./target/debug/rag index
```

Then start the server:

```bash
./target/debug/rag serve
```

In another terminal, send a real query:

```bash
echo '{"type":"Request","id":1,"request":{"query":"how do we chunk files","top_n":5,"files_only":false}}' | nc -U .ragrep/ragrep.sock
```

**Expected Output**: JSON with actual search results!

```json
{"type":"Response","id":1,"response":{"results":[{"file_path":"src/chunker.rs","start_line":42,"end_line":50,"text":"...","score":0.95}],"stats":{"total_time_ms":2341,"num_candidates":10,"num_results":5}}}
```

### Step 3.4: Verify Timing

Notice `"total_time_ms":2341` - that's ~2.3 seconds, much faster than 7.4!

Send another query immediately:

```bash
echo '{"type":"Request","id":2,"request":{"query":"error handling","top_n":5,"files_only":false}}' | nc -U .ragrep/ragrep.sock
```

Should be similarly fast - models are already loaded!

✅ **Milestone 3 Complete**: Server executes real searches!

---

## Milestone 4: Build Client

**Goal**: Create a client that connects to the server and displays results nicely.

### Step 4.1: Create `src/client.rs`

```rust
use crate::protocol::{Message, SearchRequest, SearchResponse};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use log::{debug, info};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub struct RagrepClient {
    socket_path: PathBuf,
}

impl RagrepClient {
    /// Create a new client by finding the server socket
    pub fn new(start_dir: &Path) -> Result<Self> {
        let socket_path = find_ragrep_socket(start_dir)?;
        Ok(Self { socket_path })
    }

    /// Execute a search query against the server
    pub async fn search(&self, request: SearchRequest) -> Result<SearchResponse> {
        debug!("Connecting to server at {}", self.socket_path.display());

        // Connect to server
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to server")?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Send request
        let request_msg = Message::Request {
            id: 1, // Simple client uses id=1
            request,
        };
        let request_json = serde_json::to_string(&request_msg)?;
        writer.write_all(request_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;

        debug!("Sent request, waiting for response...");

        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        // Parse response
        let response: Message = serde_json::from_str(&line)
            .context("Failed to parse response")?;

        match response {
            Message::Response { response, .. } => Ok(response),
            Message::Error { message, .. } => Err(anyhow!("Server error: {}", message)),
            _ => Err(anyhow!("Unexpected response type")),
        }
    }
}

/// Find the ragrep socket by walking up the directory tree
fn find_ragrep_socket(start_dir: &Path) -> Result<PathBuf> {
    let mut current = start_dir;

    loop {
        let socket_path = current.join(".ragrep").join("ragrep.sock");
        
        if socket_path.exists() {
            debug!("Found socket at {}", socket_path.display());
            return Ok(socket_path);
        }

        // Try parent directory
        current = current
            .parent()
            .ok_or_else(|| anyhow!("No ragrep server found (searched up to root)"))?;
    }
}
```

**Key Concepts**:
- `find_ragrep_socket`: Walks up directory tree (like git finding `.git/`)
- Single request/response per connection (simple!)
- Returns `SearchResponse` directly for easy use

### Step 4.2: Add to `src/main.rs`

Add:
```rust
mod client;
```

### Step 4.3: Update Query Logic in `main.rs`

Find the existing query handling code and wrap it:

```rust
// BEFORE:
(Some(query), None) => {
    query_codebase(&mut context, query.clone(), cli.files_only).await?;
}

// AFTER:
(Some(query), None) => {
    // Try to use server first
    if let Ok(client) = client::RagrepClient::new(&current_dir) {
        info!("Using server for query");
        
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
                // Fall back to standalone
                let mut context = AppContext::new(&current_dir).await?;
                query_codebase(&mut context, query.clone(), cli.files_only).await?;
            }
        }
    } else {
        // No server found, run standalone
        info!("No server found, running in standalone mode");
        let mut context = AppContext::new(&current_dir).await?;
        query_codebase(&mut context, query.clone(), cli.files_only).await?;
    }
}
```

### Step 4.4: Create `display_search_results` Function

Add this helper function in `src/main.rs`:

```rust
use crate::protocol::{SearchResponse};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use std::io::Write;

fn display_search_results(response: &SearchResponse, files_only: bool) -> Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    for result in &response.results {
        // Print file path in purple with line range
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Magenta)).set_bold(true))?;
        write!(stdout, "{}:", result.file_path)?;
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        writeln!(stdout, "{}:{}", result.start_line, result.end_line)?;
        stdout.reset()?;

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
        response.stats.num_results,
        response.stats.total_time_ms,
        response.stats.num_candidates
    );

    Ok(())
}
```

### Step 4.5: Test Client with Server

Terminal 1:
```bash
cargo build
./target/debug/rag serve
```

Terminal 2:
```bash
./target/debug/rag "error handling"
```

**Expected Output**:
```
[INFO] Using server for query
src/main.rs:45:67
45: fn handle_error() -> Result<()> {
...
[INFO] Found 10 results in 2341ms (from 50 candidates)
```

Notice:
- No model loading time!
- Fast results!
- `[INFO] Using server for query` message

### Step 4.6: Test Standalone Fallback

Stop the server (Ctrl+C in Terminal 1).

In Terminal 2:
```bash
./target/debug/rag "error handling"
```

**Expected Output**:
```
[INFO] No server found, running in standalone mode
[INFO] Loading models...
[TIMING] Embedder model loading: 1.465s
[TIMING] Reranker model loading: 3.144s
src/main.rs:45:67
...
```

Notice:
- Falls back gracefully
- Loads models (slow)
- Still works!

✅ **Milestone 4 Complete**: Client works with and without server!

---

## Milestone 5: Add Fallback Logic

**Goal**: Polish the fallback behavior and make it seamless.

This is mostly already done in Step 4.3, but let's improve the user experience.

### Step 5.1: Better Error Messages

Update the client connection error in `src/main.rs`:

```rust
if let Ok(client) = client::RagrepClient::new(&current_dir) {
    info!("Connected to server at {}", /* socket path */);
    // ... existing code ...
} else {
    warn!("No ragrep server found. Start one with: ragrep serve");
    warn!("Running in standalone mode (slower, loads models for each query)");
    // ... standalone code ...
}
```

### Step 5.2: Add Server Detection Helper

Add this to `src/client.rs`:

```rust
impl RagrepClient {
    /// Check if a server is available without connecting
    pub fn is_server_available(start_dir: &Path) -> bool {
        find_ragrep_socket(start_dir).is_ok()
    }
}
```

### Step 5.3: Use Detection in Main

```rust
if client::RagrepClient::is_server_available(&current_dir) {
    info!("Server detected, using fast mode");
    let client = client::RagrepClient::new(&current_dir)?;
    // ... client code ...
} else {
    warn!("No server detected. Start one with: ragrep serve");
    info!("Running in standalone mode...");
    // ... standalone code ...
}
```

### Step 5.4: Test Both Paths

**With server**:
```bash
# Terminal 1
./target/debug/rag serve &

# Terminal 2
./target/debug/rag "test query"
# Should say: "Server detected, using fast mode"
```

**Without server**:
```bash
# Kill server if running
pkill -f "rag serve"

./target/debug/rag "test query"
# Should say: "No server detected. Start one with: ragrep serve"
```

✅ **Milestone 5 Complete**: Graceful fallback with helpful messages!

---

## Milestone 6: Process Management

**Goal**: Handle PID files and clean shutdown.

### Step 6.1: Add PID File Writing

Update `src/server.rs` to write a PID file on startup:

```rust
impl RagrepServer {
    pub fn new(context: AppContext, base_path: &std::path::Path) -> Self {
        let ragrep_dir = base_path.join(".ragrep");
        let socket_path = ragrep_dir.join("ragrep.sock");
        let pid_path = ragrep_dir.join("server.pid");
        
        Self {
            context: Arc::new(context),
            socket_path,
            pid_path,  // Add this field to struct
        }
    }

    pub async fn serve(&self) -> Result<()> {
        // Check for existing server
        if let Ok(old_pid) = std::fs::read_to_string(&self.pid_path) {
            let pid: u32 = old_pid.trim().parse()?;
            
            // Check if process is still running (Unix-specific)
            if std::path::Path::new(&format!("/proc/{}", pid)).exists() {
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

        // Remove old socket if exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        // ... rest of serve() code ...
        
        // At the end, clean up
        let _ = std::fs::remove_file(&self.pid_path);
        let _ = std::fs::remove_file(&self.socket_path);
        
        Ok(())
    }
}
```

Don't forget to add `pid_path` to the struct:

```rust
pub struct RagrepServer {
    context: Arc<AppContext>,
    socket_path: PathBuf,
    pid_path: PathBuf,  // Add this
}
```

### Step 6.2: Handle Ctrl+C Gracefully

Update the server startup in `src/main.rs`:

```rust
(None, Some(Commands::Serve {})) => {
    let mut context = AppContext::new(&current_dir).await?;
    let server = server::RagrepServer::new(context, &current_dir);
    
    // Handle Ctrl+C
    let server_task = tokio::spawn(async move {
        server.serve().await
    });
    
    tokio::select! {
        result = server_task => {
            result??;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down...");
        }
    }
    
    info!("Server stopped");
}
```

### Step 6.3: Test Process Management

**Test 1: PID file is created**
```bash
./target/debug/rag serve &
ls -la .ragrep/server.pid
cat .ragrep/server.pid
```

**Expected**: File exists with the server's PID.

**Test 2: Can't start two servers**
```bash
# Server already running from Test 1
./target/debug/rag serve
```

**Expected**: Error message: "Server already running (PID: 12345)"

**Test 3: Stale PID cleanup**
```bash
# Kill server ungracefully
pkill -9 -f "rag serve"

# PID file still exists
ls .ragrep/server.pid

# Start new server - should clean up stale PID
./target/debug/rag serve
```

**Expected**: No error, stale PID cleaned up, new server starts.

**Test 4: Graceful shutdown**
```bash
./target/debug/rag serve
# Press Ctrl+C
```

**Expected**:
```
[INFO] Received Ctrl+C, shutting down...
[INFO] Server stopped
```

And PID file should be removed:
```bash
ls .ragrep/server.pid
# Should not exist
```

✅ **Milestone 6 Complete**: Process management works!

---

## Milestone 7: Integration Testing

**Goal**: Comprehensive tests to verify everything works together.

### Step 7.1: Create Integration Test File

Create `tests/integration_test.rs`:

```rust
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn test_server_client_integration() {
    // Build the binary first
    let status = Command::new("cargo")
        .args(&["build"])
        .status()
        .expect("Failed to build");
    assert!(status.success());

    // Start server in background
    let mut server = Command::new("./target/debug/rag")
        .arg("serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    // Give server time to start
    thread::sleep(Duration::from_secs(6));

    // Run a query using the client
    let output = Command::new("./target/debug/rag")
        .arg("error handling")
        .output()
        .expect("Failed to run query");

    // Should succeed
    assert!(output.status.success());

    // Should have results
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("src/")); // Should have file paths

    // Cleanup: kill server
    server.kill().expect("Failed to kill server");
}

#[test]
fn test_standalone_fallback() {
    // Make sure no server is running
    let _ = Command::new("pkill")
        .args(&["-f", "rag serve"])
        .status();

    thread::sleep(Duration::from_secs(1));

    // Run query without server
    let output = Command::new("./target/debug/rag")
        .arg("error handling")
        .output()
        .expect("Failed to run query");

    // Should still succeed
    assert!(output.status.success());

    // Should have warning about standalone mode
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("standalone"));
}
```

### Step 7.2: Run Integration Tests

```bash
cargo test --test integration_test
```

**Expected Output**:
```
running 2 tests
test test_standalone_fallback ... ok
test test_server_client_integration ... ok

test result: ok. 2 passed; 0 failed
```

### Step 7.3: Manual End-to-End Test

This is the final verification:

**Step 1**: Clean slate
```bash
pkill -f "rag serve"
rm -f .ragrep/ragrep.sock .ragrep/server.pid
cargo build --release
```

**Step 2**: Benchmark standalone
```bash
time ./target/release/rag "error handling"
```

**Expected**: ~7 seconds

**Step 3**: Start server
```bash
./target/release/rag serve &
```

**Wait 5 seconds for models to load**

**Step 4**: Benchmark with server
```bash
time ./target/release/rag "error handling"
```

**Expected**: ~2-3 seconds ⚡

**Step 5**: Multiple queries
```bash
time ./target/release/rag "authentication"
time ./target/release/rag "database"
time ./target/release/rag "parsing"
```

**Expected**: Each ~2-3 seconds, consistently fast!

**Step 6**: Test fallback
```bash
pkill -f "rag serve"
time ./target/release/rag "error handling"
```

**Expected**: Warning message, then works (slowly) in standalone mode.

✅ **Milestone 7 Complete**: Full integration works!

---

## 🎉 Phase 1 Complete!

You now have:
- ✅ Server that keeps models loaded
- ✅ Client that connects to server
- ✅ Graceful fallback to standalone
- ✅ Process management (PID files)
- ✅ Clean shutdown
- ✅ 10x faster queries when server is running!

## 📊 Performance Verification

Run this benchmark to see the improvement:

```bash
#!/bin/bash

echo "=== Standalone Mode ==="
pkill -f "rag serve"
time ./target/release/rag "error handling" > /dev/null

echo ""
echo "=== Server Mode ==="
./target/release/rag serve &
sleep 6  # Wait for models to load

time ./target/release/rag "error handling" > /dev/null
time ./target/release/rag "authentication" > /dev/null
time ./target/release/rag "database" > /dev/null

pkill -f "rag serve"
```

**Expected Results**:
```
=== Standalone Mode ===
real    0m7.432s

=== Server Mode ===
real    0m2.718s
real    0m2.645s
real    0m2.701s
```

**~63% faster!** And subsequent queries stay fast because models are loaded.

---

## 🐛 Common Issues & Solutions

### Issue: "Address already in use"

**Cause**: Old socket file exists
**Fix**: 
```bash
rm .ragrep/ragrep.sock
```

### Issue: "No ragrep server found"

**Cause**: Not running in indexed directory
**Fix**: 
```bash
cd /path/to/indexed/project
./target/debug/rag serve
```

### Issue: Server starts but queries fail

**Cause**: Database not indexed
**Fix**:
```bash
./target/debug/rag index
```

### Issue: "Server already running" but it's not

**Cause**: Stale PID file
**Fix**:
```bash
rm .ragrep/server.pid .ragrep/ragrep.sock
```

---

## 📝 Code Review Checklist

Before moving to Phase 2, verify:

- [ ] `src/protocol.rs` compiles and tests pass
- [ ] Server starts and writes PID file
- [ ] Client can connect to server
- [ ] Queries return correct results
- [ ] Standalone fallback works
- [ ] Ctrl+C stops server cleanly
- [ ] Socket and PID files are cleaned up
- [ ] Integration tests pass
- [ ] Performance is 2-3x faster with server

---

## 🚀 Ready for Phase 2?

You've successfully implemented a client/server architecture!

**Next Phase**: Add MCP (Model Context Protocol) support so AI assistants like Claude Desktop can use ragrep.

**Before Starting Phase 2**:
1. Commit your Phase 1 work
2. Run all tests one more time
3. Document any issues you found
4. Celebrate! 🎉

---

## 📚 Further Reading

- Unix Domain Sockets: https://man7.org/linux/man-pages/man7/unix.7.html
- Tokio Async: https://tokio.rs/tokio/tutorial
- Process Management: https://man7.org/linux/man-pages/man2/getpid.2.html

---

**Congratulations!** You've learned:
- Async networking with Tokio
- Unix domain sockets
- Process management
- Graceful fallback patterns
- Performance optimization through caching
- Integration testing

These skills transfer to any Rust networking project!
