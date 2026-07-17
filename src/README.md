# `src` — Rust source tree

This directory contains all Rust sources for the agent-isle sandbox runner.
The application builds a bubblewrap-based sandbox, configurable via YAML, that runs AI agents in an isolated environment with secret-file detection and optional Podman integration.

## Architecture overview

```
main.rs  ──►  load_config ──► config/  ──► (validation)
  │                                        (merge)
  │                                        (templates)
  ▼
 run/  ──►  setup ─► collect sources ─► scan secrets ─► start tools ─► launch
  │         │           │                    │               │
  │         │           ▼                    ▼               ▼
  │         │     capability_sources    secrets/         tools/
  │         │     platform/                              tools/podman/
  │         │     user_profile/
  │         ▼
  │     sandbox/ (bwrap args)
  │
  └── lightweight mode: run_cmd_bare (no sandbox)
```

## Modules

  | Module                                    | Concern                                                               |
  | ----------------------------------------- | --------------------------------------------------------------------- |
  | [`config/`](config/README.md)             | Configuration types, loading, merging, validation, templates, presets |
  | [`platform/`](platform/README.md)         | OS detection and platform-specific sandbox mounts                     |
  | [`run/`](run/README.md)                   | Sandbox runtime lifecycle orchestration                               |
  | [`sandbox/`](sandbox/README.md)           | Mount types and bubblewrap argument generation                        |
  | [`secrets/`](secrets/README.md)           | Secret file detection via betterleaks                                 |
  | [`tools/`](tools/README.md)               | Pluggable tool infrastructure and Podman integration                  |
  | [`user_profile/`](user_profile/README.md) | User environment, PATH mounts, cache isolation                        |
  | [`util/`](util/README.md)                 | Shared utilities (XDG, sockets, filesystem)                           |

## Root-level files

  | File                                                   | Concern                                           |
  | ------------------------------------------------------ | ------------------------------------------------- |
  | [`lib.rs.md`](lib.rs.md)                               | Library crate root, public re-exports             |
  | [`main.rs.md`](main.rs.md)                             | CLI binary entry point                            |
  | [`capability_sources.rs.md`](capability_sources.rs.md) | `CapabilitySource` trait and collection utilities |
  | [`load_config.rs.md`](load_config.rs.md)               | Config loading and initial merge pipeline         |
  | [`logging.rs.md`](logging.rs.md)                       | Tracing / log initialisation                      |
