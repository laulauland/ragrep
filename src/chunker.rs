use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use tree_sitter_javascript::language as js_language;
use tree_sitter_python::language as py_language;
use tree_sitter_rust::language as rust_language;
use tree_sitter_typescript::language_typescript as ts_language;

#[derive(Debug, Serialize)]
pub struct CodeChunk {
    pub content: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: String, // "function", "class", "method", etc.
    pub leading_comments: String,
    pub parent_name: Option<String>, // Name of original function/class if this is a sub-chunk
}

pub struct Chunker {
    parser: Parser,
    languages: Vec<(Language, Vec<String>)>,
    max_chunk_size: usize,
    overlap_percentage: usize,
}

impl Chunker {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();

        // Initialize supported languages with their file extensions
        let languages = vec![
            (rust_language(), vec!["rs".to_string()]),
            (py_language(), vec!["py".to_string()]),
            (js_language(), vec!["js".to_string()]),
            (tree_sitter_typescript::language_typescript(), vec!["ts".to_string()]),
        ];

        Ok(Self { 
            parser,
            languages,
            max_chunk_size: 1000, // Maximum tokens per chunk
            overlap_percentage: 15, // 15% overlap between chunks
        })
    }

    fn split_large_chunk(&self, chunk: CodeChunk) -> Vec<CodeChunk> {
        let content = chunk.content.as_str();
        let tokens: Vec<&str> = content.split_whitespace().collect();
        
        if tokens.len() <= self.max_chunk_size {
            return vec![chunk];
        }

        let overlap_size = (self.max_chunk_size * self.overlap_percentage) / 100;
        let step_size = self.max_chunk_size - overlap_size;
        let mut chunks = Vec::new();
        let mut start_token = 0;

        // Extract any inline comments from the content
        let mut inline_comments = String::new();
        if let Some(comment_start) = content.find("//") {
            inline_comments = content[comment_start..].lines().next().unwrap_or("").to_string();
        }

        while start_token < tokens.len() {
            let end_token = (start_token + self.max_chunk_size).min(tokens.len());
            let sub_content = tokens[start_token..end_token].join(" ");
            
            // Calculate byte offsets for the sub-chunk
            let start_byte = chunk.start_byte + content[..content.find(tokens[start_token]).unwrap_or(0)].len();
            let end_byte = if end_token < tokens.len() {
                chunk.start_byte + content[..content.find(tokens[end_token-1]).unwrap_or(0)].len() 
                    + tokens[end_token-1].len()
            } else {
                chunk.end_byte
            };

            // Combine leading comments with any inline comments
            let mut combined_comments = chunk.leading_comments.clone();
            if !inline_comments.is_empty() {
                if !combined_comments.is_empty() {
                    combined_comments.push('\n');
                }
                combined_comments.push_str(&inline_comments);
            }

            chunks.push(CodeChunk {
                content: sub_content,
                start_byte,
                end_byte,
                kind: chunk.kind.clone(),
                leading_comments: combined_comments,  // Include comments in all chunks
                parent_name: Some(format!("{} (part {})", chunk.kind, chunks.len() + 1)),
            });

            if end_token >= tokens.len() {
                break;
            }
            start_token += step_size;
        }

        chunks
    }

    pub fn chunk_file(&mut self, path: &Path, content: &str) -> Result<Vec<CodeChunk>> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let language = self
            .languages
            .iter()
            .find(|(_, exts)| exts.contains(&ext))
            .map(|(lang, _)| lang)
            .with_context(|| format!("Unsupported file extension: {}", ext))?;

        self.parser.set_language(*language)?;
        let tree = self
            .parser
            .parse(content, None)
            .with_context(|| "Failed to parse file")?;

        // Language-specific queries to extract meaningful chunks
        let query_str = match ext.as_str() {
            "rs" => {
                r#"
                (function_item) @function
                (struct_item) @struct
                (impl_item) @impl
                (trait_item) @trait
                (comment) @comment
            "#
            }
            "py" => {
                r#"
                (function_definition) @function
                (class_definition) @class
                (comment) @comment
            "#
            }
            "js" | "ts" => {
                r#"
                (function_declaration) @function
                (class_declaration) @class
                (method_definition) @method
                (comment) @comment
            "#
            }
            _ => return Ok(vec![]),
        };

        let query = Query::new(*language, query_str)?;
        let mut cursor = QueryCursor::new();
        let mut chunks = Vec::new();
        let mut current_comments = String::new();

        for match_ in cursor.matches(&query, tree.root_node(), content.as_bytes()) {
            for capture in match_.captures {
                if query.capture_names()[capture.index as usize] == "comment" {
                    let comment_text = &content[capture.node.byte_range()];
                    current_comments.push_str(comment_text);
                    current_comments.push('\n');
                    continue;
                }

                let range = capture.node.byte_range();
                let chunk_content = &content[range.clone()];
                let kind = query.capture_names()[capture.index as usize].to_string();

                // Extract any block comments within the chunk content
                let mut chunk_comments = current_comments.clone();
                let mut in_chunk_comments = String::new();
                
                // Look for block comments within the chunk
                if let Some(comment_start) = chunk_content.find("/*") {
                    if let Some(comment_end) = chunk_content[comment_start..].find("*/") {
                        in_chunk_comments = chunk_content[comment_start..comment_start + comment_end + 2].to_string();
                    }
                }

                // Combine all comments
                if !in_chunk_comments.is_empty() {
                    if !chunk_comments.is_empty() {
                        chunk_comments.push('\n');
                    }
                    chunk_comments.push_str(&in_chunk_comments);
                }

                let chunk = CodeChunk {
                    content: chunk_content.to_string(),
                    start_byte: range.start,
                    end_byte: range.end,
                    kind,
                    leading_comments: chunk_comments,
                    parent_name: None,
                };

                // Split large chunks and add all resulting sub-chunks
                chunks.extend(self.split_large_chunk(chunk));

                current_comments.clear();
            }
        }

        // Sort chunks by their position in the file
        chunks.sort_by_key(|chunk| chunk.start_byte);
        Ok(chunks)
    }
}
