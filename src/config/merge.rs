use super::Config;

/// Merge applies override values on top of base.
///
/// Scalar fields (agent, tool paths) are replaced by the override.
/// List fields (mounts) are **appended**, not replaced.
/// Env map entries are merged (override keys win).
/// Tools config is deep-merged per tool section.
pub fn merge(base: &Config, extra: Option<&Config>) -> Config {
    let Some(extra) = extra else {
        return base.clone();
    };

    let mut out = base.clone();

    if !extra.agent.is_empty() {
        out.agent = extra.agent.clone();
    }

    if !extra.chdir.is_empty() {
        out.chdir = extra.chdir.clone();
    }

    out.mounts.extend(extra.mounts.iter().cloned());
    for (k, v) in &extra.env {
        out.env.insert(k.clone(), v.clone());
    }

    for (name, agent_cfg) in &extra.agents {
        if let Some(existing) = out.agents.get_mut(name) {
            if !agent_cfg.binary.is_empty() {
                existing.binary = agent_cfg.binary.clone();
            }
            if !agent_cfg.chdir.is_empty() {
                existing.chdir = agent_cfg.chdir.clone();
            }
            existing.mounts.extend(agent_cfg.mounts.iter().cloned());
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

    for (k, v) in &extra.tools {
        out.tools.insert(k.clone(), v.clone());
    }

    out.bwrap_path = extra.bwrap_path.clone();
    out.betterleaks_path = extra.betterleaks_path.clone();

    out
}

#[cfg(test)]
mod tests {
    use super::super::{default, merge, AgentConfig, EnvValue, MountConfig};

    #[test]
    fn test_merge() {
        let mut base = default();
        base.mounts = vec![MountConfig::new("/base/common/ro")];
        base.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/opencode".to_string(),
                mounts: vec![MountConfig::new("/base/path")],
                lightweight_args: vec![],
                ..Default::default()
            },
        );

        let mut extra = default();
        extra.mounts = vec![MountConfig::new("/override/common/ro")];
        extra.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/override".to_string(),
                mounts: vec![MountConfig::new("/extra/path")],
                lightweight_args: vec![],
                ..Default::default()
            },
        );

        let merged = merge(&base, Some(&extra));
        assert_eq!(merged.mounts.len(), 2);
        let merged_agent = merged.agents.get("opencode").unwrap();
        assert_eq!(merged_agent.binary, "/usr/bin/override");
        assert_eq!(merged_agent.mounts.len(), 2);
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
    fn test_merge_tools_config() {
        let base = default();
        let mut extra = default();
        let podman_cfg = serde_yml::Mapping::from_iter([(
            serde_yml::Value::String("enabled".to_string()),
            serde_yml::Value::Bool(true),
        )]);
        extra.tools.insert(
            serde_yml::Value::String("podman".to_string()),
            serde_yml::Value::Mapping(podman_cfg),
        );

        let merged = merge(&base, Some(&extra));
        let podman = merged
            .tools
            .get(serde_yml::Value::String("podman".to_string()))
            .unwrap();
        assert_eq!(
            podman
                .get(serde_yml::Value::String("enabled".to_string()))
                .unwrap(),
            &serde_yml::Value::Bool(true)
        );
    }

    // new agent in extra not in base is inserted as-is.
    #[test]
    fn test_merge_new_agent() {
        let base = default();
        let mut extra = default();
        extra.agents.insert(
            "new-agent".to_string(),
            AgentConfig {
                binary: "/usr/bin/new-agent".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let merged = merge(&base, Some(&extra));
        let agent = merged.agents.get("new-agent").unwrap();
        assert_eq!(agent.binary, "/usr/bin/new-agent");
    }

    // empty binary in extra does not overwrite base binary.
    #[test]
    fn test_merge_empty_binary_preserves_base() {
        let mut base = default();
        base.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/opencode".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let mut extra = default();
        extra.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let merged = merge(&base, Some(&extra));
        let agent = merged.agents.get("opencode").unwrap();
        assert_eq!(agent.binary, "/usr/bin/opencode");
    }

    // non-empty lightweight_args in extra replaces base.
    #[test]
    fn test_merge_lightweight_args_override() {
        let mut base = default();
        base.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/opencode".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let mut extra = default();
        extra.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                lightweight_args: vec!["--help".to_string()],
                ..Default::default()
            },
        );
        let merged = merge(&base, Some(&extra));
        let agent = merged.agents.get("opencode").unwrap();
        assert_eq!(agent.lightweight_args, vec!["--help"]);
    }

    // mounts are appended, not replaced.
    #[test]
    fn test_merge_mounts_append() {
        let mut base = default();
        base.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                binary: "/usr/bin/opencode".to_string(),
                mounts: vec![MountConfig::new("/base/rw")],
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let mut extra = default();
        extra.agents.insert(
            "opencode".to_string(),
            AgentConfig {
                mounts: vec![MountConfig::new("/extra/rw")],
                lightweight_args: vec![],
                ..Default::default()
            },
        );
        let merged = merge(&base, Some(&extra));
        let agent = merged.agents.get("opencode").unwrap();
        assert_eq!(
            agent.mounts,
            vec![MountConfig::new("/base/rw"), MountConfig::new("/extra/rw")]
        );
    }

    // top-level env merge: override keys win, new keys added.
    #[test]
    fn test_merge_top_level_env() {
        let mut base = default();
        base.env
            .insert("A".to_string(), EnvValue::Static("1".to_string()));
        let mut extra = default();
        extra
            .env
            .insert("A".to_string(), EnvValue::Static("override".to_string()));
        extra
            .env
            .insert("B".to_string(), EnvValue::Static("2".to_string()));
        let merged = merge(&base, Some(&extra));
        assert_eq!(merged.env.get("A").unwrap().resolve().unwrap(), "override");
        assert_eq!(merged.env.get("B").unwrap().resolve().unwrap(), "2");
    }

    // top-level agent name override.
    #[test]
    fn test_merge_agent_name_override() {
        let mut base = default();
        base.agent = "old-agent".to_string();
        let mut extra = default();
        extra.agent = "new-agent".to_string();
        let merged = merge(&base, Some(&extra));
        assert_eq!(merged.agent, "new-agent");
    }
}
