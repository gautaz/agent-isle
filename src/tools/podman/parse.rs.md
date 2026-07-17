# `tools/podman/parse.rs` — HTTP-over-Unix-socket request parser

Reads and parses HTTP requests from a Unix stream using the `httparse` crate.
Extracts the method, path, `Content-Length`, chunked transfer encoding indicator, and raw headers.
Provides the body as raw bytes for downstream JSON deserialisation.
