use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

pub struct Indexer {
    exclude_dirs: Vec<String>,
    include_extensions: Vec<String>,
}

impl Indexer {
    pub fn new() -> Self {
        Self {
            exclude_dirs: vec![
                ".git".to_string(),
                ".ragrep".to_string(),
                "target".to_string(),
            ],
            include_extensions: vec![
                "rs".to_string(),
                "py".to_string(),
                "js".to_string(),
                "ts".to_string(),
            ],
        }
    }

    pub fn index_directory(&self, path: &Path) -> Result<Vec<String>> {
        let mut files = Vec::new();

        for entry in WalkDir::new(path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| !self.should_exclude(e))
        {
            let entry = entry?;
            if entry.file_type().is_file() && self.is_valid_extension(entry.path()) {
                if let Some(path_str) = entry.path().to_str() {
                    files.push(path_str.to_string());
                }
            }
        }

        Ok(files)
    }

    fn should_exclude(&self, entry: &walkdir::DirEntry) -> bool {
        if let Some(file_name) = entry.file_name().to_str() {
            self.exclude_dirs.iter().any(|dir| file_name == dir)
        } else {
            false
        }
    }

    fn is_valid_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                self.include_extensions
                    .iter()
                    .any(|valid_ext| valid_ext == ext)
            })
            .unwrap_or(false)
    }
}
