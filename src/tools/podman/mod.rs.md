# `tools/podman/mod.rs` — Podman integration tool

Pluggable tool that provides a Unix socket proxy for Podman.
Intercepts container `create` API requests to block mounts that reference known secret paths.
Implements both the `Tool` trait (for lifecycle management) and `CapabilitySource` (to expose the proxy socket into the sandbox).
