use serde::Deserialize;

#[derive(Deserialize)]
pub struct Mount {
    #[serde(rename = "Type")]
    pub mount_type: String,
    #[serde(rename = "Source")]
    pub source: String,
    #[allow(dead_code)]
    #[serde(rename = "Target")]
    pub target: String,
}

#[derive(Deserialize)]
pub struct HostConfig {
    #[serde(default)]
    pub binds: Vec<String>,
    #[serde(default)]
    pub mounts: Vec<Mount>,
}

#[derive(Deserialize)]
pub struct CreateConfig {
    #[serde(rename = "HostConfig")]
    pub host_config: Option<HostConfig>,
}
