use crate::chunker::Chunker;
use crate::config::ConfigManager;
use crate::db::Database;
use crate::embedder::Embedder;
use crate::indexer::{FileInfo, Indexer};
use crate::reranker::Reranker;
use anyhow::{Context as AnyhowContext, Result};
use log::{debug, info};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct AppContext {
    pub embedder: Embedder,
    pub reranker: Reranker,
    pub db: Database,
    pub ragrep_dir: PathBuf,
    pub config_manager: ConfigManager,
}

impl AppContext {
    pub async fn new(base_path: &Path) -> Result<Self> {
        let start_time = Instant::now();

        let config_manager = ConfigManager::new(Some(base_path))?;

        // Create .ragrep directory if it doesn't exist
        let ragrep_dir = base_path.join(".ragrep");
        fs::create_dir_all(&ragrep_dir)?;

        // Initialize database
        let db_path = ragrep_dir.join("ragrep.db");
        let db = Database::new(&db_path)
            .with_context(|| format!("Failed to initialize database at {}", db_path.display()))?;

        // Initialize embedder with configured model cache directory
        let model_cache_dir = config_manager.get_model_cache_dir()?;
        fs::create_dir_all(&model_cache_dir)?;
        debug!("Using model cache directory: {}", model_cache_dir.display());

        let embedder_start = Instant::now();
        let embedder = Embedder::new(&model_cache_dir)?;
        debug!(
            "[TIMING] Embedder initialization: {:.3}s",
            embedder_start.elapsed().as_secs_f64()
        );

        // Initialize reranker with BGE model
        debug!("Initializing local BGE reranker");
        let reranker_start = Instant::now();
        let reranker = Reranker::new(&model_cache_dir)?;
        debug!(
            "[TIMING] Reranker initialization: {:.3}s",
            reranker_start.elapsed().as_secs_f64()
        );

        debug!(
            "[TIMING] Total AppContext initialization: {:.3}s",
            start_time.elapsed().as_secs_f64()
        );

        Ok(Self {
            embedder,
            reranker,
            db,
            ragrep_dir,
            config_manager,
        })
    }

    /// Incrementally reindex specific files with embedding reuse
    pub async fn reindex_files(&mut self, file_paths: Vec<PathBuf>) -> Result<()> {
        info!("Incrementally reindexing {} files", file_paths.len());

        let indexer = Indexer::new();
        let mut chunker = Chunker::new()?;

        // Filter to only valid files (exist, correct extensions)
        let files: Vec<FileInfo> = indexer.index_files(file_paths.into_iter())?;

        if files.is_empty() {
            debug!("No valid files to reindex");
            return Ok(());
        }

        let start = std::time::Instant::now();
        let mut total_chunks = 0;
        let mut reused_embeddings = 0;
        let mut new_embeddings = 0;

        for file in &files {
            let file_path_str = file.path.to_string_lossy().to_string();

            // OPTIMIZATION: Load old embeddings BEFORE deleting
            let embedding_cache = self.db.get_chunks_with_embeddings(&file_path_str)?;

            // Delete old chunks for this file (clean slate)
            self.db.delete_file(&file_path_str)?;

            // Read and chunk the file
            let content = std::fs::read_to_string(&file.path)
                .with_context(|| format!("Failed to read file: {}", file.path.display()))?;

            let chunks = chunker.chunk_file(&file.path, &content)?;
            total_chunks += chunks.len();

            // Embed and save chunks, REUSING embeddings where possible
            for (idx, chunk) in chunks.iter().enumerate() {
                let hash = chunk.hash() as i64;

                // Try to reuse embedding if content unchanged
                let embedding = if let Some(cached) = embedding_cache.get(&hash) {
                    // Content unchanged! Reuse old embedding (FAST!)
                    reused_embeddings += 1;
                    cached.clone()
                } else {
                    // Content changed, need to re-embed (SLOW)
                    new_embeddings += 1;
                    let result = self
                        .embedder
                        .embed_text(&chunk.content, &file_path_str)
                        .await?;
                    result.0 // Extract Vec<f32> from Embedding wrapper
                };

                self.db.save_chunk(
                    &file_path_str,
                    idx as i32,
                    &chunk.kind,
                    chunk.parent_name.as_deref(),
                    chunk.start_line,
                    chunk.end_line,
                    &chunk.content,
                    hash as u64,
                    &embedding,
                )?;
            }
        }

        let elapsed = start.elapsed();
        info!(
            "Reindexed {} files ({} chunks) in {:.2}s - reused {} embeddings, computed {} new",
            files.len(),
            total_chunks,
            elapsed.as_secs_f64(),
            reused_embeddings,
            new_embeddings
        );

        Ok(())
    }
}
