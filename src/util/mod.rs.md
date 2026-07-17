# `util/mod.rs` — Miscellaneous utilities

General-purpose helpers used across the application:

- `detect_symlink_mode` — agent name discovery from `argv[0]`
- `home_dir` / `username` — `$HOME` / `$USER` readers with diagnostics
- `xdg_runtime_dir` / `xdg_state_home` / `xdg_config_home` — XDG directory helpers with sensible fallbacks
- `cleanup_stale_dirs` — removes rundirs of dead processes
- `sync_and_close` — durable file persistence
- `validate_socket_ownership` — Unix socket security check
