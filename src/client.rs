use crate::constants::constants;
use crate::protocol::{Message, SearchRequest, SearchResponse};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use log::debug;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub struct RagrepClient {
    socket_path: PathBuf,
}

impl RagrepClient {
    /// Create a new client by finding the server socket
    pub fn new(start_dir: &Path) -> Result<Self> {
        let socket_path = find_ragrep_socket(start_dir)?;
        Ok(Self { socket_path })
    }

    /// Get the socket path this client is connected to
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Execute a search query against the server
    pub async fn search(&self, request: SearchRequest) -> Result<SearchResponse> {
        debug!("Connecting to server at {}", self.socket_path.display());

        // Connect to server
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to server")?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Send request
        let request_msg = Message::Request {
            id: 1, // Simple client uses id=1
            request,
        };
        let request_json = serde_json::to_string(&request_msg)?;
        writer.write_all(request_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;

        debug!("Sent request, waiting for response...");

        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        // Parse response
        let response: Message = serde_json::from_str(&line).context("Failed to parse response")?;

        match response {
            Message::Response { response, .. } => Ok(response),
            Message::Error { message, .. } => Err(anyhow!("Server error: {}", message)),
            _ => Err(anyhow!("Unexpected response type")),
        }
    }

    /// Check if a server is available without connecting
    pub fn is_server_available(start_dir: &Path) -> bool {
        find_ragrep_socket(start_dir).is_ok()
    }
}

/// Find the ragrep socket by walking up the directory tree
fn find_ragrep_socket(start_dir: &Path) -> Result<PathBuf> {
    let mut current = start_dir;

    loop {
        let socket_path = current
            .join(constants::RAGREP_DIR_NAME)
            .join(constants::SOCKET_FILENAME);

        if socket_path.exists() {
            debug!("Found socket at {}", socket_path.display());
            return Ok(socket_path);
        }

        // Try parent directory
        current = current
            .parent()
            .ok_or_else(|| anyhow!("No ragrep server found (searched up to root)"))?;
    }
}
