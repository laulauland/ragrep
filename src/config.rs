use anyhow::{Context as AnyhowContext, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::constants::constants;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub model_cache_dir: Option<PathBuf>,
    pub reranker: Option<RerankerConfig>,
    #[serde(default)]
    pub git_watch: GitWatchConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GitWatchConfig {
    pub enabled: bool,
    pub debounce_ms: u64,
}

impl Default for GitWatchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 500, // 0.5 second default
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RerankerConfig {
    /// Use external reranker service (mxbai-rerank-v2) instead of local JINA reranker
    pub use_external_service: bool,
    /// URL of the external reranker service (e.g., "http://localhost:8080")
    pub service_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_cache_dir: None,
            reranker: None,
            git_watch: GitWatchConfig::default(),
        }
    }
}

pub struct ConfigManager {
    global_config: Config,
    local_config: Option<Config>,
    merged_config: Config,
    pub global_config_path: PathBuf,
    pub local_config_path: Option<PathBuf>,
}

const DEFAULT_CONFIG: &str = r#"# ragrep configuration file
# All paths can be absolute or relative to this config file

# Optional: Override the default model cache directory
# model_cache_dir = "~/.cache/ragrep/models"

# Optional: Configure external reranker service
# [reranker]
# use_external_service = true
# service_url = "http://localhost:8080"

# Optional: Configure git-based auto-reindexing
# [git_watch]
# enabled = true
# debounce_ms = 1000
"#;

impl ConfigManager {
    pub fn new(workspace_path: Option<&Path>) -> Result<Self> {
        let global_config_dir = dirs::config_dir()
            .context("Could not find config directory")?
            .join(constants::GLOBAL_CONFIG_DIR_NAME);

        fs::create_dir_all(&global_config_dir)?;
        let global_config_path = global_config_dir.join(constants::CONFIG_FILENAME);

        // Load or create global config
        let global_config = if global_config_path.exists() {
            let content = fs::read_to_string(&global_config_path)?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            let default_config = Config::default();
            fs::write(&global_config_path, DEFAULT_CONFIG)?;
            default_config
        };

        // Load local config if workspace path is provided
        let (local_config, local_config_path) = if let Some(workspace_path) = workspace_path {
            let local_config_path = workspace_path
                .join(constants::RAGREP_DIR_NAME)
                .join(constants::CONFIG_FILENAME);
            let local_config = if local_config_path.exists() {
                let content = fs::read_to_string(&local_config_path)?;
                Some(toml::from_str::<Config>(&content).unwrap_or_default())
            } else {
                None
            };
            (local_config, Some(local_config_path))
        } else {
            (None, None)
        };

        // Merge configs: local overrides global
        let mut merged_config = global_config.clone();
        if let Some(ref local_config) = local_config {
            // Merge fields: local takes precedence
            if local_config.model_cache_dir.is_some() {
                merged_config.model_cache_dir = local_config.model_cache_dir.clone();
            }
            if local_config.reranker.is_some() {
                merged_config.reranker = local_config.reranker.clone();
            }
            // git_watch always uses local if present (since it has defaults)
            merged_config.git_watch = local_config.git_watch.clone();
        }

        Ok(Self {
            global_config,
            local_config,
            merged_config,
            global_config_path,
            local_config_path,
        })
    }

    pub fn get_model_cache_dir(&self) -> Result<PathBuf> {
        // Local config overrides global config
        if let Some(local_config) = &self.local_config {
            if let Some(cache_dir) = &local_config.model_cache_dir {
                return Ok(cache_dir.clone());
            }
        }

        // Fall back to global config
        if let Some(cache_dir) = &self.global_config.model_cache_dir {
            return Ok(cache_dir.clone());
        }

        // Default to system data directory
        let data_dir = dirs::data_dir().context("Could not find data directory")?;
        Ok(data_dir
            .join(constants::GLOBAL_CONFIG_DIR_NAME)
            .join(constants::MODELS_DIR_NAME))
    }

    pub fn get_reranker_config(&self) -> Option<RerankerConfig> {
        // Local config overrides global config
        if let Some(local_config) = &self.local_config {
            if let Some(reranker_config) = &local_config.reranker {
                return Some(reranker_config.clone());
            }
        }

        // Fall back to global config
        self.global_config.reranker.clone()
    }

    /// Get the merged configuration (local overrides global)
    pub fn config(&self) -> &Config {
        &self.merged_config
    }
}
