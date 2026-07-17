use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde::de;
use serde::Deserialize;

/// Compile-time default for bwrap path. Set via `BWRAP_PATH` env var at build time.
const BWRAP_DEFAULT_PATH: &str = match option_env!("BWRAP_PATH") {
    Some(p) => p,
    None => "",
};

/// Compile-time default for betterleaks path. Set via `BETTERLEAKS_PATH` env var at build time.
const BETTERLEAKS_DEFAULT_PATH: &str = match option_env!("BETTERLEAKS_PATH") {
    Some(p) => p,
    None => "",
};

/// Compile-time default for opencode path. Set via `OPENCODE_PATH` env var at build time.
const OPENCODE_DEFAULT_PATH: &str = match option_env!("OPENCODE_PATH") {
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

/// AgentConfig holds sandbox configuration for a single agent.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub binary: String,
    #[serde(default, rename = "ro_mounts")]
    pub ro_mounts: Vec<String>,
    #[serde(default, rename = "rw_mounts")]
    pub rw_mounts: Vec<String>,
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

/// Custom deserializer for tool paths (`bwrap_path`, `betterleaks_path`).
/// When the compile-time default is set, missing key uses it.
/// When the compile-time default is empty, missing key triggers a deserialization error.
fn require_bwrap_path<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> de::Visitor<'de> for Visitor {
        type Value = String;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an absolute path to the bwrap binary")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            if BWRAP_DEFAULT_PATH.is_empty() {
                Err(E::custom(
                    "bwrap_path not set: configure in config YAML or set BWRAP_PATH at build time",
                ))
            } else {
                Ok(BWRAP_DEFAULT_PATH.to_string())
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

fn require_betterleaks_path<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> de::Visitor<'de> for Visitor {
        type Value = String;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an absolute path to the betterleaks binary")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            if BETTERLEAKS_DEFAULT_PATH.is_empty() {
                Err(E::custom("betterleaks_path not set: configure in config YAML or set BETTERLEAKS_PATH at build time"))
            } else {
                Ok(BETTERLEAKS_DEFAULT_PATH.to_string())
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
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default, rename = "ro_mounts")]
    pub ro_mounts: Vec<String>,
    #[serde(default, rename = "rw_mounts")]
    pub rw_mounts: Vec<String>,
    #[serde(default, rename = "env")]
    pub env: HashMap<String, EnvValue>,
    #[serde(default)]
    pub tools: ToolsConfig,

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

/// Validate that a path is absolute.
fn validate_absolute(path: &str, name: &str) -> Result<()> {
    if !path.starts_with('/') {
        anyhow::bail!("{name} must be an absolute path, got: {path}");
    }
    Ok(())
}

/// Validate that an agent name contains only valid characters.
fn validate_agent_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("agent name must not be empty");
    }
    if name.contains('/') {
        anyhow::bail!("agent name {name:?} must not contain '/'");
    }
    if name.contains('\0') {
        anyhow::bail!("agent name {name:?} must not contain null bytes");
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub podman: PodmanConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PodmanConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub socket_path: Option<String>,
}

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
    Config {
        agents: HashMap::new(),
        bwrap_path: BWRAP_DEFAULT_PATH.to_string(),
        betterleaks_path: BETTERLEAKS_DEFAULT_PATH.to_string(),
        ..Default::default()
    }
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

/// Validate agent names, binaries, and absolute paths.
pub fn validate(cfg: &Config) -> Result<()> {
    validate_absolute(&cfg.bwrap_path, "bwrap_path")?;
    validate_absolute(&cfg.betterleaks_path, "betterleaks_path")?;
    // Validate agent names and binaries.
    for (name, agent_cfg) in &cfg.agents {
        validate_agent_name(name)?;
        if agent_cfg.binary.is_empty() {
            anyhow::bail!("agent {name:?} missing required \"binary\" field");
        }
        validate_absolute(&agent_cfg.binary, "agent binary")?;
    }
    // Validate the selected agent's binary.
    if !cfg.agent.is_empty() {
        match cfg.agents.get(&cfg.agent) {
            Some(agent_cfg) => {
                if agent_cfg.binary.is_empty() {
                    anyhow::bail!("agent {:?} missing required \"binary\" field", cfg.agent);
                }
                validate_absolute(&agent_cfg.binary, "agent binary")?;
            }
            None => {
                anyhow::bail!(
                    "agent {:?} not found in agents map — \
                     add it to \"agents:\" in config or choose a bundled preset",
                    cfg.agent
                );
            }
        }
    }
    Ok(())
}

/// Merge applies override values on top of base.
///
/// Scalar fields (agent, tool paths) are replaced by the override.
/// List fields (ro_mounts, rw_mounts) are **appended**, not replaced.
/// Env map entries are merged (override keys win).
pub fn merge(base: &Config, extra: Option<&Config>) -> Config {
    let Some(extra) = extra else {
        return base.clone();
    };

    let mut out = base.clone();

    if !extra.agent.is_empty() {
        out.agent = extra.agent.clone();
    }

    out.ro_mounts.extend(extra.ro_mounts.iter().cloned());
    out.rw_mounts.extend(extra.rw_mounts.iter().cloned());
    for (k, v) in &extra.env {
        out.env.insert(k.clone(), v.clone());
    }

    for (name, agent_cfg) in &extra.agents {
        if let Some(existing) = out.agents.get_mut(name) {
            if !agent_cfg.binary.is_empty() {
                existing.binary = agent_cfg.binary.clone();
            }
            existing
                .ro_mounts
                .extend(agent_cfg.ro_mounts.iter().cloned());
            existing
                .rw_mounts
                .extend(agent_cfg.rw_mounts.iter().cloned());
            for (k, v) in &agent_cfg.env {
                existing.env.insert(k.clone(), v.clone());
            }
            if !agent_cfg.lightweight_args.is_empty() {
                existing.lightweight_args = agent_cfg.lightweight_args.clone();
            }
        } else {
            out.agents.insert(name.clone(), agent_cfg.clone());
        }
    }

    if let Some(enabled) = extra.tools.podman.enabled {
        out.tools.podman.enabled = Some(enabled);
    }

    out.bwrap_path = extra.bwrap_path.clone();
    out.betterleaks_path = extra.betterleaks_path.clone();

    out
}

/// Template vars regex: {var}
#[allow(clippy::expect_used)]
static TEMPLATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{(\w+)\}").expect("valid regex"));

/// Expand template variables in a string using the given TemplateVars.
fn expand_string(s: &str, v: &TemplateVars) -> Result<String> {
    for caps in TEMPLATE_RE.captures_iter(s) {
        let name = caps.get(1).map_or(s, |m| m.as_str());
        match name {
            "home" | "user" | "cwd" | "xdg_runtime" | "xdg_state" | "name" | "log_path" => {}
            unknown => {
                anyhow::bail!("unknown template variable {{{unknown}}} in configuration");
            }
        }
    }
    Ok(TEMPLATE_RE
        .replace_all(s, |caps: &regex::Captures| {
            caps.get(1).map_or_else(
                || s.to_string(),
                |m| match m.as_str() {
                    "home" => v.home.clone(),
                    "user" => v.user.clone(),
                    "cwd" => v.cwd.clone(),
                    "xdg_runtime" => v.xdg_runtime.clone(),
                    "xdg_state" => v.xdg_state.clone(),
                    "log_path" => v.log_path.clone(),
                    _ => m.as_str().to_string(),
                },
            )
        })
        .to_string())
}

/// expand_env_value expands templates in an EnvValue.
fn expand_env_value(ev: &EnvValue, v: &TemplateVars) -> Result<EnvValue> {
    match ev {
        EnvValue::Static(s) => Ok(EnvValue::Static(expand_string(s, v)?)),
        EnvValue::Command { command } => Ok(EnvValue::Command {
            command: expand_string(command, v)?,
        }),
    }
}

/// Expand templates in mount list and env map.
fn expand_mounts(
    mounts: Vec<crate::sandbox::Mount>,
    env: HashMap<String, EnvValue>,
    v: &TemplateVars,
) -> Result<(Vec<crate::sandbox::Mount>, HashMap<String, EnvValue>)> {
    let expanded_mounts = mounts
        .into_iter()
        .map(|m| {
            Ok(crate::sandbox::Mount {
                host: expand_string(&m.host, v)?,
                target: expand_string(&m.target, v)?,
                mode: m.mode,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let expanded_env = env
        .into_iter()
        .map(|(k, val)| Ok((expand_string(&k, v)?, expand_env_value(&val, v)?)))
        .collect::<Result<HashMap<_, _>>>()?;
    Ok((expanded_mounts, expanded_env))
}

/// Expanded agent mounts (host=target strings) and environment.
type ExpandedAgent = (Vec<String>, Vec<String>, HashMap<String, EnvValue>);

/// Expand templates in agent mount lists (Vec<String>) and env map.
/// Converts strings to typed Mounts with the appropriate mode.
fn expand_agent_mounts(
    ro_mounts: Vec<String>,
    rw_mounts: Vec<String>,
    env: HashMap<String, EnvValue>,
    v: &TemplateVars,
) -> Result<ExpandedAgent> {
    let ro = ro_mounts
        .into_iter()
        .map(|p| expand_string(&p, v))
        .collect::<Result<Vec<_>>>()?;
    let rw = rw_mounts
        .into_iter()
        .map(|p| expand_string(&p, v))
        .collect::<Result<Vec<_>>>()?;
    let expanded = env
        .into_iter()
        .map(|(k, val)| Ok((expand_string(&k, v)?, expand_env_value(&val, v)?)))
        .collect::<Result<HashMap<_, _>>>()?;
    Ok((ro, rw, expanded))
}

/// Complete data package needed to build a sandbox.
///
/// Created by `expand_vars` from raw config values after template expansion.
/// Consumed by `sandbox::build_args` to construct bwrap arguments.
#[derive(Debug)]
pub struct SandboxConfig {
    pub agent: AgentConfig,
    pub mounts: Vec<crate::sandbox::Mount>,
    pub env: HashMap<String, EnvValue>,
}

/// Expand template variables in config string fields.
///
/// Takes a `SandboxConfig` with raw config values, returns one with all
/// `{var}` placeholders expanded. Template expansion is applied only to
/// the data that will be used downstream for sandbox construction.
pub fn expand_vars(input: SandboxConfig, vars: &TemplateVars) -> Result<SandboxConfig> {
    let (mounts, env) = expand_mounts(input.mounts, input.env, vars)?;

    let agent = AgentConfig {
        binary: expand_string(&input.agent.binary, vars)?,
        ro_mounts: input.agent.ro_mounts,
        rw_mounts: input.agent.rw_mounts,
        env: input.agent.env,
        lightweight_args: input.agent.lightweight_args,
    };
    let (agent_ro, agent_rw, agent_env) =
        expand_agent_mounts(agent.ro_mounts, agent.rw_mounts, agent.env, vars)?;

    let agent = AgentConfig {
        ro_mounts: agent_ro,
        rw_mounts: agent_rw,
        env: agent_env,
        ..agent
    };

    Ok(SandboxConfig { agent, mounts, env })
}

/// A bundled agent preset defined as static data.
struct Preset {
    binary: &'static str,
    ro_mounts: &'static [&'static str],
    rw_mounts: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
    lightweight_args: &'static [&'static str],
}

/// Single source of truth for all bundled presets.
/// To add a new preset, insert a new entry here.
static PRESETS: LazyLock<HashMap<&'static str, Preset>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // @section:preset-example
    m.insert(
        "opencode",
        Preset {
            binary: OPENCODE_DEFAULT_PATH,
            ro_mounts: &[
                "{home}/.config/opencode",
                "{home}/.local/share/opencode",
                "{home}/.local/state/opencode",
            ],
            rw_mounts: &[],
            env: &[],
            lightweight_args: &["--help", "-h", "--version", "-v"],
        },
    );
    // @end:preset-example
    m
});

/// Apply a bundled agent preset to the config.
pub fn apply_preset(name: &str, cfg: &mut Config) -> Result<()> {
    let preset = match PRESETS.get(name) {
        Some(p) => p,
        None => {
            let available = list_presets().join(", ");
            anyhow::bail!("unknown agent preset {name:?} (available: {available})")
        }
    };

    cfg.agents.insert(
        name.to_string(),
        AgentConfig {
            binary: preset.binary.to_string(),
            ro_mounts: preset.ro_mounts.iter().map(|s| s.to_string()).collect(),
            rw_mounts: preset.rw_mounts.iter().map(|s| s.to_string()).collect(),
            env: preset
                .env
                .iter()
                .map(|(k, v)| (k.to_string(), EnvValue::Static(v.to_string())))
                .collect(),
            lightweight_args: preset
                .lightweight_args
                .iter()
                .map(|s| s.to_string())
                .collect(),
        },
    );
    Ok(())
}

/// List all available preset names.
pub fn list_presets() -> Vec<&'static str> {
    PRESETS.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    fn test_vars() -> TemplateVars {
        TemplateVars {
            home: "/home/testuser".to_string(),
            user: "testuser".to_string(),
            cwd: "/project".to_string(),
            xdg_runtime: "/tmp".to_string(),
            xdg_state: "/home/testuser/.local/state".to_string(),
            log_path: "/tmp/logs/app.log".to_string(),
        }
    }

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
                ro_mounts:
                  - /common/ro/path
                rw_mounts:
                  - /common/rw/path
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
                    ro_mounts:
                      - /custom/path
                    env:
                      MY_VAR: hello"},
        )
        .unwrap();

        let cfg = load(path.to_str().unwrap()).unwrap();
        assert_eq!(cfg.agent, "aider");
        assert_eq!(cfg.ro_mounts, vec!["/common/ro/path"]);
        assert_eq!(cfg.rw_mounts, vec!["/common/rw/path"]);
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
        assert_eq!(aider.ro_mounts, vec!["/custom/path"]);
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
    fn test_merge() {
        let mut base = default();
        base.ro_mounts = vec!["/base/common/ro".to_string()];
        base.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/opencode".to_string(),
                ro_mounts: vec!["/base/path".to_string()],
                lightweight_args: vec![],
                ..Default::default()
            },
        );

        let mut extra = default();
        extra.ro_mounts = vec!["/override/common/ro".to_string()];
        extra.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/override".to_string(),
                ro_mounts: vec!["/extra/path".to_string()],
                lightweight_args: vec![],
                ..Default::default()
            },
        );

        let merged = merge(&base, Some(&extra));
        assert_eq!(merged.ro_mounts.len(), 2);
        let merged_agent = merged.agents.get("opencode").unwrap();
        assert_eq!(merged_agent.binary, "/usr/bin/override");
        assert_eq!(merged_agent.ro_mounts.len(), 2);
    }

    #[test]
    fn test_merge_nil() {
        let base = default();
        let merged = merge(&base, None);
        assert_eq!(merged.agent, base.agent);
    }

    #[test]
    fn test_merge_env() {
        let mut base = default();
        base.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "opencode".to_string(),
                env: [
                    ("A".to_string(), EnvValue::Static("1".to_string())),
                    ("B".to_string(), EnvValue::Static("2".to_string())),
                ]
                .into(),
                ..Default::default()
            },
        );

        let mut extra = default();
        extra.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                env: [
                    ("B".to_string(), EnvValue::Static("override".to_string())),
                    ("C".to_string(), EnvValue::Static("3".to_string())),
                ]
                .into(),
                ..Default::default()
            },
        );

        let merged = merge(&base, Some(&extra));
        let agent = merged.agents.get("opencode").unwrap();
        assert_eq!(agent.env.get("A").unwrap().resolve().unwrap(), "1");
        assert_eq!(agent.env.get("B").unwrap().resolve().unwrap(), "override");
        assert_eq!(agent.env.get("C").unwrap().resolve().unwrap(), "3");
    }

    #[test]
    fn test_expand_vars() {
        use crate::sandbox::Mount;
        let input = SandboxConfig {
            agent: AgentConfig {
                binary: "opencode".to_string(),
                ro_mounts: vec!["{home}/.config/app".to_string()],
                env: [(
                    "LOG".to_string(),
                    EnvValue::Static("{log_path}".to_string()),
                )]
                .into(),
                ..Default::default()
            },
            mounts: vec![Mount::ro("{home}/.config/common", "{home}/.config/common")],
            env: [(
                "COMMON_LOG".to_string(),
                EnvValue::Static("{log_path}".to_string()),
            )]
            .into(),
        };

        let result = expand_vars(input, &test_vars()).unwrap();

        assert_eq!(result.mounts[0].host, "/home/testuser/.config/common");
        assert_eq!(result.mounts[0].target, "/home/testuser/.config/common");
        assert_eq!(
            result.env.get("COMMON_LOG").unwrap().resolve().unwrap(),
            "/tmp/logs/app.log"
        );
        assert_eq!(result.agent.ro_mounts[0], "/home/testuser/.config/app");
        assert_eq!(
            result.agent.env.get("LOG").unwrap().resolve().unwrap(),
            "/tmp/logs/app.log"
        );
    }

    #[test]
    fn test_apply_preset() {
        let mut cfg = default();
        apply_preset("opencode", &mut cfg).unwrap();
        let agent = cfg.agents.get("opencode").unwrap();
        // Binary is empty when OPENCODE_PATH is not set at compile time
        assert!(agent.binary.is_empty() || agent.binary.starts_with('/'));
        assert!(!agent.ro_mounts.is_empty());
    }

    #[test]
    fn test_apply_preset_unknown() {
        let mut cfg = default();
        assert!(apply_preset("nonexistent", &mut cfg).is_err());
    }

    #[test]
    fn test_list_presets() {
        let presets = list_presets();
        assert!(presets.contains(&"opencode"));
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
    fn test_merge_tool_path_overrides() {
        let mut base = default();
        base.bwrap_path = "/usr/bin/bwrap".to_string();
        base.betterleaks_path = "/usr/bin/betterleaks".to_string();

        let mut extra = default();
        extra.bwrap_path = "/custom/bwrap".to_string();
        extra.betterleaks_path = "/custom/betterleaks".to_string();

        let merged = merge(&base, Some(&extra));
        assert_eq!(merged.bwrap_path, "/custom/bwrap");
        assert_eq!(merged.betterleaks_path, "/custom/betterleaks");
    }

    #[test]
    fn test_merge_podman_enabled() {
        let base = default();
        let mut extra = default();
        extra.tools.podman.enabled = Some(true);

        let merged = merge(&base, Some(&extra));
        assert_eq!(merged.tools.podman.enabled, Some(true));
    }

    #[test]
    fn test_expand_env_value_command() {
        let result = expand_vars(
            SandboxConfig {
                agent: AgentConfig::default(),
                mounts: vec![],
                env: [(
                    "CMD_VAR".to_string(),
                    EnvValue::Command {
                        command: "echo expanded".to_string(),
                    },
                )]
                .into(),
            },
            &test_vars(),
        )
        .unwrap();

        assert_eq!(
            result.env.get("CMD_VAR").unwrap().resolve().unwrap(),
            "expanded"
        );
    }

    #[test]
    fn test_expand_unknown_template_variable() {
        use crate::sandbox::Mount;
        let result = expand_vars(
            SandboxConfig {
                agent: AgentConfig::default(),
                mounts: vec![Mount::ro("{unknown_var}/data", "{unknown_var}/data")],
                env: HashMap::new(),
            },
            &test_vars(),
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown template variable"),);
    }
}
