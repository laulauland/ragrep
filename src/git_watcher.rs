use anyhow::{anyhow, Context as AnyhowContext, Result};
use git2::{Repository, StatusOptions};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use log::{debug, warn};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{
    mpsc::{channel, Receiver},
    Arc, Mutex as StdMutex,
};
use tokio::time::{sleep, Duration};

use crate::constants::constants;

/// Detects file changes in a git repository
pub struct GitChangeDetector {
    repo: Repository,
}

impl GitChangeDetector {
    /// Create a new change detector for the given directory
    pub fn new(base_path: &Path) -> Result<Self> {
        let repo = Repository::discover(base_path).context("Failed to find git repository")?;

        debug!("Found git repository at: {:?}", repo.path());

        Ok(Self { repo })
    }

    /// Get list of files that have changed (modified, added, deleted, renamed)
    pub fn get_changed_files(&self) -> Result<Vec<PathBuf>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to get git status")?;

        let mut changed_files = Vec::new();

        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry
                .path()
                .ok_or_else(|| anyhow!("Invalid UTF-8 in file path"))?;

            // Include modified, new, deleted, renamed, or typechanged files
            if status.is_wt_modified()
                || status.is_wt_new()
                || status.is_wt_deleted()
                || status.is_wt_renamed()
                || status.is_wt_typechange()
            {
                let full_path = self
                    .repo
                    .workdir()
                    .ok_or_else(|| anyhow!("Repository has no working directory"))?
                    .join(path);

                changed_files.push(full_path);
                debug!("Detected change: {}", path);
            }
        }

        Ok(changed_files)
    }

    /// Check if the given path is in a git repository
    pub fn is_git_repo(path: &Path) -> bool {
        Repository::discover(path).is_ok()
    }
}

/// Watches git index file for changes
pub struct GitIndexWatcher {
    detector: GitChangeDetector,
    git_index_path: PathBuf,
}

impl GitIndexWatcher {
    /// Create a new git index watcher
    pub fn new(base_path: &Path) -> Result<Self> {
        let detector = GitChangeDetector::new(base_path)?;

        let git_index_path = detector.repo.path().join("index");

        if !git_index_path.exists() {
            return Err(anyhow!("Git index file not found at {:?}", git_index_path));
        }

        debug!("Watching git index at: {:?}", git_index_path);

        Ok(Self {
            detector,
            git_index_path,
        })
    }

    /// Start watching for changes, returns a channel that receives changed files
    pub fn watch(&self) -> Result<Receiver<Vec<PathBuf>>> {
        let (tx, rx) = channel();

        // Get the workdir path before moving into closure
        let workdir = self
            .detector
            .repo
            .workdir()
            .ok_or_else(|| anyhow!("No working directory"))?
            .to_path_buf();

        let git_index_path = self.git_index_path.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Only care about modify events
                        if matches!(event.kind, EventKind::Modify(_)) {
                            debug!("Git index modified, checking for changes...");

                            // Create a new detector for this check
                            match GitChangeDetector::new(&workdir) {
                                Ok(detector) => match detector.get_changed_files() {
                                    Ok(files) if !files.is_empty() => {
                                        debug!("Found {} changed files", files.len());
                                        let _ = tx.send(files);
                                    }
                                    Ok(_) => {
                                        debug!("No changed files detected");
                                    }
                                    Err(e) => {
                                        warn!("Error detecting changes: {}", e);
                                    }
                                },
                                Err(e) => {
                                    warn!("Error creating change detector: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => warn!("Watch error: {:?}", e),
                }
            },
            Config::default(),
        )?;

        watcher.watch(&git_index_path, RecursiveMode::NonRecursive)?;

        // Keep watcher alive by leaking it (it will live for program lifetime)
        // This is intentional for a long-running server
        std::mem::forget(watcher);

        Ok(rx)
    }

    /// Start watching with debouncing (waits for quiet period before triggering)
    pub fn watch_debounced(&self, debounce_ms: u64) -> Result<Receiver<Vec<PathBuf>>> {
        let (tx, rx) = channel();

        // Get the workdir path before moving into closures
        let workdir = self
            .detector
            .repo
            .workdir()
            .ok_or_else(|| anyhow!("No working directory"))?
            .to_path_buf();

        let git_index_path = self.git_index_path.clone();

        // Timestamp of last modification (0 means no pending changes)
        let last_modified = Arc::new(AtomicU64::new(0));
        let last_modified_clone = Arc::clone(&last_modified);

        // Spawn debounce task that checks periodically
        let tx_clone = tx.clone();
        let workdir_clone = workdir.clone();

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(debounce_ms)).await;

                let last_mod = last_modified_clone.load(Ordering::Relaxed);
                if last_mod > 0 {
                    // Check if timestamp hasn't changed (no new modifications during sleep)
                    let current_mod = last_modified_clone.load(Ordering::Relaxed);
                    if current_mod == last_mod {
                        // No new modifications, safe to trigger change detection
                        debug!("Debounce period elapsed, checking changes...");

                        match GitChangeDetector::new(&workdir_clone) {
                            Ok(detector) => match detector.get_changed_files() {
                                Ok(files) if !files.is_empty() => {
                                    debug!("Sending {} files for reindex", files.len());
                                    let _ = tx_clone.send(files);
                                    last_modified_clone.store(0, Ordering::Relaxed);
                                }
                                Ok(_) => {
                                    debug!("No changed files detected");
                                    last_modified_clone.store(0, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    warn!("Error detecting changes: {}", e);
                                }
                            },
                            Err(e) => {
                                warn!("Error creating change detector: {}", e);
                            }
                        }
                    }
                    // If timestamp changed, loop continues (new modification happened during sleep)
                }
            }
        });

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Modify(_)) {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;

                        last_modified.store(now, Ordering::Relaxed);
                        debug!("Git index modified at {}", now);
                    }
                }
                Err(e) => warn!("Watch error: {:?}", e),
            },
            Config::default(),
        )?;

        watcher.watch(&git_index_path, RecursiveMode::NonRecursive)?;
        std::mem::forget(watcher);

        Ok(rx)
    }

    /// Check if the given path is in a git repository
    pub fn is_git_repo(path: &Path) -> bool {
        GitChangeDetector::is_git_repo(path)
    }
}

/// Watches source files in working directory for changes
pub struct GitFileWatcher {
    detector: GitChangeDetector,
    watch_path: PathBuf,
    gitignore: Gitignore,
}

impl GitFileWatcher {
    /// Check if the given path is in a git repository
    pub fn is_git_repo(path: &Path) -> bool {
        GitChangeDetector::is_git_repo(path)
    }

    /// Create a new file watcher for git-tracked files
    pub fn new(base_path: &Path) -> Result<Self> {
        let detector = GitChangeDetector::new(base_path)?;

        let watch_path = detector
            .repo
            .workdir()
            .ok_or_else(|| anyhow!("No working directory"))?
            .to_path_buf();

        // Build gitignore matcher (same approach as indexer for consistency)
        let mut builder = GitignoreBuilder::new(&watch_path);

        // Add .gitignore from repo root
        let gitignore_path = watch_path.join(".gitignore");
        if gitignore_path.exists() {
            builder.add_line(None, &std::fs::read_to_string(&gitignore_path)?)?;
        }

        // Add .ragrepignore (custom ignore file, same as indexer)
        let ragrepignore_path = watch_path.join(constants::RAGREP_IGNORE_FILENAME);
        if ragrepignore_path.exists() {
            builder.add_line(None, &std::fs::read_to_string(&ragrepignore_path)?)?;
        }

        let gitignore = builder.build()?;

        debug!("Watching source files at: {:?}", watch_path);
        debug!(
            "Using .gitignore and {} for filtering",
            constants::RAGREP_IGNORE_FILENAME
        );

        Ok(Self {
            detector,
            watch_path,
            gitignore,
        })
    }

    /// Check if a path should be ignored
    fn should_ignore(&self, path: &Path) -> bool {
        // Check gitignore (includes .ragrepignore patterns)
        if self.gitignore.matched(path, path.is_dir()).is_ignore() {
            return true;
        }

        // Always ignore common build/cache directories as fallback
        // (in case .gitignore is missing)
        let components: Vec<_> = path.components().collect();
        for component in &components {
            if let Some(name) = component.as_os_str().to_str() {
                if constants::IGNORED_DIRECTORIES.contains(&name) {
                    return true;
                }
            }
        }

        false
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
                        // Only care about modify events (file saved)
                        if matches!(event.kind, EventKind::Modify(_)) {
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
                                        debug!("File modified: {}", path.display());
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
        use std::collections::HashSet;
        use std::sync::Mutex as StdMutex;
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
        assert!(GitChangeDetector::is_git_repo(&current_dir));
    }
}
