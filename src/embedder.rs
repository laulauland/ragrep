use anyhow::{Error, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use ignore::Walk;
use promkit::preset::confirm::Confirm;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use tree_sitter::{Parser, Query, QueryCursor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding(pub Vec<f32>);

pub struct Embedder {
    model: TextEmbedding,
    cache: Mutex<HashMap<u64, Embedding>>,
}

impl Embedder {
    fn model_exists(model_cache_dir: &Path) -> bool {
        Walk::new(model_cache_dir)
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.path().extension().map_or(false, |ext| ext == "onnx"))
    }

    pub fn new(model_cache_dir: &Path) -> Result<Self, Error> {
        let mut options = InitOptions::default().with_cache_dir(model_cache_dir.to_path_buf());
        options.model_name = EmbeddingModel::ModernBertEmbedLarge;

        if !Self::model_exists(model_cache_dir) {
            let size_mb = 1500; // Approximate size of the model
            let message = format!(
                "The embedding model (~{}MB) needs to be downloaded. This is a one-time operation.",
                size_mb
            );

            let mut prompt = Confirm::new(&message).prompt()?;
            let response = prompt.run()?;

            if response == "n" || response == "N" || response == "no" || response == "No" {
                return Err(Error::msg("Model download cancelled by user"));
            }
        }

        let model = TextEmbedding::try_new(options)?;
        Ok(Self {
            model,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub async fn embed_text(&self, text: &str, file_path: &str) -> Result<Embedding> {
        use std::hash::{Hash, Hasher};

        let processed = self.preprocess_code(text, file_path);

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        processed.hash(&mut hasher);
        let text_hash = hasher.finish();

        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(&text_hash) {
                return Ok(cached.clone());
            }
        }

        let embeddings = self.model.embed(vec![&processed], None)?;
        let embedding_result = Embedding(embeddings[0].clone());

        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(text_hash, embedding_result.clone());
        }

        Ok(embedding_result)
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
