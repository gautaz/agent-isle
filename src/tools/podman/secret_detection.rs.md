# `tools/podman/secret_detection.rs` — Sandbox mount policy helpers

Contains the policy logic used by the proxy to validate host bind mounts in Podman `create` requests.
Sources are canonicalised (`realpath`) before any comparison so symlinks and `..` segments cannot bypass the checks.
Provides `contains_secret` (source equals or contains a known secret file), `exists` (host existence check), `parse_bind_spec` (mount option parsing, `None` for named volumes), and `authorized_by_sandbox` (source must be a sandbox mount or a descendant of one, with read-only inheritance).
