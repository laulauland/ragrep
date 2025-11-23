# Phase 3 Implementation Guide: Git-Based Auto-Reindexing

**Goal**: Automatically detect file changes using git and incrementally reindex only modified files, keeping the search index fresh without manual reindexing.

**Time Estimate**: 1 week  
**Lines of Code**: ~300 lines  
**Difficulty**: Intermediate-Advanced

---

## 📚 Background: Why Are We Doing This?

Right now, after you edit files in your codebase:
1. The ragrep index becomes stale (doesn't include your changes)
2. You must manually run `ragrep index` to update it
3. Full reindexing takes time (processes ALL files, even unchanged ones)
4. Easy to forget to reindex, leading to incorrect search results

With git-based auto-reindexing:
- **Server watches git**: Monitors `.git/index` for changes
- **Detects changed files**: Uses git to find exactly which files changed
- **Incremental update**: Only reindexes the files that changed
- **Automatic**: Happens in the background, you don't think about it
- **Fast**: 1-2 seconds to reindex a handful of files vs 30+ seconds for full reindex

**Example workflow**:
```bash
# Start server (once)
$ ragrep serve
[INFO] Server ready, watching for file changes...

# Edit some files in your editor (NO git add needed!)
$ vim src/main.rs       # Edit and save
[DEBUG] File queued for reindex: src/main.rs
[INFO] Reindexed 1 file in 0.2s ⚡

$ ragrep "new function"  # Immediately finds your new code!
```

---

## 🎯 What We're Building

```
┌─────────────────────────────────────────────────────────────┐
│ BEFORE (Phase 2)                                            │
│                                                             │
│  $ ragrep serve                                            │
│    └─ Server runs, keeps models loaded                    │
│                                                             │
│  $ vim src/main.rs  # Edit files                           │
│  $ ragrep "new code"                                       │
│    ❌ Not found (index is stale)                           │
│                                                             │
│  $ ragrep index  # Manual reindex required                 │
│    ⏱️  Full reindex: 30+ seconds                           │
│                                                             │
│  $ ragrep "new code"                                       │
│    ✅ Found                                                 │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ AFTER (Phase 3)                                            │
│                                                             │
│  $ ragrep serve                                            │
│    ├─ Server runs, keeps models loaded                    │
│    └─ Git watcher starts automatically                    │
│                                                             │
│  $ vim src/main.rs  # Edit and save (NO git add!)           │
│    [DEBUG] File queued for reindex: src/main.rs            │
│    [INFO] Reindexed 1 file in 0.2s ⚡                      │
│                                                             │
│  $ ragrep "new code"                                       │
│    ✅ Found immediately (auto-reindexed!)                  │
└─────────────────────────────────────────────────────────────┘
```

---

## 📋 Implementation Milestones

We'll build this in 7 milestones, each with verifiable behavior:

1. **Git Change Detection** - Detect which files changed using git2
2. **File System Watching** - Watch `.git/index` for modifications
3. **Debouncing Logic** - Avoid reindexing on every keystroke
4. **Incremental Reindexing** - Update only changed files in database
5. **Server Integration** - Wire watcher into server lifecycle
6. **Non-Git Handling** - Gracefully handle non-git projects
7. **Testing & Verification** - Ensure it works end-to-end

---

## Milestone 1: Git Change Detection

**Goal**: Use git2 to detect which files have changed since last reindex.

**Why First**: Need to reliably identify changed files before watching for changes.

### Step 1.1: Add Dependencies

Add git2 to `Cargo.toml`:

```bash
cargo add git2
```

### Step 1.2: Create `src/git_watcher.rs`

Create a new file with git change detection:

```rust
use anyhow::{anyhow, Context as AnyhowContext, Result};
use git2::{Repository, StatusOptions};
use log::{debug, warn};
use std::path::{Path, PathBuf};

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
```

### Step 1.3: Update `src/main.rs` Module Declaration

Add the new module to `src/main.rs` at the top:

```rust
mod git_watcher;
```

### Step 1.4: Test Git Detection

Build and verify it compiles:

```bash
cargo build
```

**Expected Output**:
```
   Compiling ragrep v0.1.0
    Finished dev [unoptimized + debuginfo] target(s) in 2.1s
```

**Verify**:
```bash
cargo test test_is_git_repo
```

**Expected Output**:
```
running 1 test
test git_watcher::tests::test_is_git_repo ... ok

test result: ok. 1 passed; 0 failed
```

---

## Milestone 2: File System Watching

**Goal**: Watch source files directly for modifications to trigger instant reindexing.

**Why**: Waiting for `git add` is bad UX. Developers expect changes to be indexed immediately on save.

**Design Decision**: We'll watch actual source files (`.rs`, `.py`, etc.) in the working directory, not `.git/index`. This gives instant feedback while still using git to determine which files are relevant.

### Step 2.1: Add Notify Dependency

Add file watching library:

```bash
cargo add notify --features "macos_fsevent"
cargo add tokio --features "sync"
```

### Step 2.2: Add File Watcher to `git_watcher.rs`

Add to the existing `src/git_watcher.rs`:

```rust
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// Watches source files in working directory for changes
pub struct GitFileWatcher {
    detector: GitChangeDetector,
    watch_path: PathBuf,
}

impl GitFileWatcher {
    /// Create a new file watcher for git-tracked files
    pub fn new(base_path: &Path) -> Result<Self> {
        let detector = GitChangeDetector::new(base_path)?;
        
        let watch_path = detector.repo.workdir()
            .ok_or_else(|| anyhow!("No working directory"))?
            .to_path_buf();
        
        debug!("Watching source files at: {:?}", watch_path);
        
        Ok(Self {
            detector,
            watch_path,
        })
    }

    /// Start watching for changes, returns a channel that receives changed file paths
    pub fn watch(&self) -> Result<Receiver<PathBuf>> {
        let (tx, rx) = channel();
        
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Only care about modify events (file saved)
                        if matches!(event.kind, EventKind::Modify(_)) {
                            for path in event.paths {
                                // Only process source files (rs, py, js, ts)
                                if let Some(ext) = path.extension() {
                                    if matches!(ext.to_str(), Some("rs" | "py" | "js" | "ts")) {
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
        let detector = GitChangeDetector::new(
            self.detector.repo.workdir()
                .ok_or_else(|| anyhow!("No working directory"))?
        )?;
        
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Only care about modify events
                        if matches!(event.kind, EventKind::Modify(_)) {
                            debug!("Git index modified, checking for changes...");
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
                    }
                    Err(e) => warn!("Watch error: {:?}", e),
                }
            },
            Config::default(),
        )?;

        watcher.watch(&self.git_index_path, RecursiveMode::NonRecursive)?;
        
        // Keep watcher alive by leaking it (it will live for program lifetime)
        // This is intentional for a long-running server
        std::mem::forget(watcher);
        
        Ok(rx)
    }
}
```

### Step 2.3: Test Watcher (Manual Test)

Create a simple test program to verify watching works:

```bash
# In another terminal, create a test
cargo run -- serve &

# Make a change to trigger reindex (NO git add needed!)
echo "// test" >> src/main.rs

# Check server logs for detection
```

**Expected Behavior**: Server logs should show "File modified: src/main.rs" immediately on save!

**Key Point**: Notice we did NOT run `git add`. The watcher detects file saves directly, giving instant feedback!

---

## Milestone 3: Debouncing Logic

**Goal**: Avoid reindexing multiple times when multiple files change rapidly (e.g., git checkout, bulk edits).

**Why**: Without debouncing, switching branches or saving multiple files triggers many reindexes.

### Step 3.1: Add Debouncing to Watcher

Update `GitFileWatcher` in `src/git_watcher.rs` to add debouncing:

```rust
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::collections::HashSet;
use tokio::time::{sleep, Duration as TokioDuration};

impl GitFileWatcher {
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
                sleep(TokioDuration::from_millis(debounce_ms)).await;
                
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
                    debug!("Debounce period elapsed, reindexing {} files", files_to_reindex.len());
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
```

### Step 3.2: Configure Debounce Time

Add configuration to `.ragrep/config.toml` support. Update `src/config.rs`:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct GitWatchConfig {
    pub enabled: bool,
    pub debounce_ms: u64,
}

impl Default for GitWatchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 1000, // 1 second default
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    // ... existing fields ...
    
    #[serde(default)]
    pub git_watch: GitWatchConfig,
}
```

**Test**: Modify multiple files quickly, verify they're batched into one reindex:

```bash
# Edit 3 files rapidly
echo "// change 1" >> src/main.rs
echo "// change 2" >> src/lib.rs  
echo "// change 3" >> src/utils.rs

# Wait 1+ second (debounce period)
sleep 2

# Check logs - should show ONE reindex with 3 files, not 3 separate reindexes
```

**Expected output**:
```
[DEBUG] File queued for reindex: src/main.rs
[DEBUG] File queued for reindex: src/lib.rs
[DEBUG] File queued for reindex: src/utils.rs
[DEBUG] Debounce period elapsed, reindexing 3 files
[INFO] Reindexed 3 files (45 chunks) in 0.8s
```

---

## Milestone 4: Incremental Reindexing with Smart Caching

**Goal**: Update only changed files in database, reusing embeddings for unchanged chunks.

**Why**: Makes reindex feel instant (~200ms) instead of slow (2+ seconds).

**Key Optimization**: Delete-then-insert strategy BUT with embedding reuse:
1. Load old embeddings into cache (by content hash)
2. Delete old chunks from database (clean slate)
3. Generate new chunks
4. Reuse embeddings where content hash matches (FAST!)
5. Only re-embed chunks that actually changed (SLOW but rare)

**Result**: Best of both worlds - correctness + speed!

### Step 4.1: Add Methods to Database for Smart Reindexing

Add to `src/db.rs`:

```rust
use std::collections::HashMap;

impl Database {
    /// Get all chunks for a file with their hashes and embeddings (for reuse)
    pub fn get_chunks_with_embeddings(&self, file_path: &str) -> Result<HashMap<i64, Vec<f32>>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT c.hash, v.embedding
            FROM chunks c
            JOIN chunks_vec v ON v.rowid = c.id
            WHERE c.file_path = ?1
            "#
        )?;
        
        let mut cache = HashMap::new();
        let mut rows = stmt.query([file_path])?;
        
        while let Some(row) = rows.next()? {
            let hash: i64 = row.get(0)?;
            let embedding_bytes: Vec<u8> = row.get(1)?;
            
            // Convert bytes back to f32 array
            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            
            cache.insert(hash, embedding);
        }
        
        debug!("Loaded {} embeddings for reuse from {}", cache.len(), file_path);
        Ok(cache)
    }
    
    /// Delete all chunks for a specific file
    pub fn delete_file(&mut self, file_path: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        
        // Get all row IDs for this file
        let mut stmt = tx.prepare(
            "SELECT id FROM chunks WHERE file_path = ?1"
        )?;
        
        let row_ids: Vec<i64> = stmt.query_map([file_path], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        // Delete from vector table
        for row_id in &row_ids {
            tx.execute(
                "DELETE FROM chunks_vec WHERE rowid = ?1",
                [row_id],
            )?;
        }
        
        // Delete from chunks table
        tx.execute(
            "DELETE FROM chunks WHERE file_path = ?1",
            [file_path],
        )?;
        
        tx.commit()?;
        
        debug!("Deleted {} chunks for file: {}", row_ids.len(), file_path);
        
        Ok(())
    }
    
    /// Delete multiple files
    pub fn delete_files(&mut self, file_paths: &[String]) -> Result<()> {
        for path in file_paths {
            self.delete_file(path)?;
        }
        Ok(())
    }
}
```

### Step 4.2: Add Smart Incremental Reindex to Context

Add to `src/context.rs`:

```rust
use crate::chunker::Chunker;
use crate::indexer::{Indexer, FileInfo};
use log::info;
use std::collections::HashMap;

impl AppContext {
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
                    let result = self.embedder.embed_text(&chunk.content, &file_path_str).await?;
                    result.0  // Extract Vec<f32> from Embedding wrapper
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
```

### Step 4.3: Test Incremental Reindex

Create a test:

```bash
# Start server
cargo run -- serve &

# Edit a file
echo "// test change" >> src/main.rs

# Check that only src/main.rs is reindexed (not all files)
```

**Expected Output in logs**:
```
[INFO] Detected 1 changed file
[INFO] Reindexing: src/main.rs
[INFO] Reindexed 1 files (12 chunks) in 1.24s
```

---

## Milestone 5: Server Integration

**Goal**: Wire the git watcher into the server lifecycle.

**Why**: The watcher should start with the server and reindex automatically.

### Step 5.1: Update Server to Start Watcher

Update `src/server.rs`:

```rust
use crate::git_watcher::GitIndexWatcher;
use std::sync::mpsc::Receiver;

impl RagrepServer {
    /// Start the server with git watching enabled
    pub async fn serve(&mut self) -> Result<()> {
        // ... existing PID and socket setup ...

        // Start git watcher if enabled and in a git repo
        let git_watcher = self.start_git_watcher()?;
        
        info!("Server listening on {}", self.socket_path.display());
        
        // Accept connections in a loop
        loop {
            tokio::select! {
                // Handle client connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            let context = Arc::clone(&self.context);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, context).await {
                                    error!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                        }
                    }
                }
                
                // Handle git changes
                Some(changed_files) = git_watcher.as_ref().and_then(|rx| rx.recv().ok()) => {
                    self.handle_git_changes(changed_files).await;
                }
            }
        }
    }
    
    fn start_git_watcher(&self) -> Result<Option<Receiver<Vec<PathBuf>>>> {
        // Check config
        if !self.context.lock().await.config_manager.config.git_watch.enabled {
            info!("Git watching disabled in config");
            return Ok(None);
        }
        
        // Check if in git repo
        let base_path = self.socket_path.parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("Invalid socket path"))?;
        
        if !GitIndexWatcher::is_git_repo(base_path) {
            warn!("Not in a git repository, git watching disabled");
            return Ok(None);
        }
        
        // Start watcher
        let watcher = GitIndexWatcher::new(base_path)?;
        let debounce = self.context.lock().await.config_manager.config.git_watch.debounce_ms;
        let rx = watcher.watch_debounced(debounce)?;
        
        info!("Git watcher started (debounce: {}ms)", debounce);
        
        Ok(Some(rx))
    }
    
    async fn handle_git_changes(&mut self, changed_files: Vec<PathBuf>) {
        info!("Detected {} changed files, reindexing...", changed_files.len());
        
        for file in &changed_files {
            debug!("  - {}", file.display());
        }
        
        let mut context = self.context.lock().await;
        match context.reindex_files(changed_files).await {
            Ok(()) => {
                info!("Reindex complete");
            }
            Err(e) => {
                error!("Reindex failed: {}", e);
            }
        }
    }
}
```

### Step 5.2: Make Server Mutable

Update server field to support mutation:

```rust
pub struct RagrepServer {
    context: Arc<Mutex<AppContext>>,
    socket_path: PathBuf,
    pid_path: PathBuf,
}
```

### Step 5.3: Test End-to-End

Start server and edit files:

```bash
# Terminal 1: Start server
cargo run -- serve

# Terminal 2: Edit files (NO git add needed!)
echo "// change 1" >> src/main.rs

# Watch Terminal 1 for automatic reindex logs
```

**Expected Output in Terminal 1**:
```
[INFO] File watcher started (debounce: 1000ms)
[INFO] Server listening on .ragrep/ragrep.sock
[DEBUG] File queued for reindex: src/main.rs
[DEBUG] Debounce period elapsed, reindexing 1 files
[INFO] Reindexed 1 files (12 chunks, reused 0, computed 12) in 0.3s
[INFO] Reindex complete
```

---

## Milestone 6: Non-Git Handling

**Goal**: Gracefully handle projects that aren't git repositories.

**Why**: Not all projects use git. Server should still work, just without auto-reindex.

### Step 6.1: Add is_git_repo Helper

Already added in Milestone 1, but update usage:

```rust
impl GitChangeDetector {
    pub fn is_git_repo(path: &Path) -> bool {
        Repository::discover(path).is_ok()
    }
}
```

### Step 6.2: Update Server to Handle Non-Git

The code in Milestone 5 already handles this:

```rust
if !GitIndexWatcher::is_git_repo(base_path) {
    warn!("Not in a git repository, git watching disabled");
    return Ok(None);
}
```

### Step 6.3: Test Non-Git Project

```bash
# Create a non-git test directory
mkdir /tmp/test-ragrep
cd /tmp/test-ragrep
echo "fn main() {}" > test.rs

# Index it
ragrep index

# Start server
ragrep serve
```

**Expected Output**:
```
[WARN] Not in a git repository, git watching disabled
[INFO] Server listening on .ragrep/ragrep.sock
```

Server runs but without git watching. ✅

---

## Milestone 7: Testing & Verification

**Goal**: Comprehensive testing to ensure everything works together.

**Why**: Catch edge cases and ensure production readiness.

### Step 7.1: Integration Test

Add to `tests/integration_test.rs`:

```rust
#[tokio::test]
async fn test_git_watcher() -> Result<()> {
    use ragrep::git_watcher::GitChangeDetector;
    
    // Check if we're in a git repo
    let current_dir = std::env::current_dir()?;
    if !GitChangeDetector::is_git_repo(&current_dir) {
        println!("Skipping git watcher test (not in git repo)");
        return Ok(());
    }
    
    let detector = GitChangeDetector::new(&current_dir)?;
    let changed = detector.get_changed_files()?;
    
    println!("Found {} changed files", changed.len());
    for file in &changed {
        println!("  - {}", file.display());
    }
    
    Ok(())
}
```

### Step 7.2: Manual End-to-End Test

Complete workflow test:

```bash
# 1. Clean start
rm -rf .ragrep
cargo build --release

# 2. Initial index
./target/release/rag index

# 3. Start server in background
./target/release/rag serve > server.log 2>&1 &
SERVER_PID=$!
sleep 2  # Let server start

# 4. Query before changes
./target/release/rag "test function" | tee before.txt

# 5. Add new code
cat >> src/main.rs << 'EOF'

fn test_auto_reindex_function() {
    println!("This function should be auto-indexed!");
}
EOF

# 6. Wait for debounce + reindex (NO git add needed!)
sleep 3

# 8. Query after changes
./target/release/rag "test_auto_reindex_function" | tee after.txt

# 9. Verify new function is found
if grep -q "test_auto_reindex_function" after.txt; then
    echo "✅ SUCCESS: Auto-reindex works!"
else
    echo "❌ FAIL: Function not found after auto-reindex"
fi

# 10. Cleanup
kill $SERVER_PID
git restore src/main.rs
```

### Step 7.3: Performance Test

Measure reindex performance:

```bash
# Edit multiple files
for file in src/*.rs; do
    echo "// benchmark change" >> "$file"
done

git add src/

# Measure time from git add to reindex complete
# Check server.log for timing:
# [INFO] Reindexed 8 files (243 chunks) in 3.45s
```

**Success Criteria**:
- 10 files reindex in < 5 seconds
- No duplicate reindexes (debouncing works)
- Memory usage stable (no leaks)

---

## 🎓 What You Learned

After completing Phase 3, you now understand:

1. **Git Internals**: How `.git/index` tracks file changes
2. **File System Watching**: Using `notify` for cross-platform file monitoring
3. **Debouncing**: Preventing rapid-fire events from overwhelming system
4. **Incremental Updates**: Efficiently updating only changed data
5. **Async Coordination**: Using tokio::select! to handle multiple event streams
6. **Production Patterns**: Graceful degradation, configuration, error handling

---

## 🎯 Success Checklist

- [ ] Git change detection works (`git add` triggers reindex)
- [ ] Debouncing prevents multiple rapid reindexes
- [ ] Incremental reindex faster than full reindex (3-5x speedup)
- [ ] Non-git projects handled gracefully (warning, no crash)
- [ ] Server logs show clear reindex progress
- [ ] Search results updated immediately after file changes
- [ ] Memory usage stable over time (no leaks)
- [ ] Works across git operations (checkout, pull, merge)

---

## 🚧 Common Issues

### Issue: "Failed to find git repository"

**Cause**: Running in a directory that isn't a git repo or is above .git folder

**Fix**: 
```bash
# Ensure you're in the git repo
git rev-parse --show-toplevel

# If not in git repo, either:
git init  # Initialize git
# OR
# Accept that git watching will be disabled
```

### Issue: Reindex triggered constantly

**Cause**: Debounce time too short or file being modified continuously

**Fix**: Increase debounce in config:
```toml
[git_watch]
enabled = true
debounce_ms = 2000  # Increase to 2 seconds
```

### Issue: Changes not detected

**Cause**: File extension not in watched list or file outside working directory

**Fix**: Check file extension is supported:
```bash
# Supported extensions (triggers reindex) ✅
vim src/main.rs   # .rs
vim app.py        # .py  
vim index.js      # .js
vim app.ts        # .ts

# NOT supported (no reindex) ❌
vim README.md     # .md not watched
vim data.json     # .json not watched
```

To add more extensions, update the watcher code to include them.

### Issue: High CPU usage

**Cause**: Watcher polling too frequently or debounce task spinning

**Fix**: Check debounce implementation, ensure proper sleep intervals

---

## 📊 Performance Expectations

### Full Reindex (Phase 1)
```
100 files: 30-40 seconds
- Chunk 100 files
- Embed 2,000 chunks
- Insert into database
```

### Incremental Reindex (Phase 3) - WITH SMART CACHING
```
Edit 1 function in 1 file (file has 10 chunks):
- Load old embeddings: ~10ms
- Delete old chunks: ~5ms
- Chunk file: ~20ms
- Reuse 9 embeddings: instant ✅
- Compute 1 new embedding: ~150ms
- Insert 10 chunks: ~10ms
Total: ~200ms ⚡⚡⚡

Edit 3 files (15 chunks each, 5 changed per file):
- Process 3 files × (45 chunks total)
- Reuse 30 embeddings: instant ✅
- Compute 15 new embeddings: ~750ms
- Database ops: ~50ms
Total: ~800ms ⚡⚡

100x-150x faster than full reindex for typical edits!
```

### Key Optimization: Embedding Reuse

**Strategy**: Hash-based embedding cache
- Delete-then-insert ensures correct line numbers
- But REUSE embeddings for unchanged chunk content
- Only re-embed chunks that actually changed

**Result**: Typical single-function edit feels INSTANT (<250ms)

### Debounce Timing
```
debounce_ms = 1000:
- Save file at 0ms
- Save file at 500ms  ← Updates timer
- Save file at 800ms  ← Updates timer
- Wait...
- Reindex at 1800ms (1000ms after last change)

Result: One reindex instead of three! ✅
```

---

## 🔄 Workflow Comparison

### Before Phase 3
```bash
$ ragrep serve &
$ vim src/main.rs  # Edit file
$ ragrep "new code"
  ❌ Not found

$ ragrep index  # Manual reindex (30s)
$ ragrep "new code"
  ✅ Found
```

### After Phase 3
```bash
$ ragrep serve &
$ vim src/main.rs  # Edit and save (NO git add!)
# Auto-reindex happens instantly (0.2-0.8s)
$ ragrep "new code"
  ✅ Found immediately!
```

---

## 🚀 Next Steps

After Phase 3, you can:

1. **Use It Daily**: Let auto-reindex keep your index fresh
2. **Phase 2**: Add MCP integration for AI assistants (optional)
3. **Phase 4**: Production polish (better errors, logging, metrics)
4. **Optimize**: Profile and tune reindex performance
5. **Extend**: Add support for other VCS (svn, hg) if needed

---

## 📝 Configuration Reference

Add to `.ragrep/config.toml`:

```toml
[git_watch]
# Enable/disable git-based auto-reindexing
enabled = true

# Debounce time in milliseconds
# Higher = fewer reindexes but slightly staler index
# Lower = more reindexes but fresher index
debounce_ms = 1000

# Future: Which git events to watch
# events = ["modified", "added", "deleted", "renamed"]
```

---

## 🎉 Congratulations!

You've implemented automatic git-based reindexing! Your ragrep server now:

✅ Keeps models loaded (Phase 1)  
✅ Detects file changes via git (Phase 3)  
✅ Incrementally reindexes only changed files  
✅ Handles non-git projects gracefully  
✅ Provides 10x faster queries + automatic index updates

**Total Performance Win**:
- **Query time**: 7.4s → 2.7s (Phase 1)
- **Reindex time**: 30s → 2s (Phase 3)
- **Manual steps**: Index command → Automatic

You now have a production-ready semantic code search tool! 🚀

---

**Last Updated**: November 24, 2025  
**Status**: Phase 3 Complete, Ready for Implementation  
**Next**: Optional Phase 2 (MCP) or Phase 4 (Production Polish)
