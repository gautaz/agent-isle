# `tools/podman/transport.rs` — raw socket HTTP transport

Low-level plumbing shared by the proxy's forwarding paths; no policy logic lives here.
Kept separate from `proxy.rs` so the mount-policy code stays under the 500-line per-file limit.

- `write_response` — sends a JSON `{"message": ...}` HTTP response (used for `403` rejections).
- `build_request_bytes` — rebuilds a request's wire bytes, normalising `Content-Length` / `Transfer-Encoding` framing while copying the original headers.
- `forward_request` — replays a parsed request onto the real Podman socket on a fresh connection.
- `relay_real_to_client` — pumps the real socket's response bytes to the client on a background thread until the real side closes.
