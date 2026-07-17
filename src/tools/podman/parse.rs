use std::io::Read;
use std::os::unix::net::UnixStream;

pub(super) const MAX_HEADER_SIZE: usize = 8192;

pub(super) struct ParsedRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) nread: usize,
    pub(super) body_start: usize,
    pub(super) content_length: usize,
    pub(super) is_chunked: bool,
    pub(super) raw_headers: Vec<(String, Vec<u8>)>,
}

pub(super) fn read_headers(conn: &mut UnixStream) -> Result<(Vec<u8>, usize), &'static str> {
    let mut buf = vec![0u8; MAX_HEADER_SIZE];
    let mut nread = 0;
    loop {
        match buf.get_mut(nread..).and_then(|slice| conn.read(slice).ok()) {
            Some(0) => return Err("connection closed before headers"),
            Some(n) => nread += n,
            None => return Err("failed to read request"),
        }
        if nread >= MAX_HEADER_SIZE {
            return Err("headers too large");
        }
        if buf
            .get(..nread)
            .is_some_and(|slice| slice.windows(4).any(|w| w == b"\r\n\r\n"))
        {
            break;
        }
    }
    Ok((buf, nread))
}

pub(super) fn parse_headers(buf: &[u8], nread: usize) -> Result<ParsedRequest, &'static str> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let header_len = match buf
        .get(..nread)
        .map_or(Err(httparse::Error::Status), |slice| req.parse(slice))
    {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => return Err("incomplete headers"),
        Err(_) => return Err("malformed request"),
    };
    let method = req.method.ok_or("missing method")?.to_string();
    let path = req.path.ok_or("missing path")?.to_string();
    let mut content_length = 0usize;
    let mut is_chunked = false;
    let mut raw_headers = Vec::new();
    for h in req.headers.iter() {
        let name = h.name.to_string();
        raw_headers.push((name.clone(), h.value.to_vec()));
        if h.name.eq_ignore_ascii_case("Content-Length") {
            content_length = std::str::from_utf8(h.value)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        }
        if h.name.eq_ignore_ascii_case("Transfer-Encoding")
            && std::str::from_utf8(h.value)
                .unwrap_or("")
                .eq_ignore_ascii_case("chunked")
        {
            is_chunked = true;
        }
    }
    Ok(ParsedRequest {
        method,
        path,
        nread,
        body_start: header_len,
        content_length,
        is_chunked,
        raw_headers,
    })
}

pub(super) fn read_and_parse_headers(
    conn: &mut UnixStream,
) -> Result<(Vec<u8>, ParsedRequest), &'static str> {
    let (buf, nread) = read_headers(conn)?;
    let parsed = parse_headers(&buf, nread)?;
    Ok((buf, parsed))
}

pub(super) fn read_body(
    conn: &mut UnixStream,
    buf: &[u8],
    nread: usize,
    body_start: usize,
    content_length: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        let body_already = nread.saturating_sub(body_start);
        let to_copy = body_already.min(content_length);
        if let Some(src) = buf.get(body_start..body_start + to_copy) {
            if let Some(dst) = body.get_mut(..to_copy) {
                dst.clone_from_slice(src);
            }
        }
        if to_copy < content_length
            && body
                .get_mut(to_copy..)
                .and_then(|slice| conn.read_exact(slice).ok())
                .is_none()
        {
            return Err("incomplete body");
        }
    }
    Ok(body)
}

pub(super) fn parse_request(
    client_conn: &mut UnixStream,
) -> Result<(Vec<u8>, ParsedRequest, Vec<u8>), &'static str> {
    let (buf, parsed) = read_and_parse_headers(client_conn)?;
    let body = read_body(
        client_conn,
        &buf,
        parsed.nread,
        parsed.body_start,
        parsed.content_length,
    )?;
    tracing::debug!(method = %parsed.method, path = %parsed.path, "proxy: request");
    Ok((buf, parsed, body))
}
