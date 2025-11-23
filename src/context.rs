use crate::config::ConfigManager;
use crate::db::Database;
use crate::embedder::Embedder;
use crate::reranker::Reranker;
use anyhow::{Context as AnyhowContext, Result};
use log::debug;
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
        debug!("[TIMING] Embedder initialization: {:.3}s", embedder_start.elapsed().as_secs_f64());

        // Initialize reranker with BGE model
        debug!("Initializing local BGE reranker");
        let reranker_start = Instant::now();
        let reranker = Reranker::new(&model_cache_dir)?;
        debug!("[TIMING] Reranker initialization: {:.3}s", reranker_start.elapsed().as_secs_f64());

        debug!("[TIMING] Total AppContext initialization: {:.3}s", start_time.elapsed().as_secs_f64());

        Ok(Self {
            embedder,
            reranker,
            db,
            ragrep_dir,
            config_manager,
        })
    }
}
