# Phase 2 Implementation Guide: MCP Integration

**Goal**: Add Model Context Protocol (MCP) support so AI assistants like Claude Desktop and Cursor can use ragrep to search code.

**Time Estimate**: 1 week  
**Lines of Code**: ~300 lines  
**Difficulty**: Intermediate-Advanced

**Prerequisites**: Phase 1 completed (server/client architecture working)

---

## 📚 Background: What is MCP and Why Do We Need It?

### The Problem

Right now, when you ask Claude "Find error handling code in my project", Claude can't actually search your code. It has no way to access your codebase.

### The Solution: Model Context Protocol (MCP)

MCP is a standard protocol that lets AI assistants call "tools" - functions that do real work. Think of it like this:

```
You: "Claude, find authentication code in my project"
  ↓
Claude: "I should use the search_code tool..."
  ↓
Claude → MCP → ragrep → searches your code → returns results
  ↓
Claude: "I found 8 matches for authentication logic. Here they are..."
```

### What We're Building

```
┌────────────────────────────────────────────────────────┐
│ Claude Desktop                                          │
│   "Find error handling code"                           │
└──────────────────────┬─────────────────────────────────┘
                       │ MCP Protocol (stdio)
                       ↓
┌────────────────────────────────────────────────────────┐
│ ragrep --mcp (MCP Server)                              │
│   Tool: search_code                                    │
│   - Load models (once)                                 │
│   - Process queries from Claude                        │
│   - Return formatted results                           │
└────────────────────────────────────────────────────────┘
```

### Key Concepts

**Tool**: A function that Claude can call (we'll implement `search_code`)
**stdio Transport**: Communication happens over standard input/output (simple!)
**rust-mcp-sdk**: The library that handles all the MCP protocol details for us

---

## 🎯 What We're Building

By the end of this phase, you'll be able to:

1. Run `ragrep --mcp` to start an MCP server
2. Configure Claude Desktop to use ragrep
3. Ask Claude to search your code
4. Get results formatted for AI consumption

---

## 📋 Implementation Milestones

1. **Add MCP Dependencies** - Get rust-mcp-sdk integrated
2. **Define Search Tool** - Create the `search_code` tool definition
3. **Implement MCP Handler** - Handle tool calls from Claude
4. **Add MCP Server Mode** - Wire up to main.rs
5. **Test with MCP Inspector** - Verify tool works
6. **Configure Claude Desktop** - Real AI assistant integration
7. **Production Polish** - Error handling and logging

---

## Milestone 1: Add MCP Dependencies

**Goal**: Get rust-mcp-sdk into the project and verify it compiles.

### Step 1.1: Add the Dependency

```bash
cargo add rust-mcp-sdk --features "server,macros,stdio"
```

This adds:
- `rust-mcp-sdk`: The core MCP library
- `server` feature: Server-side functionality
- `macros` feature: `#[mcp_tool]` macro for easy tool definitions
- `stdio` feature: Standard input/output transport

### Step 1.2: Verify Cargo.toml

Open `Cargo.toml` and verify you see:

```toml
[dependencies]
# ... existing dependencies ...
rust-mcp-sdk = { version = "0.7", features = ["server", "macros", "stdio"] }
```

### Step 1.3: Check It Compiles

```bash
cargo check
```

**Expected Output**:
```
   Compiling rust-mcp-sdk v0.7.4
   Compiling ragrep v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.34s
```

It will take a bit longer since it's compiling new dependencies.

### Step 1.4: Explore the MCP SDK

Let's see what we just added:

```bash
cargo doc --open --package rust-mcp-sdk
```

This opens the documentation. Browse around to see:
- `ServerHandler` trait (what we'll implement)
- `#[mcp_tool]` macro (how we define tools)
- Transport options (stdio, HTTP, SSE)

✅ **Milestone 1 Complete**: MCP SDK is integrated!

---

## Milestone 2: Define Search Tool

**Goal**: Create a Rust struct that represents our `search_code` tool.

### Step 2.1: Create `src/mcp.rs`

Create a new file with the basic structure:

```rust
//! MCP (Model Context Protocol) integration for ragrep
//! 
//! This module provides an MCP server that exposes ragrep's search
//! functionality to AI assistants like Claude Desktop and Cursor.

use crate::context::AppContext;
use crate::embedder::Embedding;
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use rust_mcp_sdk::{
    mcp_tool,
    schema::*,
    errors::{CallToolError, RpcError},
    server::{ServerHandler, McpServer},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The search_code tool definition
/// 
/// This macro (#[mcp_tool]) automatically:
/// - Generates JSON schema for Claude to understand the tool
/// - Creates a `tool()` method that returns MCP Tool metadata
/// - Handles serialization/deserialization
#[mcp_tool(
    name = "search_code",
    description = "Search for code snippets in the indexed codebase using natural language queries. \
                   Returns relevant code matches with file paths, line numbers, and relevance scores."
)]
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchCodeTool {
    /// Natural language search query describing what code you're looking for.
    /// Examples: "error handling", "authentication logic", "database queries"
    pub query: String,
    
    /// Maximum number of results to return. Default is 10, maximum is 20.
    #[serde(default = "default_top_n")]
    pub top_n: Option<usize>,
}

/// Default value for top_n
fn default_top_n() -> Option<usize> {
    Some(10)
}
```

**What's Happening Here**:
- `#[mcp_tool]`: This macro is magic! It generates all the boilerplate for MCP.
- `JsonSchema`: Generates JSON Schema so Claude knows what parameters to send.
- Doc comments (`///`): These become descriptions that Claude sees!
- `default_top_n`: Makes `top_n` optional with a sensible default.

### Step 2.2: Add to `src/main.rs`

```rust
mod mcp;
```

### Step 2.3: Test Tool Generation

Add a test at the bottom of `src/mcp.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_code_tool_schema() {
        // The #[mcp_tool] macro generates a tool() method
        let tool = SearchCodeTool::tool();
        
        // Verify basic properties
        assert_eq!(tool.name, "search_code");
        assert!(tool.description.contains("Search for code"));
        
        // Verify it has input schema
        assert!(tool.input_schema.is_some());
        
        println!("Tool schema: {:#?}", tool);
    }
    
    #[test]
    fn test_search_code_tool_deserialization() {
        // Test that we can deserialize JSON into our tool
        let json = r#"{
            "query": "error handling",
            "top_n": 5
        }"#;
        
        let tool: SearchCodeTool = serde_json::from_str(json).unwrap();
        assert_eq!(tool.query, "error handling");
        assert_eq!(tool.top_n, Some(5));
    }
    
    #[test]
    fn test_search_code_tool_defaults() {
        // Test default values
        let json = r#"{"query": "test"}"#;
        let tool: SearchCodeTool = serde_json::from_str(json).unwrap();
        
        assert_eq!(tool.query, "test");
        assert_eq!(tool.top_n, Some(10)); // Should use default
    }
}
```

### Step 2.4: Run the Tests

```bash
cargo test test_search_code_tool
```

**Expected Output**:
```
running 3 tests
test mcp::tests::test_search_code_tool_defaults ... ok
test mcp::tests::test_search_code_tool_deserialization ... ok
test mcp::tests::test_search_code_tool_schema ... ok

test result: ok. 3 passed; 0 failed
```

The first test will also print the generated schema. You should see something like:

```
Tool schema: Tool {
    name: "search_code",
    description: "Search for code snippets...",
    input_schema: Some({
        "type": "object",
        "properties": {
            "query": { "type": "string", ... },
            "top_n": { "type": "integer", ... }
        },
        "required": ["query"]
    })
}
```

This is what Claude will see!

✅ **Milestone 2 Complete**: Tool definition is ready!

---

## Milestone 3: Implement MCP Handler

**Goal**: Create the handler that processes tool calls from Claude.

### Step 3.1: Add the Handler Struct

Add this to `src/mcp.rs`:

```rust
/// MCP Server Handler for ragrep
/// 
/// This struct implements the ServerHandler trait to handle requests
/// from MCP clients (like Claude Desktop)
pub struct RagrepMcpHandler {
    /// Shared reference to AppContext (contains loaded models)
    context: Arc<AppContext>,
}

impl RagrepMcpHandler {
    pub fn new(context: Arc<AppContext>) -> Self {
        Self { context }
    }
}
```

### Step 3.2: Implement ServerHandler Trait

This is the core of MCP integration:

```rust
#[async_trait]
impl ServerHandler for RagrepMcpHandler {
    /// List available tools
    /// Claude calls this on startup to discover what tools we offer
    async fn handle_list_tools_request(
        &self,
        _request: ListToolsRequest,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        debug!("[MCP] Listing tools");
        
        Ok(ListToolsResult {
            tools: vec![SearchCodeTool::tool()],
            meta: None,
            next_cursor: None,
        })
    }

    /// Handle tool invocation
    /// This is called when Claude decides to use the search_code tool
    async fn handle_call_tool_request(
        &self,
        request: CallToolRequest,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<CallToolResult, CallToolError> {
        debug!("[MCP] Call tool request: {}", request.tool_name());
        
        // Verify it's our tool
        if request.tool_name() != "search_code" {
            return Err(CallToolError::unknown_tool(
                request.tool_name().to_string(),
            ));
        }

        // Extract arguments
        let args = request.arguments()
            .ok_or_else(|| CallToolError::invalid_params())?;
        
        // Deserialize into our SearchCodeTool struct
        let tool: SearchCodeTool = serde_json::from_value(args.clone())
            .map_err(|e| {
                debug!("[MCP] Failed to parse arguments: {}", e);
                CallToolError::invalid_params()
            })?;

        // Validate and normalize top_n
        let top_n = tool.top_n
            .unwrap_or(10)  // Default to 10
            .clamp(1, 20);   // Clamp between 1 and 20

        info!(
            "[MCP] Searching for '{}' (top_n={})",
            tool.query, top_n
        );

        // Execute the search
        match self.execute_search(&tool.query, top_n).await {
            Ok(formatted_results) => {
                debug!("[MCP] Search completed successfully");
                
                // Return results as text content
                Ok(CallToolResult::text_content(vec![
                    TextContent::from(formatted_results)
                ]))
            }
            Err(e) => {
                debug!("[MCP] Search failed: {}", e);
                Err(CallToolError::internal_error(
                    format!("Search failed: {}", e)
                ))
            }
        }
    }
}
```

**What's Happening**:
- `handle_list_tools_request`: Called once when Claude starts. We return our tool list.
- `handle_call_tool_request`: Called when Claude uses the tool. We execute the search.
- Error handling: Use MCP-specific errors (`CallToolError`)
- Logging: Prefix with `[MCP]` to distinguish from other logs

### Step 3.3: Implement the Search Logic

Add this method to the `impl RagrepMcpHandler` block:

```rust
impl RagrepMcpHandler {
    // ... existing new() method ...
    
    /// Execute a search and format results for Claude
    async fn execute_search(&self, query: &str, top_n: usize) -> Result<String> {
        use std::time::Instant;
        
        let start = Instant::now();
        
        // Step 1: Generate embedding
        let Embedding(query_embedding) = self.context.embedder
            .embed_query(query)
            .await?;
        
        // Step 2: Search database
        let initial_results = self.context.db
            .find_similar_chunks(&query_embedding, top_n)?;
        
        if initial_results.is_empty() {
            return Ok("No results found.".to_string());
        }
        
        // Step 3: Rerank
        let documents: Vec<String> = initial_results
            .iter()
            .map(|(text, _, _, _, _, _)| text.clone())
            .collect();
        
        let reranked = self.context.reranker
            .rerank(query, &documents, Some(top_n))?;
        
        // Step 4: Format results for Claude
        let mut output = String::new();
        output.push_str(&format!(
            "Found {} results for '{}' in {}ms:\n\n",
            reranked.len(),
            query,
            start.elapsed().as_millis()
        ));
        
        for (idx, (result_idx, score)) in reranked.iter().enumerate() {
            let (text, file_path, start_line, end_line, node_type, _) =
                &initial_results[*result_idx];
            
            output.push_str(&format!(
                "{}. {} (lines {}-{}) [score: {:.3}]\n",
                idx + 1,
                file_path,
                start_line,
                end_line,
                score
            ));
            
            // Add code snippet (first 5 lines max to keep readable)
            let lines: Vec<&str> = text.lines().take(5).collect();
            for line in lines {
                output.push_str("   ");
                output.push_str(line);
                output.push('\n');
            }
            
            if text.lines().count() > 5 {
                output.push_str("   ...\n");
            }
            
            output.push('\n');
        }
        
        Ok(output)
    }
}
```

**Formatting Strategy**:
- Clear numbering (1. 2. 3.)
- File path and line numbers (Claude can reference these)
- Relevance score (helps Claude understand quality)
- Code snippet preview (limited to 5 lines to avoid overwhelming)
- Clean separation between results

### Step 3.4: Test the Handler

Add this test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // ... existing tests ...
    
    #[tokio::test]
    async fn test_mcp_handler_tool_list() {
        // Create a test context
        let context = Arc::new(
            AppContext::new(std::path::Path::new(".")).await.unwrap()
        );
        
        let handler = RagrepMcpHandler::new(context);
        
        // Call handle_list_tools_request
        let request = ListToolsRequest::default();
        let result = handler.handle_list_tools_request(
            request,
            Arc::new(/* mock runtime */),
        ).await;
        
        // Should succeed
        assert!(result.is_ok());
        
        // Should have our tool
        let tools = result.unwrap().tools;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search_code");
    }
}
```

Note: This test requires mocking the runtime. For now, we'll skip it and test manually instead.

✅ **Milestone 3 Complete**: Handler can process tool calls!

---

## Milestone 4: Add MCP Server Mode

**Goal**: Wire up the MCP handler to `main.rs` so we can start the MCP server.

### Step 4.1: Add Server Startup Function

Add to `src/mcp.rs`:

```rust
use rust_mcp_sdk::server::{server_runtime, StdioTransport, TransportOptions};

/// Start the MCP server using stdio transport
pub async fn start_mcp_server(context: Arc<AppContext>) -> Result<()> {
    info!("[MCP] Starting MCP server...");
    
    // Define server details
    let server_details = InitializeResult {
        server_info: Implementation {
            name: "ragrep".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: Some("ragrep - Semantic Code Search".to_string()),
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools {
                list_changed: None,
            }),
            ..Default::default()
        },
        meta: None,
        instructions: Some(
            "Use the search_code tool to search for code in the indexed codebase. \
             Provide natural language queries like 'error handling' or 'authentication logic'."
                .to_string()
        ),
        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
    };
    
    // Create stdio transport
    let transport = StdioTransport::new(TransportOptions::default())?;
    
    // Create handler
    let handler = RagrepMcpHandler::new(context);
    
    // Create and start server
    info!("[MCP] Server ready, waiting for requests...");
    let server = server_runtime::create_server(server_details, transport, handler);
    server.start().await
}
```

**What's Happening**:
- `InitializeResult`: Describes our server to Claude
- `ServerCapabilities`: Tells Claude we support tools
- `instructions`: Gives Claude guidance on how to use our tool
- `StdioTransport`: Uses stdin/stdout for communication
- `server_runtime::create_server`: Creates the MCP server
- `server.start().await`: Runs until Ctrl+C

### Step 4.2: Update `src/main.rs`

Add the MCP command:

```rust
#[derive(Subcommand)]
enum Commands {
    Index { 
        #[arg(short, long)] 
        path: Option<String> 
    },
    Serve {},
    Mcp {},  // ← Add this
}
```

Add the handler in the match statement:

```rust
(None, Some(Commands::Mcp {})) => {
    // Load models (this is the slow part)
    let context = AppContext::new(&current_dir).await?;
    
    // Start MCP server
    mcp::start_mcp_server(Arc::new(context)).await?;
}
```

### Step 4.3: Build and Test Startup

```bash
cargo build
./target/debug/rag --mcp
```

**Expected Output**:
```
[INFO] Loading models...
[TIMING] Embedder model loading: 1.465s
[TIMING] Reranker model loading: 3.144s
[INFO] [MCP] Starting MCP server...
[INFO] [MCP] Server ready, waiting for requests...
```

The server is now waiting for input on stdin!

### Step 4.4: Test Manual Input

With the server running, type this JSON and press Enter:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
```

**Expected Response**:
```json
{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"search_code","description":"Search for code snippets...","inputSchema":{...}}]}}
```

Great! The server responds to MCP requests!

Press Ctrl+C to stop.

✅ **Milestone 4 Complete**: MCP server mode works!

---

## Milestone 5: Test with MCP Inspector

**Goal**: Use the official MCP Inspector tool to test our server.

### Step 5.1: Start the Server

```bash
./target/debug/rag --mcp
```

Leave it running.

### Step 5.2: Open MCP Inspector

In your browser, go to:
https://modelcontextprotocol.io/docs/tools/inspector

### Step 5.3: Configure Inspector

In the Inspector, click "Connect to Server" and enter:

- **Transport**: stdio
- **Command**: Full path to your binary (e.g., `/Users/you/ragrep/target/debug/rag`)
- **Arguments**: `--mcp`
- **Environment Variables**: (leave empty)

Click "Connect".

### Step 5.4: Verify Connection

You should see:
- Server name: "ragrep"
- Version: (your version)
- Tools: 1 tool listed
  - search_code

### Step 5.5: Test the search_code Tool

In the Inspector:

1. Click on "search_code" tool
2. Fill in parameters:
   - query: "error handling"
   - top_n: 5
3. Click "Call Tool"

**Expected Result**:

You should see formatted search results with:
- File paths and line numbers
- Code snippets
- Relevance scores

### Step 5.6: Test Edge Cases

Try these in the Inspector:

**Empty query**:
- query: ""
- Expected: Error message

**Invalid top_n**:
- query: "test"
- top_n: 100
- Expected: Clamped to 20 results

**Query with no results**:
- query: "xyzabc123nonexistent"
- Expected: "No results found."

### Step 5.7: Check Server Logs

In your server terminal, you should see:

```
[DEBUG] [MCP] Call tool request: search_code
[INFO] [MCP] Searching for 'error handling' (top_n=5)
[DEBUG] [MCP] Search completed successfully
```

✅ **Milestone 5 Complete**: MCP Inspector confirms it works!

---

## Milestone 6: Configure Claude Desktop

**Goal**: Get Claude Desktop using ragrep!

### Step 6.1: Find Claude Config File

The config file location depends on your OS:

**macOS**:
```bash
~/.config/Claude/claude_desktop_config.json
```

**Windows**:
```bash
%APPDATA%\Claude\claude_desktop_config.json
```

**Linux**:
```bash
~/.config/Claude/claude_desktop_config.json
```

### Step 6.2: Create/Edit Config

Open the file (create it if it doesn't exist) and add:

```json
{
  "mcpServers": {
    "ragrep": {
      "command": "/full/path/to/your/ragrep/target/debug/rag",
      "args": ["--mcp"]
    }
  }
}
```

**Important**: Use the FULL absolute path! Find it with:

```bash
pwd
# Then append /target/debug/rag
```

Example:
```json
{
  "mcpServers": {
    "ragrep": {
      "command": "/Users/yourname/projects/ragrep/target/debug/rag",
      "args": ["--mcp"]
    }
  }
}
```

### Step 6.3: Restart Claude Desktop

Completely quit Claude Desktop (Cmd+Q on Mac) and restart it.

### Step 6.4: Verify Connection

In Claude Desktop, look for the 🔌 icon in the bottom right. Click it.

You should see:
- ragrep: Connected
- Tools: search_code

If you see "Failed to connect", check:
- Path is absolute and correct
- Binary is executable (`chmod +x target/debug/rag`)
- You're in a project with an indexed database

### Step 6.5: Test with Claude!

In Claude Desktop, try:

**Prompt 1**:
> "Use ragrep to search for error handling code in my project"

Claude should:
1. Recognize it should use the search_code tool
2. Call it with query="error handling"
3. Display the results to you

**Prompt 2**:
> "Find authentication logic and explain how it works"

Claude should:
1. Use search_code to find auth code
2. Read the results
3. Explain the authentication flow

**Prompt 3**:
> "What database queries do we have?"

Claude should search for database-related code.

### Step 6.6: Debug Connection Issues

If Claude shows "Failed to connect":

**Check 1: Can you run manually?**
```bash
/full/path/to/rag --mcp
```

**Check 2: Is the path correct?**
```bash
cat ~/.config/Claude/claude_desktop_config.json
```

**Check 3: View Claude's logs**
macOS:
```bash
tail -f ~/Library/Logs/Claude/mcp*.log
```

**Check 4: Is database indexed?**
```bash
cd /your/project
./target/debug/rag index
```

### Step 6.7: Celebrate! 🎉

Once you see Claude successfully using your tool, you've done it!

**Example Conversation**:

> **You**: "Find all the error handling code"
> 
> **Claude**: "I'll search for error handling code using ragrep."
> 
> *[Uses search_code tool]*
> 
> **Claude**: "I found 8 matches for error handling. Here are the main ones:
> 1. src/main.rs (lines 45-67) - Main error handler
> 2. src/server.rs (lines 123-145) - Connection error handling
> ..."

✅ **Milestone 6 Complete**: Claude Desktop integration works!

---

## Milestone 7: Production Polish

**Goal**: Make the MCP integration production-ready.

### Step 7.1: Better Error Messages

Update `execute_search` to give Claude more helpful errors:

```rust
async fn execute_search(&self, query: &str, top_n: usize) -> Result<String> {
    // Validate query
    if query.trim().is_empty() {
        return Ok("Error: Query cannot be empty. Please provide a search term.".to_string());
    }
    
    if query.len() > 500 {
        return Ok("Error: Query too long. Please use a shorter search term (max 500 chars).".to_string());
    }
    
    // ... rest of search logic ...
    
    // Better "no results" message
    if initial_results.is_empty() {
        return Ok(format!(
            "No results found for '{}'. Try:\n\
             - Using different keywords\n\
             - Being more general\n\
             - Checking if the codebase is indexed (run: ragrep index)",
            query
        ));
    }
    
    // ... rest of formatting ...
}
```

### Step 7.2: Add Result Limits

Update the formatting to handle large results better:

```rust
// In execute_search, after reranking:

let results_to_show = reranked.len().min(top_n);

output.push_str(&format!(
    "Found {} results for '{}' in {}ms (showing top {}):\n\n",
    reranked.len(),
    query,
    start.elapsed().as_millis(),
    results_to_show
));

for (idx, (result_idx, score)) in reranked.iter().take(results_to_show).enumerate() {
    // ... existing formatting ...
    
    // Truncate very long snippets
    let lines: Vec<&str> = text.lines().take(10).collect();
    for (i, line) in lines.iter().enumerate().take(5) {
        // Truncate long lines
        let display_line = if line.len() > 120 {
            format!("{}...", &line[..120])
        } else {
            line.to_string()
        };
        
        output.push_str("   ");
        output.push_str(&display_line);
        output.push('\n');
    }
    
    if text.lines().count() > 5 {
        output.push_str(&format!("   ... ({} more lines)\n", text.lines().count() - 5));
    }
    
    output.push('\n');
}
```

### Step 7.3: Add Timing Breakdowns

Help debug performance:

```rust
async fn execute_search(&self, query: &str, top_n: usize) -> Result<String> {
    use std::time::Instant;
    
    let total_start = Instant::now();
    let mut timings = Vec::new();
    
    // Embedding
    let start = Instant::now();
    let Embedding(query_embedding) = self.context.embedder.embed_query(query).await?;
    timings.push(("embedding", start.elapsed()));
    
    // Database search
    let start = Instant::now();
    let initial_results = self.context.db.find_similar_chunks(&query_embedding, top_n)?;
    timings.push(("database", start.elapsed()));
    
    if initial_results.is_empty() {
        return Ok("No results found.".to_string());
    }
    
    // Reranking
    let start = Instant::now();
    let documents: Vec<String> = initial_results.iter().map(|(text, _, _, _, _, _)| text.clone()).collect();
    let reranked = self.context.reranker.rerank(query, &documents, Some(top_n))?;
    timings.push(("reranking", start.elapsed()));
    
    // Format results...
    let mut output = String::new();
    
    // ... existing formatting ...
    
    // Add timing breakdown at the end
    output.push_str("\n---\n");
    output.push_str(&format!("Total time: {}ms\n", total_start.elapsed().as_millis()));
    output.push_str("Breakdown:\n");
    for (name, duration) in timings {
        output.push_str(&format!("  - {}: {}ms\n", name, duration.as_millis()));
    }
    
    Ok(output)
}
```

### Step 7.4: Add Health Check Logging

At server startup, verify everything is ready:

```rust
pub async fn start_mcp_server(context: Arc<AppContext>) -> Result<()> {
    info!("[MCP] Starting MCP server...");
    
    // Verify database is accessible
    info!("[MCP] Checking database connection...");
    // Add a simple query to verify DB works
    
    // Verify models are loaded
    info!("[MCP] Models loaded:");
    info!("[MCP]   - Embedder: ✓");
    info!("[MCP]   - Reranker: ✓");
    
    // ... rest of startup ...
}
```

### Step 7.5: Add Graceful Shutdown

```rust
pub async fn start_mcp_server(context: Arc<AppContext>) -> Result<()> {
    // ... server setup ...
    
    let server = server_runtime::create_server(server_details, transport, handler);
    
    // Set up Ctrl+C handler
    tokio::select! {
        result = server.start() => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("[MCP] Received shutdown signal, stopping gracefully...");
        }
    }
    
    info!("[MCP] Server stopped");
    Ok(())
}
```

### Step 7.6: Write Integration Tests

Create `tests/mcp_integration_test.rs`:

```rust
use std::process::{Command, Stdio};
use std::io::Write;
use std::thread;
use std::time::Duration;

#[test]
fn test_mcp_server_startup() {
    // Build first
    let status = Command::new("cargo")
        .args(&["build"])
        .status()
        .expect("Failed to build");
    assert!(status.success());

    // Start MCP server
    let mut child = Command::new("./target/debug/rag")
        .arg("--mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start MCP server");

    // Give it time to initialize
    thread::sleep(Duration::from_secs(6));

    // Send a tools/list request
    let stdin = child.stdin.as_mut().unwrap();
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
"#;
    stdin.write_all(request.as_bytes()).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    // Give it time to respond
    thread::sleep(Duration::from_secs(1));

    // Kill the server
    child.kill().expect("Failed to kill server");

    // If we got here without panicking, test passed
}
```

Run it:
```bash
cargo test --test mcp_integration_test
```

### Step 7.7: Document for Users

Create a user guide in `docs/MCP_USAGE.md`:

```markdown
# Using ragrep with Claude Desktop

## Setup

1. Index your codebase:
   ```bash
   cd /your/project
   ragrep index
   ```

2. Configure Claude Desktop (`~/.config/Claude/claude_desktop_config.json`):
   ```json
   {
     "mcpServers": {
       "ragrep": {
         "command": "/path/to/ragrep/target/debug/rag",
         "args": ["--mcp"]
       }
     }
   }
   ```

3. Restart Claude Desktop

## Usage Examples

**Find code**:
> "Use ragrep to find error handling code"

**Explain code**:
> "Search for authentication logic and explain how it works"

**Specific queries**:
> "Find all database query functions"
> "Show me HTTP request handlers"
> "Where do we parse JSON?"

## Troubleshooting

**Connection failed**:
- Check the path in claude_desktop_config.json is absolute
- Verify the binary exists and is executable
- Check that you indexed the codebase

**No results**:
- Make sure you ran `ragrep index` in your project
- Try broader search terms
- Check Claude's MCP logs

## Logs

Server logs: `.ragrep/server.log`
Claude logs: `~/Library/Logs/Claude/mcp*.log` (macOS)
```

✅ **Milestone 7 Complete**: Production-ready MCP integration!

---

## 🎉 Phase 2 Complete!

You now have:
- ✅ MCP server mode (`ragrep --mcp`)
- ✅ `search_code` tool for AI assistants
- ✅ Claude Desktop integration
- ✅ Cursor IDE integration (same config pattern)
- ✅ Error handling and logging
- ✅ Production-ready code

## 🧪 Final Verification

### Test 1: Manual MCP Server

```bash
./target/debug/rag --mcp
# Should start and wait for input
# Press Ctrl+C to stop
```

### Test 2: MCP Inspector

1. Go to https://modelcontextprotocol.io/docs/tools/inspector
2. Connect to your MCP server
3. Call search_code tool
4. Verify results

### Test 3: Claude Desktop

1. Configure Claude Desktop
2. Restart Claude
3. Ask: "Use ragrep to find error handling code"
4. Verify Claude uses the tool and shows results

### Test 4: Performance

With debug logging:
```bash
RUST_LOG=debug ./target/debug/rag --mcp
```

Then use it and check logs show:
- `[MCP] Searching for '...'`
- Timing breakdowns
- No errors

### Test 5: Error Handling

In MCP Inspector, try:
- Empty query → helpful error
- Very long query → handled gracefully
- Query with no results → clear message

## 📊 What We've Achieved

```
BEFORE Phase 2:
- ragrep only accessible via CLI
- Claude can't search your code
- Manual copy-paste needed

AFTER Phase 2:
- ragrep accessible via CLI AND MCP
- Claude can search automatically
- AI-assisted code exploration! 🚀
```

## 🐛 Common Issues & Solutions

### "Server failed to start"
**Cause**: Models not found or database not indexed
**Fix**:
```bash
ragrep index  # Make sure you indexed first
```

### "Claude can't find the tool"
**Cause**: Config path is wrong or relative
**Fix**: Use absolute path in config:
```bash
pwd  # Get current directory
# Use full path like /Users/you/project/target/debug/rag
```

### "Connection reset"
**Cause**: Server crashed during startup
**Fix**: Check logs, ensure enough memory, try release build:
```bash
cargo build --release
# Use target/release/rag instead
```

### "No results found" always
**Cause**: Wrong working directory
**Fix**: Start MCP server from the indexed project directory

## 📚 What You Learned

- MCP protocol basics
- Tool definition with `#[mcp_tool]` macro
- ServerHandler trait implementation
- stdio transport communication
- AI assistant integration
- Error handling for AI consumption
- Result formatting for readability

## 🚀 Next Steps

**Phase 3**: Add git-based reindexing so the index stays current automatically.

**Before Starting Phase 3**:
1. Commit your Phase 2 work
2. Test with real Claude conversations
3. Gather feedback on result formatting
4. Celebrate! You've built an AI-accessible code search tool! 🎉

---

## 🎓 Advanced: Understanding What Just Happened

### The MCP Request Flow

```
1. You ask Claude: "Find error handling code"
   ↓
2. Claude decides: "I should use the search_code tool"
   ↓
3. Claude → MCP → ragrep: tools/call
   {
     "name": "search_code",
     "arguments": {"query": "error handling", "top_n": 10}
   }
   ↓
4. ragrep:
   - Deserializes into SearchCodeTool struct
   - Calls execute_search()
   - Runs embedding + vector search + reranking
   - Formats results as text
   ↓
5. ragrep → MCP → Claude: CallToolResult
   {
     "content": [{"type": "text", "text": "Found 8 results..."}]
   }
   ↓
6. Claude reads results and responds to you!
```

### Why stdio Transport?

**Pros**:
- Simple: just JSON over stdin/stdout
- Secure: process isolation
- No networking: works offline
- Perfect for local development

**Cons**:
- One client at a time (Claude starts new process per session)
- Not for remote access

**Alternative**: HTTP transport (Phase 2.5 if needed)

### The Magic of `#[mcp_tool]`

This macro generates:
- `tool()` method returning MCP Tool metadata
- JSON Schema from struct fields
- Serialization/deserialization
- Type-safe argument handling

Without it, you'd write ~100 lines of boilerplate!

---

**Congratulations!** You've added professional-grade MCP integration to ragrep. Claude can now search your code as easily as you can. This is the future of developer tools! 🚀
