# agent-isle

General containment environment for AI agents.
Wraps any CLI-based AI agent inside a bubblewrap sandbox with secret detection and tool proxying.

## Scope

agent-isle provides:

- **Filesystem sandboxing** via bubblewrap (bwrap) — agents can only access explicitly allowed paths
- **Secret detection** via betterleaks — files containing secrets are masked with `/dev/null`
- **Container proxying** via Podman — intercepts container create requests to prevent secret leaks
- **Agent flexibility** — run opencode, aider, or any custom AI agent inside the same sandbox

The sandbox is **fixed infrastructure** (not user-configurable).
What changes is which agent runs inside it.

## Installation

### NixOS

All dependencies are handled automatically by Nix.

#### System-wide (configuration.nix)

Add agent-isle as a flake input and configure it in your system:

``` nix
{
  inputs = {
    # Your existing nixpkgs input
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    agent-isle.url = "github:gautaz/phoenix?dir=agent-isle";
    agent-isle.inputs.nixpkgs.follows = nixpkgs;
  };

  outputs = { self, nixpkgs, agent-isle, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        {
          environment.systemPackages = [ agent-isle.packages.x86_64-linux.default ];
        }
      ];
    };
  };
}
```

#### Per-user (Home Manager)

Add to your Home Manager configuration:

``` nix
{ inputs, ... }:

{
  home.packages = [
    inputs.agent-isle.packages.x86_64-linux.default
  ];
}
```

Or build and install manually:

``` bash
nix build
cp ./result/bin/agent-isle ~/.local/bin/
```

> [!NOTE]
> The Nix build has tool paths hardcoded via compile-time environment variables.

#### Configuring agent support

By default, no agent binaries are bundled.
Use `mkAgentIsle` to include agent support at compile time:

``` nix
# In your flake
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    agent-isle.url = "github:gautaz/phoenix?dir=agent-isle";
    agent-isle.inputs.nixpkgs.follows = nixpkgs;
  };

  outputs = { self, nixpkgs, agent-isle, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        {
          environment.systemPackages = [
            (agent-isle.packages.x86_64-linux.mkAgentIsle {
              agents = {
                opencode = pkgs.opencode;  # or your custom package
              };
              maskedAgents = [ "opencode" ];  # creates opencode → agent-isle symlink
            })
          ];
        }
      ];
    };
  };
}
```

This sets `OPENCODE_PATH` at compile time, so the binary path is absolute and requires no runtime configuration.

When `maskedAgents` is set, symlinks are created so running the agent name (e.g., `opencode`) invokes agent-isle with that agent.

> [!NOTE]
> When using `maskedAgents`, do not include the original agent package in system or user packages — it would conflict with the symlink.
> The `agents` attribute provides the binary path at compile time only.

### Generic Linux

Requires Rust 1.80+ and the following tools:

| Tool                                                   | Purpose            |
|--------------------------------------------------------|--------------------|
| [bubblewrap](https://github.com/containers/bubblewrap) | Filesystem sandbox |
| [betterleaks](https://github.com/gopasspw/betterleaks) | Secret detection   |

Install with Cargo:

``` bash
cargo install --path .
```

Or build and copy manually:

``` bash
cargo build --release
cp target/release/agent-isle ~/.local/bin/
```

> [!NOTE]
> The Cargo-built binary expects tools in `$PATH` or configured in `config.yml`.

### Post-install verification

``` bash
agent-isle --help
```

This should display the help message with available flags.

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

## Configuration

Configuration is optional when using `--agent` flag or symlinks.

### Config file location

agent-isle looks for a config file at:

    ${XDG_CONFIG_HOME:-~/.config}/agent-isle/config.yml

If found, it’s loaded automatically.
Override with `--config` flag:

``` bash
agent-isle --config /path/to/config.yml -- --help
```

### Config merging order

1.  Built-in defaults — base sandbox configuration
2.  Agent preset — agent-specific defaults (mounts, env vars)
3.  User config file — your customizations
4.  CLI flags — final overrides

### Configuration reference

See [example-config.yml](example-config.yml) for a documented configuration file.

#### Top-level fields

``` yaml
agent: opencode

# Tool paths (absolute required). Set at compile time via BWRAP_PATH/BETTERLEAKS_PATH
# env vars, or override here. Leave empty to use compile-time default.
bwrap_path: /usr/bin/bwrap
betterleaks_path: /usr/bin/betterleaks

ro_mounts:                      # appended to all agent presets
  - "/usr/share/fonts"
  - "{home}/.config/shared-tool"
rw_mounts:                      # appended to all agent presets
  - "{cwd}/.cache"
env:                            # merged with all agent presets (per-key overwrite)
  COMMON_VAR: "value"
  ANTHROPIC_API_KEY:
    command: "pass show api/anthropic"
  AGENT_ISLE_LOGS: "{log_path}"  # diagnostics: exposes log path to agent
```

#### Per-agent configuration

``` yaml
agents:
  opencode:
    binary: /nix/store/.../bin/opencode
    ro_mounts:                  # appended to preset mounts
      - "{home}/.config/my-other-tool"
    rw_mounts:                  # appended to preset mounts
      - "{cwd}/.opencode"
    env:                        # merged with preset env (per-key overwrite)
      MY_VAR: "some-value"
      OPENAI_API_KEY:
        command: "gopass show api/openai"
    lightweight_args:           # mandatory, empty = no lightweight mode
      - "--help"
      - "-h"
      - "--version"
      - "-v"

  custom:
    binary: /usr/bin/custom-agent
    ro_mounts:                  # appended to preset mounts
      - "{home}/.config/custom-agent"
    lightweight_args:           # mandatory, empty = no lightweight mode
      - "--help"
      - "--version"
```

#### Tools

``` yaml
tools:
  podman:
    enabled: true
    socket_path: "{xdg_runtime}/podman/podman.sock"
```

#### Merge behavior

Slices (mounts) are appended during merge.
Maps (env vars) are overwritten per-key.
Scalars (agent, binary) are replaced if non-empty.
`tools.podman.enabled` is a `*bool` — nil means auto-detect, false means explicitly disabled.

### Template variables

Template variables are expanded in string values:

| Variable        | Description               | Example                                       |
|-----------------|---------------------------|-----------------------------------------------|
| `{home}`        | User home directory       | `/home/user`                                  |
| `{user}`        | Username                  | `user`                                        |
| `{cwd}`         | Current working directory | `/home/user/project`                          |
| `{xdg_runtime}` | XDG_RUNTIME_DIR           | `/run/user/1000`                              |
| `{xdg_state}`   | XDG_STATE_HOME            | `/home/user/.local/state`                     |
| `{name}`        | Agent binary name         | `opencode`                                    |
| `{log_path}`    | Path to current log file  | `/home/user/.local/state/agent-isle/logs/...` |
