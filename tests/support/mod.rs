use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

/// Start a mock Podman backend on `path` that captures the request and returns a 200 response.
pub fn mock_podman_backend(path: &str) -> (tempfile::TempDir, Arc<Mutex<Vec<u8>>>) {
    let dir = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(path).unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let mut data = Vec::new();
        let mut buf = vec![0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    data.extend_from_slice(&buf[..n]);
                    if data.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "proxy: listener accept failed; terminating");
                    break;
                }
            }
        }
        *captured_clone.lock().unwrap() = data;
        let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    });
    (dir, captured)
}

/// Read the full response from a Unix stream (helper for integration tests).
pub fn read_response(client: &mut UnixStream) -> String {
    let mut response = vec![0u8; 4096];
    match client.read(&mut response) {
        Ok(n) => String::from_utf8_lossy(&response[..n]).to_string(),
        Err(_) => String::new(),
    }
}
