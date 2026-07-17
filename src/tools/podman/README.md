# `tools::podman` — Podman integration

Concerns managed in this module:

- **Unix socket proxy** that intercepts Podman container `create` requests
- **Secret-path blocking** — prevents sandboxed agents from mounting secret files inside containers
- **HTTP request parsing** over Unix sockets
- **Podman API type deserialisation** for container creation payloads

## Files

  | File                                               | Concern                                            |
  | -------------------------------------------------- | -------------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md)                           | Tool lifecycle, capability source, socket setup    |
  | [`proxy.rs.md`](proxy.rs.md)                       | Core proxy: forwarding, secret blocking, streaming |
  | [`parse.rs.md`](parse.rs.md)                       | HTTP-over-Unix-socket request parser               |
  | [`secret_detection.rs.md`](secret_detection.rs.md) | Checking bind/volume mounts against secret paths   |
  | [`http.rs.md`](http.rs.md)                         | Podman API route and path utilities                |
  | [`types.rs.md`](types.rs.md)                       | Serde types for Podman container create API        |
