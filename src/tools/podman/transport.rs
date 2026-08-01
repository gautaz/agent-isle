use std::io;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::thread;

use anyhow::Result;

/// Write a JSON `{"message": ...}` HTTP response to the client.
pub(super) fn write_response(conn: &mut UnixStream, status: u16, msg: &str) {
    let body = serde_json::json!({"message": msg}).to_string();
    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        match status {
            403 => "Forbidden",
            502 => "Bad Gateway",
            _ => "Unknown",
        },
        body.len()
    );
    if conn.write_all(response.as_bytes()).is_err() || conn.flush().is_err() {
        tracing::warn!("proxy: failed to write response to client");
    }
}

/// Rebuild a request's wire bytes for forwarding, reusing the client's
/// headers while normalising the framing headers (`Content-Length` /
/// `Transfer-Encoding`).
pub(super) fn build_request_bytes(
    method: &str,
    path: &str,
    raw_headers: &[(String, Vec<u8>)],
    content_length: usize,
    is_chunked: bool,
    body: &[u8],
) -> Vec<u8> {
    let mut forward = format!("{method} {path} HTTP/1.1\r\n");
    forward.push_str(&format!("Content-Length: {content_length}\r\n"));
    if is_chunked {
        forward.push_str("Transfer-Encoding: chunked\r\n");
    }
    for (name, value) in raw_headers {
        if name.eq_ignore_ascii_case("Content-Length")
            || name.eq_ignore_ascii_case("Transfer-Encoding")
        {
            continue;
        }
        forward.push_str(name);
        forward.push_str(": ");
        forward.push_str(std::str::from_utf8(value).unwrap_or(""));
        forward.push_str("\r\n");
    }
    forward.push_str("\r\n");
    let mut bytes = forward.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Forward a parsed request to the real Podman socket on a fresh connection.
pub(super) fn forward_request(
    real_path: &str,
    method: &str,
    path: &str,
    raw_headers: &[(String, Vec<u8>)],
    content_length: usize,
    is_chunked: bool,
    body: &[u8],
) -> Result<UnixStream, &'static str> {
    let bytes = build_request_bytes(method, path, raw_headers, content_length, is_chunked, body);
    let mut real_conn = UnixStream::connect(real_path)
        .map_err(|_e| -> &'static str { "cannot connect to podman" })?;
    if real_conn.write_all(&bytes).is_err() {
        tracing::warn!("proxy: failed to forward request");
    }
    Ok(real_conn)
}

/// Pump the real socket's response bytes to the client on a background thread
/// until the real side closes. Returns the handle so the caller can join.
pub(super) fn relay_real_to_client(
    real_conn: &UnixStream,
    client_conn: &UnixStream,
) -> Option<thread::JoinHandle<()>> {
    let client_clone = client_conn.try_clone().ok()?;
    let real_clone = real_conn.try_clone().ok()?;
    Some(thread::spawn(move || {
        if io::copy(&mut &real_clone, &mut &client_clone).is_err() {
            tracing::warn!("proxy: real-to-client copy failed");
        }
        let _ = client_clone.shutdown(Shutdown::Write);
    }))
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    use super::*;

    fn receive_write_response(status: u16, msg: &'static str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_response(&mut stream, status, msg);
        });

        let mut client = UnixStream::connect(&sock_path).unwrap();
        handle.join().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn test_write_response() {
        let response = receive_write_response(403, "forbidden");
        assert!(response.contains("HTTP/1.1 403 Forbidden"));
        assert!(response.contains("forbidden"));
    }

    #[test]
    fn test_write_response_502() {
        let response = receive_write_response(502, "bad gateway");
        assert!(response.contains("HTTP/1.1 502 Bad Gateway"));
    }

    #[test]
    fn test_write_response_unknown() {
        let response = receive_write_response(200, "ok");
        assert!(response.contains("HTTP/1.1 200 Unknown"));
    }
}
