# `config/template.rs` — Template variable expansion

Expands `{variable}` placeholders inside configuration strings.
Supported variables: `home`, `user`, `cwd`, `xdg_runtime`, `xdg_state`, `log_path`.

Applied to plain strings, environment values, and mount paths (both at the global and per-agent level).
