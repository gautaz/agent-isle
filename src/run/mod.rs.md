# `run/mod.rs` — Sandbox runtime orchestration

The top-level runner.
Coordinates the full sandbox lifecycle: 1.
Setting up the runtime environment (`setup_runtime`) 2.
Expanding template variables in the sandbox config 3.
Collecting capability sources from every layer (platform, config, agent, tools, user profile) 4.
Running secret scanning via betterleaks 5.
Starting pluggable tools (e.g. Podman proxy) 6.
Launching the bubblewrap sandbox

Also provides `run_cmd_bare()` for lightweight mode (no sandbox).
