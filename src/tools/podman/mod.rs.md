# `tools/podman/mod.rs` — Podman integration tool

Pluggable tool that provides a Unix socket proxy for Podman.
Intercepts container `create` API requests and enforces the sandbox mount policy: no secret-leaking mounts, no mounts outside the sandbox's own host mounts, no read-write mounts of read-only sandbox trees, and no non-existent sources.
During `start`, the proxy receives the sandbox mount list via `ToolStartContext` and builds its allowlist.
Implements both the `Tool` trait (for lifecycle management) and `CapabilitySource` (to expose the proxy socket into the sandbox).
Mount policy lives in `proxy.rs`; raw socket HTTP transport in `transport.rs`.
