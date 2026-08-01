# `tools/podman/proxy.rs` — Unix socket proxy with mount policy enforcement

The core of the Podman tool.
Listens on a Unix socket, accepts client connections, parses each HTTP request, and inspects container `create` requests for bind mounts that violate the sandbox policy.
Create payloads are accepted in both wire formats Podman supports — the docker-compat `HostConfig` shape and the libpod specgen top-level `mounts` array (see `types.rs.md` for why both are needed).
A mount source is rejected when it is or contains a known secret, lies outside the sandbox's own host mounts, mounts a read-only sandbox tree read-write, or does not exist on the host.
Legitimate requests are forwarded to the real Podman socket; rejected requests receive a `403 Forbidden` response.
Podman's Go client keeps one connection alive across all its operations, so `stream_requests` validates *every* request on a connection — not just the first — instead of blind byte streaming.
Raw socket transport (`build_request_bytes`, `forward_request`, `write_response`) lives in `transport.rs`.
