use anyhow::{Error, Result};
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
use log::debug;
use std::path::Path;
use std::sync::Mutex;

pub struct Reranker {
    model: Mutex<TextRerank>,
}

impl Reranker {
    pub fn new(model_cache_dir: &Path) -> Result<Self, Error> {
        debug!("Initializing reranker model...");
        let options = RerankInitOptions::new(RerankerModel::BGERerankerV2M3)
            .with_cache_dir(model_cache_dir.to_path_buf())
            .with_show_download_progress(true);

        let model = TextRerank::try_new(options)?;
        debug!("Reranker model initialized successfully");
        Ok(Self { model: Mutex::new(model) })
    }

    /// Rerank search results based on their relevance to the query
    ///
    /// # Arguments
    /// * `query` - The search query
    /// * `documents` - List of document texts to rerank
    /// * `top_n` - Maximum number of results to return
    ///
    /// # Returns
    /// Vector of (document_index, relevance_score) tuples, sorted by relevance (highest first)
    pub fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: Option<usize>,
    ) -> Result<Vec<(usize, f32)>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Reranking {} documents for query: {}", documents.len(), query);

        // Convert documents to &str for the rerank API
        let doc_refs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();

        // Perform reranking
        let mut model = self.model.lock().unwrap();
        let results = model.rerank(query, doc_refs, true, top_n)?;

        // Convert results to (index, score) tuples
        let mut ranked: Vec<(usize, f32)> = results
            .iter()
            .map(|r| (r.index, r.score))
            .collect();

        // Sort by score descending (highest relevance first)
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        debug!(
            "Reranking complete. Top score: {:.4}, Bottom score: {:.4}",
            ranked.first().map(|r| r.1).unwrap_or(0.0),
            ranked.last().map(|r| r.1).unwrap_or(0.0)
        );

        Ok(ranked)
    }
}
