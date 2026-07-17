# `platform` — Operating system abstraction

Concerns managed in this module:

- **OS detection** — identify the host operating system at runtime
- **`OSConfig` trait** — platform-specific sandbox mounts and environment
- **Linux implementation** — generic mounts (`/usr/lib`, `/lib`, `/etc`, ...)
- **NixOS implementation** — mounts `/nix/store` and related paths
- **CapabilitySource adapter** — expose OS-level mounts to the sandbox

## Files

  | File                     | Concern                                             |
  | ------------------------ | --------------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md) | `OSConfig` trait, `Linux`/`NixOS` impls, `detect()` |
