use anyhow::{anyhow, Context as AnyhowContext, Result};
use git2::{Repository, StatusOptions};
use log::{debug, warn};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc::{channel, Receiver}};
use tokio::time::{sleep, Duration};

/// Detects file changes in a git repository
pub struct GitChangeDetector {
    repo: Repository,
}

impl GitChangeDetector {
    /// Create a new change detector for the given directory
    pub fn new(base_path: &Path) -> Result<Self> {
        let repo = Repository::discover(base_path)
            .context("Failed to find git repository")?;
        
        debug!("Found git repository at: {:?}", repo.path());
        
        Ok(Self { repo })
    }

    /// Get list of files that have changed (modified, added, deleted, renamed)
    pub fn get_changed_files(&self) -> Result<Vec<PathBuf>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        
        let statuses = self.repo.statuses(Some(&mut opts))
            .context("Failed to get git status")?;
        
        let mut changed_files = Vec::new();
        
        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry.path()
                .ok_or_else(|| anyhow!("Invalid UTF-8 in file path"))?;
            
            // Include modified, new, deleted, renamed, or typechanged files
            if status.is_wt_modified() 
                || status.is_wt_new()
                || status.is_wt_deleted()
                || status.is_wt_renamed()
                || status.is_wt_typechange()
            {
                let full_path = self.repo.workdir()
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
        let workdir = self.detector.repo.workdir()
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
                                Ok(detector) => {
                                    match detector.get_changed_files() {
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
                                    }
                                }
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
        let workdir = self.detector.repo.workdir()
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
                            Ok(detector) => {
                                match detector.get_changed_files() {
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
                                }
                            }
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
            move |res: Result<Event, notify::Error>| {
                match res {
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
                }
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

