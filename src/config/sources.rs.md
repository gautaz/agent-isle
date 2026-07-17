# `config/sources.rs` — CapabilitySource wrappers for config data

Implements `CapabilitySource` for:

- `ConfigSource` — wraps the top-level `SandboxConfig` (global mounts & env)
- `AgentSource` — wraps the per-agent `AgentConfig` (agent-specific mounts & env)

These are used by the runtime to collect all mounts and environment variables that originate from the configuration layer.
