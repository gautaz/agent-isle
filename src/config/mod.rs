mod merge;
mod presets;
mod sources;
pub mod template;
mod validate;

use std::collections::HashMap;
use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use serde::de;
use serde::Deserialize;

use crate::sandbox::{Mount, MountMode, SecretsPolicy};

/// Compile-time default for bwrap path. Set via `BWRAP_PATH` env var at build time.
pub(crate) const BWRAP_DEFAULT_PATH: &str = match option_env!("BWRAP_PATH") {
    Some(p) => p,
    None => "",
};

/// Compile-time default for betterleaks path. Set via `BETTERLEAKS_PATH` env var at build time.
pub(crate) const BETTERLEAKS_DEFAULT_PATH: &str = match option_env!("BETTERLEAKS_PATH") {
    Some(p) => p,
    None => "",
};

/// EnvValue holds either a static string or a shell command to execute.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    Static(String),
    Command { command: String },
}

impl EnvValue {
    pub fn resolve(&self) -> Result<String> {
        match self {
            EnvValue::Static(s) => Ok(s.clone()),
            EnvValue::Command { command } => {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .with_context(|| format!("execute command: {command}"))?;
                if !output.status.success() {
                    let code = output.status.code().unwrap_or(-1);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("command failed (exit code {code}): {command}\n{stderr}");
                }
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
        }
    }
}

fn default_mount_mode() -> MountMode {
    MountMode::Ro
}

/// Mount configuration from YAML.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MountConfig {
    /// Host path to mount.
    pub path: String,
    /// Target path inside the sandbox (defaults to `path` if omitted).
    #[serde(default)]
    pub target: Option<String>,
    /// Mount mode: "ro" (read-only) or "rw" (read-write).
    #[serde(default = "default_mount_mode")]
    pub mode: MountMode,
    /// How to handle secrets in this mount.
    #[serde(default = "default_secrets_policy")]
    pub secrets_policy: SecretsPolicy,
}

fn default_secrets_policy() -> SecretsPolicy {
    SecretsPolicy::Mask
}

impl MountConfig {
    /// Create a new MountConfig with the given path (no separate target, default mask policy, read-only).
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            target: None,
            mode: MountMode::Ro,
            secrets_policy: SecretsPolicy::Mask,
        }
    }

    /// Convert to a sandbox Mount.
    pub fn to_mount(&self) -> Mount {
        let target = self.target.clone().unwrap_or_else(|| self.path.clone());
        Mount {
            host: self.path.clone(),
            target,
            mode: self.mode,
            secrets_policy: self.secrets_policy,
        }
    }
}

/// AgentConfig holds sandbox configuration for a single agent.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub binary: String,
    #[serde(default)]
    pub chdir: String,
    #[serde(default, rename = "mounts")]
    pub mounts: Vec<MountConfig>,
    #[serde(default, rename = "env")]
    pub env: HashMap<String, EnvValue>,
    #[serde(
        rename = "lightweight_args",
        deserialize_with = "require_lightweight_args"
    )]
    pub lightweight_args: Vec<String>,
}

/// Custom deserializer: `lightweight_args` is mandatory in YAML.
/// Missing key triggers a deserialization error immediately.
fn require_lightweight_args<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> de::Visitor<'de> for Visitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a list of command-line flags (lightweight_args)")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Err(E::missing_field("lightweight_args"))
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Err(E::missing_field("lightweight_args"))
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(elem) = seq.next_element::<String>()? {
                v.push(elem);
            }
            Ok(v)
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// Custom deserializer for tool paths. Missing key uses compile-time default or errors.
macro_rules! require_tool_path {
    ($name:ident, $default:expr, $field:expr, $expecting:expr) => {
        fn $name<'de, D>(deserializer: D) -> Result<String, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            struct Visitor;
            impl<'de> de::Visitor<'de> for Visitor {
                type Value = String;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str($expecting)
                }
                fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                    if $default.is_empty() {
                        Err(E::custom(concat!(
                            $field,
                            " not set: configure in config YAML or set ",
                            $field,
                            " at build time"
                        )))
                    } else {
                        Ok($default.to_string())
                    }
                }
                fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                    self.visit_none()
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                    Ok(v.to_string())
                }
                fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                    Ok(v)
                }
            }
            deserializer.deserialize_any(Visitor)
        }
    };
}

require_tool_path!(
    require_bwrap_path,
    BWRAP_DEFAULT_PATH,
    "bwrap_path",
    "an absolute path to the bwrap binary"
);
require_tool_path!(
    require_betterleaks_path,
    BETTERLEAKS_DEFAULT_PATH,
    "betterleaks_path",
    "an absolute path to the betterleaks binary"
);

impl AgentConfig {
    /// Check whether the given args trigger lightweight mode for this agent.
    pub fn is_lightweight_op(&self, args: &[String]) -> bool {
        if self.lightweight_args.is_empty() {
            return false;
        }
        args.iter()
            .any(|a| self.lightweight_args.iter().any(|f| a == f))
    }
}

/// Config holds the complete agent-isle configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub agent: String,
    pub agents: HashMap<String, AgentConfig>,
    pub chdir: String,
    #[serde(rename = "mounts")]
    pub mounts: Vec<MountConfig>,
    #[serde(rename = "env")]
    pub env: HashMap<String, EnvValue>,
    pub tools: ToolsConfig,

    /// Secrets policy for PATH-derived mounts. Default: mask.
    pub path_secrets_policy: SecretsPolicy,

    /// Absolute path to bwrap binary. Set at compile time or via config.
    #[serde(rename = "bwrap_path", deserialize_with = "require_bwrap_path")]
    pub bwrap_path: String,

    /// Absolute path to betterleaks binary. Set at compile time or via config.
    #[serde(
        rename = "betterleaks_path",
        deserialize_with = "require_betterleaks_path"
    )]
    pub betterleaks_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            agent: String::new(),
            agents: HashMap::new(),
            chdir: String::new(),
            mounts: Vec::new(),
            env: HashMap::new(),
            tools: serde_yml::Mapping::new(),
            path_secrets_policy: SecretsPolicy::Mask,
            bwrap_path: BWRAP_DEFAULT_PATH.to_string(),
            betterleaks_path: BETTERLEAKS_DEFAULT_PATH.to_string(),
        }
    }
}

/// Generic tool configuration — each tool owns its own section.
pub type ToolsConfig = serde_yml::Mapping;

/// Template variables available for expansion in config values.
pub struct TemplateVars {
    pub home: String,
    pub user: String,
    pub cwd: String,
    pub xdg_runtime: String,
    pub xdg_state: String,
    pub log_path: String,
}

/// Default config with sensible defaults.
pub fn default() -> Config {
    Config::default()
}

/// Load reads a YAML config file. If path is empty, returns defaults.
pub fn load(path: &str) -> Result<Config> {
    if path.is_empty() {
        return Ok(default());
    }
    let data = fs::read_to_string(path).with_context(|| format!("read config: {path}"))?;
    let cfg: Config =
        serde_yml::from_str(&data).with_context(|| format!("parse config: {path}"))?;
    Ok(cfg)
}

/// Re-export validate for callers.
pub use validate::validate;

/// Re-export merge for callers.
pub use merge::merge;

/// Complete data package needed to build a sandbox.
///
/// Created by `expand_vars` from raw config values after template expansion.
/// Consumed by `sandbox::build_args` to construct bwrap arguments.
#[derive(Debug)]
pub struct SandboxConfig {
    pub agent: AgentConfig,
    pub chdir: String,
    pub mounts: Vec<crate::sandbox::Mount>,
    pub env: HashMap<String, EnvValue>,
}

/// Re-export template expansion for callers.
pub use template::expand_vars;

/// Re-export preset functions for callers.
pub use presets::{apply_preset, list_presets};

/// Re-export capability sources for callers.
pub use sources::{AgentSource, ConfigSource};

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_default() {
        let cfg = default();
        assert!(cfg.agent.is_empty());
    }

    #[test]
    fn test_load_empty() {
        let cfg = load("").unwrap();
        assert!(cfg.agent.is_empty());
    }

    #[test]
    fn test_load_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            indoc! {"\
                agent: aider
                bwrap_path: /usr/bin/bwrap
                betterleaks_path: /usr/bin/betterleaks
                mounts:
                  - path: /common/ro/path
                  - path: /common/rw/path
                    mode: rw
                env:
                  COMMON_VAR: common_value
                  CMD_VAR:
                    command: echo cmd_value
                agents:
                  aider:
                    binary: /usr/bin/aider
                    lightweight_args:
                      - --help
                      - --version
                    mounts:
                      - path: /custom/path
                    env:
                      MY_VAR: hello"},
        )
        .unwrap();

        let cfg = load(path.to_str().unwrap()).unwrap();
        assert_eq!(cfg.agent, "aider");
        assert_eq!(cfg.mounts.len(), 2);
        assert_eq!(cfg.mounts[0], MountConfig::new("/common/ro/path"));
        assert_eq!(cfg.mounts[1].path, "/common/rw/path");
        assert_eq!(cfg.mounts[1].mode, MountMode::Rw);
        assert_eq!(
            cfg.env.get("COMMON_VAR").unwrap().resolve().unwrap(),
            "common_value"
        );
        assert_eq!(
            cfg.env.get("CMD_VAR").unwrap().resolve().unwrap(),
            "cmd_value"
        );

        let aider = cfg.agents.get("aider").unwrap();
        assert_eq!(aider.binary, "/usr/bin/aider");
        assert_eq!(aider.mounts, vec![MountConfig::new("/custom/path")]);
        assert_eq!(aider.env.get("MY_VAR").unwrap().resolve().unwrap(), "hello");
    }

    #[test]
    fn test_load_missing() {
        assert!(load("/nonexistent/config.yaml").is_err());
    }

    #[test]
    fn test_load_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.yaml");
        fs::write(&path, ":::not yaml:::[").unwrap();
        assert!(load(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn test_env_value_command_resolve() {
        let ev = EnvValue::Command {
            command: "echo hello".to_string(),
        };
        assert_eq!(ev.resolve().unwrap(), "hello");
    }

    #[test]
    fn test_env_value_command_resolve_error() {
        let ev = EnvValue::Command {
            command: "exit 1".to_string(),
        };
        assert!(ev.resolve().is_err());
    }

    #[test]
    fn test_is_lightweight_op_empty_args() {
        let agent = AgentConfig {
            lightweight_args: vec![],
            ..Default::default()
        };
        assert!(!agent.is_lightweight_op(&["--help".to_string()]));
    }

    #[test]
    fn test_is_lightweight_op_matching_arg() {
        let agent = AgentConfig {
            lightweight_args: vec!["--help".to_string(), "--version".to_string()],
            ..Default::default()
        };
        assert!(agent.is_lightweight_op(&["--help".to_string()]));
    }

    #[test]
    fn test_is_lightweight_op_non_matching_arg() {
        let agent = AgentConfig {
            lightweight_args: vec!["--help".to_string(), "--version".to_string()],
            ..Default::default()
        };
        assert!(!agent.is_lightweight_op(&["--other".to_string()]));
    }

    #[test]
    fn test_is_lightweight_op_no_args() {
        let agent = AgentConfig {
            lightweight_args: vec!["--help".to_string()],
            ..Default::default()
        };
        assert!(!agent.is_lightweight_op(&[]));
    }
}
