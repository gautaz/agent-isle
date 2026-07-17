use anyhow::Result;

use super::Config;

/// Validate that a path is absolute.
pub fn validate_absolute(path: &str, name: &str) -> Result<()> {
    if !path.starts_with('/') {
        anyhow::bail!("{name} must be an absolute path, got: {path}");
    }
    Ok(())
}

/// Validate that an agent name contains only valid characters.
pub fn validate_agent_name(name: &str) -> Result<()> {
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

/// Validate the complete config: paths, agent names, and binaries.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_sources::CapabilitySource;
    use crate::config::{default, AgentConfig, ConfigSource, EnvValue, SandboxConfig};
    use crate::sandbox::Mount;

    #[test]
    fn test_validate_absolute() {
        assert!(validate_absolute("/usr/bin/bwrap", "bwrap_path").is_ok());
        assert!(validate_absolute("relative/path", "bwrap_path").is_err());
        assert!(validate_absolute("", "bwrap_path").is_err());
    }

    #[test]
    fn test_validate_agent_name() {
        assert!(validate_agent_name("opencode").is_ok());
        assert!(validate_agent_name("").is_err());
        assert!(validate_agent_name("has/slash").is_err());
        assert!(validate_agent_name("has\0null").is_err());
    }

    #[test]
    fn test_validate_happy_path() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        cfg.agent = "opencode".to_string();
        cfg.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/opencode".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn test_validate_no_agent_selected() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        // agent is "" by default — no selected agent to validate.
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn test_validate_relative_bwrap_path() {
        let mut cfg = default();
        cfg.bwrap_path = "relative/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        let err = validate(&cfg).unwrap_err().to_string();
        assert!(err.contains("bwrap_path"));
    }

    #[test]
    fn test_validate_relative_betterleaks_path() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "relative/betterleaks".to_string();
        let err = validate(&cfg).unwrap_err().to_string();
        assert!(err.contains("betterleaks_path"));
    }

    #[test]
    fn test_validate_agent_not_in_map() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        cfg.agent = "nonexistent".to_string();
        let err = validate(&cfg).unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_validate_empty_binary() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        cfg.agent = "opencode".to_string();
        cfg.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let err = validate(&cfg).unwrap_err().to_string();
        assert!(err.contains("missing required"));
    }

    #[test]
    fn test_validate_relative_binary() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        cfg.agent = "opencode".to_string();
        cfg.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "relative/opencode".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let err = validate(&cfg).unwrap_err().to_string();
        assert!(err.contains("agent binary"));
    }

    #[test]
    fn test_validate_invalid_agent_name() {
        let mut cfg = default();
        cfg.bwrap_path = "/usr/bin/bwrap".to_string();
        cfg.betterleaks_path = "/usr/bin/betterleaks".to_string();
        cfg.agents.insert(
            "has/slash".to_string(),
            AgentConfig {
                binary: "/usr/bin/agent".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let err = validate(&cfg).unwrap_err().to_string();
        assert!(err.contains("/"));
    }

    #[test]
    fn test_agent_source_mounts() {
        use crate::config::MountConfig;
        use crate::sandbox::MountMode;

        let agent = AgentConfig {
            mounts: vec![
                MountConfig::new("/ro/path"),
                MountConfig {
                    path: "/rw/path".to_string(),
                    mode: MountMode::Rw,
                    ..MountConfig::new("")
                },
            ],
            env: [("KEY".to_string(), EnvValue::Static("val".to_string()))].into(),
            ..Default::default()
        };
        let source = crate::config::AgentSource::new(&agent);
        let mounts = source.mounts();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].mode, MountMode::Ro);
        assert_eq!(mounts[0].host, "/ro/path");
        assert_eq!(mounts[1].mode, MountMode::Rw);
        assert_eq!(mounts[1].host, "/rw/path");
    }

    #[test]
    fn test_agent_source_env() {
        let agent = AgentConfig {
            env: [("MY_VAR".to_string(), EnvValue::Static("hello".to_string()))].into(),
            ..Default::default()
        };
        let source = crate::config::AgentSource::new(&agent);
        let env = source.env();
        assert_eq!(env.get("MY_VAR").unwrap().resolve().unwrap(), "hello");
    }

    #[test]
    fn test_config_source_mounts() {
        let sb = SandboxConfig {
            agent: AgentConfig::default(),
            chdir: "/".to_string(),
            mounts: vec![Mount::ro("/data", "/data")],
            env: std::collections::HashMap::new(),
        };
        let source = ConfigSource::new(sb);
        let mounts = source.mounts();
        assert!(mounts.iter().any(|m| m.host == "/data"));
    }
}
