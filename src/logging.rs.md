# `logging.rs` — Tracing / log initialisation

Configures the `tracing` subscriber.
Debug and info messages are written to a file inside the state directory; warnings and errors also go to stderr.
Falls back to all-stderr when the log file cannot be created.
