# `run/setup.rs` — Runtime environment gathering

Collects ambient runtime information: current working directory, `$HOME`, `$USER`, XDG directory variables, process PID, and creates a unique run directory (`rundir`).
Returns a `RuntimeSetup` struct consumed by the rest of the runtime.
