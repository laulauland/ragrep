/// Centralized constants for ragrep
pub mod constants {
    /// Filename for ragrep ignore file (similar to .gitignore)
    pub const RAGREP_IGNORE_FILENAME: &str = ".ragrepignore";

    /// Directory name for ragrep metadata (hidden directory in project root)
    pub const RAGREP_DIR_NAME: &str = ".ragrep";

    /// Database filename
    pub const DATABASE_FILENAME: &str = "ragrep.db";

    /// Unix socket filename for server communication
    pub const SOCKET_FILENAME: &str = "ragrep.sock";

    /// PID file filename for server process tracking
    pub const PID_FILENAME: &str = "server.pid";

    /// Configuration filename
    pub const CONFIG_FILENAME: &str = "config.toml";

    /// Global config directory name (in user config/data directories)
    pub const GLOBAL_CONFIG_DIR_NAME: &str = "ragrep";

    /// Models subdirectory name
    pub const MODELS_DIR_NAME: &str = "models";

    /// Default file extensions to index
    pub const DEFAULT_FILE_EXTENSIONS: &[&str] = &["rs", "py", "js", "ts"];

    /// Common build/cache directories to ignore
    pub const IGNORED_DIRECTORIES: &[&str] = &[
        "node_modules",
        "target",
        ".git",
        "__pycache__",
        ".next",
        "dist",
        "build",
    ];
}
