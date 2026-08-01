# `tools::podman` — Podman integration

Concerns managed in this module:

- **Unix socket proxy** that intercepts Podman container `create` requests
- **Mount policy enforcement** — rejects mounts that leak secrets, escape the sandbox, override read-only trees, or do not exist on the host
- **HTTP request parsing** over Unix sockets
- **Podman API type deserialisation** for container creation payloads, in both supported wire formats (docker-compat `HostConfig` and libpod specgen `mounts`)

## Files

  | File                                               | Concern                                                   |
  | -------------------------------------------------- | --------------------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md)                           | Tool lifecycle, capability source, socket setup           |
  | [`proxy.rs.md`](proxy.rs.md)                       | Mount policy enforcement, request orchestration           |
  | [`transport.rs.md`](transport.rs.md)               | Raw socket HTTP forwarding and response writing           |
  | [`parse.rs.md`](parse.rs.md)                       | HTTP-over-Unix-socket request parser                      |
  | [`secret_detection.rs.md`](secret_detection.rs.md) | Sandbox mount policy: secrets, allowlist, read-only flags |
  | [`http.rs.md`](http.rs.md)                         | Podman API route and path utilities                       |
  | [`types.rs.md`](types.rs.md)                       | Serde types for Podman container create API               |
