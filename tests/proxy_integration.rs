#![cfg(feature = "podman")]

mod support;

use std::io::{Read, Write};

use agent_isle::tools::podman::proxy::start_proxy;
use support::{mock_podman_backend, read_response};

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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"HostConfig":{"binds":["/home/user/.ssh:/mnt/ssh"]}}"#;

    let request = format!(
        "POST /v5/containers/create HTTP/1.1\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );

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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"HostConfig":{"mounts":[{"Type":"bind","Source":"/home/user/.ssh/id_rsa","Target":"/mnt/ssh"}]}}"#;

    let request = format!(
        "POST /v5/containers/create HTTP/1.1\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );

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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"Image":"alpine"}"#;
    let request = format!(
        "POST /v5/containers/create HTTP/1.1\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"Signal":"SIGTERM"}"#;
    let request = format!(
        "POST /v5/containers/test/kill HTTP/1.1\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"AttachStdin":false}"#;
    let request = format!(
        "POST /v5/containers/create HTTP/1.1\r\n\
         Content-Type: application/json\r\n\
         Transfer-Encoding: chunked\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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
        backend_str.contains("Host: localhost"),
        "Host header should be forwarded: {backend_str}"
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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
    let backend_listener =
        std::os::unix::net::UnixListener::bind(backend_dir.path().join("backend.sock")).unwrap();
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

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
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
