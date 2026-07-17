use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Mount {
    #[serde(rename = "Type")]
    mount_type: String,
    #[serde(rename = "Source")]
    source: String,
    #[allow(dead_code)]
    #[serde(rename = "Target")]
    target: String,
}

#[derive(Deserialize)]
struct HostConfig {
    #[serde(default)]
    binds: Vec<String>,
    #[serde(default)]
    mounts: Vec<Mount>,
}

#[derive(Deserialize)]
struct CreateConfig {
    #[serde(rename = "HostConfig")]
    host_config: Option<HostConfig>,
}

/// Report whether mount_source overlaps with any secret path.
pub fn contains_secret(mount_source: &str, secrets: &[String]) -> bool {
    let clean = normalize_absolute_path(mount_source);
    for s in secrets {
        let cs = normalize_absolute_path(s);
        if cs == clean
            || clean.starts_with(&format!("{cs}/"))
            || cs.starts_with(&format!("{clean}/"))
        {
            return true;
        }
    }
    false
}

/// Return bind mount sources that contain secrets.
pub fn find_secret_binds(binds: &[String], secrets: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for b in binds {
        let source = b.split_once(':').map(|(s, _)| s).unwrap_or(b);
        if contains_secret(source, secrets) {
            found.push(source.to_string());
        }
    }
    found
}

/// Return mount sources that contain secrets.
pub fn find_secret_mounts(mounts: &[Mount], secrets: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for m in mounts {
        if m.mount_type == "bind" && !m.source.is_empty() && contains_secret(&m.source, secrets) {
            found.push(m.source.clone());
        }
    }
    found
}

/// Extract a version prefix (e.g. "v5") from a URL path.
pub fn extract_api_version(path: &str) -> String {
    let base = path.trim_start_matches('/');
    let first = base.split('/').next().unwrap_or("");
    if first.len() > 1 && first.starts_with('v') {
        if let Some(&c) = first.as_bytes().get(1) {
            if c.is_ascii_digit() {
                return first.to_string();
            }
        }
    }
    String::new()
}

/// Report whether the request is a container create operation.
pub fn is_create_op(method: &str, path: &str) -> bool {
    if method != "POST" {
        return false;
    }
    let mut base = path.trim_start_matches('/');
    if let Some(ver) = extract_api_version(path).into() {
        let ver: String = ver;
        base = base
            .strip_prefix(&ver)
            .unwrap_or(base)
            .trim_start_matches('/');
    }
    base == "containers/create" || base.ends_with("/containers/create")
}

/// Normalize an absolute path by resolving `.` and `..` segments.
/// Panics in debug builds if given a relative path.
fn normalize_absolute_path(path: &str) -> String {
    debug_assert!(path.starts_with('/'), "expected absolute path, got: {path}");
    std::path::PathBuf::from(path)
        .clean()
        .to_string_lossy()
        .to_string()
}

trait PathClean {
    fn clean(&self) -> PathBuf;
}

impl PathClean for Path {
    fn clean(&self) -> PathBuf {
        let mut components = Vec::new();
        for comp in self.components() {
            match comp {
                std::path::Component::ParentDir => {
                    if matches!(components.last(), Some(std::path::Component::Normal(_))) {
                        components.pop();
                    }
                }
                std::path::Component::Normal(_) | std::path::Component::RootDir => {
                    components.push(comp);
                }
                _ => {}
            }
        }
        components.iter().collect()
    }
}

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
    let _ = conn.write_all(response.as_bytes());
    let _ = conn.flush();
}

const MAX_HEADER_SIZE: usize = 8192;

fn proxy_handle(mut client_conn: UnixStream, real_path: &str, secrets: &[String]) {
    // Read enough bytes for headers using httparse
    let mut buf = vec![0u8; MAX_HEADER_SIZE];
    let mut nread = 0;
    loop {
        match buf
            .get_mut(nread..)
            .and_then(|slice| client_conn.read(slice).ok())
        {
            Some(0) => {
                write_response(&mut client_conn, 502, "connection closed before headers");
                return;
            }
            Some(n) => nread += n,
            None => {
                write_response(&mut client_conn, 502, "failed to read request");
                return;
            }
        }
        if nread >= MAX_HEADER_SIZE {
            write_response(&mut client_conn, 502, "headers too large");
            return;
        }
        // Check if we have the complete header section
        if buf
            .get(..nread)
            .is_some_and(|slice| slice.windows(4).any(|w| w == b"\r\n\r\n"))
        {
            break;
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let status = buf
        .get(..nread)
        .map_or(Err(httparse::Error::Status), |slice| req.parse(slice));
    match status {
        Ok(httparse::Status::Complete(header_len)) => {
            let method = match req.method {
                Some(m) => m,
                None => {
                    write_response(&mut client_conn, 502, "missing method");
                    return;
                }
            };
            let path = match req.path {
                Some(p) => p,
                None => {
                    write_response(&mut client_conn, 502, "missing path");
                    return;
                }
            };

            // Extract Content-Length and Transfer-Encoding from parsed headers
            let mut content_length = 0usize;
            let mut is_chunked = false;
            for h in req.headers.iter() {
                if h.name.eq_ignore_ascii_case("Content-Length") {
                    content_length = std::str::from_utf8(h.value)
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
                if h.name.eq_ignore_ascii_case("Transfer-Encoding") {
                    let val = std::str::from_utf8(h.value).unwrap_or("");
                    if val.eq_ignore_ascii_case("chunked") {
                        is_chunked = true;
                    }
                }
            }

            // Read body: we already have header_len bytes consumed, body follows
            let body_start = header_len;
            let body_already = nread.saturating_sub(body_start);
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                // Copy what we already read past the headers
                let to_copy = body_already.min(content_length);
                if let Some(src) = buf.get(body_start..body_start + to_copy) {
                    if let Some(dst) = body.get_mut(..to_copy) {
                        dst.clone_from_slice(src);
                    }
                }
                // Read the rest from the stream
                if to_copy < content_length
                    && body
                        .get_mut(to_copy..)
                        .and_then(|slice| client_conn.read_exact(slice).ok())
                        .is_none()
                {
                    write_response(&mut client_conn, 502, "incomplete body");
                    return;
                }
            }

            // Check if this is a container create operation
            if is_create_op(method, path) {
                if let Ok(cfg) = serde_json::from_slice::<CreateConfig>(&body) {
                    if let Some(host) = &cfg.host_config {
                        let bind_secrets = find_secret_binds(&host.binds, secrets);
                        let mount_secrets = find_secret_mounts(&host.mounts, secrets);
                        let mut all_secrets = bind_secrets;
                        all_secrets.extend(mount_secrets);
                        if !all_secrets.is_empty() {
                            let paths = all_secrets.join(", ");
                            write_response(
                                &mut client_conn,
                                403,
                                &format!("mount sources contain secret paths: {paths}"),
                            );
                            return;
                        }
                    }
                }
            }

            // Connect to real podman socket
            let real_conn = UnixStream::connect(real_path);
            let mut real_conn = match real_conn {
                Ok(c) => c,
                Err(e) => {
                    write_response(
                        &mut client_conn,
                        502,
                        &format!("cannot connect to podman: {e}"),
                    );
                    return;
                }
            };

            // Forward request
            let mut forward = format!("{method} {path} HTTP/1.1\r\n");
            forward.push_str(&format!("Content-Length: {}\r\n", body.len()));
            if is_chunked {
                forward.push_str("Transfer-Encoding: chunked\r\n");
            }
            // Forward all original headers except Host (reconstruct)
            for h in req.headers.iter() {
                if h.name.eq_ignore_ascii_case("Host") {
                    continue;
                }
                if h.name.eq_ignore_ascii_case("Content-Length")
                    || h.name.eq_ignore_ascii_case("Transfer-Encoding")
                {
                    continue; // already handled above
                }
                forward.push_str(h.name);
                forward.push_str(": ");
                forward.push_str(std::str::from_utf8(h.value).unwrap_or(""));
                forward.push_str("\r\n");
            }
            forward.push_str("\r\n");
            let _ = real_conn.write_all(forward.as_bytes());
            if !body.is_empty() {
                let _ = real_conn.write_all(&body);
            }

            // Bidirectional relay
            let client_clone = match client_conn.try_clone() {
                Ok(c) => c,
                Err(_) => {
                    write_response(&mut client_conn, 502, "failed to clone client connection");
                    return;
                }
            };
            let real_clone = match real_conn.try_clone() {
                Ok(c) => c,
                Err(_) => {
                    write_response(&mut client_conn, 502, "failed to clone real connection");
                    return;
                }
            };
            let client_clone2 = match client_conn.try_clone() {
                Ok(c) => c,
                Err(_) => {
                    write_response(&mut client_conn, 502, "failed to clone client connection");
                    return;
                }
            };
            let real_clone2 = match real_conn.try_clone() {
                Ok(c) => c,
                Err(_) => {
                    write_response(&mut client_conn, 502, "failed to clone real connection");
                    return;
                }
            };

            let t1 = thread::spawn(move || {
                io::copy(&mut &client_clone, &mut &real_clone).ok();
            });
            let t2 = thread::spawn(move || {
                io::copy(&mut &real_clone2, &mut &client_clone2).ok();
            });

            let _ = t1.join();
            let _ = t2.join();
        }
        Ok(httparse::Status::Partial) => {
            write_response(&mut client_conn, 502, "incomplete headers");
        }
        Err(_) => {
            write_response(&mut client_conn, 502, "malformed request");
        }
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
        let _ = std::fs::set_permissions(listen_path, std::fs::Permissions::from_mode(0o700));
    }

    let secrets = Arc::new(secrets);
    let real_path = real_path.to_string();
    let listen_path = listen_path.to_string();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // Spawn accept loop with non-blocking polling
    listener.set_nonblocking(true).ok();
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
    use super::*;

    #[test]
    fn test_contains_secret() {
        let secrets = vec![
            "/home/user/.ssh/id_rsa".to_string(),
            "/home/user/.env".to_string(),
        ];

        assert!(contains_secret("/home/user/.ssh/id_rsa", &secrets));
        assert!(!contains_secret("/home/user/.ssh/keys/old", &secrets));
        assert!(contains_secret("/home/user/.ssh", &secrets));
        assert!(!contains_secret("/home/user/Documents/file.txt", &secrets));
        assert!(contains_secret("/home/user/../user/.ssh/id_rsa", &secrets));
    }

    #[test]
    fn test_find_secret_binds() {
        let secrets = vec![
            "/home/user/.ssh/id_rsa".to_string(),
            "/home/user/.env".to_string(),
        ];
        let binds = vec![
            "/home/user/Documents:/mnt/docs".to_string(),
            "/home/user/.ssh:/mnt/ssh".to_string(),
            "/home/user/.env:/mnt/env".to_string(),
        ];
        let result = find_secret_binds(&binds, &secrets);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"/home/user/.ssh".to_string()));
        assert!(result.contains(&"/home/user/.env".to_string()));
    }

    #[test]
    fn test_find_secret_binds_clean() {
        let secrets = vec!["/home/user/.env".to_string()];
        let binds = vec!["/data:/mnt/data".to_string()];
        assert_eq!(find_secret_binds(&binds, &secrets), Vec::<String>::new());
    }

    #[test]
    fn test_find_secret_mounts() {
        let secrets = vec![
            "/home/user/.env".to_string(),
            "/home/user/.ssh/id_rsa".to_string(),
        ];
        let mounts = vec![
            Mount {
                mount_type: "bind".into(),
                source: "/home/user/.env".into(),
                target: "/app/.env".into(),
            },
            Mount {
                mount_type: "bind".into(),
                source: "/home/user/.ssh/id_rsa".into(),
                target: "/app/.ssh".into(),
            },
            Mount {
                mount_type: "volume".into(),
                source: "".into(),
                target: "/data".into(),
            },
        ];
        let result = find_secret_mounts(&mounts, &secrets);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"/home/user/.env".to_string()));
        assert!(result.contains(&"/home/user/.ssh/id_rsa".to_string()));
    }

    #[test]
    fn test_find_secret_mounts_no_bind() {
        let secrets = vec!["/home/user/.env".to_string()];
        let mounts = vec![Mount {
            mount_type: "volume".into(),
            source: "".into(),
            target: "/data".into(),
        }];
        assert_eq!(find_secret_mounts(&mounts, &secrets), Vec::<String>::new());
    }

    #[test]
    fn test_extract_api_version() {
        assert_eq!(extract_api_version("/v5/containers/create"), "v5");
        assert_eq!(
            extract_api_version("/v1.25/libpod/containers/create"),
            "v1.25"
        );
        assert_eq!(extract_api_version("/containers/create"), "");
        assert_eq!(extract_api_version("/foo/bar"), "");
    }

    #[test]
    fn test_is_create_op() {
        assert!(is_create_op("POST", "/containers/create"));
        assert!(is_create_op("POST", "/v5/containers/create"));
        assert!(is_create_op("POST", "/v5/libpod/containers/create"));
        assert!(!is_create_op("GET", "/containers/create"));
        assert!(!is_create_op("POST", "/containers/list"));
    }

    #[test]
    fn test_normalize_absolute_path() {
        assert_eq!(normalize_absolute_path("/a/b/../c"), "/a/c");
        assert_eq!(normalize_absolute_path("/a/./b"), "/a/b");
        assert_eq!(normalize_absolute_path("/a/b/../../c"), "/c");
        assert_eq!(normalize_absolute_path("/a/b/../../../c"), "/c");
        assert_eq!(normalize_absolute_path("/a/b/c"), "/a/b/c");
        assert_eq!(normalize_absolute_path("/"), "/");
    }

    #[test]
    fn test_normalize_absolute_path_trailing_slash() {
        assert_eq!(normalize_absolute_path("/a/b/c/"), "/a/b/c");
        assert_eq!(normalize_absolute_path("/a/b/../"), "/a");
        assert_eq!(normalize_absolute_path("/a/./"), "/a");
    }

    #[test]
    #[should_panic(expected = "expected absolute path")]
    fn test_normalize_absolute_path_rejects_relative() {
        normalize_absolute_path("a/b/../c");
    }

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

        // save the path before stop() moves it into the closure
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

    #[test]
    fn test_start_proxy_rejects_secret_mount() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec!["/home/user/.ssh/id_rsa".to_string()],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let body = r#"{"HostConfig":{"binds":["/home/user/.ssh:/mnt/ssh"]}}"#;

        let request = indoc::formatdoc! {"\
            POST /v5/containers/create HTTP/1.1\r\n\
            Content-Type: application/json\r\n\
            Content-Length: {body_len}\r\n\
            \r\n\
            {body}",
            body_len = body.len(),
            body = body,
        };

        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("403"),
            "Expected 403 in response: {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_rejects_secret_mount_type() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec!["/home/user/.ssh/id_rsa".to_string()],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let body = r#"{"HostConfig":{"mounts":[{"Type":"bind","Source":"/home/user/.ssh/id_rsa","Target":"/mnt/ssh"}]}}"#;

        let request = indoc::formatdoc! {"\
            POST /v5/containers/create HTTP/1.1\r\n\
            Content-Type: application/json\r\n\
            Content-Length: {body_len}\r\n\
            \r\n\
            {body}",
            body_len = body.len(),
            body = body,
        };

        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("403"),
            "Expected 403 in response: {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_headers_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let huge_header = format!("X-Big: {}\r\n", "A".repeat(8200));
        let request = format!("POST /containers/create HTTP/1.1\r\n{huge_header}");
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let mut response = vec![0u8; 4096];
        let n = client.read(&mut response).unwrap_or(0);
        let response = String::from_utf8_lossy(&response[..n]);
        assert!(
            response.contains("502"),
            "Expected 502 in response: {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_malformed_request() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        client.write_all(b"not http at all\r\n\r\n").unwrap();
        client.flush().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("502"),
            "Expected 502 in response: {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_partial_headers() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        client
            .write_all(b"POST /containers/create HTTP/1.1\r\n")
            .unwrap();
        client.flush().unwrap();
        let _ = client.shutdown(std::net::Shutdown::Write);

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("502"),
            "Expected 502 in response: {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_non_create_post_forwards() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let request = "POST /v5/containers/start HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("502"),
            "Expected 502 (cannot connect to podman): {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_connection_closed_before_headers() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        drop(client);

        std::thread::sleep(std::time::Duration::from_millis(50));

        stop();
    }

    #[test]
    fn test_proxy_create_no_host_config() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec!["/secret".to_string()],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let body = r#"{"Image":"alpine"}"#;
        let request = indoc::formatdoc! {"\
            POST /v5/containers/create HTTP/1.1\r\n\
            Content-Length: {body_len}\r\n\
            \r\n\
            {body}",
            body_len = body.len(),
            body = body,
        };
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("502"),
            "Expected 502 (cannot connect to podman): {response}"
        );

        stop();
    }

    fn mock_podman_backend(
        path: &str,
    ) -> (tempfile::TempDir, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(path).unwrap();
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
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
                        Err(_) => break,
                    }
                }
                *captured_clone.lock().unwrap() = data;
                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        (dir, captured)
    }

    fn read_response(client: &mut UnixStream) -> String {
        let mut response = vec![0u8; 4096];
        match client.read(&mut response) {
            Ok(n) => String::from_utf8_lossy(&response[..n]).to_string(),
            Err(_) => String::new(),
        }
    }

    #[test]
    fn test_proxy_forwards_request_to_backend() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let request = "POST /v5/containers/start HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let response = read_response(&mut client);
        assert!(
            response.contains("200"),
            "Expected 200 in response: {response}"
        );

        let backend_data = captured.lock().unwrap();
        let backend_str = String::from_utf8_lossy(&backend_data);
        assert!(
            backend_str.contains("POST /v5/containers/start"),
            "Backend should receive forwarded request: {backend_str}"
        );

        stop();
    }

    #[test]
    fn test_proxy_forwards_with_body() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let body = r#"{"Signal":"SIGTERM"}"#;
        let request = indoc::formatdoc! {"\
            POST /v5/containers/test/kill HTTP/1.1\r\n\
            Content-Type: application/json\r\n\
            Content-Length: {body_len}\r\n\
            \r\n\
            {body}",
            body_len = body.len(),
            body = body,
        };
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let response = read_response(&mut client);
        assert!(response.contains("200"), "Expected 200: {response}");

        let backend_data = captured.lock().unwrap();
        let backend_str = String::from_utf8_lossy(&backend_data);
        assert!(
            backend_str.contains("containers/test/kill"),
            "Backend should receive path: {backend_str}"
        );
        assert!(
            backend_str.contains(body),
            "Backend should receive body: {backend_str}"
        );

        stop();
    }

    #[test]
    fn test_proxy_forwards_chunked_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let body = r#"{"AttachStdin":false}"#;
        let request = indoc::formatdoc! {"\
            POST /v5/containers/create HTTP/1.1\r\n\
            Content-Type: application/json\r\n\
            Transfer-Encoding: chunked\r\n\
            Content-Length: {body_len}\r\n\
            \r\n\
            {body}",
            body_len = body.len(),
            body = body,
        };
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let response = read_response(&mut client);
        assert!(response.contains("200"), "Expected 200: {response}");

        let backend_data = captured.lock().unwrap();
        let backend_str = String::from_utf8_lossy(&backend_data);
        assert!(
            backend_str.contains("Transfer-Encoding: chunked"),
            "Backend should see chunked header: {backend_str}"
        );

        stop();
    }

    #[test]
    fn test_proxy_forwards_extra_headers() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let request = "GET /v5/info HTTP/1.1\r\nHost: localhost\r\nX-Custom: test-value\r\nAccept: application/json\r\n\r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let response = read_response(&mut client);
        assert!(response.contains("200"), "Expected 200: {response}");

        let backend_data = captured.lock().unwrap();
        let backend_str = String::from_utf8_lossy(&backend_data);
        assert!(
            !backend_str.contains("Host:"),
            "Host header should be stripped: {backend_str}"
        );
        assert!(
            backend_str.contains("X-Custom: test-value"),
            "Custom headers should be forwarded: {backend_str}"
        );
        assert!(
            backend_str.contains("Accept: application/json"),
            "Accept header should be forwarded: {backend_str}"
        );

        stop();
    }

    #[test]
    fn test_proxy_empty_body_post() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let request = "POST /v5/containers/test/start HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let response = read_response(&mut client);
        assert!(response.contains("200"), "Expected 200: {response}");

        let backend_data = captured.lock().unwrap();
        let backend_str = String::from_utf8_lossy(&backend_data);
        assert!(
            backend_str.contains("POST /v5/containers/test/start"),
            "Backend should receive request: {backend_str}"
        );

        stop();
    }

    #[test]
    fn test_proxy_incomplete_body() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");
        let real_path = dir.path().join("real.sock");

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            real_path.to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let request = "POST /v5/containers/create HTTP/1.1\r\nContent-Length: 200\r\n\r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let partial = vec![0xAB_u8; 10];
        client.write_all(&partial).unwrap();
        client.flush().unwrap();
        let _ = client.shutdown(std::net::Shutdown::Write);

        let mut response = vec![0u8; 4096];
        let n = client.read(&mut response).unwrap_or(0);
        let response = String::from_utf8_lossy(&response[..n]);
        assert!(response.contains("502"), "Expected 502: {response}");
        assert!(
            response.contains("incomplete body"),
            "Expected incomplete body: {response}"
        );

        stop();
    }

    #[test]
    fn test_proxy_backend_closes_without_response() {
        let dir = tempfile::tempdir().unwrap();
        let listen_path = dir.path().join("proxy.sock");

        let backend_dir = tempfile::tempdir().unwrap();
        let backend_listener = UnixListener::bind(backend_dir.path().join("backend.sock")).unwrap();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = backend_listener.accept() {
                drop(stream);
            }
        });

        let stop = start_proxy(
            listen_path.to_str().unwrap(),
            backend_dir.path().join("backend.sock").to_str().unwrap(),
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut client = UnixStream::connect(&listen_path).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_millis(200)))
            .unwrap();

        let request = "GET /v5/info HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.flush().unwrap();

        let mut response = vec![0u8; 4096];
        let n = client.read(&mut response).unwrap_or(0);
        assert!(n == 0, "Expected empty response, got {n} bytes");

        stop();
    }
}
