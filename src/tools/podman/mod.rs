mod http;
mod parse;
pub mod proxy;
mod secret_detection;
pub(crate) mod types;

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::capability_sources::CapabilitySource;
use crate::config::EnvValue;
use crate::sandbox::Mount;
use crate::tools::Tool;
use crate::util;

#[derive(Debug, Default, serde::Deserialize)]
struct PodmanConfig {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    socket_path: Option<String>,
}

pub struct PodmanTool {
    config: PodmanConfig,
    config_valid: bool,
    proxy_socket: String,
}

impl PodmanTool {
    pub fn new(config: serde_yml::Value, rundir: &str) -> Self {
        let (config, config_valid) = match serde_yml::from_value(config) {
            Ok(c) => (c, true),
            Err(_) => {
                tracing::warn!("podman tool config is malformed, using defaults");
                (PodmanConfig::default(), false)
            }
        };
        let proxy_socket = format!("{rundir}/podman-proxy.sock");
        Self {
            config,
            config_valid,
            proxy_socket,
        }
    }

    fn podman_socket_path(&self) -> String {
        match &self.config.socket_path {
            Some(p) => p.clone(),
            None => {
                let xdg_runtime = util::xdg_runtime_dir();
                format!("{xdg_runtime}/podman/podman.sock")
            }
        }
    }

    fn is_available(&self) -> bool {
        if !self.config_valid {
            tracing::debug!("podman tool: config is invalid");
            return false;
        }
        if self.config.enabled == Some(false) {
            tracing::debug!("podman tool: disabled in config");
            return false;
        }
        let socket_path = self.podman_socket_path();
        if let Err(e) = util::validate_socket_ownership(&socket_path) {
            tracing::debug!(error = %e, "podman tool: socket ownership validation failed");
            return false;
        }
        if !Path::new(&socket_path).exists() {
            tracing::debug!(path = %socket_path, "podman tool: socket not found");
            return false;
        }
        true
    }
}

impl Tool for PodmanTool {
    fn id(&self) -> &str {
        "podman"
    }

    fn capabilities(&self) -> Option<&dyn CapabilitySource> {
        if self.is_available() {
            Some(self)
        } else {
            None
        }
    }

    fn start(&mut self, secret_files: &[String]) -> Result<Option<Box<dyn FnOnce()>>> {
        if !self.is_available() {
            tracing::debug!("podman not available");
            return Ok(None);
        }

        let socket_path = self.podman_socket_path();

        let stop = crate::tools::podman::proxy::start_proxy(
            &self.proxy_socket,
            &socket_path,
            secret_files.to_vec(),
        )?;

        tracing::info!(socket = %self.proxy_socket, "podman proxy started");

        for i in 0..20 {
            if Path::new(&self.proxy_socket).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            if i == 19 {
                tracing::warn!("proxy socket did not appear in time");
            }
        }

        let proxy_socket = self.proxy_socket.clone();
        let shutdown: Box<dyn FnOnce()> = Box::new(move || {
            stop();
            let _ = std::fs::remove_file(&proxy_socket);
        });

        Ok(Some(shutdown))
    }
}

impl CapabilitySource for PodmanTool {
    fn mounts(&self) -> Vec<Mount> {
        vec![Mount::rw(&self.proxy_socket, "/tmp/podman-proxy.sock")]
    }

    fn env(&self) -> HashMap<String, EnvValue> {
        let mut env = HashMap::new();
        env.insert(
            "CONTAINER_HOST".into(),
            EnvValue::Static("unix:///tmp/podman-proxy.sock".into()),
        );
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yml::Value;

    fn temp_rundir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_capabilities_disabled() {
        let dir = temp_rundir();
        let rundir = dir.path().to_str().unwrap();
        let tool = PodmanTool::new(serde_yml::from_str("enabled: false").unwrap(), rundir);
        assert!(tool.capabilities().is_none());
    }

    #[test]
    fn test_capabilities_no_socket() {
        let dir = temp_rundir();
        let rundir = dir.path().to_str().unwrap();
        let tool = PodmanTool::new(
            serde_yml::from_str("socket_path: /tmp/nonexistent.sock").unwrap(),
            rundir,
        );
        assert!(tool.capabilities().is_none());
    }

    #[test]
    fn test_capabilities_malformed_config() {
        let dir = temp_rundir();
        let rundir = dir.path().to_str().unwrap();
        let tool = PodmanTool::new(Value::String("not a mapping".to_string()), rundir);
        assert!(tool.capabilities().is_none());
    }

    #[test]
    fn test_start_returns_none_when_unavailable() {
        let dir = temp_rundir();
        let rundir = dir.path().to_str().unwrap();
        let mut tool = PodmanTool::new(
            serde_yml::from_str("socket_path: /tmp/nonexistent.sock").unwrap(),
            rundir,
        );
        let result = tool.start(&[]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_capabilities_returns_self_when_available() {
        let dir = temp_rundir();
        let xdg_runtime = dir.path().to_str().unwrap();
        let podman_socket = format!("{xdg_runtime}/podman/podman.sock");
        std::fs::create_dir_all(format!("{xdg_runtime}/podman")).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(&podman_socket).unwrap();

        let old_xdg = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());

        let rundir_dir = temp_rundir();
        let rundir = rundir_dir.path().to_str().unwrap();
        let tool = PodmanTool::new(serde_yml::from_str("enabled: true").unwrap(), rundir);
        assert!(tool.capabilities().is_some());

        match old_xdg {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
    }

    #[test]
    fn test_mounts_and_env_use_deterministic_path() {
        let dir = temp_rundir();
        let rundir = dir.path().to_str().unwrap();
        let tool = PodmanTool::new(serde_yml::Value::Null, rundir);

        let mounts = tool.mounts();
        assert_eq!(mounts.len(), 1);
        assert!(mounts[0].host.contains("proxy.sock"));
        assert_eq!(mounts[0].target, "/tmp/podman-proxy.sock");

        let env = tool.env();
        let container_host = env.get("CONTAINER_HOST").unwrap();
        match container_host {
            EnvValue::Static(v) => assert_eq!(v, "unix:///tmp/podman-proxy.sock"),
            _ => panic!("expected static CONTAINER_HOST"),
        }
    }
}
