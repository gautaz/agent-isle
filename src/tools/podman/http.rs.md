# `tools/podman/http.rs` — Podman API route & path utilities

Utility functions for the Podman API proxy: extracting the API version from a request path, detecting container-create operations, and normalising absolute paths (resolving `..` / `.` segments).
Also provides a `PathClean` trait.
