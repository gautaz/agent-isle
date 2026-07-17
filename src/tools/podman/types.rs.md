# `tools/podman/types.rs` — Podman API deserialisation types

Minimal serde-deserialisable structs for the Podman container creation API: `Mount` (type, source, target), `HostConfig` (binds, mounts), and `CreateConfig`.
Used by the proxy to inspect incoming container-create requests.
