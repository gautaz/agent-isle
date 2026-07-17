# `sandbox/mod.rs` — Sandbox types & bwrap argument builder

Defines the core sandbox types:

- `MountMode` — read-only (`Ro`) or read-write (`Rw`)
- `SecretsPolicy` — `Mask` or `Show`
- `Mount` — a bind mount with host path, target path, mode, and secrets policy

The `build_args()` function constructs the full `bwrap` CLI argument vector (proc, dev, tmpfs, chdir, bind mounts, environment variables).
The `build_minimal_args()` variant is used for lightweight (no-sandbox) mode.
