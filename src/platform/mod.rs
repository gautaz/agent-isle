use std::collections::HashMap;
use std::path::Path;

use crate::capability_sources::CapabilitySource;
use crate::config::EnvValue;
use crate::sandbox::{Mount, SecretsPolicy};

/// OSConfig defines the OS-specific behavior for sandbox construction.
///
/// Base methods (`base_mounts`) provide shared Linux defaults.
/// Platform methods (`platform_mounts`, `platform_env`) add OS-specific paths.
/// Public methods (`mounts`) compose base + platform automatically.
pub trait OSConfig: Send + Sync {
    /// OS-specific environment variables.
    fn platform_env(&self) -> HashMap<String, String>;

    /// Minimal read-only mounts for lightweight operations (--help, --version).
    fn minimal_ro_mounts(&self) -> Vec<String>;

    /// Shared Linux mounts (DNS, dynamic linker, CA certs). Override to customize.
    fn base_mounts(&self) -> Vec<Mount> {
        vec![
            Mount::ro("/etc/passwd", "/etc/passwd").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/etc/group", "/etc/group").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/etc/hosts", "/etc/hosts").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/etc/nsswitch.conf", "/etc/nsswitch.conf")
                .secrets_policy(SecretsPolicy::Show),
            Mount::ro("/etc/resolv.conf", "/etc/resolv.conf").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/etc/ssl/certs", "/etc/ssl/certs").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/lib64", "/lib64").secrets_policy(SecretsPolicy::Show),
        ]
    }

    /// OS-specific infrastructure mounts beyond the base set.
    fn platform_mounts(&self) -> Vec<Mount> {
        vec![]
    }

    /// All mounts: base + platform.
    fn mounts(&self) -> Vec<Mount> {
        let mut m = self.base_mounts();
        m.extend(self.platform_mounts());
        m
    }

    /// Mask mounts that hide secret files from the sandbox.
    /// Default: binds /dev/null over each path.
    fn secret_mounts(&self, paths: &[String]) -> Vec<Mount> {
        paths
            .iter()
            .map(|p| Mount::rw("/dev/null", p).secrets_policy(SecretsPolicy::Show))
            .collect()
    }
}

/// Linux implements OSConfig for generic Linux systems.
pub struct Linux;

impl OSConfig for Linux {
    fn platform_mounts(&self) -> Vec<Mount> {
        vec![
            Mount::ro("/usr/lib", "/usr/lib").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/lib", "/lib").secrets_policy(SecretsPolicy::Show),
        ]
    }

    fn platform_env(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn minimal_ro_mounts(&self) -> Vec<String> {
        vec![
            "/usr/bin".to_string(),
            "/bin".to_string(),
            "/usr/lib".to_string(),
            "/lib".to_string(),
            "/lib64".to_string(),
        ]
    }
}

/// NixOS implements OSConfig for NixOS systems.
pub struct NixOS;

impl OSConfig for NixOS {
    fn platform_mounts(&self) -> Vec<Mount> {
        vec![
            Mount::ro("/usr/bin", "/usr/bin").secrets_policy(SecretsPolicy::Show),
            Mount::ro("/nix/store", "/nix/store").secrets_policy(SecretsPolicy::Show),
        ]
    }

    fn platform_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        if let Ok(val) = std::env::var("SSL_CERT_FILE") {
            env.insert("SSL_CERT_FILE".to_string(), val);
        }
        env
    }

    fn minimal_ro_mounts(&self) -> Vec<String> {
        vec!["/nix/store".to_string(), "/usr/bin".to_string()]
    }
}

/// Platform mounts (OS-specific paths, DNS).
pub struct PlatformSource<'a> {
    pub os_cfg: &'a dyn OSConfig,
}

impl CapabilitySource for PlatformSource<'_> {
    fn mounts(&self) -> Vec<Mount> {
        self.os_cfg.mounts()
    }

    fn env(&self) -> HashMap<String, EnvValue> {
        self.os_cfg
            .platform_env()
            .into_iter()
            .map(|(k, v)| (k, EnvValue::Static(v)))
            .collect()
    }
}

/// Detect identifies the current OS and returns the appropriate config.
pub fn detect() -> Box<dyn OSConfig> {
    if Path::new("/nix/store").exists() {
        tracing::info!("detected platform: NixOS");
        Box::new(NixOS)
    } else {
        tracing::info!("detected platform: Linux");
        Box::new(Linux)
    }
}

// NOTE: No tests for OSConfig impls or detect() — platform structs are
// hardcoded data definitions. The trait's default composition logic (mounts,
// env, secret_mounts) is tested indirectly via sandbox integration tests.
// detect() has branching but testing it properly requires mocking filesystem
// state to simulate both NixOS and generic Linux platforms.
