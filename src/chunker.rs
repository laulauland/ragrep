use anyhow::{Context, Result};
use log::debug;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use tree_sitter_javascript::LANGUAGE as JS_LANGUAGE;
use tree_sitter_python::LANGUAGE as PYTHON_LANGUAGE;
use tree_sitter_rust::LANGUAGE as RUST_LANGUAGE;
use tree_sitter_typescript::LANGUAGE_TYPESCRIPT as TS_LANGUAGE;

#[derive(Debug, Serialize)]
pub struct CodeChunk {
    pub content: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub kind: String, // "function", "class", "method", etc.
    pub leading_comments: String,
    pub parent_name: Option<String>, // Name of original function/class if this is a sub-chunk
}

impl CodeChunk {
    pub fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.content.hash(&mut hasher);
        self.kind.hash(&mut hasher);
        hasher.finish()
    }
}

pub struct Chunker {
    parser: Parser,
    // max_chunk_size: usize,
    // overlap_percentage: usize,
}

impl Chunker {
    pub fn new() -> Result<Self> {
        let parser = Parser::new();

        Ok(Self {
            parser,
            // max_chunk_size: 1000,   // Maximum tokens per chunk
            // overlap_percentage: 15, // 15% overlap between chunks
        })
    }

    // fn split_large_chunk(&self, chunk: CodeChunk) -> Vec<CodeChunk> {
    //     let content = chunk.content.as_str();
    //     let tokens: Vec<&str> = content.split_whitespace().collect();

    //     if tokens.len() <= self.max_chunk_size {
    //         return vec![chunk];
    //     }

    //     let overlap_size = (self.max_chunk_size * self.overlap_percentage) / 100;
    //     let step_size = self.max_chunk_size - overlap_size;
    //     let mut chunks = Vec::new();
    //     let mut start_token = 0;

    //     // Extract any inline comments from the content
    //     let mut inline_comments = String::new();
    //     if let Some(comment_start) = content.find("//") {
    //         inline_comments = content[comment_start..]
    //             .lines()
    //             .next()
    //             .unwrap_or("")
    //             .to_string();
    //     }

    //     while start_token < tokens.len() {
    //         let end_token = (start_token + self.max_chunk_size).min(tokens.len());
    //         let sub_content = tokens[start_token..end_token].join(" ");

    //         // Calculate byte offsets for the sub-chunk
    //         let start_byte =
    //             chunk.start_byte + content[..content.find(tokens[start_token]).unwrap_or(0)].len();
    //         let end_byte = if end_token < tokens.len() {
    //             chunk.start_byte
    //                 + content[..content.find(tokens[end_token - 1]).unwrap_or(0)].len()
    //                 + tokens[end_token - 1].len()
    //         } else {
    //             chunk.end_byte
    //         };

    //         // Combine leading comments with any inline comments
    //         let mut combined_comments = chunk.leading_comments.clone();
    //         if !inline_comments.is_empty() {
    //             if !combined_comments.is_empty() {
    //                 combined_comments.push('\n');
    //             }
    //             combined_comments.push_str(&inline_comments);
    //         }

    //         chunks.push(CodeChunk {
    //             content: sub_content,
    //             start_byte,
    //             end_byte,
    //             start_line: 0,
    //             end_line: 0,
    //             kind: chunk.kind.clone(),
    //             leading_comments: combined_comments, // Include comments in all chunks
    //             parent_name: Some(format!("{} (part {})", chunk.kind, chunks.len() + 1)),
    //         });

    //         if end_token >= tokens.len() {
    //             break;
    //         }
    //         start_token += step_size;
    //     }

    //     chunks
    // }

    pub fn chunk_file(&mut self, path: &Path, content: &str) -> Result<Vec<CodeChunk>> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let language: Language = match ext {
            "rs" => RUST_LANGUAGE.into(),
            "py" => PYTHON_LANGUAGE.into(),
            "ts" => TS_LANGUAGE.into(),
            "js" => JS_LANGUAGE.into(),
            _ => return Err(anyhow::anyhow!("Unsupported file extension: {}", ext)),
        };

        self.parser.set_language(&language)?;
        let tree = self
            .parser
            .parse(content, None)
            .with_context(|| "Failed to parse file")?;

        let query_str = match ext {
            "rs" => {
                r#"
                ([(line_comment)* (block_comment)*] @comment
                 [(function_item) @function
                  (impl_item) @impl
                  (trait_item) @trait])
                "#
            }
            "py" => {
                r#"
                ((comment)* @comment
                 (function_definition) @function)
                "#
            }
            "js" | "ts" => {
                r#"
                ((comment)* @comment
                 [(function_declaration) @function
                  (method_definition) @function])
                "#
            }
            _ => return Ok(vec![]),
        };

        let query = Query::new(&language, query_str)?;
        let mut cursor = QueryCursor::new();
        let mut chunks = Vec::new();
        let mut seen_hashes = HashSet::new();

        // Pre-calculate line starts for efficient line number lookup
        let line_starts: Vec<_> = content
            .match_indices('\n')
            .map(|(i, _)| i)
            .chain(std::iter::once(content.len()))
            .collect();

        let mut query_matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
        while let Some(match_) = query_matches.next() {
            let mut comments = String::new();
            let mut chunk_content = String::new();

            for capture in match_.captures {
                let capture_text = &content[capture.node.byte_range()];

                if query.capture_names()[capture.index as usize] == "comment" {
                    comments.push_str(capture_text);
                    comments.push('\n');
                } else {
                    chunk_content = capture_text.to_string();
                }
            }

            if !chunk_content.is_empty() {
                let start_byte = match_.captures[0].node.start_byte();
                let end_byte = match_.captures[0].node.end_byte();

                // Convert byte offsets to line numbers
                let start_line = line_starts
                    .iter()
                    .position(|&pos| pos >= start_byte)
                    .unwrap_or(0)
                    + 1;
                let end_line = line_starts
                    .iter()
                    .position(|&pos| pos >= end_byte)
                    .unwrap_or(line_starts.len())
                    + 1;

                let chunk = CodeChunk {
                    content: chunk_content,
                    start_byte,
                    end_byte,
                    start_line,
                    end_line,
                    kind: query.capture_names()[match_.captures[0].index as usize].to_string(),
                    leading_comments: comments,
                    parent_name: None,
                };

                let hash = chunk.hash();
                if seen_hashes.insert(hash) {
                    chunks.push(chunk);
                } else {
                    debug!(
                        "Duplicate chunk detected for file {} at lines {}-{}",
                        path.display(),
                        start_line,
                        end_line
                    );
                }
            }
        }

        chunks.sort_by_key(|chunk| chunk.start_byte);
        Ok(chunks)
    }
}
