# `tools/podman/types.rs` — Podman API deserialisation types

Minimal serde-deserialisable structs for the Podman container creation API: `Mount` (type, source), `HostConfig` (binds, mounts), `SpecgenMount` (libpod specgen mount), and `CreateRequest`.
Used by the proxy to inspect incoming container-create requests.

## Why both `HostConfig` and `SpecgenMount` are supported

The proxy is a blind passthrough for the Podman socket: it does not control which client connects to it, so it must be able to inspect container-create payloads from every API surface Podman exposes.
The same bind mount arrives in two different shapes depending on the endpoint:

  | Endpoint                                      | Payload shape          | Field location                                                                                          | Typical client                                      |
  | --------------------------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
  | Docker-compat API (`/v1.x/containers/create`) | Docker `CreateConfig`  | `HostConfig.Binds` (`"host:/dest:ro"` strings) and `HostConfig.Mounts` (`{"Type","Source","ReadOnly"}`) | Docker CLI, `docker compose`, curl, Docker SDKs     |
  | Libpod API (`/v5.x/libpod/containers/create`) | Podman `SpecGenerator` | top-level `mounts` (`{"destination","type","source","options"}`)                                        | The `podman` CLI (all `run`/`create`/`build` calls) |

Both formats must be parsed and validated:

- The current `podman` client serialises a libpod `SpecGenerator` whose mounts live in the top-level `mounts` array — there is no `HostConfig` at all.
  Failing to parse it means every `podman run --volume ...` bypasses the proxy's mount policy (this was a real bypass).
- The docker-compat endpoint is part of the public API surface; Docker-oriented tooling (compose, scripts using `curl`, Docker SDKs) sends `HostConfig`.
  Dropping it would reopen the same bypass for those clients.

`CreateRequest` therefore deserialises both optional sections of a single request body, and `validate_create_mounts` (see `proxy.rs.md`) checks the union of both.
