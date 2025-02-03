use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Embedding(Vec<f32>);

pub struct Embedder {
    api_key: String,
}

impl Embedder {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn embed_text(&self, text: &str) -> Result<Embedding> {
        // TODO: Implement actual embedding generation
        // For now, return a dummy embedding
        Ok(Embedding(vec![0.0; 384]))
    }
}
