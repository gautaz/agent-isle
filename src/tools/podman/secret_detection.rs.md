# `tools/podman/secret_detection.rs` — Secret path checks in Podman mounts

Checks bind-mount and volume-mount sources inside Podman `create` requests against a known list of secret file paths.
Paths are normalised before comparison.
Returns any mounts that should be blocked.
