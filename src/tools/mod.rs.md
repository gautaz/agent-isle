# `tools/mod.rs` — Pluggable tool infrastructure

Defines the `Tool` trait (with `id`, `capabilities`, `start` methods) and provides registration, validation, and listing of compiled-in tools.
The `podman` submodule is conditionally compiled when the `podman` feature is enabled.
