# `tools/podman/proxy.rs` — Unix socket proxy with secret blocking

The core of the Podman tool.
Listens on a Unix socket, accepts client connections, parses each HTTP request, and inspects container `create` requests for bind/volume mounts referencing secret paths.
Legitimate requests are forwarded to the real Podman socket; requests with secret-path mounts receive a `403 Forbidden` response.
Manages bidirectional streaming for all other API calls.
