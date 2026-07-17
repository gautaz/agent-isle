# `sandbox` — Sandbox types & bubblewrap argument construction

Concerns managed in this module:

- **Mount types** — `MountMode` (ro/rw), `SecretsPolicy` (mask/show), `Mount`
- **bwrap argument generation** — `build_args()` for full sandbox, `build_minimal_args()` for lightweight mode
- **Argument deduplication** — skips nonexistent host paths

## Files

  | File                     | Concern                                           |
  | ------------------------ | ------------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md) | Sandbox types, `build_args`, `build_minimal_args` |
