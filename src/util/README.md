# `util` — Shared utilities

Concerns managed in this module:

- **Symlink agent detection** — derive agent name from `argv[0]`
- **Home directory & username** — read `$HOME` / `$USER` with diagnostics
- **XDG directory helpers** — `xdg_runtime_dir`, `xdg_state_home`, `xdg_config_home` with fallbacks
- **Stale directory cleanup** — remove rundirs of dead processes
- **File sync** — durable write to disk
- **Socket ownership validation** — ensure a Unix socket belongs to the user or root

## Files

  | File                     | Concern               |
  | ------------------------ | --------------------- |
  | [`mod.rs.md`](mod.rs.md) | All utility functions |
