# `run/launch.rs` — Bubblewrap process launcher

Builds the bubblewrap (`bwrap`) argument list from the expanded sandbox config (mounts, environment, working directory), spawns the `bwrap` process, waits for it to finish, runs shutdown hooks, and returns the exit code.
