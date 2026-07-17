![Agent Isle logo](./agent-isle-logo.svg)

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

### Security limitations

The bubblewrap sandbox protects secrets by mounting `/dev/null` over files detected as containing secrets.
This works well when modifying existing files, but has important limitations:

- **New secret files**: betterleaks scans at startup only.
  A new secret file created in a mounted directory **after** the sandbox started will not be detected, leaving its secrets exposed to the agent.
- **Deleted and recreated files**: If a mounted secret file is deleted and recreated, the new inode is not masked and is readable from inside the sandbox.
  This is because bind mounts are tied to specific inodes, not file paths (see [Shared Subtrees](https://docs.kernel.org/filesystems/sharedsubtree.html) and [VFS](https://docs.kernel.org/filesystems/vfs.html)).

These limitations are inherent to the `/dev/null` masking approach.
For maximum security, avoid exposing sensitive directories to the agent.

## Installation

### NixOS

All dependencies are handled automatically by Nix.

#### System-wide (configuration.nix)

Add agent-isle as a flake input and configure it in your system:

```nix
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

```nix
{ inputs, ... }:

{
  home.packages = [
    inputs.agent-isle.packages.x86_64-linux.default
  ];
}
```

Or build and install manually:

```bash
nix build
cp ./result/bin/agent-isle ~/.local/bin/
```

> [!NOTE]
> The Nix build has tool paths hardcoded via compile-time environment variables.

#### Configuring agent support

The default Nix build includes all supported agents.
Use `mkAgentIsle` to customize which agents are included:

```nix
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

  | Tool                                                      | Purpose            |
  | --------------------------------------------------------- | ------------------ |
  | [bubblewrap](https://github.com/containers/bubblewrap)    | Filesystem sandbox |
  | [betterleaks](https://github.com/betterleaks/betterleaks) | Secret detection   |

Install with Cargo:

```bash
cargo install --path .
```

Or build and copy manually:

```bash
cargo build --release
cp target/release/agent-isle ~/.local/bin/
```

> [!NOTE]
> The Cargo-built binary expects tools in `$PATH` or configured in `config.yml`.

### Post-install verification

```bash
agent-isle --help
```

This should display the help message with available flags.

## Usage

```bash
agent-isle [flags] [-- <args forwarded to agent>]
```

### Examples

```bash
# Run a specific agent
agent-isle --agent opencode -- --help

# Run a specific agent
agent-isle --agent aider -- --version

# Dry run (print bwrap args without executing)
agent-isle --agent opencode --dry-run -- --help
```

### Flags

  | Flag        | Short | Description                        | Default                                               |
  | ----------- | ----- | ---------------------------------- | ----------------------------------------------------- |
  | `--agent`   | `-a`  | Agent name (selects preset)        | —                                                     |
  | `--config`  | `-c`  | Config file path                   | `${XDG_CONFIG_HOME:-~/.config}/agent-isle/config.yml` |
  | `--dry-run` | —     | Print bwrap args without executing | `false`                                               |

When invoked as `agent-isle`, `--agent` is required (unless set in config or via symlink).

Arguments after `--` are forwarded to the agent.

### Agent Selection

Agent selection is resolved in this order:

1. **Symlink** — executable name is used as agent name
2. **`--agent` flag** — explicit CLI selection (only when invoked as `agent-isle`)
3. **Config file** — `agent:` field in config.yml

If no agent is resolved, agent-isle exits with an error.

#### Bundled presets

- **opencode** — [opencode](https://github.com/opencode-ai/opencode) AI coding assistant

### Logs

Logs are written to:

```
${XDG_STATE_HOME:-~/.local/state}/agent-isle/logs/<timestamp>_<pid>.log
```

The log path is available in config via the `{log_path}` template variable.

### License

See [LICENSE](./LICENSE) for details.

## Configuration

Configuration is optional when using `--agent` flag or symlinks.

### Config file location

agent-isle looks for a config file at:

```
${XDG_CONFIG_HOME:-~/.config}/agent-isle/config.yml
```

If found, it’s loaded automatically.
Override with `--config` flag:

```bash
agent-isle --config /path/to/config.yml -- --help
```

### Config merging order

1. Built-in defaults — base sandbox configuration
2. Agent preset — agent-specific defaults (mounts, env vars)
3. User config file — your customizations
4. CLI flags — final overrides

### Template variables

Template variables are expanded in string values (paths, env values, chdir):

  | Variable        | Description               | Example                                       |
  | --------------- | ------------------------- | --------------------------------------------- |
  | `{home}`        | User home directory       | `/home/user`                                  |
  | `{user}`        | Username                  | `user`                                        |
  | `{cwd}`         | Current working directory | `/home/user/project`                          |
  | `{xdg_runtime}` | XDG_RUNTIME_DIR           | `/run/user/1000`                              |
  | `{xdg_state}`   | XDG_STATE_HOME            | `/home/user/.local/state`                     |
  | `{log_path}`    | Path to current log file  | `/home/user/.local/state/agent-isle/logs/...` |

Any unknown template variable causes an error at startup.

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

### Configuration reference

See [example-config.yml](./example-config.yml) for a complete documented example.

#### Top-level fields

```yaml
agent: opencode

# Binary paths (absolute required). Set at compile time via BWRAP_PATH/BETTERLEAKS_PATH
# env vars. Optional if compile-time defaults exist; override to use different paths.
bwrap_path: /usr/bin/bwrap
betterleaks_path: /usr/bin/betterleaks
path_secrets_policy: mask  # secrets policy for PATH mounts (mask or show)

# The following chdir and mounts are sensible settings if you want all agents
# to access your current working dir and start in this directory
chdir: "{cwd}"                  # defaults to "/" if unset
mounts:                         # appended to all agent presets
  - path: "{cwd}"
    mode: rw

env:                            # merged with all agent presets (per-key overwrite)
  COMMON_VAR: "value"
  ANTHROPIC_API_KEY:
    command: "pass show api/anthropic"
```

  | Field                 | Type                       | Default                         | Description                                                                                                          |
  | --------------------- | -------------------------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
  | `agent`               | string                     | `""`                            | Default agent name. Must match a key in `agents` map or a bundled preset if non-empty.                               |
  | `chdir`               | string                     | `"/"`                           | Working directory inside the sandbox. Supports template variables.                                                   |
  | `bwrap_path`          | string                     | compile-time `BWRAP_PATH`       | Absolute path to bubblewrap binary.                                                                                  |
  | `betterleaks_path`    | string                     | compile-time `BETTERLEAKS_PATH` | Absolute path to betterleaks binary.                                                                                 |
  | `path_secrets_policy` | string                     | `"mask"`                        | Secrets policy for PATH-derived host directories. `"mask"` (scan for secrets) or `"show"` (expose without scanning). |
  | `mounts`              | list\[MountConfig\]        | `[]`                            | Host mounts. Appended to all agent presets.                                                                          |
  | `env`                 | map\[string, EnvValue\]    | `{}`                            | Environment variables. Merged with presets; per-key overwrite wins.                                                  |
  | `agents`              | map\[string, AgentConfig\] | `{}`                            | Per-agent configuration blocks.                                                                                      |
  | `tools`               | ToolsConfig                | `{}`                            | Tool configuration (e.g., Podman). Deep-merged per tool section.                                                     |

##### `chdir`

Working directory inside the sandbox.
Defaults to `"/"` if unset.

Supports template variables (e.g. `"{cwd}"`).

**Current project access**: To give agents access to your current project, set `chdir: "{cwd}"` and mount `{cwd}` read-write:

```yaml
chdir: "{cwd}"
mounts:
  - path: "{cwd}"
    mode: rw
```

##### Binary paths

`bwrap_path` and `betterleaks_path` are set at compile time via `BWRAP_PATH` and `BETTERLEAKS_PATH` environment variables.
Override in config to use different paths, or if compile-time defaults were not set (missing or empty).

```yaml
bwrap_path: /usr/bin/bwrap
betterleaks_path: /usr/bin/betterleaks
```

##### `path_secrets_policy`

Controls whether PATH-derived host directories (from the host’s `$PATH`) are scanned for secrets.

- `"mask"` (default) — betterleaks scans each PATH directory for secrets.
  Detected files get `/dev/null` bind mounts, hiding them from the agent.
- `"show"` — PATH directories are exposed without secret scanning.

```yaml
path_secrets_policy: mask
```

##### Mount options

Each mount entry supports:

  | Field            | Type    | Default  | Description                                                       |
  | ---------------- | ------- | -------- | ----------------------------------------------------------------- |
  | `path`           | string  | —        | **Required.** Host path to mount. Supports template variables.    |
  | `target`         | string? | `path`   | Target path inside sandbox. Defaults to `path` if omitted.        |
  | `mode`           | string  | `"ro"`   | `"ro"` (read-only) or `"rw"` (read-write).                        |
  | `secrets_policy` | string  | `"mask"` | `"mask"` (hide secrets) or `"show"` (expose to agent). See below. |

###### `secrets_policy`

Controls how secrets detection handles files in this mount:

- `"mask"` (default) — betterleaks scans this mount for secrets.
  Detected files get `/dev/null` bind mounts, hiding them from the agent.
- `"show"` — secrets are visible to the agent.
  Mount is skipped during scanning.

**Warning**: `"show"` exposes any secrets in this mount to the AI agent.

##### Environment variables

Each env var entry supports two forms:

**Static value**:

```yaml
MY_VAR: "some-value"
```

**Command execution** (stdout becomes the value):

```yaml
SECRET_VAR:
  command: "pass show api/my-key"
```

Both forms support template variables (e.g. `command: "echo {home}/.config"`).

##### Per-agent configuration

```yaml
agents:
  opencode:                     # these settings will update the opencode preset
    binary: /nix/store/.../bin/opencode
    mounts:                     # appended to preset mounts
      - path: "{home}/.config/my-other-tool"
      - path: "{log-path}"      # diagnostics: exposes log path to agent
    env:                        # merged with preset env (per-key overwrite)
      AGENT_ISLE_LOGS: "{log_path}"
      OPENAI_API_KEY:
        command: "gopass show api/openai"

  custom:
    binary: /usr/bin/custom-agent
    chdir: "/tmp"               # overrides top-level chdir for this agent
    mounts:                     # appended to preset mounts
      - path: "{home}/experimental"
        target: "/tmp"
        mode: rw
        secrets_policy: show    # disable secrets scanning for this mount.
                                # /!\ any secret in this mount is available
                                # to the AI agent.
      - path: "{home}/.config/custom-agent"
    lightweight_args:           # mandatory, empty = no lightweight mode
      - "--help"
      - "-h"
```

Each agent block supports:

  | Field              | Type                    | Description                                                                           |
  | ------------------ | ----------------------- | ------------------------------------------------------------------------------------- |
  | `binary`           | string                  | **Required.** Absolute path to agent executable. Supports template variables.         |
  | `chdir`            | string?                 | Agent-specific working directory override. Falls back to top-level `chdir`.           |
  | `mounts`           | list\[MountConfig\]     | Agent-specific mounts. **Appended** to preset and top-level mounts.                   |
  | `env`              | map\[string, EnvValue\] | Agent-specific env vars. Merged with preset and top-level; per-key overwrite wins.    |
  | `lightweight_args` | list\[string\]          | **Mandatory.** Flags that trigger lightweight mode. Empty `[]` = no lightweight mode. |

`lightweight_args` is required for every agent.
Missing key is a validation error at startup.

##### Tools

```yaml
tools:
  podman:
    enabled: true
    socket_path: "{xdg_runtime}/podman/podman.sock"
```

###### Podman

  | Field         | Type    | Default                              | Description                                                           |
  | ------------- | ------- | ------------------------------------ | --------------------------------------------------------------------- |
  | `enabled`     | bool?   | `null` (auto-detect)                 | `true` = force enable, `false` = force disable, `null` = auto-detect. |
  | `socket_path` | string? | `"{xdg_runtime}/podman/podman.sock"` | Path to Podman socket. Supports template variables.                   |
