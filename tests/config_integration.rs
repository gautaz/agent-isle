use std::fs;

use indoc::indoc;
use tempfile::tempdir;

#[test]
fn test_config_load_empty() {
    let cfg = agent_isle::config::load("").unwrap();
    assert!(cfg.agent.is_empty());
}

#[test]
fn test_config_load_yaml() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    fs::write(
        &path,
        indoc! {"\
            agent: opencode
            bwrap_path: /usr/bin/bwrap
            betterleaks_path: /usr/bin/betterleaks
            ro_mounts:
              - /common/ro/path
            rw_mounts:
              - /common/rw/path
            env:
              COMMON_VAR: common_value
            agents:
              opencode:
                binary: /usr/bin/opencode
                lightweight_args:
                  - --help
                  - --version
                ro_mounts:
                  - /custom/path
                env:
                  MY_VAR: hello"},
    )
    .unwrap();

    let cfg = agent_isle::config::load(path.to_str().unwrap()).unwrap();
    assert_eq!(cfg.agent, "opencode");
    assert_eq!(cfg.ro_mounts, vec!["/common/ro/path"]);
    assert_eq!(cfg.rw_mounts, vec!["/common/rw/path"]);
    assert_eq!(
        cfg.env.get("COMMON_VAR").unwrap().resolve().unwrap(),
        "common_value"
    );

    let opencode = cfg.agents.get("opencode").unwrap();
    assert_eq!(opencode.binary, "/usr/bin/opencode");
    assert_eq!(opencode.ro_mounts, vec!["/custom/path"]);
    assert_eq!(
        opencode.env.get("MY_VAR").unwrap().resolve().unwrap(),
        "hello"
    );
}

#[test]
fn test_config_load_missing() {
    assert!(agent_isle::config::load("/nonexistent/config.yaml").is_err());
}

#[test]
fn test_config_load_invalid() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.yaml");
    fs::write(&path, ":::not yaml:::[").unwrap();
    assert!(agent_isle::config::load(path.to_str().unwrap()).is_err());
}

#[test]
fn test_config_merge() {
    let mut base = agent_isle::config::default();
    base.ro_mounts = vec!["/base/common/ro".to_string()];
    base.agents.insert(
        "opencode".to_string(),
        agent_isle::config::AgentConfig {
            binary: "/usr/bin/opencode".to_string(),
            ro_mounts: vec!["/base/path".to_string()],
            ..Default::default()
        },
    );

    let mut override_cfg = agent_isle::config::default();
    override_cfg.ro_mounts = vec!["/override/common/ro".to_string()];
    override_cfg.agents.insert(
        "opencode".to_string(),
        agent_isle::config::AgentConfig {
            binary: "/usr/bin/override".to_string(),
            ro_mounts: vec!["/override/path".to_string()],
            ..Default::default()
        },
    );

    let merged = agent_isle::config::merge(&base, Some(&override_cfg));
    assert_eq!(merged.ro_mounts.len(), 2);
    let merged_agent = merged.agents.get("opencode").unwrap();
    assert_eq!(merged_agent.binary, "/usr/bin/override");
    assert_eq!(merged_agent.ro_mounts.len(), 2);
}

#[test]
fn test_config_validate_empty_bwrap() {
    let mut cfg = agent_isle::config::default();
    cfg.bwrap_path = String::new();
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_config_validate_empty_betterleaks() {
    let mut cfg = agent_isle::config::default();
    cfg.betterleaks_path = String::new();
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_config_validate_relative_bwrap() {
    let mut cfg = agent_isle::config::default();
    cfg.bwrap_path = "relative/path".to_string();
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_config_template_expansion() {
    let input = agent_isle::config::SandboxConfig {
        agent: agent_isle::config::AgentConfig::default(),
        mounts: vec![agent_isle::sandbox::Mount::ro(
            "{home}/.config",
            "{home}/.config",
        )],
        env: [(
            "USER_DIR".to_string(),
            agent_isle::config::EnvValue::Static("{home}/.local".to_string()),
        )]
        .into(),
    };

    let result = agent_isle::config::expand_vars(
        input,
        &agent_isle::config::TemplateVars {
            home: "/home/testuser".to_string(),
            user: "testuser".to_string(),
            cwd: "/tmp/test".to_string(),
            xdg_runtime: "/run/user/1000".to_string(),
            xdg_state: "/home/testuser/.local/state".to_string(),
            log_path: "/tmp/test.log".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.mounts[0].host, "/home/testuser/.config");
    assert_eq!(result.mounts[0].target, "/home/testuser/.config");
    assert_eq!(
        result.env.get("USER_DIR").unwrap().resolve().unwrap(),
        "/home/testuser/.local"
    );
}
