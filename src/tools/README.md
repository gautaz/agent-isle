# `tools` — Pluggable tools

Concerns managed in this module:

- **`Tool` trait** — common interface for tools that run alongside the agent
- **Tool registration & validation** — discover and verify compiled-in tools
- **Podman proxy** — sandbox-aware Podman integration (feature-gated)

## Files

  | File                     | Concern                              |
  | ------------------------ | ------------------------------------ |
  | [`mod.rs.md`](mod.rs.md) | Tool trait, registration, validation |

## Submodules

  | Module                        | Concern                                           |
  | ----------------------------- | ------------------------------------------------- |
  | [`podman/`](podman/README.md) | Podman socket proxy with mount policy enforcement |
