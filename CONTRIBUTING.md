# agent-isle contributing

## Development

### Development dependencies

With Nix, enter the dev shell to get all tools:

``` bash
nix develop
```

Without Nix, install these tools manually:

| Tool                                                        | Purpose                        |
|-------------------------------------------------------------|--------------------------------|
| [Rust](https://rustup.rs/) 1.80+                            | Compiler                       |
| [clippy](https://doc.rust-lang.org/clippy/)                 | Linting                        |
| [rustfmt](https://github.com/rust-lang/rustfmt)             | Formatting                     |
| [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | Coverage                       |
| [pandoc](https://pandoc.org/installing.html)                | Documentation generation       |
| [bubblewrap](https://github.com/containers/bubblewrap)      | Sandbox (for testing)          |
| [betterleaks](https://github.com/gopasspw/betterleaks)      | Secret detection (for testing) |

### Development build

``` bash
cargo build
```

### Running tests

``` bash
cargo test
```

### Linting

``` bash
cargo clippy
cargo fmt --check
```

### Git hooks

Git hooks enforce formatting, linting, and tests automatically.

| Hook         | When               | Checks                              |
|--------------|--------------------|-------------------------------------|
| `pre-commit` | Before each commit | `cargo fmt --check`, `cargo clippy` |
| `pre-push`   | Before each push   | `cargo test`                        |

With Nix, hooks are configured automatically when entering the dev shell.

Without Nix, run the setup script once:

``` bash
scripts/setup-githooks.sh
```

### Coverage

Text summary:

``` bash
cargo llvm-cov
```

HTML report:

``` bash
cargo llvm-cov --html
```

LCOV output (for CI or visualization tools):

``` bash
cargo llvm-cov --lcov --output-path lcov.info
```

With Nix, `LLVM_COV` and `LLVM_PROFDATA` are set automatically in the dev shell.
Without Nix, set them manually if `llvm-tools-preview` is not installed via rustup:

``` bash
export LLVM_COV=$(which llvm-cov)
export LLVM_PROFDATA=$(which llvm-profdata)
```

### Adding a new agent preset

Edit `src/config/mod.rs`:

1.  Add a new entry to the `PRESETS` HashMap with the agent’s configuration

`Preset` fields:

| Field | Type | Description |
|----|----|----|
| `binary` | `&'static str` | Agent executable path (set at compile time) |
| `ro_mounts` | `&'static [&'static str]` | Read-only bind mounts for this agent |
| `rw_mounts` | `&'static [&'static str]` | Read-write bind mounts for this agent |
| `env` | `&'static [(&str, &str)]` | Environment variables as `(name, value)` tuples |
| `lightweight_args` | `&'static [&'static str]` | Agent flags that trigger lightweight mode (mandatory, empty `[]` = no lightweight mode) |

The binary path is set at compile time via environment variables (e.g., `OPENCODE_PATH`).
Users can override it in config YAML.
All external tool paths must be absolute to prevent PATH injection attacks.

Example:

``` rust
    m.insert(
        "opencode",
        Preset {
            binary: OPENCODE_DEFAULT_PATH,
            ro_mounts: &[
                "{home}/.config/opencode",
                "{home}/.local/share/opencode",
                "{home}/.local/state/opencode",
            ],
            rw_mounts: &[],
            env: &[],
            lightweight_args: &["--help", "-h", "--version", "-v"],
        },
    );
```

Template variables (`{home}`, `{cwd}`, etc.) are expanded at runtime.

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

## Conventions

### Extension points

1.  Agent presets (`src/config/mod.rs` — `PRESETS` HashMap)
2.  Platform (`src/platform/mod.rs`)
3.  Tools (`src/tools/mod.rs`)

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
- Tool paths use compile-time defaults via `option_env!()`, overridable in config YAML

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
