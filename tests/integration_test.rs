use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Get the path to the ragrep binary
fn get_binary_path() -> String {
    // Try to use CARGO_BIN_EXE_rag if available (set by cargo test)
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_rag") {
        return path;
    }
    // Fallback to relative path
    "./target/debug/rag".to_string()
}

#[test]
fn test_server_client_integration() {
    // Build the binary first
    let status = Command::new("cargo")
        .args(&["build"])
        .status()
        .expect("Failed to build");
    assert!(status.success(), "Failed to build binary");

    let binary = get_binary_path();

    // Make sure no existing server is running
    let _ = Command::new("pkill").args(&["-f", "rag serve"]).status();
    thread::sleep(Duration::from_secs(1));

    // Clean up any stale socket/PID files
    let _ = std::fs::remove_file(".ragrep/ragrep.sock");
    let _ = std::fs::remove_file(".ragrep/server.pid");

    // Start server in background
    let mut server = Command::new(&binary)
        .arg("serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    // Give server time to start (models need to load)
    thread::sleep(Duration::from_secs(6));

    // Run a query using the client
    let output = Command::new(&binary)
        .arg("error handling")
        .output()
        .expect("Failed to run query");

    // Should succeed
    assert!(
        output.status.success(),
        "Query failed. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Should have results (either file paths or "No similar code found" message)
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // Check for either results or server connection message
    assert!(
        combined.contains("src/")
            || combined.contains("Server detected")
            || combined.contains("No similar code found"),
        "Expected results or server message. stdout: {}, stderr: {}",
        stdout,
        stderr
    );

    // Cleanup: kill server
    server.kill().expect("Failed to kill server");
    let _ = server.wait(); // Wait for process to finish

    // Clean up socket and PID files
    thread::sleep(Duration::from_millis(100));
    let _ = std::fs::remove_file(".ragrep/ragrep.sock");
    let _ = std::fs::remove_file(".ragrep/server.pid");
}

#[test]
fn test_standalone_fallback() {
    // Make sure no server is running
    let _ = Command::new("pkill").args(&["-f", "rag serve"]).status();

    thread::sleep(Duration::from_secs(1));

    // Clean up any stale socket/PID files
    let _ = std::fs::remove_file(".ragrep/ragrep.sock");
    let _ = std::fs::remove_file(".ragrep/server.pid");

    let binary = get_binary_path();

    // Run query without server
    let output = Command::new(&binary)
        .arg("error handling")
        .output()
        .expect("Failed to run query");

    // Should still succeed
    assert!(
        output.status.success(),
        "Standalone query failed. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Should have warning about standalone mode
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("standalone") || stderr.contains("No server detected"),
        "Expected standalone mode message. stderr: {}",
        stderr
    );
}
