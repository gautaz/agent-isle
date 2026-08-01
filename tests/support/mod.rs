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

/// Start a keep-alive Podman backend on `path` that answers each request with
/// the given status (in order) and stays open. Captures all request bytes.
pub fn mock_keepalive_backend(
    path: &str,
    statuses: Vec<u16>,
) -> (tempfile::TempDir, Arc<Mutex<Vec<u8>>>) {
    let dir = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(path).unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut buf = vec![0u8; 8192];
        for status in &statuses {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => captured_clone.lock().unwrap().extend_from_slice(&buf[..n]),
            }
            let reason = if *status == 201 { "Created" } else { "OK" };
            let resp = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 2\r\n\r\n{{}}");
            if stream.write_all(resp.as_bytes()).is_err() {
                return;
            }
            let _ = stream.flush();
        }
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(200)));
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
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
