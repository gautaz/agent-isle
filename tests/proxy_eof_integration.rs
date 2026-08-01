#![cfg(feature = "podman")]

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

use agent_isle::tools::podman::proxy::start_proxy;

const CREATE_REQUEST: &[u8] = b"POST /v5/containers/create HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";

fn recv_headers(conn: &mut UnixStream) {
    let mut buf = vec![0u8; 8192];
    let mut n = 0;
    loop {
        let r = conn.read(&mut buf[n..]).unwrap();
        if r == 0 {
            return;
        }
        n += r;
        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
            return;
        }
    }
}

fn recv_until(client: &mut UnixStream, out: &mut Vec<u8>, needle: &[u8], msg: &str) {
    let start = Instant::now();
    let mut buf = [0u8; 4096];
    while !out.windows(needle.len()).any(|w| w == needle) {
        match client.read(&mut buf) {
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("client read error: {e}"),
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{msg}; got {} bytes",
            out.len()
        );
    }
}

fn recv_until_eof(client: &mut UnixStream, out: &mut Vec<u8>, msg: &str) {
    let start = Instant::now();
    let mut buf = [0u8; 4096];
    loop {
        match client.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("client read error: {e}"),
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{msg}; got {} bytes",
            out.len()
        );
    }
}

#[test]
fn test_streaming_response_propagates_eof() {
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");

    let backend_dir = tempfile::tempdir().unwrap();
    let backend_path = backend_dir.path().join("backend.sock");
    let backend_listener = UnixListener::bind(&backend_path).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = backend_listener.accept().unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\n\r\nstreamed payload")
            .unwrap();
        stream.flush().unwrap();
        drop(stream);
    });

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        backend_path.to_str().unwrap(),
        vec![],
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(50));

    let mut client = UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client.write_all(CREATE_REQUEST).unwrap();
    client.flush().unwrap();

    let mut out = Vec::new();
    recv_until_eof(&mut client, &mut out, "client never received EOF");

    assert!(String::from_utf8_lossy(&out).contains("streamed payload"));
    drop(client);
    stop();
}

#[test]
fn test_keepalive_connection_allows_followup_request() {
    let dir = tempfile::tempdir().unwrap();
    let listen_path = dir.path().join("proxy.sock");

    let backend_dir = tempfile::tempdir().unwrap();
    let backend_path = backend_dir.path().join("backend.sock");
    let backend_listener = UnixListener::bind(&backend_path).unwrap();
    std::thread::spawn(move || {
        let (mut conn, _) = backend_listener.accept().unwrap();
        recv_headers(&mut conn);
        conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst")
            .unwrap();
        conn.flush().unwrap();
        recv_headers(&mut conn);
        conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecond")
            .unwrap();
        conn.flush().unwrap();
    });

    let stop = start_proxy(
        listen_path.to_str().unwrap(),
        backend_path.to_str().unwrap(),
        vec![],
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(50));

    let mut client = UnixStream::connect(&listen_path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut out = Vec::new();
    client.write_all(CREATE_REQUEST).unwrap();
    client.flush().unwrap();
    recv_until(
        &mut client,
        &mut out,
        b"\r\n\r\nfirst",
        "waiting for first response",
    );

    client.write_all(CREATE_REQUEST).unwrap();
    client.flush().unwrap();
    recv_until_eof(&mut client, &mut out, "waiting for EOF");

    assert!(
        out.windows(b"second".len()).any(|w| w == b"second"),
        "must receive second response on same connection"
    );
    drop(client);
    stop();
}
