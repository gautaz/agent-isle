use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;

use super::{AgentConfig, Config, EnvValue, MountConfig};
use crate::sandbox::{MountMode, SecretsPolicy};

const OPENCODE_DEFAULT_PATH: &str = match option_env!("OPENCODE_PATH") {
    Some(p) => p,
    None => "",
};

/// A bundled agent preset defined as static data.
struct Preset {
    binary: &'static str,
    mounts: &'static [(&'static str, MountMode)],
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
            mounts: &[
                ("{home}/.config/opencode", MountMode::Ro),
                ("{home}/.local/share/opencode", MountMode::Rw),
                ("{home}/.local/state/opencode", MountMode::Rw),
            ],
            env: &[],
            lightweight_args: &["--help", "-h", "--version", "-v"],
        },
    );
    // @end:preset-example
    m
});

/// Apply a bundled agent preset to the config.
/// If the agent is already defined in `agents:`, the user definition takes precedence.
pub fn apply_preset(name: &str, cfg: &mut Config) -> Result<()> {
    if cfg.agents.contains_key(name) {
        return Ok(());
    }

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
            mounts: preset
                .mounts
                .iter()
                .map(|(s, mode)| MountConfig {
                    path: s.to_string(),
                    target: None,
                    mode: *mode,
                    secrets_policy: SecretsPolicy::Mask,
                })
                .collect(),
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
            ..Default::default()
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

    fn default_cfg() -> Config {
        super::super::default()
    }

    #[test]
    fn test_apply_preset() {
        let mut cfg = default_cfg();
        apply_preset("opencode", &mut cfg).unwrap();
        let agent = cfg.agents.get("opencode").unwrap();
        assert!(agent.binary.is_empty() || agent.binary.starts_with('/'));
        assert!(!agent.mounts.is_empty());
    }

    #[test]
    fn test_apply_preset_preserves_user_agent() {
        let mut cfg = default_cfg();
        cfg.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/bin/sh".to_string(),
                lightweight_args: vec!["-c".to_string()],
                ..Default::default()
            },
        );
        apply_preset("opencode", &mut cfg).unwrap();
        let agent = cfg.agents.get("opencode").unwrap();
        assert_eq!(agent.binary, "/bin/sh");
        assert_eq!(agent.lightweight_args, &["-c"]);
    }

    #[test]
    fn test_apply_preset_accepts_custom_name_with_inline_agent() {
        let mut cfg = default_cfg();
        cfg.agents.insert(
            "my-custom-agent".to_string(),
            AgentConfig {
                binary: "/bin/sh".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        apply_preset("my-custom-agent", &mut cfg).unwrap();
        let agent = cfg.agents.get("my-custom-agent").unwrap();
        assert_eq!(agent.binary, "/bin/sh");
    }

    #[test]
    fn test_apply_preset_unknown() {
        let mut cfg = default_cfg();
        assert!(apply_preset("nonexistent", &mut cfg).is_err());
    }

    #[test]
    fn test_list_presets() {
        let presets = list_presets();
        assert!(presets.contains(&"opencode"));
    }
}
