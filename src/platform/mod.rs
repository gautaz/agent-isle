use std::collections::HashMap;
use std::path::Path;

use crate::sandbox::Mount;

/// OSConfig defines the OS-specific behavior for sandbox construction.
///
/// Base methods (`base_mounts`, `base_env`) provide shared Linux defaults.
/// Platform methods (`platform_mounts`, `platform_env`) add OS-specific paths.
/// Public methods (`mounts`, `env`) compose base + platform automatically.
pub trait OSConfig: Send + Sync {
    /// OS-specific mounts beyond the base Linux set.
    fn platform_mounts(&self, home: &str, user: &str) -> Vec<Mount>;

    /// OS-specific environment variables beyond PATH and XDG_RUNTIME_DIR.
    fn platform_env(&self, xdg_runtime: &str) -> HashMap<String, String>;

    /// Minimal read-only mounts for lightweight operations (--help, --version).
    fn minimal_ro_mounts(&self) -> Vec<String>;

    /// Shared Linux mounts (DNS, bin dirs). Override to customize.
    fn base_mounts(&self) -> Vec<Mount> {
        vec![
            Mount::ro("/etc/hosts", "/etc/hosts"),
            Mount::ro("/etc/nsswitch.conf", "/etc/nsswitch.conf"),
            Mount::ro("/etc/resolv.conf", "/etc/resolv.conf"),
            Mount::ro("/usr/bin", "/usr/bin"),
            Mount::ro("/bin", "/bin"),
        ]
    }

    /// All mounts: base + platform.
    fn mounts(&self, home: &str, user: &str) -> Vec<Mount> {
        let mut m = self.base_mounts();
        m.extend(self.platform_mounts(home, user));
        m
    }

    /// All environment variables: base + platform.
    fn env(&self, xdg_runtime: &str) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin:/bin".into());
        env.insert("XDG_RUNTIME_DIR".into(), xdg_runtime.into());
        env.extend(self.platform_env(xdg_runtime));
        env
    }

    /// Mask mounts that hide secret files from the sandbox.
    /// Default: binds /dev/null over each path.
    fn secret_mounts(&self, paths: &[String]) -> Vec<Mount> {
        paths.iter().map(|p| Mount::rw("/dev/null", p)).collect()
    }
}

/// Linux implements OSConfig for generic Linux systems.
pub struct Linux;

impl OSConfig for Linux {
    fn platform_mounts(&self, _home: &str, _user: &str) -> Vec<Mount> {
        vec![]
    }

    fn platform_env(&self, _xdg_runtime: &str) -> HashMap<String, String> {
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
    fn platform_mounts(&self, home: &str, user: &str) -> Vec<Mount> {
        vec![
            Mount::ro(format!("{home}/.local/bin"), format!("{home}/.local/bin")),
            Mount::ro(
                format!("/etc/profiles/per-user/{user}"),
                format!("/etc/profiles/per-user/{user}"),
            ),
            Mount::ro(
                format!("{home}/.nix-profile"),
                format!("{home}/.nix-profile"),
            ),
            Mount::ro("/nix/store", "/nix/store"),
            Mount::ro("/run/current-system/sw", "/run/current-system/sw"),
            Mount::ro("/run/wrappers", "/run/wrappers"),
        ]
    }

    fn platform_env(&self, _xdg_runtime: &str) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(
            "PATH".into(),
            "/run/wrappers/bin:/run/current-system/sw/bin:/usr/bin:/bin".into(),
        );
        env
    }

    fn minimal_ro_mounts(&self) -> Vec<String> {
        vec!["/nix/store".to_string()]
    }
}

/// Detect identifies the current OS and returns the appropriate config.
pub fn detect() -> Box<dyn OSConfig> {
    if Path::new("/nix/store").exists() {
        Box::new(NixOS)
    } else {
        Box::new(Linux)
    }
}

// NOTE: No tests for OSConfig impls or detect() — platform structs are
// hardcoded data definitions. The trait's default composition logic (mounts,
// env, secret_mounts) is tested indirectly via sandbox integration tests.
// detect() has branching but testing it properly requires mocking filesystem
// state to simulate both NixOS and generic Linux platforms.
