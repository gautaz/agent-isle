# agent-isle agent reference

## Scope

- Bubblewrap sandbox (filesystem isolation)
- Betterleaks (secret detection, masks with /dev/null)
- Podman proxy (blocks secret-leaking mounts)
- Agent presets (opencode, or custom via config)

## Usage

``` bash
agent-isle [flags] [-- <args forwarded to agent>]
```

### Examples

``` bash
# Run a specific agent
agent-isle --agent opencode -- --help

# Run a specific agent
agent-isle --agent aider -- --version

# Dry run (print bwrap args without executing)
agent-isle --agent opencode --dry-run -- --help
```

### Flags

| Flag        | Short | Description                        | Default                                               |
|-------------|-------|------------------------------------|-------------------------------------------------------|
| `--agent`   | `-a`  | Agent name (selects preset)        | —                                                     |
| `--config`  | `-c`  | Config file path                   | `${XDG_CONFIG_HOME:-~/.config}/agent-isle/config.yml` |
| `--dry-run` | —     | Print bwrap args without executing | `false`                                               |

When invoked as `agent-isle`, `--agent` is required (unless set in config or via symlink).

Arguments after `--` are forwarded to the agent.

### Agent Selection

Agent selection is resolved in this order:

1.  **Symlink** — executable name is used as agent name
2.  **`--agent` flag** — explicit CLI selection (only when invoked as `agent-isle`)
3.  **Config file** — `agent:` field in config.yml

If no agent is resolved, agent-isle exits with an error.

#### Bundled presets

- **opencode** — [opencode](https://github.com/opencode-ai/opencode) AI coding assistant

### Logs

Logs are written to:

    ${XDG_STATE_HOME:-~/.local/state}/agent-isle/logs/<timestamp>_<pid>.log

The log path is available in config via the `{log_path}` template variable.

### License

See [LICENSE](LICENSE) for details.

### Symlinks

``` bash
ln -s agent-isle opencode
./opencode --help  # no -- needed
```

Executable name selects the agent.
When using symlinks, all arguments go directly to the agent (no flag parsing).

## Configuration

### Config struct

See `example-config.yml` for a documented config reference.

``` yaml
agent: opencode
ro_mounts: []                   # appended to all agent presets
rw_mounts: []                   # appended to all agent presets
env:                            # merged with all agent presets (per-key overwrite)
  STATIC_VAR: "value"
  SECRET_VAR:
    command: "pass show secret/path"  # example using pass
agents:
  opencode:
    binary: "opencode"
    ro_mounts: []               # appended to preset mounts
    rw_mounts: []               # appended to preset mounts
    env: {}                     # merged with preset env (per-key overwrite)
    lightweight_args:           # mandatory, empty = no lightweight mode
      - "--help"
      - "-h"
      - "--version"
      - "-v"
tools:
  podman:
    enabled: true   # nil = auto-detect
```

Merge semantics: slices (mounts) append, maps (env vars) overwrite per-key, scalars replace if non-empty.
`lightweight_args` is **mandatory** for every agent — missing key is a validation error.
Empty list `[]` means no lightweight mode.
`tools.podman.enabled` is `Option<bool>` — None = auto-detect, Some(false) = explicitly disabled.

## Architecture

### Project Structure

    src/
      main.rs                          Entry point, CLI flags, tracing init
      config/mod.rs                    Config struct, serde YAML, merge, presets, validation
      platform/mod.rs                  OSConfig trait (NixOS, Linux)
      sandbox/mod.rs                   Bubblewrap argument builder
      tools/mod.rs                     Podman socket proxy with secret interception
      secrets/mod.rs                   Secret detection via betterleaks
      util/mod.rs                      Helpers, XDG dirs, stale cleanup
    build.rs                           Compile-time warnings for missing tool paths
    flake.nix                          Nix build
    pandoc/
      sources/                         Documentation source files
      scripts/                         Pandoc Lua filters
    scripts/
      build-docs.sh                    Documentation builder
    example-config.yml                 Config reference (used by docs)

### Containment layers

- bubblewrap – filesystem sandbox
- betterleaks – secret detection
- Podman proxy – blocks secret-leaking mounts
- Lightweight mode: `--help`, `-h`, `--version`, `-v` skip full sandbox setup (minimal bwrap only, no betterleaks, no podman proxy)

### Execution flow

1.  Parse CLI flags, load config (see `src/main.rs`)
2.  Detect platform, run betterleaks, start podman proxy
3.  Build bwrap args, launch sandboxed agent, forward signals

## Development

### Commands

``` bash
cargo build                           # development build
cargo test                            # test
cargo clippy                          # lint
cargo fmt                             # format
```

`bwrap_path` and `betterleaks_path` are set via compile-time environment variables (`BWRAP_PATH`, `BETTERLEAKS_PATH`) or configured in `config.yml`.

## Conventions

### Updating documentation

**Do not edit `AGENT.md`, `README.md`, or `CONTRIBUTING.md` directly.**
All are generated from theme files.
Always edit the relevant theme file in `pandoc/sources/themes/`, then rebuild.

With Nix:

``` bash
nix develop -c scripts/build-docs.sh
```

Without Nix:

``` bash
source scripts/ai-dev-env.sh
scripts/build-docs.sh
```

Theme files use pandoc syntax with audience markers:

``` markdown
::: {.readme}
This content appears only in README.md
:::

::: {.agent}
This content appears only in AGENT.md
:::

::: {.contributing}
This content appears only in CONTRIBUTING.md
:::

::: {.agent .contributing}
This content appears in AGENT.md and CONTRIBUTING.md
:::

This content appears in all files
```

### AI development constraints

- Do not run `nix` commands
- The off-the-shelf AI development environment is Nix specific.
  On non-nix platforms, the user must ensure the AI has access to the required tools (see “Development dependencies”).
- If `flake.nix` changes, ask the user to rebuild: `nix develop -c build-ai-dev-env`
