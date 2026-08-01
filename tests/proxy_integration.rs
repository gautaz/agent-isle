#![cfg(feature = "podman")]

mod support;

use std::io::{Read, Write};

use agent_isle::tools::podman::proxy::start_proxy;
use agent_isle::tools::podman::types::SandboxMount;
use support::{mock_keepalive_backend, mock_podman_backend, read_response};

#[test]
fn test_start_proxy_rejects_secret_mount() {
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec!["/home/user/.ssh/id_rsa".to_string()],
        vec![],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"HostConfig":{"Binds":["/home/user/.ssh:/mnt/ssh"]}}"#;

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
        vec![],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"HostConfig":{"Mounts":[{"Type":"bind","Source":"/home/user/.ssh/id_rsa","Target":"/mnt/ssh"}]}}"#;

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
fn test_proxy_rejects_mount_outside_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"HostConfig":{"Binds":["/etc/passwd:/mnt/passwd"]}}"#;
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
    assert!(
        response.contains("outside the sandbox mounts"),
        "Expected outside-sandbox reason: {response}"
    );

    stop();
}

#[test]
fn test_proxy_allows_mount_within_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let source = allowed_dir.path().to_string_lossy();
    let body = format!(r#"{{"HostConfig":{{"Binds":["{source}:/mnt/data"]}}}}"#);
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

    let response = read_response(&mut client);
    assert!(response.contains("200"), "Expected 200: {response}");

    let backend_data = captured.lock().unwrap();
    let backend_str = String::from_utf8_lossy(&backend_data);
    assert!(
        backend_str.contains("containers/create"),
        "Backend should receive request: {backend_str}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_rw_mount_on_ro_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: true,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let source = allowed_dir.path().to_string_lossy();
    let body = format!(r#"{{"HostConfig":{{"Binds":["{source}:/mnt/data"]}}}}"#);
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
    assert!(
        response.contains("read-only"),
        "Expected read-only reason: {response}"
    );

    stop();
}

#[test]
fn test_proxy_allows_ro_mount_on_ro_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: true,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let source = allowed_dir.path().to_string_lossy();
    let body = format!(r#"{{"HostConfig":{{"Binds":["{source}:/mnt/data:ro"]}}}}"#);
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

    let response = read_response(&mut client);
    assert!(response.contains("200"), "Expected 200: {response}");

    let backend_data = captured.lock().unwrap();
    let backend_str = String::from_utf8_lossy(&backend_data);
    assert!(
        backend_str.contains("containers/create"),
        "Backend should receive request: {backend_str}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_nonexistent_mount_source() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let missing = allowed_dir.path().join("does-not-exist");
    let body = format!(
        r#"{{"HostConfig":{{"Binds":["{}:/mnt/data"]}}}}"#,
        missing.display()
    );
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
    assert!(
        response.contains("does not exist"),
        "Expected does-not-exist reason: {response}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_symlink_escape() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let link = allowed_dir.path().join("link");
    std::os::unix::fs::symlink(outside_dir.path(), &link).unwrap();

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = format!(
        r#"{{"HostConfig":{{"Binds":["{}:/mnt/link"]}}}}"#,
        link.display()
    );
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
fn test_proxy_rejects_specgen_secret_mount() {
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec!["/home/user/.ssh/id_rsa".to_string()],
        vec![],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body =
        r#"{"mounts":[{"destination":"/mnt/ssh","type":"bind","source":"/home/user/.ssh"}]}"#;

    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
fn test_proxy_outside_sandbox_with_secret_reports_sandbox_reason() {
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let outside = tempfile::tempdir().unwrap();
    let secret_file = outside.path().join("secret.env");
    std::fs::write(&secret_file, "TOKEN=secret").unwrap();

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![secret_file.to_string_lossy().into_owned()],
        vec![],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = format!(
        r#"{{"HostConfig":{{"Binds":["{}:/mnt/x"]}}}}"#,
        outside.path().display()
    );
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
    assert!(
        response.contains("outside the sandbox mounts"),
        "Expected sandbox reason: {response}"
    );
    assert!(
        !response.contains("secret file"),
        "Secret reason must not take precedence: {response}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_specgen_secret_mount_file() {
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec!["/home/user/.ssh/id_rsa".to_string()],
        vec![],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"mounts":[{"destination":"/mnt/ssh","type":"bind","source":"/home/user/.ssh/id_rsa"}]}"#;

    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
fn test_proxy_rejects_specgen_mount_outside_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = r#"{"mounts":[{"destination":"/mnt/passwd","type":"bind","source":"/etc/passwd"}]}"#;
    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
    assert!(
        response.contains("outside the sandbox mounts"),
        "Expected outside-sandbox reason: {response}"
    );

    stop();
}

#[test]
fn test_proxy_allows_specgen_mount_within_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let source = allowed_dir.path().to_string_lossy();
    let body = format!(
        r#"{{"mounts":[{{"destination":"/mnt/data","type":"bind","source":"{source}"}}]}}"#
    );
    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
        backend_str.contains("containers/create"),
        "Backend should receive request: {backend_str}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_specgen_rw_mount_on_ro_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: true,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let source = allowed_dir.path().to_string_lossy();
    let body = format!(
        r#"{{"mounts":[{{"destination":"/mnt/data","type":"bind","source":"{source}"}}]}}"#
    );
    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
    assert!(
        response.contains("read-only"),
        "Expected read-only reason: {response}"
    );

    stop();
}

#[test]
fn test_proxy_allows_specgen_ro_mount_on_ro_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let (_backend_dir, captured) = mock_podman_backend(real_path.to_str().unwrap());

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: true,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let source = allowed_dir.path().to_string_lossy();
    let body = format!(
        r#"{{"mounts":[{{"destination":"/mnt/data","type":"bind","source":"{source}","options":["ro"]}}]}}"#
    );
    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
        backend_str.contains("containers/create"),
        "Backend should receive request: {backend_str}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_specgen_nonexistent_mount_source() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let missing = allowed_dir.path().join("does-not-exist");
    let body = format!(
        r#"{{"mounts":[{{"destination":"/mnt/data","type":"bind","source":"{}"}}]}}"#,
        missing.display()
    );
    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
    assert!(
        response.contains("does not exist"),
        "Expected does-not-exist reason: {response}"
    );

    stop();
}

#[test]
fn test_proxy_rejects_specgen_symlink_escape() {
    let dir = tempfile::tempdir().unwrap();
    let allowed_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let link = allowed_dir.path().join("link");
    std::os::unix::fs::symlink(outside_dir.path(), &link).unwrap();

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.path().to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut client = std::os::unix::net::UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let body = format!(
        r#"{{"mounts":[{{"destination":"/mnt/link","type":"bind","source":"{}"}}]}}"#,
        link.display()
    );
    let request = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
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
        vec![],
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
#[test]
fn test_proxy_validates_second_request_on_keepalive_connection() {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");

    let allowed_dir = dir.path().join("allowed");
    std::fs::create_dir_all(&allowed_dir).unwrap();

    let (_backend_dir, _) = mock_keepalive_backend(real_path.to_str().unwrap(), vec![200]);

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_dir.to_string_lossy().into_owned(),
            read_only: false,
        }],
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let mut client = UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    // First request on the connection: benign ping, must be forwarded.
    client
        .write_all(b"GET /v5.8.4/libpod/_ping HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    client.flush().unwrap();
    assert!(read_response(&mut client).contains("200"), "ping forwarded");

    // Second request on the SAME connection: unauthorized bind, must be blocked.
    let body = r#"{"mounts":[{"source":"/home/user/private","type":"bind","destination":"/mnt/private","options":["rw"]}]}"#;
    let create = format!(
        "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body,
    );
    client.write_all(create.as_bytes()).unwrap();
    client.flush().unwrap();
    assert!(
        read_response(&mut client).contains("403"),
        "second request blocked"
    );

    stop();
}

#[test]
fn test_proxy_forwards_second_request_on_keepalive_connection() {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");
    let real_path = dir.path().join("real.sock");
    let allowed_dir = dir.path().join("allowed");
    std::fs::create_dir_all(&allowed_dir).unwrap();
    let allowed_path = allowed_dir.to_string_lossy().into_owned();
    let (_backend_dir, captured) =
        mock_keepalive_backend(real_path.to_str().unwrap(), vec![200, 201]);
    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        real_path.to_str().unwrap(),
        vec![],
        vec![SandboxMount {
            host: allowed_path.clone(),
            read_only: false,
        }],
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let mut client = UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
        .write_all(b"GET /v5.8.4/libpod/_ping HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    client.flush().unwrap();
    assert!(read_response(&mut client).contains("200"), "ping forwarded");
    let body = format!(
        r#"{{"mounts":[{{"source":"{allowed_path}","type":"bind","destination":"/mnt/x","options":["rw"]}}]}}"#
    );
    let head = "POST /v5.8.4/libpod/containers/create HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: ";
    let create = format!("{head}{}\r\n\r\n{}", body.len(), body);
    client.write_all(create.as_bytes()).unwrap();
    client.flush().unwrap();
    assert!(
        read_response(&mut client).contains("201"),
        "second request forwarded"
    );
    let backend_data = captured.lock().unwrap();
    let backend_str = String::from_utf8_lossy(&backend_data);
    assert!(
        backend_str.contains("containers/create"),
        "backend received second request"
    );
    assert!(backend_str.contains(&body), "backend received create body");
    stop();
}
