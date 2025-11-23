# ragrep Architecture: Client/Server + MCP

## Overview

ragrep uses a hybrid architecture supporting three modes of operation:

1. **Standalone Mode** - Traditional CLI, loads models per query (slow but always works)
2. **Server/Client Mode** - Unix socket server keeps models loaded (10x faster)
3. **MCP Mode** - Exposes semantic search to AI assistants via Model Context Protocol

## Architecture Diagram

```
ragrep binary
├─ CLI Mode: ragrep "search query" [--compact]
│  ├─ Check for server (.ragrep/ragrep.sock exists)
│  ├─ YES → Connect to server, send query, display results ⚡ FAST
│  └─ NO  → Standalone mode (load models, query, exit) 🐌 SLOW
│
├─ Server Mode: ragrep serve
│  ├─ Load models once (4.6s startup)
│  ├─ Listen on Unix socket (.ragrep/ragrep.sock)
│  ├─ Handle concurrent queries
│  ├─ Auto-reindex on git changes (git2 crate)
│  └─ Log to .ragrep/server.log
│
└─ MCP Mode: ragrep --mcp
   ├─ Load models once (4.6s startup)  
   ├─ Expose search_code tool via stdio
   ├─ Works with Claude Desktop, Cursor
   └─ Future: Add HTTP transport for remote access
```

## Design Decisions

### 1. No `ragrep shutdown` command
- User uses Ctrl+C to stop server
- Server cleans up socket on graceful shutdown
- Rationale: Simpler UX, standard Unix behavior

### 2. No auto-start
- User explicitly runs `ragrep serve`
- Client falls back to standalone if no server
- Rationale: Explicit is better than implicit

### 3. One server per project
- Socket location: `.ragrep/ragrep.sock` in project root
- Client walks up directory tree to find server
- Supports monorepos (one server for entire repo)
- Rationale: Simple, predictable, matches git model

### 4. No authentication (local only)
- Unix socket with owner-only permissions
- For remote access later: add auth to HTTP transport
- Rationale: Local-only, file permissions are sufficient

### 5. Git-based reindexing
- Use `git2` crate to detect changes
- Watch `.git/index` file for modifications
- Incremental reindex only changed files
- Rationale: Leverage existing git tracking vs reinventing with file watchers

### 6. Dual interface: Unix socket + MCP
- Unix socket for CLI clients (custom JSON-RPC protocol)
- MCP stdio for AI assistants (standard MCP protocol)
- NOT unified - separate, purpose-built interfaces
- Rationale: Optimize each interface for its use case

## File Structure

```
src/
├── main.rs           # CLI entry point, command routing
├── context.rs        # AppContext (models, db)
├── embedder.rs       # Embedding model
├── reranker.rs       # Reranking model  
├── db.rs             # SQLite + vector search
├── indexer.rs        # File indexing
├── chunker.rs        # Code chunking
├── config.rs         # Configuration
├── server.rs         # Unix socket server (NEW)
├── client.rs         # Unix socket client (NEW)
├── protocol.rs       # IPC protocol types (NEW)
├── watcher.rs        # Git change detection (NEW)
└── mcp.rs            # MCP server integration (NEW)

.ragrep/
├── ragrep.db         # SQLite database
├── ragrep.sock       # Unix socket (when server running)
├── server.pid        # Server process ID (when server running)
└── server.log        # Server logs
```

## Communication Protocols

### Unix Socket Protocol (CLI ↔ Server)

Simple JSON-RPC over Unix domain socket.

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "search",
  "params": {
    "query": "error handling",
    "top_n": 10,
    "files_only": false
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "results": [
      {
        "file_path": "src/main.rs",
        "start_line": 42,
        "end_line": 50,
        "text": "...",
        "score": 0.95
      }
    ],
    "stats": {
      "total_time_ms": 150,
      "num_candidates": 50,
      "num_results": 10
    }
  }
}
```

### MCP Protocol (AI Assistants ↔ Server)

Standard Model Context Protocol over stdio (using rust-mcp-sdk v0.7.4).

**Tool Definition**:
```json
{
  "name": "search_code",
  "description": "Search codebase semantically using natural language",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Natural language search query"
      },
      "top_n": {
        "type": "integer",
        "description": "Number of results to return",
        "default": 10
      }
    },
    "required": ["query"]
  }
}
```

## Implementation Phases

### Phase 1: Core Server (Week 1)

**Goal**: Get server/client working for CLI speedup

**Tasks**:
1. Create `src/protocol.rs` - IPC message types (~50 lines)
2. Create `src/server.rs` - Unix socket server (~200 lines)
   - Listen on `.ragrep/ragrep.sock`
   - Load AppContext once (keep models in memory)
   - Handle concurrent queries with tokio
   - Write PID file on startup
   - Clean up socket on shutdown
3. Create `src/client.rs` - Unix socket client (~100 lines)
   - Detect server by walking up to find `.ragrep/`
   - Send query, receive results
   - Fall back to standalone if no server
4. Update `src/main.rs` (~50 lines)
   - Add `Commands::Serve {}`
   - Route to server or client based on command
   - Preserve existing standalone behavior

**Total**: ~400 lines of code

**Success Criteria**:
- Server starts and keeps models loaded
- CLI queries 10x faster with server running
- Graceful fallback when no server
- Clean shutdown with Ctrl+C

### Phase 2: MCP Integration (Week 2)

**Goal**: Enable AI assistant integration

**Tasks**:
1. Add dependency: `cargo add rust-mcp-sdk --features "server,macros,stdio"`
2. Create `src/mcp.rs` (~250 lines)
   - Define `SearchCodeTool` with `#[mcp_tool]` macro
   - Implement `RagrepMcpHandler: ServerHandler` trait
   - Add `start_mcp_server()` function
   - Wire up to existing AppContext
3. Update `src/main.rs` (~20 lines)
   - Add `Commands::Mcp {}`
   - Route to MCP server mode
4. Test with MCP Inspector
5. Test with Claude Desktop

**Total**: ~250 lines of code

**Success Criteria**:
- Claude Desktop can invoke search_code tool
- Results formatted correctly for AI consumption
- Works with existing server/client architecture
- No breaking changes to CLI

### Phase 3: Git Integration (Week 3)

**Goal**: Auto-reindex on file changes

**Tasks**:
1. Add dependency: `cargo add git2`
2. Create `src/watcher.rs` (~200 lines)
   - Use `git2` to get list of changed files
   - Watch `.git/index` for modifications
   - Debounce changes (1 second)
   - Trigger incremental reindex
3. Update `src/server.rs` (~50 lines)
   - Start watcher when server starts
   - Handle reindex requests from watcher
   - Log reindex operations
4. Handle non-git projects gracefully

**Total**: ~250 lines of code

**Success Criteria**:
- File edits trigger reindex automatically
- Only changed files are reindexed (not full reindex)
- Works in git repositories
- Gracefully handles non-git directories
- Configurable debounce timing

### Phase 4: Polish (Week 4)

**Goal**: Production-ready quality

**Tasks**:
- Better error messages and logging
- Connection retry logic for client
- Graceful degradation on failures
- User documentation (README updates)
- Performance profiling and tuning
- Integration tests

## Performance Expectations

### Current (Standalone)
```
Every query: 7.4s
├─ Model loading: 4.6s (62%)
└─ Query execution: 2.7s (38%)
```

### With Server (After startup)
```
Server startup (once): 4.6s
├─ Embedder: 1.5s
└─ Reranker: 3.1s

Each query: ~2.7s (63% faster!)
├─ Query embedding: 0.3s
├─ Vector search: 0.005s
└─ Reranking: 2.4s

With release build:
Each query: ~0.5-1.0s (85-93% faster!)
```

### MCP Overhead
```
MCP protocol overhead: <5ms (<2% of query time)
- JSON-RPC parsing: ~1ms
- Serialization: ~2ms
- Transport (stdio): <1ms
```

## Configuration

**`.ragrep/config.toml`**:

```toml
[server]
socket_path = ".ragrep/ragrep.sock"
log_file = ".ragrep/server.log"
max_connections = 10
ping_interval_secs = 30

[server.git_watch]
enabled = true
debounce_ms = 1000  # Wait 1s after change before reindex

[mcp]
enabled = true
# Future: HTTP transport configuration
# host = "127.0.0.1"
# port = 8080
```

## User Workflows

### Workflow 1: Developer Daily Use

```bash
# Start of day - start the server
$ cd ~/my-project
$ ragrep serve &
[INFO] Loading embedder model...
[INFO] Loading reranker model...
[INFO] Server ready at .ragrep/ragrep.sock

# Use throughout the day - instant queries
$ ragrep "error handling"       # 0.5s ⚡
$ ragrep "authentication logic" # 0.5s ⚡
$ ragrep "database migrations"  # 0.5s ⚡

# End of day - stop the server
$ fg  # Bring to foreground
^C    # Ctrl+C to stop
[INFO] Shutting down gracefully...
```

### Workflow 2: Using with Claude Desktop

```bash
# Start MCP server (once per session)
$ ragrep --mcp &

# Configure Claude Desktop (~/.config/Claude/claude_desktop_config.json)
{
  "mcpServers": {
    "ragrep": {
      "command": "/path/to/ragrep",
      "args": ["--mcp"]
    }
  }
}

# Restart Claude Desktop, then ask:
You: "Search my codebase for authentication logic"
Claude: *uses search_code tool*
Claude: "I found 8 matches for authentication logic..."
```

### Workflow 3: No Server (Backwards Compatible)

```bash
# Works exactly as before
$ ragrep "search term"
[WARN] No server found, running in standalone mode...
[INFO] Loading models... (this will take a few seconds)
# Results in 7.4s
```

### Workflow 4: Multiple Projects

```bash
# Each project gets its own server
$ cd ~/project-a
$ ragrep serve &

$ cd ~/project-b  
$ ragrep serve &

# Queries automatically use the right server
$ cd ~/project-a
$ ragrep "test"  # Uses project-a's server

$ cd ~/project-b
$ ragrep "test"  # Uses project-b's server
```

## Error Handling & Edge Cases

### Stale Socket Files
**Problem**: Server crashes, socket file remains  
**Solution**: Check PID file on startup, remove stale sockets

### Multiple Servers
**Problem**: User tries to start server twice  
**Solution**: Check PID file, return error if server already running

### Model Loading Time
**Problem**: 4.6s startup feels slow  
**Solution**: Show progress indicator, explain one-time cost

### Non-Git Projects
**Problem**: Git watcher fails in non-git directories  
**Solution**: Gracefully detect and disable watcher, log warning

### Concurrent Model Access
**Problem**: Embedder/reranker aren't inherently thread-safe  
**Solution**: Already using `Mutex`, maintain current design

### Client Connection Failures
**Problem**: Server dies mid-query  
**Solution**: Client catches error, falls back to standalone

## Testing Strategy

### Unit Tests
- Protocol serialization/deserialization
- Client message handling
- Server message handling
- MCP tool invocation
- Git watcher logic

### Integration Tests
1. Start server in test mode
2. Send queries from test client
3. Verify results match standalone mode
4. Test concurrent queries
5. Test graceful shutdown

### Manual Testing Checklist

**Server Lifecycle**:
- [ ] Server starts successfully
- [ ] Multiple clients can connect
- [ ] Concurrent queries work
- [ ] Ctrl+C stops server cleanly
- [ ] Socket file removed on shutdown
- [ ] PID file removed on shutdown

**Fallback Behavior**:
- [ ] Query without server uses standalone
- [ ] Warning message is clear
- [ ] Results identical to server mode

**MCP Integration**:
- [ ] Configure Claude Desktop
- [ ] Tool appears in Claude
- [ ] Search returns results
- [ ] Results formatted correctly
- [ ] Errors handled gracefully

**Git Integration**:
- [ ] File edits trigger reindex
- [ ] Debouncing works (not every keystroke)
- [ ] Only changed files reindexed
- [ ] Non-git projects don't crash

## Security Considerations

### Unix Socket Permissions
- Socket created with `0600` (owner read/write only)
- PID file created with `0644` (owner write, all read)
- Log file created with `0640` (owner write, group read)

### MCP Stdio (Local Only)
- No network exposure
- Process isolation (one process per client)
- Inherits user permissions
- Safe for local development

### Future: HTTP Transport
- When adding HTTP transport:
  - Bind to `127.0.0.1` only (not `0.0.0.0`)
  - Add authentication (OAuth or token-based)
  - Consider TLS for sensitive codebases
  - DNS rebinding protection

## Dependencies

### New Crates (Phase 1)
```toml
[dependencies]
# For Unix socket server/client
tokio = { version = "1.0", features = ["full", "net"] }
serde_json = "1.0"
```

### New Crates (Phase 2)
```toml
[dependencies]
# For MCP integration
rust-mcp-sdk = { version = "0.7", features = ["server", "macros", "stdio"] }
```

### New Crates (Phase 3)
```toml
[dependencies]
# For git integration
git2 = "0.20"
```

## Logging & Observability

### Log Levels

**DEBUG**: Model loading, connection lifecycle, query execution
**INFO**: Server start/stop, reindex operations, client connections
**WARN**: Fallback to standalone, stale sockets, non-git directories
**ERROR**: Model load failures, connection errors, query failures

### Log Locations

- **Server logs**: `.ragrep/server.log`
- **Client logs**: stderr (visible to user)
- **MCP logs**: stderr (captured by Claude/Cursor)

### Metrics to Track

- Query latency (p50, p95, p99)
- Concurrent connections
- Cache hit rates (if caching added)
- Reindex frequency and duration
- Model memory usage

## Future Enhancements

### Short-term (3-6 months)
1. **Query caching** - Cache recent query results
2. **More MCP tools** - `list_functions`, `find_tests`, etc.
3. **HTTP transport for MCP** - Remote AI assistant access
4. **Better progress indicators** - Show what's loading

### Long-term (6-12 months)
1. **Multi-project server** - One server handles multiple projects
2. **Streaming results** - For very large result sets
3. **Distributed search** - Multiple machines for large repos
4. **Web UI** - Browser-based search interface

## Success Criteria

- [ ] Server starts in <5s, stays stable
- [ ] CLI queries in <1s when server running
- [ ] Falls back gracefully when no server
- [ ] MCP integration works with Claude Desktop
- [ ] Git changes trigger reindex automatically
- [ ] Concurrent queries handled correctly
- [ ] Clean shutdown (Ctrl+C removes socket)
- [ ] Logs helpful for debugging
- [ ] Code is maintainable (~1000 new lines total)
- [ ] No breaking changes to existing CLI
- [ ] Performance improvement: 5-10x faster queries

## References

- **MCP Specification**: https://modelcontextprotocol.io/specification/
- **rust-mcp-sdk**: https://github.com/rust-mcp-stack/rust-mcp-sdk
- **git2-rs**: https://github.com/rust-lang/git2-rs
- **MCP Research**: See `MCP_RESEARCH_SUMMARY.md` and `/tmp/FINAL_RECOMMENDATIONS.md`

---

**Last Updated**: November 23, 2025  
**Status**: Design Complete, Ready for Implementation
