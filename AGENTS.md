# agent-isle agent reference

## Session Setup

Before working on this project, you must load the development environment.
This makes all required tools (`cargo`, `clippy`, `rustfmt`, `panache`, `lychee`, etc.) available in your PATH.

Run this before any other command:

```bash
source scripts/ai-dev-env.sh
```

This is the only way to access the development tools.
Do not run `nix` commands directly — you do not have the permissions required.

All development commands (`cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`, `panache format`) must be run after sourcing the environment script.
If you start a new shell session during work, re-source the script.
If your environment does not persist variables between tool invocations (e.g. each command runs in a fresh shell), source the script and run the command in the same invocation, e.g.:

```bash
source scripts/ai-dev-env.sh && scripts/build-docs.sh
```

### Constraints

- Always run `source scripts/ai-dev-env.sh` first.
  Never skip this step.
- Do not run `nix` commands directly.
  They will fail.
- Do not run `git` commands.
- If `flake.nix` changes, ask the user to rebuild: `nix develop -c build-ai-dev-env`

## Scope

- Bubblewrap sandbox (filesystem isolation)
- Betterleaks (secret detection, masks with /dev/null)
- Podman proxy (blocks secret-leaking mounts)
- Agent presets (opencode, or custom via config)

## Configuration

### Config structure

```yaml
agent: "default agent to use"             # must match agents key or bundled preset
chdir: "{cwd}"                            # working directory inside sandbox (default: "/")
bwrap_path: "/usr/bin/bwrap"              # absolute path to bubblewrap binary
betterleaks_path: "/usr/bin/betterleaks"  # absolute path to betterleaks binary
path_secrets_policy: mask                 # secrets policy for PATH mounts (mask or show)
mounts: []                                # appended to all agent presets
env:                                      # merged with all agent presets (per-key overwrite)
  STATIC_VAR: "value"
  SECRET_VAR:
    command: "shell command"              # command to retrieve the secret
agents:
  agent-name:
    binary: "/absolute/path"
    chdir: "/override"                    # optional, falls back to top-level chdir
    mounts: []                            # appended to preset mounts
    env: {}                               # merged with preset env (per-key overwrite)
    lightweight_args: []                  # mandatory, empty = no lightweight mode
tools:
  podman:
    enabled: true                         # nil = auto-detect
    socket_path: "{xdg_runtime}/podman/podman.sock"
```

`lightweight_args` is **mandatory** for every agent — missing key is a validation error.
Empty list `[]` means no lightweight mode.

### Merge behavior

  | Field type       | Behavior                          |
  | ---------------- | --------------------------------- |
  | Lists (`mounts`) | **Appended** (never replaced)     |
  | Maps (`env`)     | Merged per-key; override keys win |
  | Scalars          | Replaced by override if non-empty |

## Architecture

The sandbox provides capabilities to the agent: filesystem access, environment variables, secrets masking, container connectivity.
Each capability is provided by a source that knows about one domain of the system.
The `CapabilitySource` trait is the common interface all sources implement.
Collecting capabilities from every source into the final sandbox configuration happens in `capability_sources.rs`.

### Extension points

The system is extensible in three areas:

1. **Agent presets** (`src/config/presets.rs`) – bundled agent configurations
2. **Platforms** (`src/platform/mod.rs`) – OS-specific mounts, env vars, and secret handling
3. **Tools** (`src/tools/mod.rs`) – pluggable external tool integrations

### Project Structure

```
src/
  lib.rs                            Crate root, deny lints
  main.rs                           Entry point, CLI flags, orchestration
  capability_sources.rs             CapabilitySource trait, capability collection
  load_config.rs                    Config loading and merging
  logging.rs                        Tracing setup (stderr + file)
  config/
    mod.rs                          Config structs, serde, validation, ConfigSource, AgentSource
    merge.rs                        Config merging
    presets.rs                      Bundled agent presets
    template.rs                     Template variable expansion
    validate.rs                     Path/agent validation
  platform/mod.rs                   OSConfig trait (NixOS, Linux), PlatformSource
  user_profile/mod.rs               UserProfileSource — PATH mounts, cache, user env
  sandbox/mod.rs                    Bubblewrap argument builder, Mount types
  secrets/mod.rs                    Secret detection via betterleaks
  tools/
    mod.rs                          Tool trait, ToolStartContext, registry
    podman/
      mod.rs                        PodmanTool implementation
      http.rs                       HTTP parsing utilities
      proxy.rs                      Podman socket proxy, mount policy enforcement, ProxySource
      secret_detection.rs           Secret file detection
      types.rs                      Podman JSON structs
  util/mod.rs                       Helpers, XDG dirs, socket validation, stale cleanup
build.rs                            Compile-time warnings for missing tool paths
flake.nix                           Nix build
pandoc/
  sources/                          Documentation source files
  scripts/                          Pandoc Lua filters
scripts/
  build-docs.sh                     Documentation builder
example-config.yml                  Config reference (used by docs)
```

### Containment layers

- bubblewrap – filesystem sandbox
- betterleaks – secret detection
- Podman proxy – blocks secret-leaking and non-sandbox mounts (pluggable tool)
- Lightweight mode: `--help`, `-h`, `--version`, `-v` skip full sandbox setup (minimal bwrap only, no betterleaks, no tools)

#### Podman mount policy

The podman proxy intercepts container create requests and rejects host bind mounts that violate the sandbox policy.
A bind source is rejected when it:

1. is or contains a known secret file
2. lies outside the sandbox’s own host mounts (it must be a sandbox mount or a descendant of one)
3. mounts a read-only sandbox tree read-write
4. does not exist on the host (podman would otherwise create it)

Sources are canonicalized (`realpath`) before matching so symlinks and `..` segments cannot bypass the checks.

### Tool plugins

Tools are pluggable via the `Tool` trait.
Each tool owns its config, detection logic, setup, and lifecycle.
Tools are registered in `tools::registered_tools()` and conditionally compiled via feature flags.

```rust
pub trait Tool: Send {
    fn id(&self) -> &str;
    fn capabilities(&self) -> Option<&dyn CapabilitySource>;
    fn start(&mut self, ctx: &ToolStartContext) -> Result<Option<Box<dyn FnOnce()>>>;
}
```

Adding a new tool: create `tools/newtool/mod.rs`, implement `Tool`, add one entry to `registered_tools()`.
No changes to `main.rs`, `config/mod.rs`, or `load_config.rs`.
If a user configures a tool not compiled in, agent-isle errors with rebuild instructions.

### Mount configuration

YAML config uses `MountConfig` objects for all mounts:

```yaml
mounts:
  - path: /usr/share/fonts
  - path: "{home}/.config/app"
    target: /home/user/.config/app    # optional, defaults to path
    mode: ro                          # optional, defaults to ro
    secrets_policy: show              # optional, defaults to mask
```

Each mount declares a `SecretsPolicy`:

- `mask` (default) – betterleaks scans this mount path for secrets; detected files get `/dev/null` bind mounts
- `show` – secrets are visible to the agent; mount is skipped during scanning

Platform mounts (OS paths, DNS, NixOS paths) use `show` policy.
User and agent mounts default to `mask`.

### Capabilities

Sources provide mounts and environment variables to the sandbox.

  | Source            | Mounts                                                      | Env vars              |
  | ----------------- | ----------------------------------------------------------- | --------------------- |
  | PlatformSource    | OS paths (show), DNS, NixOS paths                           | (none)                |
  | UserProfileSource | PATH dirs (policy from config), cache (show)                | PATH, XDG_RUNTIME_DIR |
  | ConfigSource      | User-configured mounts (with user-specified SecretsPolicy)  | User-configured env   |
  | AgentSource       | Agent-specific mounts (converted via MountConfig::to_mount) | Agent-specific env    |
  | ProxySource       | Podman proxy socket bind                                    | CONTAINER_HOST        |

Secret masking mounts (`/dev/null` binds) are created directly by `OSConfig::secret_mounts()` and appended after all other mounts.
No separate `SecretSource` exists – masking is a platform capability applied at the end of mount collection.

### Secret detection flow

Secret detection is mount-aware via `SecretsPolicy`:

1. Collect mounts from all sources (platform, agent, user profile, config, tool capabilities)
2. Extract target paths from all `mask`-policy mounts
3. Run betterleaks on those paths – detected secrets become masking mount candidates
4. Start tools: each tool receives the secret file paths and the full non-secret mount list via `ToolStartContext` (the podman proxy builds its mount allowlist from this list)
5. Append masking mounts last – `/dev/null` binds over detected secret files

The mount list is complete before tools start: tool capability mounts are collected in step 1, and `Tool::start` returns only a shutdown hook, never new mounts.
The masking mounts are deliberately excluded from the list passed to tools.

`show`-policy mounts are never scanned and never masked.

### Execution flow

1. Parse CLI flags, load and merge config (`main.rs`, `load_config.rs`)
2. Apply agent preset, validate configuration, validate tool config against compiled tools
3. Detect platform, expand template variables in mounts and env
4. Collect base mounts (platform, config, agent)
5. Scan base mounts for secrets via betterleaks
6. Activate tools: detect, setup, collect tool-provided capabilities
7. Scan tool mounts for secrets
8. Append masking mounts, build bwrap args, launch sandboxed agent
9. Shut down active tools, clean up run directory, sync logs

## Development

### Commands

```bash
cargo build                            # development build
cargo test                             # test
cargo clippy                           # lint
cargo fmt                              # format
panache format .                       # format markdown
```

`bwrap_path` and `betterleaks_path` are set via compile-time environment variables (`BWRAP_PATH`, `BETTERLEAKS_PATH`) or configured in `config.yml`.

### Running tests

```bash
cargo test
```

### Linting

```bash
cargo clippy
cargo fmt --check
panache format --check .
```

### Git hooks

Git hooks enforce formatting, linting, and tests automatically.

  | Hook         | When               | Checks                                                                      |
  | ------------ | ------------------ | --------------------------------------------------------------------------- |
  | `pre-commit` | Before each commit | `cargo fmt --check`, `cargo clippy`, `panache format --check .`, `lychee .` |
  | `pre-push`   | Before each push   | `cargo test`                                                                |

Run the setup script once:

```bash
scripts/setup-githooks.sh
```

### Adding a new agent preset

Edit `src/config/presets.rs`:

1. Add a new entry to the `PRESETS` HashMap with the agent’s configuration

`Preset` fields:

  | Field              | Type                                   | Description                                                                             |
  | ------------------ | -------------------------------------- | --------------------------------------------------------------------------------------- |
  | `binary`           | `&'static str`                         | Agent executable path (set at compile time)                                             |
  | `mounts`           | `&'static [(&'static str, MountMode)]` | Bind mounts for this agent as `(path, mode)` tuples                                     |
  | `env`              | `&'static [(&str, &str)]`              | Environment variables as `(name, value)` tuples                                         |
  | `lightweight_args` | `&'static [&'static str]`              | Agent flags that trigger lightweight mode (mandatory, empty `[]` = no lightweight mode) |

The binary path is set at compile time via environment variables (e.g., `OPENCODE_PATH`).
Users can override it in config YAML.
All external tool paths must be absolute to prevent PATH injection attacks.

Example:

```rust
    m.insert(
        "opencode",
        Preset {
            binary: OPENCODE_DEFAULT_PATH,
            mounts: &[
                ("{home}/.config/opencode", MountMode::Ro),
                ("{home}/.local/share/opencode", MountMode::Rw),
                ("{home}/.local/state/opencode", MountMode::Rw),
            ],
            env: &[],
            lightweight_args: &["--help", "-h", "--version", "-v"],
        },
    );
```

Template variables (`{home}`, `{cwd}`, etc.) are expanded at runtime.

## Conventions

### Updating documentation

**Do not edit `AGENTS.md`, `README.md`, or `CONTRIBUTING.md` directly.** All are generated from theme files.
Always edit the relevant theme file in `pandoc/sources/themes/`, then rebuild.

```bash
source scripts/ai-dev-env.sh
scripts/build-docs.sh
```

Theme files use pandoc syntax with audience markers:

```markdown
::: {.readme}
This content appears only in README.md
:::

::: {.agent}
This content appears only in AGENTS.md
:::

::: {.contributing}
This content appears only in CONTRIBUTING.md
:::

::: {.agent .contributing}
This content appears in AGENTS.md and CONTRIBUTING.md
:::

This content appears in all files
```
