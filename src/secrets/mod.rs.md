# `secrets/mod.rs` — Secret file detection via betterleaks

Wraps the external `betterleaks` tool to detect secret files inside directories.
Parses the JSON output, deduplicates and sorts the findings.
Provides high-level functions to scan a single directory or a set of paths.
