# `user_profile/mod.rs` — User environment & PATH mounts

Provides the user's `$PATH` directories as read-only mounts inside the sandbox, creates a dedicated cache directory (`~/.cache/agent-isle`), and sets `PATH` and `XDG_RUNTIME_DIR` for the sandboxed process.
Implements `CapabilitySource` so the runtime can collect these contributions.
