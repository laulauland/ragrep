use anyhow::{Error, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};

#[derive(Debug, Serialize, Deserialize)]
pub struct Embedding(pub Vec<f32>);

pub struct Embedder {
    model: TextEmbedding,
}

impl Embedder {
    pub fn new(cache_dir: &Path) -> Result<Self, Error> {
        let mut options = InitOptions::default().with_cache_dir(cache_dir.to_path_buf());
        options.model_name = EmbeddingModel::ModernBertEmbedLarge;

        let model = TextEmbedding::try_new(options)?;
        Ok(Self { model })
    }

    pub async fn embed_text(&self, text: &str, file_path: &str) -> Result<Embedding> {
        let processed = self.preprocess_code(text, file_path);
        let embeddings = self.model.embed(vec![&processed], None)?;
        Ok(Embedding(embeddings[0].clone()))
    }

    pub async fn embed_query(&self, query: &str) -> Result<Embedding> {
        let embeddings = self.model.embed(vec![query], None)?;
        Ok(Embedding(embeddings[0].clone()))
    }

    fn preprocess_code(&self, text: &str, file_path: &str) -> String {
        let mut parser = Parser::new();

        // Detect language from file extension
        let language = match Path::new(file_path)
            .extension()
            .and_then(|ext| ext.to_str())
        {
            Some("rs") => tree_sitter_rust::language(),
            Some("py") => tree_sitter_python::language(),
            Some("js" | "ts") => tree_sitter_javascript::language(),
            _ => tree_sitter_javascript::language(), // default
        };

        parser
            .set_language(language)
            .expect("Failed to set language");

        let tree = match parser.parse(text, None) {
            Some(tree) => tree,
            None => return format!("FILE: {} {}", file_path, text),
        };

        let query_str = if language == tree_sitter_rust::language() {
            r#"
            (function_item 
                name: (identifier) @name
                parameters: (parameters) @params
            ) @function

            (impl_item
                trait: (type_identifier)? @trait_name
                type: (type_identifier) @type_name
            ) @impl

            (trait_item
                name: (identifier) @trait_name
            ) @trait
            "#
        } else if language == tree_sitter_python::language() {
            r#"
            (function_definition
                name: (identifier) @name
                parameters: (parameters) @params
                body: (block)? @body
            ) @function

            (class_definition
                name: (identifier) @name
                body: (block) @body
            ) @class
            "#
        } else {
            r#"
            (function_declaration
                name: (identifier) @name
                parameters: (formal_parameters) @params
                body: (statement_block) @body
            ) @function

            (method_definition
                name: (property_identifier) @name
                parameters: (formal_parameters) @params
                body: (statement_block) @body
            ) @method

            (class_declaration
                name: (identifier) @name
                body: (class_body) @body
            ) @class
            "#
        };

        let query = match Query::new(language, query_str) {
            Ok(q) => q,
            Err(_) => return format!("FILE: {} {}", file_path, text),
        };

        let mut cursor = QueryCursor::new();
        let mut processed = text.to_string();

        for match_ in cursor.matches(&query, tree.root_node(), text.as_bytes()) {
            for capture in match_.captures {
                let range = capture.node.byte_range();
                let capture_name = &query.capture_names()[capture.index as usize];

                let prefix = match capture_name.as_str() {
                    "function" | "method" => "FUNCTION ",
                    "class" => "CLASS ",
                    "impl" => "IMPLEMENTATION ",
                    "trait" => "TRAIT ",
                    "name" => "NAME ",
                    "params" => "PARAMETERS ",
                    _ => continue,
                };

                processed.insert_str(range.start, prefix);
            }
        }

        processed.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}
