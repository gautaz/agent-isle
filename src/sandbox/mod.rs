use std::collections::HashMap;
use std::path::Path;

use crate::config::EnvValue;
use crate::platform::OSConfig;
use crate::util;

/// Whether a mount is read-only or read-write inside the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountMode {
    /// Read-only: maps to `--ro-bind`.
    Ro,
    /// Read-write: maps to `--bind`.
    Rw,
}

/// A host-to-sandbox mount mapping with access mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub host: String,
    pub target: String,
    pub mode: MountMode,
}

impl Mount {
    pub fn ro(host: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            target: target.into(),
            mode: MountMode::Ro,
        }
    }

    pub fn rw(host: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            target: target.into(),
            mode: MountMode::Rw,
        }
    }
}

/// Parameters for building bubblewrap sandbox arguments.
///
/// Groups the narrow inputs that `build_args` needs from the caller,
/// avoiding a function with too many individual parameters.
pub struct BuildArgs<'a> {
    pub mounts: &'a [Mount],
    pub env: &'a HashMap<String, EnvValue>,
    pub pwd: &'a str,
}

/// build_args constructs bubblewrap arguments from caller-provided mounts and env.
///
/// The caller is responsible for assembling all contributions (platform mounts,
/// config mounts, agent mounts, secret mounts, proxy bind, etc.) into the
/// `mounts` and `env` parameters. The sandbox module only emits bwrap boilerplate
/// and iterates over what it receives.
pub fn build_args(params: BuildArgs) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    args.extend_from_slice(&[
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--chdir".into(),
        params.pwd.into(),
    ]);

    // Emit bwrap bind flags, filtering out paths that don't exist
    for m in params.mounts {
        if Path::new(&m.host).exists() {
            let flag = match m.mode {
                MountMode::Ro => "--ro-bind",
                MountMode::Rw => "--bind",
            };
            args.extend_from_slice(&[flag.into(), m.host.clone(), m.target.clone()]);
        }
    }

    // Top-level environment variables
    for (k, v) in params.env {
        if let Ok(resolved) = v.resolve() {
            args.extend_from_slice(&["--setenv".into(), k.clone(), resolved]);
        }
    }

    args
}

/// sandbox_mounts returns the sandbox-specific mounts: PWD and isolated cache.
pub fn sandbox_mounts(pwd: &str) -> anyhow::Result<Vec<Mount>> {
    let home = util::home_dir().map_err(|e| anyhow::anyhow!(e))?;
    let mut mounts = vec![Mount::rw(pwd, pwd)];
    let cache_source = format!("{home}/.cache/agent-isle");
    let cache_target = format!("{home}/.cache");
    if std::fs::create_dir_all(&cache_source).is_ok() {
        mounts.push(Mount::rw(cache_source, cache_target));
    }
    Ok(mounts)
}

/// build_minimal_args constructs minimal bubblewrap arguments for lightweight
/// operations like --help and --version.
pub fn build_minimal_args(os_cfg: &dyn OSConfig) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
    ];

    for p in os_cfg.minimal_ro_mounts() {
        args.extend_from_slice(&["--ro-bind".into(), p]);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;

    #[test]
    fn test_build_args() {
        let args = build_args(BuildArgs {
            mounts: &[],
            env: &std::collections::HashMap::new(),
            pwd: "/project",
        });

        assert!(args.contains(&"--proc".to_string()));
        assert!(args.contains(&"--dev".to_string()));
        assert!(args.contains(&"--tmpfs".to_string()));
        // Check --chdir followed by pwd
        let idx = args
            .iter()
            .position(|a| a == "--chdir")
            .expect("--chdir not found");
        assert_eq!(args[idx + 1], "/project");
    }

    #[test]
    fn test_build_args_secret_masking() {
        let secrets = vec![
            "/home/user/.env".to_string(),
            "/home/user/.ssh/id_rsa".to_string(),
        ];
        let os_cfg = platform::detect();
        let secret_mounts = os_cfg.secret_mounts(&secrets);

        let args = build_args(BuildArgs {
            mounts: &secret_mounts,
            env: &std::collections::HashMap::new(),
            pwd: "/project",
        });

        let dev_null_count = args
            .windows(2)
            .filter(|w| w[0] == "--bind" && w[1] == "/dev/null")
            .count();
        assert_eq!(dev_null_count, secrets.len());
    }

    #[test]
    fn test_build_args_proxy_bind() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let proxy_sock = dir.path().join("proxy.sock");
        std::fs::write(&proxy_sock, b"").unwrap();

        let target = "/tmp/podman-proxy.sock".to_string();
        let args = build_args(BuildArgs {
            mounts: &[Mount::rw(proxy_sock.to_str().unwrap(), &target)],
            env: &std::collections::HashMap::new(),
            pwd: "/project",
        });

        let found = args.windows(3).any(|w| {
            w[0] == "--bind" && w[1] == proxy_sock.to_str().unwrap() && w[2] == target.as_str()
        });
        assert!(found);
    }

    #[test]
    fn test_build_args_agent_mounts() {
        let ro_dir = tempfile::tempdir().unwrap();
        let rw_dir = tempfile::tempdir().unwrap();
        let ro_path = ro_dir.path().join("agent_ro");
        let rw_path = rw_dir.path().join("agent_rw");
        std::fs::create_dir(&ro_path).unwrap();
        std::fs::create_dir(&rw_path).unwrap();

        let mounts = vec![
            Mount::ro(ro_path.to_str().unwrap(), ro_path.to_str().unwrap()),
            Mount::rw(rw_path.to_str().unwrap(), rw_path.to_str().unwrap()),
        ];

        let args = build_args(BuildArgs {
            mounts: &mounts,
            env: &std::collections::HashMap::new(),
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
    fn test_build_args_skips_nonexistent_paths() {
        let args = build_args(BuildArgs {
            mounts: &[
                Mount::ro("/nonexistent/top/ro", "/nonexistent/top/ro"),
                Mount::rw("/nonexistent/top/rw", "/nonexistent/top/rw"),
            ],
            env: &std::collections::HashMap::new(),
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
    fn test_build_args_agent_env() {
        let mut env = HashMap::new();
        env.insert(
            "AGENT_VAR".to_string(),
            EnvValue::Static("agent_value".to_string()),
        );

        let args = build_args(BuildArgs {
            mounts: &[],
            env: &env,
            pwd: "/tmp/test",
        });

        let env_idx = args
            .windows(3)
            .position(|w| w[0] == "--setenv" && w[1] == "AGENT_VAR" && w[2] == "agent_value");
        assert!(env_idx.is_some(), "agent env var should be set");
    }

    #[test]
    fn test_build_minimal_args() {
        let os_cfg = platform::detect();
        let args = build_minimal_args(os_cfg.as_ref());
        assert!(!args.is_empty());
        assert!(args.contains(&"--proc".to_string()));
        assert!(args.contains(&"--dev".to_string()));
        assert!(args.contains(&"--tmpfs".to_string()));
    }
}
