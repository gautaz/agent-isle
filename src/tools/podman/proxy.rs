use std::io::{self, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};

use super::http::*;
use super::parse::*;
use super::secret_detection::*;
use super::types::*;

fn write_response(conn: &mut UnixStream, status: u16, msg: &str) {
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

fn validate_create_secrets(
    method: &str,
    path: &str,
    body: &[u8],
    secrets: &[String],
) -> Option<String> {
    if !is_create_op(method, path) {
        return None;
    }
    let cfg = serde_json::from_slice::<CreateConfig>(body).ok()?;
    let host = cfg.host_config.as_ref()?;
    let mut all_secrets = find_secret_binds(&host.binds, secrets);
    all_secrets.extend(find_secret_mounts(&host.mounts, secrets));
    if all_secrets.is_empty() {
        None
    } else {
        Some(all_secrets.join(", "))
    }
}

fn forward_request(
    real_path: &str,
    method: &str,
    path: &str,
    raw_headers: &[(String, Vec<u8>)],
    content_length: usize,
    is_chunked: bool,
    body: &[u8],
) -> Result<UnixStream, &'static str> {
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
    let mut real_conn = UnixStream::connect(real_path)
        .map_err(|_e| -> &'static str { "cannot connect to podman" })?;
    if real_conn.write_all(forward.as_bytes()).is_err() {
        tracing::warn!("proxy: failed to forward request headers");
    }
    if !body.is_empty() && real_conn.write_all(body).is_err() {
        tracing::warn!("proxy: failed to forward request body");
    }
    Ok(real_conn)
}

fn bidirectional_proxy(
    client_conn: &mut UnixStream,
    real_conn: &mut UnixStream,
) -> Result<(), &'static str> {
    let client_clone = client_conn
        .try_clone()
        .map_err(|_| "failed to clone client connection")?;
    let real_clone = real_conn
        .try_clone()
        .map_err(|_| "failed to clone real connection")?;
    let client_clone2 = client_conn
        .try_clone()
        .map_err(|_| "failed to clone client connection")?;
    let real_clone2 = real_conn
        .try_clone()
        .map_err(|_| "failed to clone real connection")?;
    let t1 = thread::spawn(move || {
        if io::copy(&mut &client_clone, &mut &real_clone).is_err() {
            tracing::warn!("proxy: client-to-real copy failed");
        }
    });
    let t2 = thread::spawn(move || {
        if io::copy(&mut &real_clone2, &mut &client_clone2).is_err() {
            tracing::warn!("proxy: real-to-client copy failed");
        }
    });
    if t1.join().is_err() {
        tracing::warn!("proxy: client-to-real thread panicked");
    }
    if t2.join().is_err() {
        tracing::warn!("proxy: real-to-client thread panicked");
    }
    Ok(())
}

fn check_secrets_blocking(
    client_conn: &mut UnixStream,
    method: &str,
    path: &str,
    body: &[u8],
    secrets: &[String],
) -> bool {
    if let Some(paths) = validate_create_secrets(method, path, body, secrets) {
        tracing::warn!(paths = %paths, "proxy: blocked container create with secret paths");
        write_response(
            client_conn,
            403,
            &format!("mount sources contain secret paths: {paths}"),
        );
        return true;
    }
    false
}

fn proxy_handle(mut client_conn: UnixStream, real_path: &str, secrets: &[String]) {
    let (_buf, parsed, body) = match parse_request(&mut client_conn) {
        Ok(v) => v,
        Err(msg) => {
            tracing::debug!(error = msg, "proxy: request parse failed");
            write_response(&mut client_conn, 502, msg);
            return;
        }
    };

    if check_secrets_blocking(
        &mut client_conn,
        &parsed.method,
        &parsed.path,
        &body,
        secrets,
    ) {
        return;
    }

    let mut real_conn = match forward_request(
        real_path,
        &parsed.method,
        &parsed.path,
        &parsed.raw_headers,
        parsed.content_length,
        parsed.is_chunked,
        &body,
    ) {
        Ok(c) => c,
        Err(msg) => {
            tracing::error!(error = msg, "proxy: forward failed");
            write_response(&mut client_conn, 502, msg);
            return;
        }
    };

    if bidirectional_proxy(&mut client_conn, &mut real_conn).is_err() {
        tracing::error!("proxy: bidirectional proxy failed");
        write_response(&mut client_conn, 502, "proxy setup failed");
    }
}

/// Start a Unix socket proxy that intercepts container create requests
/// and rejects those that would mount secret files.
pub fn start_proxy(
    listen_path: &str,
    real_path: &str,
    secrets: Vec<String>,
) -> Result<impl FnOnce()> {
    if let Some(parent) = Path::new(listen_path).parent() {
        std::fs::create_dir_all(parent).context("create proxy dir")?;
    }

    let listener = UnixListener::bind(listen_path).context(format!("listen on {listen_path}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) =
            std::fs::set_permissions(listen_path, std::fs::Permissions::from_mode(0o700))
        {
            tracing::warn!(error = %e, "proxy: failed to set socket permissions");
        }
    }

    let secrets = Arc::new(secrets);
    let real_path = real_path.to_string();
    let listen_path = listen_path.to_string();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    if let Err(e) = listener.set_nonblocking(true) {
        tracing::warn!(error = %e, "proxy: failed to set non-blocking mode");
    }
    let handle = thread::spawn(move || loop {
        if shutdown_clone.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let secrets = Arc::clone(&secrets);
                let real_path = real_path.clone();
                thread::spawn(move || {
                    proxy_handle(stream, &real_path, &secrets);
                });
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(_) => break,
        }
    });

    let stop = move || {
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = handle.join();
        let _ = std::fs::remove_file(&listen_path);
    };

    Ok(stop)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn test_write_response() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_response(&mut stream, 403, "forbidden");
        });

        let mut client = UnixStream::connect(&sock_path).unwrap();
        handle.join().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("HTTP/1.1 403 Forbidden"));
        assert!(response.contains("forbidden"));
    }

    #[test]
    fn test_write_response_502() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_response(&mut stream, 502, "bad gateway");
        });

        let mut client = UnixStream::connect(&sock_path).unwrap();
        handle.join().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("HTTP/1.1 502 Bad Gateway"));
    }

    #[test]
    fn test_write_response_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_response(&mut stream, 200, "ok");
        });

        let mut client = UnixStream::connect(&sock_path).unwrap();
        handle.join().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("HTTP/1.1 200 Unknown"));
    }

    #[test]
    fn test_start_proxy_and_stop() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        assert!(listen_path.exists());

        let client = UnixStream::connect(&listen_path);
        assert!(client.is_ok());
        drop(client);

        let saved_path = listen_path.clone();
        stop();

        assert!(
            !saved_path.exists(),
            "Socket file should be removed after stop"
        );

        let result = UnixStream::connect(&saved_path);
        assert!(
            result.is_err(),
            "Expected connection to fail after stop, but it succeeded"
        );
    }
}
