# ragrep 🔍

> [!WARNING]
> **WIP** big time. This codebase is full of broken glass, sharp edges, and dragons.

A semantic code search tool that uses embeddings to find similar code snippets across your codebase. Use it to search for things like:

- "how do we handle http request errors?"
- "where is our data validation?"

## Features

- **Semantic search** - Find code by meaning, not just keywords
- **Fully local** - No API keys, no cloud dependencies
- **Fast server mode** - Keep models loaded for 10x faster queries (0.5s vs 7s)
- **Auto-reindex** - File changes trigger instant reindexing (~200ms)
- **Multi-language** - Rust, Python, JavaScript, TypeScript via tree-sitter
- **Smart caching** - Reuse embeddings for unchanged code chunks

## Installation

```bash
cargo install ragrep
```

### Building from Source

#### Prerequisites

- Rust toolchain (1.75.0 or later recommended)
- SQLite 3.x

```bash
git clone https://github.com/yourusername/ragrep.git
cd ragrep
cargo build --release
```

The binary will be available at `target/release/ragrep`

## Quick Start

> [!IMPORTANT]
> First run downloads models (~1.5GB) to `~/.cache/ragrep/models/`

### 1. Index Your Codebase

```bash
ragrep index
```

### 2. Start the Server (Recommended)

```bash
# Start server in background
ragrep serve &

# Server loads models once (4.6s)
# Queries now run in 0.5s instead of 7s
# File edits auto-reindex in ~200ms
```

### 3. Search

```bash
ragrep "handle http request error"
```

## Usage Modes

### Server Mode (Fast, Recommended)

```bash
# Terminal 1: Start server
$ ragrep serve
[INFO] Loading embedder model...
[INFO] Loading reranker model...
[INFO] File watcher started
[INFO] Server listening on .ragrep/ragrep.sock

# Terminal 2: Search (uses server automatically)
$ ragrep "error handling"  # 0.5s ⚡

# Edit files - auto-reindexed!
$ vim src/main.rs  # Save triggers reindex (~200ms)
```

### Standalone Mode (Fallback)

```bash
# Works without server (loads models each time)
$ ragrep "search query"  # 7s 🐌
```

## Auto-Reindexing

When server is running:
- Watches `.rs`, `.py`, `.js`, `.ts` files
- Respects `.gitignore` and `.ragrepignore`
- Debounced (default 1000ms)
- Smart caching reuses embeddings for unchanged chunks
- Only git repositories (gracefully disabled otherwise)

Configuration in `.ragrep/config.toml`:
```toml
[server.git_watch]
enabled = true
debounce_ms = 1000
```

## Supported Languages

- Rust (`.rs`)
- Python (`.py`)
- JavaScript (`.js`)
- TypeScript (`.ts`)

More languages can be added via tree-sitter parsers.

## How It Works

**Indexing**:
- Scan files (respects `.gitignore` and `.ragrepignore`)
- Parse with tree-sitter into semantic chunks (functions, classes, etc.)
- Generate 1024-dim embeddings (mixedbread-ai/mxbai-embed-large-v1)
- Store in SQLite with `sqlite-vec` extension

**Searching**:
- Embed query → vector similarity search → rerank with BAAI/bge-reranker-base
- Results show file path, line numbers, and relevant code

**Auto-Reindexing**:
- Watch source files for changes
- Incremental reindex with smart embedding cache
- Only re-embed modified chunks (10-15x faster than full reindex)

## Development

See [DEVELOPING.md](DEVELOPING.md) for architecture, setup, and contribution guidelines.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
