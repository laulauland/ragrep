# ragrep Development Guide

## Architecture

ragrep uses a client/server architecture for fast semantic code search:

```
ragrep binary
├─ CLI Mode: ragrep "search query"
│  ├─ Check for server (.ragrep/ragrep.sock)
│  ├─ YES → Send query to server (0.5-1s)
│  └─ NO  → Standalone mode (7s, loads models each time)
│
└─ Server Mode: ragrep serve
   ├─ Load models once (4.6s startup)
   ├─ Listen on Unix socket
   ├─ Auto-reindex on file changes
   └─ Handle concurrent queries
```

**Key Design Decisions:**
- **No auto-start** - Explicit `ragrep serve`, falls back to standalone gracefully
- **One server per project** - Socket at `.ragrep/ragrep.sock`, client walks up to find it
- **No authentication** - Local-only Unix socket with owner-only permissions
- **File watching** - Uses `notify` crate to watch source files, respects gitignore

## How It Works

### Indexing
1. Scan for `.rs`, `.py`, `.js`, `.ts` files (respects `.gitignore` and `.ragrepignore`)
2. Parse with tree-sitter into AST
3. Chunk code into semantic blocks (functions, classes, etc.)
4. Generate embeddings using the mxbai-embed-large-v1 model
5. Store in SQLite with `sqlite-vec` extension

### Searching
1. Embed query → cosine similarity search → rerank with BAAI/bge-reranker-base
2. Return ranked results with file paths and line numbers

### Auto-Reindexing (Smart Caching)
When server is running:
1. Watch source files via `notify` crate
2. Debounce changes (default 1000ms)
3. Incremental reindex:
   - Load old embeddings before deleting chunks
   - Reuse embeddings for unchanged chunks (matched by content hash)
   - Only re-embed modified chunks
   - **Result: 200ms vs 30s full reindex (10-15x faster)**

## File Structure

```
.ragrep/
├── ragrep.db         # SQLite database (chunks + embeddings)
├── ragrep.sock       # Unix socket (when server running)
├── server.pid        # Server PID (when server running)
└── config.toml       # Configuration

~/.cache/ragrep/models/  # Global model cache (~1.5GB)
```

## Configuration

`.ragrep/config.toml`:
```toml
[server.git_watch]
enabled = true
debounce_ms = 1000  # Wait 1s after change before reindex
```

## Performance

| Mode | Time | Notes |
|------|------|-------|
| Standalone query | 7.4s | Loads models each time |
| Server startup | 4.6s | One-time cost |
| Server query | 0.5-1.0s | **85-93% faster** |
| Full reindex | 30-40s | 100 files |
| Incremental reindex | 200ms | **10-15x faster** with smart caching |

## Development

### Building
```bash
cargo build              # Debug build
cargo build --release    # Release build (much faster)
cargo test               # Run tests
```

### Workflow
```bash
# Terminal 1: Start server
cargo run -- serve

# Terminal 2: Search
cargo run -- "error handling"

# Edit files - auto-reindexed!
vim src/main.rs  # Save triggers reindex
```

### Debugging
```bash
# Enable debug logs
RUST_LOG=debug cargo run -- serve
RUST_LOG=debug cargo run -- "query"

# Check server status
ls -la .ragrep/
cat .ragrep/server.pid

# Inspect database
sqlite3 .ragrep/ragrep.db "SELECT COUNT(*) FROM chunks;"
```

## Common Issues

**Server won't start**
- Check for existing instance: `cat .ragrep/server.pid`
- Remove stale files: `rm .ragrep/ragrep.sock .ragrep/server.pid`

**Slow queries**
- Use server mode: `ragrep serve &`
- Build with `--release`
- Check model cache exists: `ls ~/.cache/ragrep/models/`

**File not reindexing**
- Ensure you're in a git repo
- Check file extension is supported (`.rs`, `.py`, `.js`, `.ts`)
- Check if file is gitignored
- Verify config: `[server.git_watch] enabled = true`

## Adding Features

### New Language
1. Add tree-sitter parser to `Cargo.toml`
2. Update `src/chunker.rs` with language support
3. Add extension to `src/constants.rs::DEFAULT_FILE_EXTENSIONS`

### New Command
1. Add variant to `Commands` enum in `src/main.rs`
2. Implement handler
3. Update CLI parser

## Code Style
- Use `cargo fmt` and `cargo clippy`
- Follow Rust conventions
- Keep functions small and focused

## Future Roadmap

**MCP Integration** (Designed, not implemented)
- Model Context Protocol interface for AI coding agents (Claude Code)

**Performance**
- Query result caching
- Multi-threaded indexing
- Streaming results

**Features**
- More languages (Java, Go, C++)
- Custom chunking strategies
- Web UI
