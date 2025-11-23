use anyhow::{Context, Result};
use ignore::WalkBuilder;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::constants::constants;

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}

pub struct Indexer {
    include_extensions: Vec<String>,
}

impl Indexer {
    pub fn new() -> Self {
        Self {
            include_extensions: constants::DEFAULT_FILE_EXTENSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    pub fn index_directory(&self, path: &Path) -> Result<Vec<FileInfo>> {
        let base_path = path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize base path: {}", path.display()))?;
        let mut files = Vec::new();

        let walker = WalkBuilder::new(&base_path)
            .hidden(false) // Include hidden files/dirs
            .add_custom_ignore_filename(constants::RAGREP_IGNORE_FILENAME)
            .git_ignore(true) // Use .gitignore
            .git_global(true) // Use global gitignore
            .git_exclude(true) // Use .git/info/exclude
            .require_git(false) // Don't require git repo
            .follow_links(true)
            .build();

        for result in walker {
            let entry = result.with_context(|| "Failed to read directory entry")?;
            if entry.file_type().map_or(false, |ft| ft.is_file())
                && self.is_valid_extension(entry.path())
            {
                let canonical_path = entry.path().canonicalize().with_context(|| {
                    format!("Failed to canonicalize path: {}", entry.path().display())
                })?;

                let metadata = canonical_path.metadata().with_context(|| {
                    format!("Failed to get metadata for: {}", canonical_path.display())
                })?;

                files.push(FileInfo {
                    path: canonical_path,
                    size: metadata.len(),
                    modified: metadata.modified()?,
                });
            }
        }

        Ok(files)
    }

    // New method for partial indexing given a list of file paths.
    pub fn index_files<I: IntoIterator<Item = PathBuf>>(&self, paths: I) -> Result<Vec<FileInfo>> {
        let mut files = Vec::new();

        for path in paths {
            if self.is_valid_extension(&path) {
                let canonical_path = path
                    .canonicalize()
                    .with_context(|| format!("Failed to canonicalize path: {}", path.display()))?;
                let metadata = canonical_path.metadata().with_context(|| {
                    format!("Failed to get metadata for: {}", canonical_path.display())
                })?;
                files.push(FileInfo {
                    path: canonical_path,
                    size: metadata.len(),
                    modified: metadata.modified()?,
                });
            }
        }

        Ok(files)
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
