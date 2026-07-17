# `config` — Configuration

Concerns managed in this module:

- **Configuration types** — `Config`, `AgentConfig`, `MountConfig`, `SandboxConfig`, `EnvValue`, `TemplateVars`
- **YAML serialisation / deserialisation** — load user-provided config files
- **Deep merge** — combine defaults, file config, and CLI overrides
- **Template variable expansion** — `{home}`, `{user}`, `{cwd}`, etc.
- **Bundled presets** — pre-defined agent configurations
- **Validation** — path absoluteness, agent name format, binary existence
- **CapabilitySource wrappers** — expose config data as sandbox capability sources

## Files

  | File                               | Concern                                 |
  | ---------------------------------- | --------------------------------------- |
  | [`mod.rs.md`](mod.rs.md)           | Core types, serde, top-level re-exports |
  | [`merge.rs.md`](merge.rs.md)       | Deep-merge two `Config` values          |
  | [`presets.rs.md`](presets.rs.md)   | Bundled agent presets (e.g. opencode)   |
  | [`sources.rs.md`](sources.rs.md)   | `CapabilitySource` for config and agent |
  | [`template.rs.md`](template.rs.md) | `{variable}` expansion in strings       |
  | [`validate.rs.md`](validate.rs.md) | Config validation rules                 |
