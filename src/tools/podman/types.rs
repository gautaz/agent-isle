use serde::Deserialize;

#[derive(Deserialize)]
pub struct Mount {
    #[serde(rename = "Type")]
    pub mount_type: String,
    #[serde(rename = "Source")]
    pub source: String,
    #[serde(rename = "ReadOnly", default)]
    pub read_only: Option<bool>,
}

/// A host path the sandbox authorizes for container bind mounts,
/// together with the sandbox mount's read-only mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxMount {
    pub host: String,
    pub read_only: bool,
}

#[derive(Deserialize)]
pub struct HostConfig {
    #[serde(rename = "Binds", alias = "binds", default)]
    pub binds: Vec<String>,
    #[serde(rename = "Mounts", alias = "mounts", default)]
    pub mounts: Vec<Mount>,
}

/// A bind mount as sent by the libpod API (podman CLI specgen body).
/// Field names are lowercase in the wire format, unlike the docker-compat
/// `Mount`/`HostConfig` casing.
#[derive(Deserialize)]
pub struct SpecgenMount {
    #[serde(rename = "source", default)]
    pub source: String,
    #[serde(rename = "type", default)]
    pub mount_type: String,
    #[serde(rename = "options", default)]
    pub options: Vec<String>,
}

/// Container create request body covering both wire formats:
/// - docker-compat: `HostConfig` with `Binds`/`Mounts`
/// - libpod specgen (podman CLI): top-level `mounts` array
#[derive(Deserialize)]
pub struct CreateRequest {
    #[serde(rename = "HostConfig", default)]
    pub host_config: Option<HostConfig>,
    #[serde(rename = "mounts", default)]
    pub mounts: Vec<SpecgenMount>,
}
