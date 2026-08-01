# `tools/mod.rs` — Pluggable tool infrastructure

Defines the `Tool` trait (`id`, `capabilities`, `start`) and `ToolStartContext` (which carries the detected secret file paths and the sandbox mount list into each tool's `start` call), plus registration, validation, and listing of compiled-in tools.
The `podman` submodule is conditionally compiled when the `podman` feature is enabled.
