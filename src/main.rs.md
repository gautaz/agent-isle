# `main.rs` — Binary entry point

Parses CLI arguments via `clap`, loads and validates the configuration, detects the host OS, checks for lightweight mode (`--help` / `--version`), and delegates to `run::run` (full sandbox) or `run::run_cmd_bare` (lightweight).
