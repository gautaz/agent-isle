use std::collections::HashMap;

use rstest::rstest;

use agent_isle::config::{AgentConfig, Config};

#[test]
fn test_validate_relative_bwrap_rejected() {
    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "relative/path".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_empty_bwrap_rejected() {
    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: String::new(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_relative_betterleaks_rejected() {
    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "relative/path".to_string(),
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_empty_betterleaks_rejected() {
    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: String::new(),
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_absolute_paths_pass() {
    let mut agents = HashMap::new();
    agents.insert(
        "opencode".to_string(),
        AgentConfig {
            binary: "/usr/bin/opencode".to_string(),
            lightweight_args: vec![],
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_ok());
}

#[test]
fn test_validate_agent_binary_relative_rejected() {
    let mut agents = HashMap::new();
    agents.insert(
        "opencode".to_string(),
        AgentConfig {
            binary: "relative/opencode".to_string(),
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_empty_agent_binary_rejected() {
    let mut agents = HashMap::new();
    agents.insert(
        "opencode".to_string(),
        AgentConfig {
            binary: String::new(),
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_agent_name_slash_rejected() {
    let mut agents = HashMap::new();
    agents.insert(
        "my/agent".to_string(),
        AgentConfig {
            binary: "/usr/bin/agent".to_string(),
            lightweight_args: vec![],
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "my/agent".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_agent_name_null_rejected() {
    let mut agents = HashMap::new();
    agents.insert(
        "my\0agent".to_string(),
        AgentConfig {
            binary: "/usr/bin/agent".to_string(),
            lightweight_args: vec![],
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "my\0agent".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[test]
fn test_validate_agent_name_valid() {
    let mut agents = HashMap::new();
    for name in ["opencode", "my-agent", "my_agent", "agent.v2", "a"] {
        agents.insert(
            name.to_string(),
            AgentConfig {
                binary: "/usr/bin/agent".to_string(),
                lightweight_args: vec![],
                ..Default::default()
            },
        );
    }

    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_ok());
}

#[test]
fn test_validate_agent_not_in_map_rejected() {
    let cfg = Config {
        agent: "nonexistent".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}

#[rstest]
#[case("/usr/bin/opencode")]
#[case("/usr/local/bin/opencode")]
#[case("/opt/agents/opencode")]
fn test_validate_various_absolute_paths(#[case] path: &str) {
    let mut agents = HashMap::new();
    agents.insert(
        "opencode".to_string(),
        AgentConfig {
            binary: path.to_string(),
            lightweight_args: vec![],
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_ok());
}

#[rstest]
#[case("relative/opencode")]
#[case("../opencode")]
#[case("./opencode")]
fn test_validate_various_relative_paths_rejected(#[case] path: &str) {
    let mut agents = HashMap::new();
    agents.insert(
        "opencode".to_string(),
        AgentConfig {
            binary: path.to_string(),
            ..Default::default()
        },
    );

    let cfg = Config {
        agent: "opencode".to_string(),
        bwrap_path: "/usr/bin/bwrap".to_string(),
        betterleaks_path: "/usr/bin/betterleaks".to_string(),
        agents,
        ..Default::default()
    };
    assert!(agent_isle::config::validate(&cfg).is_err());
}
