use std::io;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};

use super::http::*;
use super::parse::*;
use super::secret_detection::*;
use super::transport::*;
use super::types::*;

/// Validate a single container mount source against the sandbox policy.
/// Returns a human-readable reason when the mount must be rejected.
///
/// The sandbox-membership check runs before secret detection so an
/// outside-sandbox path that merely happens to contain a secret file is
/// reported for the actual violation (not a confusing secret message).
fn check_mount_source(
    source: &str,
    read_only: bool,
    secrets: &[String],
    allowed: &[SandboxMount],
) -> Option<String> {
    let clean = canonical(source);
    match authorized_by_sandbox(&clean, allowed) {
        None => return Some(format!("{clean}: outside the sandbox mounts")),
        Some(true) if !read_only => return Some(format!("{clean}: sandbox mount is read-only")),
        _ => {}
    }
    if contains_secret(&clean, secrets) {
        return Some(format!("{clean}: is or contains a secret file"));
    }
    if !exists(source) {
        return Some(format!("{clean}: host path does not exist"));
    }
    None
}

fn validate_create_mounts(
    method: &str,
    path: &str,
    body: &[u8],
    secrets: &[String],
    allowed: &[SandboxMount],
) -> Option<String> {
    if !is_create_op(method, path) {
        return None;
    }
    let cfg = serde_json::from_slice::<CreateRequest>(body).ok()?;
    let mut violations: Vec<String> = Vec::new();
    if let Some(host) = &cfg.host_config {
        for bind in &host.binds {
            if let Some((source, read_only)) = parse_bind_spec(bind) {
                if let Some(reason) = check_mount_source(&source, read_only, secrets, allowed) {
                    violations.push(reason);
                }
            }
        }
        for mount in &host.mounts {
            if mount.mount_type == "bind" && !mount.source.is_empty() {
                let read_only = mount.read_only.unwrap_or(false);
                if let Some(reason) = check_mount_source(&mount.source, read_only, secrets, allowed)
                {
                    violations.push(reason);
                }
            }
        }
    }
    for mount in &cfg.mounts {
        if mount.mount_type == "bind" && !mount.source.is_empty() {
            let read_only = mount.options.iter().any(|o| o.eq_ignore_ascii_case("ro"));
            if let Some(reason) = check_mount_source(&mount.source, read_only, secrets, allowed) {
                violations.push(reason);
            }
        }
    }
    if violations.is_empty() {
        None
    } else {
        Some(violations.join(", "))
    }
}

fn check_mount_blocking(
    client_conn: &mut UnixStream,
    method: &str,
    path: &str,
    body: &[u8],
    secrets: &[String],
    allowed: &[SandboxMount],
) -> bool {
    if let Some(reasons) = validate_create_mounts(method, path, body, secrets, allowed) {
        tracing::warn!(reasons = %reasons, "proxy: blocked container create with unauthorized mounts");
        write_response(
            client_conn,
            403,
            &format!("container mounts violate sandbox policy: {reasons}"),
        );
        return true;
    }
    false
}

fn proxy_handle(
    mut client_conn: UnixStream,
    real_path: &str,
    secrets: &[String],
    allowed: &[SandboxMount],
) {
    // Initialise the real-side connection with the first request, so that a
    // validation error or connect failure is reported before any streaming.
    let (_buf, parsed, body) = match parse_request(&mut client_conn) {
        Ok(v) => v,
        Err(msg) => {
            tracing::debug!(error = msg, "proxy: request parse failed");
            write_response(&mut client_conn, 502, msg);
            return;
        }
    };

    if check_mount_blocking(
        &mut client_conn,
        &parsed.method,
        &parsed.path,
        &body,
        secrets,
        allowed,
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

    stream_requests(&mut client_conn, &mut real_conn, secrets, allowed);
}

/// Write a parsed request onto an existing connection to the real socket.
fn forward_on(real_conn: &mut UnixStream, parsed: &ParsedRequest, body: &[u8]) -> bool {
    let bytes = build_request_bytes(
        &parsed.method,
        &parsed.path,
        &parsed.raw_headers,
        parsed.content_length,
        parsed.is_chunked,
        body,
    );
    if real_conn.write_all(&bytes).is_err() {
        tracing::warn!("proxy: failed to forward request");
        false
    } else {
        true
    }
}

/// Relay the real-side response stream to the client, and validate every
/// subsequent request on the client connection — not just the first. Podman's
/// Go client keeps one connection alive across all its operations, so the
/// create request that matters is often not the first one on the wire.
fn stream_requests(
    client_conn: &mut UnixStream,
    real_conn: &mut UnixStream,
    secrets: &[String],
    allowed: &[SandboxMount],
) {
    let t2 = match relay_real_to_client(real_conn, client_conn) {
        Some(t) => t,
        None => return,
    };

    loop {
        let (_buf, parsed, body) = match parse_request(client_conn) {
            Ok(v) => v,
            Err(msg) => {
                tracing::debug!(error = msg, "proxy: keep-alive request parse failed");
                break;
            }
        };
        if check_mount_blocking(
            client_conn,
            &parsed.method,
            &parsed.path,
            &body,
            secrets,
            allowed,
        ) {
            break;
        }
        if !forward_on(real_conn, &parsed, &body) {
            break;
        }
    }

    let _ = real_conn.shutdown(Shutdown::Write);
    if t2.join().is_err() {
        tracing::warn!("proxy: real-to-client thread panicked");
    }
}

fn canonicalize_allowed(allowed_mounts: Vec<SandboxMount>) -> Arc<Vec<SandboxMount>> {
    Arc::new(
        allowed_mounts
            .into_iter()
            .map(|m| SandboxMount {
                host: canonical(&m.host),
                read_only: m.read_only,
            })
            .collect::<Vec<_>>(),
    )
}

/// Start a Unix socket proxy that intercepts container create requests
/// and rejects mounts that leak secrets or escape the sandbox surface.
pub fn start_proxy(
    listen_path: &str,
    real_path: &str,
    secrets: Vec<String>,
    allowed_mounts: Vec<SandboxMount>,
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
    let allowed = canonicalize_allowed(allowed_mounts);
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
                let allowed = Arc::clone(&allowed);
                let real_path = real_path.clone();
                thread::spawn(move || {
                    proxy_handle(stream, &real_path, &secrets, &allowed);
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
    fn test_validate_create_mounts_podman_casing() {
        let dir = tempfile::tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        let denied_dir = dir.path().join("denied");
        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&denied_dir).unwrap();
        let allowed_path = allowed_dir.to_string_lossy().into_owned();
        let denied_path = denied_dir.to_string_lossy().into_owned();

        let secrets: Vec<String> = vec![];
        let allowed = vec![SandboxMount {
            host: allowed_path.clone(),
            read_only: false,
        }];
        let podman_body = format!(
            r#"{{"HostConfig":{{"Binds":["{denied_path}:/mnt/denied"],"Mounts":[{{"Type":"bind","Source":"{allowed_path}/file.txt","Target":"/mnt/allowed"}}]}}}}"#
        );
        std::fs::write(allowed_dir.join("file.txt"), b"x").unwrap();

        let reason = validate_create_mounts(
            "POST",
            "/v5.4.0/libpod/containers/create",
            podman_body.as_bytes(),
            &secrets,
            &allowed,
        )
        .expect("mounts must be detected");
        assert!(
            reason.contains(&format!("{denied_path}: outside the sandbox mounts")),
            "unexpected reason: {reason}"
        );
    }

    #[test]
    fn test_validate_create_mounts_allows_authorized_bind() {
        let dir = tempfile::tempdir().unwrap();
        let allowed_dir = dir.path().join("project");
        std::fs::create_dir_all(&allowed_dir).unwrap();
        let allowed_path = allowed_dir.to_string_lossy().into_owned();

        let secrets: Vec<String> = vec![];
        let allowed = vec![SandboxMount {
            host: allowed_path.clone(),
            read_only: false,
        }];
        let body = format!(r#"{{"HostConfig":{{"Binds":["{allowed_path}:/mnt/project"]}}}}"#);
        assert_eq!(
            validate_create_mounts(
                "POST",
                "/containers/create",
                body.as_bytes(),
                &secrets,
                &allowed
            ),
            None
        );
    }

    #[test]
    fn test_validate_create_mounts_libpod_specgen() {
        let dir = tempfile::tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        let denied_dir = dir.path().join("denied");
        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&denied_dir).unwrap();
        let allowed_path = allowed_dir.to_string_lossy().into_owned();
        let denied_path = denied_dir.to_string_lossy().into_owned();

        let secrets: Vec<String> = vec![];
        let allowed = vec![SandboxMount {
            host: allowed_path.clone(),
            read_only: false,
        }];
        // Real podman CLI specgen body: mounts live in the top-level "mounts"
        // array with lowercase field names, not in HostConfig.
        let body = format!(
            r#"{{"name":"capx","image":"alpine","mounts":[{{"destination":"/mnt/denied","type":"bind","source":"{denied_path}"}},{{"destination":"/mnt/allowed","type":"bind","source":"{allowed_path}"}}]}}"#
        );

        let reason = validate_create_mounts(
            "POST",
            "/v5.8.4/libpod/containers/create",
            body.as_bytes(),
            &secrets,
            &allowed,
        )
        .expect("specgen mounts must be detected");
        assert!(
            reason.contains(&format!("{denied_path}: outside the sandbox mounts")),
            "unexpected reason: {reason}"
        );
        assert!(
            !reason.contains(&allowed_path),
            "authorized mount must not be flagged: {reason}"
        );
    }

    #[test]
    fn test_validate_create_mounts_specgen_read_only_options() {
        let dir = tempfile::tempdir().unwrap();
        let ro_dir = dir.path().join("ro");
        std::fs::create_dir_all(&ro_dir).unwrap();
        let ro_path = ro_dir.to_string_lossy().into_owned();

        let secrets: Vec<String> = vec![];
        let allowed = vec![SandboxMount {
            host: ro_path.clone(),
            read_only: true,
        }];
        // A ro bind of a read-only sandbox mount must pass; an rw bind must not.
        let ro_body = format!(
            r#"{{"image":"alpine","mounts":[{{"destination":"/mnt/x","type":"bind","source":"{ro_path}","options":["ro"]}}]}}"#
        );
        assert_eq!(
            validate_create_mounts(
                "POST",
                "/v5.8.4/libpod/containers/create",
                ro_body.as_bytes(),
                &secrets,
                &allowed
            ),
            None
        );

        let rw_body = format!(
            r#"{{"image":"alpine","mounts":[{{"destination":"/mnt/x","type":"bind","source":"{ro_path}"}}]}}"#
        );
        let reason = validate_create_mounts(
            "POST",
            "/v5.8.4/libpod/containers/create",
            rw_body.as_bytes(),
            &secrets,
            &allowed,
        )
        .expect("rw bind of a ro sandbox mount must be rejected");
        assert!(
            reason.contains(&format!("{ro_path}: sandbox mount is read-only")),
            "unexpected reason: {reason}"
        );
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
