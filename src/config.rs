use anyhow::{Context as AnyhowContext, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub model_cache_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_cache_dir: None,
        }
    }
}

pub struct ConfigManager {
    global_config: Config,
    local_config: Option<Config>,
    pub global_config_path: PathBuf,
    pub local_config_path: Option<PathBuf>,
}

const DEFAULT_CONFIG: &str = r#"# ragrep configuration file
# All paths can be absolute or relative to this config file

# Optional: Override the default model cache directory
# model_cache_dir = "~/.cache/ragrep/models"
"#;

impl ConfigManager {
    pub fn new(workspace_path: Option<&Path>) -> Result<Self> {
        let global_config_dir = dirs::config_dir()
            .context("Could not find config directory")?
            .join("ragrep");

        fs::create_dir_all(&global_config_dir)?;
        let global_config_path = global_config_dir.join("config.toml");

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
            let local_config_path = workspace_path.join(".ragrep").join("config.toml");
            let local_config = if local_config_path.exists() {
                let content = fs::read_to_string(&local_config_path)?;
                Some(toml::from_str(&content).unwrap_or_default())
            } else {
                None
            };
            (local_config, Some(local_config_path))
        } else {
            (None, None)
        };

        Ok(Self {
            global_config,
            local_config,
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
        Ok(data_dir.join("ragrep").join("models"))
    }
}
