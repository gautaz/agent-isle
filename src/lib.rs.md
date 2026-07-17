# `lib.rs` — Library crate root

Re-exports all public modules so that consumers of the library can access every component from a single entry point.

Clippy lints (`unwrap_used`, `expect_used`, `panic`, `indexing_slicing`) are relaxed globally — they are re-enabled inside tests.
