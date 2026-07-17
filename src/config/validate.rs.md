# `config/validate.rs` — Configuration validation

Validates the fully resolved config: checks that paths are absolute, agent names are well-formed (alphanumeric + `-` / `_`), and referenced binaries exist.
Also contains unit tests for `AgentSource` and `ConfigSource`.
