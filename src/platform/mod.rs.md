# `platform/mod.rs` — OS detection & platform sandboxing

Defines the `OSConfig` trait that each operating system implements to provide base mounts, platform-specific mounts, environment variables, and secret-mount paths.
Ships with two implementations:

- `Linux` — generic Linux (mounts `/usr/lib`, `/lib`, etc.)
- `NixOS` — NixOS-specific (mounts `/nix/store`)

`PlatformSource` wraps an `OSConfig` as a `CapabilitySource`.
The `detect()` function selects the correct implementation at runtime.
