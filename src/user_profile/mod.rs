use std::collections::HashMap;
use std::env;

use crate::capability_sources::CapabilitySource;
use crate::config::EnvValue;
use crate::sandbox::{Mount, SecretsPolicy};

/// UserProfileSource provides PATH-derived mounts, cache isolation, and user env vars.
///
/// Reads the host's `$PATH`, mounts each directory into the sandbox,
/// isolates the user's cache, and sets `PATH` and `XDG_RUNTIME_DIR` in the sandbox.
pub struct UserProfileSource {
    pub secrets_policy: SecretsPolicy,
    home: String,
    path_override: Option<String>,
}

impl UserProfileSource {
    pub fn new(secrets_policy: SecretsPolicy, home: &str, _xdg_runtime: &str) -> Self {
        Self {
            secrets_policy,
            home: home.to_string(),
            path_override: None,
        }
    }

    fn host_path(&self) -> String {
        self.path_override
            .clone()
            .or_else(|| env::var("PATH").ok())
            .unwrap_or_else(|| {
                tracing::warn!(
                    "$PATH is not set and no path_override provided; falling back to /usr/bin:/bin"
                );
                "/usr/bin:/bin".to_string()
            })
    }

    fn cache_mounts(&self) -> Vec<Mount> {
        let source = format!("{}/.cache/agent-isle", self.home);
        let target = format!("{}/.cache", self.home);
        if std::fs::create_dir_all(&source).is_ok() {
            vec![Mount::rw(source, target).secrets_policy(SecretsPolicy::Show)]
        } else {
            tracing::warn!(
                cache_dir = %source,
                "failed to create cache directory; agent cache writes will consume RAM on the sandbox tmpfs"
            );
            vec![]
        }
    }
}

impl CapabilitySource for UserProfileSource {
    fn mounts(&self) -> Vec<Mount> {
        let mut mounts = self.cache_mounts();
        let path = self.host_path();
        mounts.extend(
            path.split(':')
                .filter(|s| !s.is_empty())
                .map(|dir| Mount::ro(dir, dir).secrets_policy(self.secrets_policy)),
        );
        mounts
    }

    fn env(&self) -> HashMap<String, EnvValue> {
        let mut env = HashMap::new();
        env.insert("PATH".into(), EnvValue::Static(self.host_path()));
        env.insert(
            "XDG_RUNTIME_DIR".into(),
            EnvValue::Static("/tmp".to_string()),
        );
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_with_path(path: &str, policy: SecretsPolicy) -> UserProfileSource {
        let home = tempfile::tempdir().unwrap();
        let home_str = home.path().to_str().unwrap().to_string();
        // Leak the TempDir so it lives for the test duration.
        Box::leak(Box::new(home));
        UserProfileSource {
            secrets_policy: policy,
            home: home_str,
            path_override: Some(path.to_string()),
        }
    }

    #[test]
    fn test_user_profile_source_mounts() {
        let source = source_with_path("/usr/bin:/bin:/home/user/.local/bin", SecretsPolicy::Show);
        let mounts = source.mounts();

        // 1 cache mount + 3 PATH mounts
        assert_eq!(mounts.len(), 4);
        assert!(mounts[0].host.contains(".cache/agent-isle"));
        assert_eq!(mounts[1].host, "/usr/bin");
        assert_eq!(mounts[2].host, "/bin");
        assert_eq!(mounts[3].host, "/home/user/.local/bin");
    }

    #[test]
    fn test_user_profile_source_env() {
        let source = source_with_path("/usr/bin:/bin", SecretsPolicy::Mask);
        let env_map = source.env();

        assert_eq!(env_map.len(), 2);
        assert_eq!(
            env_map.get("PATH").unwrap().resolve().unwrap(),
            "/usr/bin:/bin"
        );
        assert_eq!(
            env_map.get("XDG_RUNTIME_DIR").unwrap().resolve().unwrap(),
            "/tmp"
        );
    }

    #[test]
    fn test_user_profile_source_filters_empty_entries() {
        let source = source_with_path("/usr/bin::/bin:", SecretsPolicy::Mask);
        let mounts = source.mounts();

        // 1 cache mount + 2 PATH mounts
        let path_mounts: Vec<_> = mounts
            .iter()
            .filter(|m| !m.host.contains(".cache/agent-isle"))
            .collect();
        assert_eq!(path_mounts.len(), 2);
        assert_eq!(path_mounts[0].host, "/usr/bin");
        assert_eq!(path_mounts[1].host, "/bin");
    }

    #[test]
    fn test_user_profile_source_default_path() {
        let home = tempfile::tempdir().unwrap();
        let home_str = home.path().to_str().unwrap().to_string();
        let source = UserProfileSource {
            secrets_policy: SecretsPolicy::Mask,
            home: home_str,
            path_override: None,
        };
        let env_map = source.env();

        let path_val = env_map.get("PATH").unwrap().resolve().unwrap();
        assert!(!path_val.is_empty());
    }

    #[test]
    fn test_user_profile_source_mask_policy() {
        let source = source_with_path("/usr/bin", SecretsPolicy::Mask);
        let mounts = source.mounts();

        let path_mounts: Vec<_> = mounts
            .iter()
            .filter(|m| !m.host.contains(".cache/agent-isle"))
            .collect();
        assert_eq!(path_mounts[0].secrets_policy, SecretsPolicy::Mask);
    }

    #[test]
    fn test_cache_mount_failure_path() {
        let home = tempfile::tempdir().unwrap();
        let home_file = home.path().join("not_a_dir");
        std::fs::write(&home_file, b"").unwrap();
        let home_str = home_file.to_str().unwrap().to_string();
        let source = UserProfileSource {
            secrets_policy: SecretsPolicy::Mask,
            home: home_str,

            path_override: Some("".to_string()),
        };
        let mounts = source.mounts();
        assert!(mounts.is_empty());
    }

    #[test]
    fn test_cache_mount_secrets_policy_show() {
        let home = tempfile::tempdir().unwrap();
        let home_str = home.path().to_str().unwrap().to_string();
        let source = UserProfileSource {
            secrets_policy: SecretsPolicy::Mask,
            home: home_str,

            path_override: Some("/usr/bin".to_string()),
        };
        let mounts = source.mounts();
        assert!(!mounts.is_empty());
        assert_eq!(mounts[0].secrets_policy, SecretsPolicy::Show);
    }

    #[test]
    fn test_home_trailing_slash() {
        let home = tempfile::tempdir().unwrap();
        let home_str = format!("{}/", home.path().to_str().unwrap());
        let source = UserProfileSource {
            secrets_policy: SecretsPolicy::Mask,
            home: home_str,

            path_override: Some("/usr/bin".to_string()),
        };
        let mounts = source.mounts();
        let cache_mounts: Vec<_> = mounts
            .iter()
            .filter(|m| m.host.contains(".cache/agent-isle"))
            .collect();
        assert_eq!(cache_mounts.len(), 1);
        assert!(cache_mounts[0].host.contains("//"));
    }

    #[test]
    fn test_empty_xdg_runtime() {
        let source = UserProfileSource {
            secrets_policy: SecretsPolicy::Mask,
            home: "/tmp/test".to_string(),
            path_override: Some("/usr/bin".to_string()),
        };
        let env_map = source.env();
        assert_eq!(
            env_map.get("XDG_RUNTIME_DIR").unwrap().resolve().unwrap(),
            "/tmp"
        );
    }

    #[test]
    fn test_nonexistent_path_dirs_mounted_by_source() {
        let home = tempfile::tempdir().unwrap();
        let home_file = home.path().join("not_a_dir");
        std::fs::write(&home_file, b"").unwrap();
        let home_str = home_file.to_str().unwrap().to_string();
        let source = UserProfileSource {
            secrets_policy: SecretsPolicy::Mask,
            home: home_str,

            path_override: Some("/nonexistent/a:/nonexistent/b".to_string()),
        };
        let mounts = source.mounts();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].host, "/nonexistent/a");
        assert_eq!(mounts[1].host, "/nonexistent/b");

        // Verify build_args filters them
        let args = crate::sandbox::build_args(crate::sandbox::BuildArgs {
            mounts: &mounts,
            env: &std::collections::HashMap::new(),
            chdir: "/tmp",
        });
        assert!(
            !args.windows(2).any(|w| w[1] == "/nonexistent/a"),
            "non-existent PATH dir should be filtered by build_args"
        );
        assert!(
            !args.windows(2).any(|w| w[1] == "/nonexistent/b"),
            "non-existent PATH dir should be filtered by build_args"
        );
    }
}
