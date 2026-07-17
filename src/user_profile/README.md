# `user_profile` — User environment & cache isolation

Concerns managed in this module:

- **PATH mounts** — expose each `$PATH` directory as a read-only mount inside the sandbox
- **Cache isolation** — create and mount `~/.cache/agent-isle` for sandboxed processes
- **Environment variables** — set `PATH` and `XDG_RUNTIME_DIR` for the sandboxed process
- **CapabilitySource adapter** — expose user-profile contributions to the sandbox runtime

## Files

  | File                     | Concern                                      |
  | ------------------------ | -------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md) | `UserProfileSource`, PATH mounts, cache, env |
