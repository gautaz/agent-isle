# `run` — Sandbox runtime lifecycle

Concerns managed in this module:

- **Runtime orchestration** — the main `run()` function that sequences every step: environment setup, template expansion, source collection, secret scanning, tool startup, sandbox launch
- **Environment gathering** — discover cwd, home, user, XDG dirs, PID, create rundir
- **Bubblewrap process launch** — build bwrap arguments, spawn, wait, clean up

## Files

  | File                           | Concern                                         |
  | ------------------------------ | ----------------------------------------------- |
  | [`mod.rs.md`](mod.rs.md)       | Top-level orchestration (`run`, `run_cmd_bare`) |
  | [`setup.rs.md`](setup.rs.md)   | Runtime environment discovery (`RuntimeSetup`)  |
  | [`launch.rs.md`](launch.rs.md) | Bubblewrap process launcher (`launch_sandbox`)  |
