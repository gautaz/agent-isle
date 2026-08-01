# agent-isle contributing

## Development

### Development dependencies

With Nix, enter the dev shell to get all tools:

```bash
nix develop
```

Without Nix, install these tools manually:

  | Tool                                                        | Purpose                        |
  | ----------------------------------------------------------- | ------------------------------ |
  | [Rust](https://rustup.rs/) 1.80+                            | Compiler                       |
  | [clippy](https://doc.rust-lang.org/clippy/)                 | Linting                        |
  | [rustfmt](https://github.com/rust-lang/rustfmt)             | Formatting                     |
  | [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | Coverage                       |
  | [pandoc](https://pandoc.org/installing.html)                | Documentation generation       |
  | [panache](https://panache.bz)                               | Markdown formatting/linting    |
  | [lychee](https://github.com/lycheeverse/lychee)             | Link checking                  |
  | [bubblewrap](https://github.com/containers/bubblewrap)      | Sandbox (for testing)          |
  | [betterleaks](https://github.com/betterleaks/betterleaks)   | Secret detection (for testing) |

### Development build

```bash
cargo build
```

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

With Nix, hooks are configured automatically when entering the dev shell.

Without Nix, run the setup script once:

```bash
scripts/setup-githooks.sh
```

### Coverage

Text summary:

```bash
cargo llvm-cov
```

HTML report:

```bash
cargo llvm-cov --html
```

LCOV output (for CI or visualization tools):

```bash
cargo llvm-cov --lcov --output-path lcov.info
```

With Nix, `LLVM_COV` and `LLVM_PROFDATA` are set automatically in the dev shell.
Without Nix, set them manually if `llvm-tools-preview` is not installed via rustup:

```bash
export LLVM_COV=$(which llvm-cov)
export LLVM_PROFDATA=$(which llvm-profdata)
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

## Conventions

### Extension points

1. Agent presets (`src/config/mod.rs` — `PRESETS` HashMap)
2. Platform (`src/platform/mod.rs`)
3. Tools (`src/tools/mod.rs`)

### Adding OS support

Create a new struct implementing `OSConfig` trait in `src/platform/mod.rs`.
Add detection in the `detect()` function.
Currently only Linux is supported (NixOS and generic).

### Code conventions

- No globals — pass deps explicitly (exception: compile-time paths via `option_env!()`)
- `anyhow::Result` for error handling
- `serde` for YAML/JSON serialization
- `tracing` for structured logging
- rustfmt formatting
- clippy linting
- Table-driven tests
- Mandatory `fsync` before log close
- All external tool paths must be absolute (prevents PATH injection attacks)
- Binary paths use compile-time defaults via `option_env!()`, overridable in config YAML

### Updating documentation

**Do not edit `AGENTS.md`, `README.md`, or `CONTRIBUTING.md` directly.** All are generated from theme files.
Always edit the relevant theme file in `pandoc/sources/themes/`, then rebuild.

With Nix:

```bash
nix develop -c scripts/build-docs.sh
```

Without Nix:

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
