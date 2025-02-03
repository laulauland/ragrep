use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use tree_sitter_javascript::language as js_language;
use tree_sitter_python::language as py_language;
use tree_sitter_rust::language as rust_language;
use tree_sitter_typescript::language_typescript as ts_language;

#[derive(Debug)]
pub struct CodeChunk {
    pub content: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: String, // "function", "class", "method", etc.
    pub leading_comments: String,
}

pub struct Chunker {
    parser: Parser,
    languages: Vec<(Language, Vec<String>)>,
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

        Ok(Self { parser, languages })
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

                chunks.push(CodeChunk {
                    content: chunk_content.to_string(),
                    start_byte: range.start,
                    end_byte: range.end,
                    kind,
                    leading_comments: current_comments.clone(),
                });

                current_comments.clear();
            }
        }

        // Sort chunks by their position in the file
        chunks.sort_by_key(|chunk| chunk.start_byte);
        Ok(chunks)
    }
}
