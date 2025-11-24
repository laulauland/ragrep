use anyhow::{anyhow, Context as AnyhowContext, Result};
use git2::Repository;
use ignore::gitignore::GitignoreBuilder;
use log::{debug, warn};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{channel, Receiver},
    Arc, Mutex as StdMutex,
};
use tokio::time::{sleep, Duration};

use crate::constants::constants;

/// Get the git working directory for a path
fn get_git_workdir(path: &Path) -> Result<PathBuf> {
    let repo = Repository::discover(path).context("Failed to find git repository")?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow!("Repository has no working directory"))?
        .to_path_buf();

    debug!("Found git working directory: {:?}", workdir);

    Ok(workdir)
}

/// Check if the given path is in a git repository
fn is_git_repo(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

/// Watches source files in working directory for changes
pub struct GitFileWatcher {
    watch_path: PathBuf,
}

impl GitFileWatcher {
    /// Check if the given path is in a git repository
    pub fn is_git_repo(path: &Path) -> bool {
        is_git_repo(path)
    }

    /// Create a new file watcher for git-tracked files
    pub fn new(base_path: &Path) -> Result<Self> {
        let watch_path = get_git_workdir(base_path)?;

        debug!("Watching source files at: {:?}", watch_path);
        debug!(
            "Using .gitignore and {} for filtering",
            constants::RAGREP_IGNORE_FILENAME
        );

        Ok(Self { watch_path })
    }

    /// Start watching for changes, returns a channel that receives changed file paths
    pub fn watch(&self) -> Result<Receiver<PathBuf>> {
        let (tx, rx) = channel();
        let watch_path = self.watch_path.clone();

        // Rebuild gitignore matcher in closure (since Gitignore isn't easily cloneable)
        let mut builder = GitignoreBuilder::new(&watch_path);

        // Add .gitignore from repo root
        let gitignore_path = watch_path.join(".gitignore");
        if gitignore_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
                let _ = builder.add_line(None, &content);
            }
        }

        // Add .ragrepignore if exists
        let ragrepignore_path = watch_path.join(constants::RAGREP_IGNORE_FILENAME);
        if ragrepignore_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&ragrepignore_path) {
                let _ = builder.add_line(None, &content);
            }
        }

        let gitignore = builder
            .build()
            .unwrap_or_else(|_| GitignoreBuilder::new(&watch_path).build().unwrap());

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Handle modify, remove, and create events
                        let should_process = matches!(
                            event.kind,
                            EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Create(_)
                        );

                        if should_process {
                            for path in event.paths {
                                // Check if path should be ignored (gitignore, ragrepignore, build dirs, etc.)
                                let relative_path = path.strip_prefix(&watch_path).unwrap_or(&path);
                                if gitignore.matched(relative_path, path.is_dir()).is_ignore() {
                                    debug!(
                                        "Ignoring file (gitignore/ragrepignore): {}",
                                        path.display()
                                    );
                                    continue;
                                }

                                // Check common build directories
                                let components: Vec<_> = path.components().collect();
                                let mut should_skip = false;
                                for component in &components {
                                    if let Some(name) = component.as_os_str().to_str() {
                                        if constants::IGNORED_DIRECTORIES.contains(&name) {
                                            should_skip = true;
                                            break;
                                        }
                                    }
                                }
                                if should_skip {
                                    continue;
                                }

                                // Only process source files
                                if let Some(ext) = path.extension() {
                                    if ext
                                        .to_str()
                                        .map(|e| constants::DEFAULT_FILE_EXTENSIONS.contains(&e))
                                        .unwrap_or(false)
                                    {
                                        match event.kind {
                                            EventKind::Modify(_) => {
                                                debug!("File modified: {}", path.display());
                                            }
                                            EventKind::Remove(_) => {
                                                debug!("File removed: {}", path.display());
                                            }
                                            EventKind::Create(_) => {
                                                debug!("File created: {}", path.display());
                                            }
                                            _ => {}
                                        }
                                        let _ = tx.send(path);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => warn!("Watch error: {:?}", e),
                }
            },
            Config::default(),
        )?;

        // Watch the entire working directory recursively
        watcher.watch(&self.watch_path, RecursiveMode::Recursive)?;

        // Keep watcher alive
        std::mem::forget(watcher);

        Ok(rx)
    }

    /// Start watching with debouncing (collects changed files and waits for quiet period)
    pub fn watch_debounced(&self, debounce_ms: u64) -> Result<Receiver<Vec<PathBuf>>> {
        let (tx, rx) = channel();
        let (file_tx, file_rx) = channel::<PathBuf>();

        // Shared set of changed files
        let changed_files = Arc::new(StdMutex::new(HashSet::new()));
        let changed_files_clone = Arc::clone(&changed_files);

        // Spawn debounce task
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(debounce_ms)).await;

                // Check if we have any changed files
                let files_to_reindex: Vec<PathBuf> = {
                    let mut guard = changed_files_clone.lock().unwrap();
                    if guard.is_empty() {
                        Vec::new()
                    } else {
                        let files: Vec<PathBuf> = guard.iter().cloned().collect();
                        guard.clear();
                        files
                    }
                };

                if !files_to_reindex.is_empty() {
                    debug!(
                        "Debounce period elapsed, reindexing {} files",
                        files_to_reindex.len()
                    );
                    let _ = tx.send(files_to_reindex);
                }
            }
        });

        // Spawn file collector task
        let changed_files_for_collector = Arc::clone(&changed_files);
        std::thread::spawn(move || {
            while let Ok(path) = file_rx.recv() {
                let mut guard = changed_files_for_collector.lock().unwrap();
                guard.insert(path.clone());
                debug!("File queued for reindex: {}", path.display());
            }
        });

        // Start the file watcher
        let watch_rx = self.watch()?;
        std::thread::spawn(move || {
            while let Ok(path) = watch_rx.recv() {
                let _ = file_tx.send(path);
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_repo() {
        // This test directory should be in a git repo
        let current_dir = std::env::current_dir().unwrap();
        assert!(is_git_repo(&current_dir));
    }
}
