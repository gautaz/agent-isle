use std::collections::HashMap;

use tempfile::tempdir;

use agent_isle::config::{AgentConfig, EnvValue};
use agent_isle::platform;
use agent_isle::sandbox;
use agent_isle::sandbox::Mount;

/// Helper: build an AgentConfig with no lightweight_args.
fn agent_none() -> AgentConfig {
    AgentConfig {
        lightweight_args: vec![],
        ..Default::default()
    }
}

/// Helper: build an AgentConfig with custom lightweight_args.
fn agent_with_args(flags: &[&str]) -> AgentConfig {
    AgentConfig {
        lightweight_args: flags.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

/// Helper: build an AgentConfig with no lightweight_args (unvalidated default).
fn agent_no_lightweight() -> AgentConfig {
    AgentConfig::default()
}

#[test]
fn test_sandbox_is_lightweight_op_help() {
    let agent = agent_with_args(&["--help", "-h", "--version", "-v"]);
    assert!(agent.is_lightweight_op(&["--help".to_string()]));
}

#[test]
fn test_sandbox_is_lightweight_op_version() {
    let agent = agent_with_args(&["--help", "-h", "--version", "-v"]);
    assert!(agent.is_lightweight_op(&["--version".to_string()]));
}

#[test]
fn test_sandbox_is_lightweight_op_short_help() {
    let agent = agent_with_args(&["--help", "-h", "--version", "-v"]);
    assert!(agent.is_lightweight_op(&["-h".to_string()]));
}

#[test]
fn test_sandbox_is_not_lightweight_op() {
    let agent = agent_with_args(&["--help", "-h", "--version", "-v"]);
    let args = vec!["--some-other-flag".to_string()];
    assert!(!agent.is_lightweight_op(&args));
}

#[test]
fn test_sandbox_is_lightweight_op_empty_args() {
    let agent = agent_none();
    assert!(!agent.is_lightweight_op(&["--help".to_string()]));
    assert!(!agent.is_lightweight_op(&["--version".to_string()]));
}

#[test]
fn test_sandbox_is_lightweight_op_none_args() {
    let agent = agent_no_lightweight();
    // Unvalidated config: lightweight_args is None, returns false conservatively.
    assert!(!agent.is_lightweight_op(&["--help".to_string()]));
}

#[test]
fn test_sandbox_build_args_masks_secrets() {
    let os_cfg = platform::detect();
    let secret_files = vec!["/path/to/secret.txt".to_string()];
    let secret_mounts = os_cfg.secret_mounts(&secret_files);

    let args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &secret_mounts,
        env: &HashMap::new(),
        pwd: "/tmp/test",
    });

    let mask_idx = args
        .windows(3)
        .position(|w| w[0] == "--bind" && w[1] == "/dev/null" && w[2] == "/path/to/secret.txt");
    assert!(
        mask_idx.is_some(),
        "secret file should be masked with /dev/null"
    );
}

#[test]
fn test_sandbox_build_args_proxy_bind() {
    let dir = tempdir().unwrap();
    let proxy_sock = dir.path().join("proxy.sock");
    std::fs::write(&proxy_sock, b"").unwrap();

    let args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &[Mount::rw(
            proxy_sock.to_str().unwrap(),
            "/tmp/podman-proxy.sock",
        )],
        env: &HashMap::new(),
        pwd: "/tmp/test",
    });

    let bind_idx = args.windows(3).position(|w| {
        w[0] == "--bind" && w[1] == proxy_sock.to_str().unwrap() && w[2] == "/tmp/podman-proxy.sock"
    });
    assert!(bind_idx.is_some(), "proxy socket should be bound");
}

#[test]
fn test_sandbox_build_args_env() {
    let mut env = HashMap::new();
    env.insert(
        "MY_VAR".to_string(),
        EnvValue::Static("my_value".to_string()),
    );

    let args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &[],
        env: &env,
        pwd: "/tmp/test",
    });

    let env_idx = args
        .windows(3)
        .position(|w| w[0] == "--setenv" && w[1] == "MY_VAR" && w[2] == "my_value");
    assert!(env_idx.is_some(), "env var should be set");
}

#[test]
fn test_sandbox_build_args_agent_mounts() {
    let ro_dir = tempdir().unwrap();
    let rw_dir = tempdir().unwrap();
    let ro_path = ro_dir.path().join("agent_ro");
    let rw_path = rw_dir.path().join("agent_rw");
    std::fs::create_dir(&ro_path).unwrap();
    std::fs::create_dir(&rw_path).unwrap();

    let mounts = vec![
        Mount::ro(ro_path.to_str().unwrap(), ro_path.to_str().unwrap()),
        Mount::rw(rw_path.to_str().unwrap(), rw_path.to_str().unwrap()),
    ];

    let args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &mounts,
        env: &HashMap::new(),
        pwd: "/tmp/test",
    });

    assert!(
        args.windows(3)
            .any(|w| w[0] == "--ro-bind" && w[1] == ro_path.to_str().unwrap()),
        "agent ro mount should be present"
    );
    assert!(
        args.windows(3)
            .any(|w| w[0] == "--bind" && w[1] == rw_path.to_str().unwrap()),
        "agent rw mount should be present"
    );
}

#[test]
fn test_sandbox_build_args_skips_nonexistent_paths() {
    let args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &[
            Mount::ro("/nonexistent/top/ro", "/nonexistent/top/ro"),
            Mount::rw("/nonexistent/top/rw", "/nonexistent/top/rw"),
        ],
        env: &HashMap::new(),
        pwd: "/tmp/test",
    });

    assert!(
        !args.windows(2).any(|w| w[1] == "/nonexistent/top/ro"),
        "non-existent ro mount should be excluded"
    );
    assert!(
        !args.windows(2).any(|w| w[1] == "/nonexistent/top/rw"),
        "non-existent rw mount should be excluded"
    );
}

#[test]
fn test_sandbox_build_args_agent_env() {
    let mut env = HashMap::new();
    env.insert(
        "AGENT_VAR".to_string(),
        EnvValue::Static("agent_value".to_string()),
    );

    let args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &[],
        env: &env,
        pwd: "/tmp/test",
    });

    let env_idx = args
        .windows(3)
        .position(|w| w[0] == "--setenv" && w[1] == "AGENT_VAR" && w[2] == "agent_value");
    assert!(env_idx.is_some(), "env var should be set");
}
