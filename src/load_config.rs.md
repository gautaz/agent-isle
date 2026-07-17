# `load_config.rs` — Config loading & initial merge pipeline

Orchestrates the construction of the final `Config` by merging: 1.
Hard-coded defaults 2.
An optional YAML file (from `$XDG_CONFIG_HOME/agent-isle/config.yml` or an explicit path passed on the CLI) 3.
CLI overrides (agent name, flags)

Returns a fully resolved `Config` ready for validation.
