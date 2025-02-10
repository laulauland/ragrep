# ragrep 🔍

> [!WARNING]
> **WIP** big time. This codebase is full of broken glass, sharp edges, and dragons.

A semantic code search tool that uses embeddings to find similar code snippets across your codebase. Use it to search for things like:

- "how do we handle http request errors?"
- "where is our data validation?"

## Features

- Semantic code search using embeddings
- Fully local, no API keys or dependencies
- Supports multiple programming languages through tree-sitter
- Fast SQLite-based storage for embeddings and code chunks
- Intelligent code chunking based on AST

## Installation

```bash
# Not actually live yet - still need to publish it 
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

## Usage

> [!IMPORTANT]
> The first time you run ragrep, it will download a model and cache it in is global data directory. This might take a minute and will use about 1.5GB of disk space.

### Indexing Your Codebase

Before searching, you need to index your codebase:

```bash
# Index the current directory
ragrep index

# Index a specific directory
ragrep index --path /path/to/your/code
```

### Searching Code

```bash
# Search for code similar to your query
ragrep "handle http request error"
```

The search results will show relevant code snippets along with their file locations, formatted in a familiar ripgrep-style output.

### Debug Mode

To see similarity scores in the output:

```bash
RUST_LOG=debug ragrep "your query"
```

## Supported Languages

- Rust
- Python
- JavaScript
- TypeScript

More languages can be added by including their respective tree-sitter parsers.

## How It Works

1. **Indexing**:
   - Scans your codebase for supported files
   - Uses tree-sitter to parse code into meaningful chunks
   - Generates embeddings for each code chunk
   - Stores chunks and embeddings in a SQLite database

2. **Searching**:
   - Converts your search query into an embedding
   - Finds code chunks with similar embeddings using vector similarity
   - Ranks and displays the most relevant results

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
