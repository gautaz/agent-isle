use std::env;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

/// Return the user's home directory from $HOME.
/// Returns Err if $HOME is not set.
pub fn home_dir() -> Result<String, String> {
    env::var("HOME")
        .map_err(|_| "$HOME is not set — agent-isle requires a home directory".to_string())
}

/// Return the current username from $USER.
/// Returns Err if $USER is not set.
pub fn username() -> Result<String, String> {
    env::var("USER").map_err(|_| "$USER is not set — agent-isle requires a username".to_string())
}

/// Return $XDG_RUNTIME_DIR or a fallback.
pub fn xdg_runtime_dir() -> String {
    env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let uid = unsafe { libc::getuid() };
        format!("/run/user/{uid}")
    })
}

/// Return $XDG_STATE_HOME or a fallback.
pub fn xdg_state_home() -> String {
    env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = home_dir().unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.local/state")
    })
}

/// Return $XDG_CONFIG_HOME or a fallback.
pub fn xdg_config_home() -> String {
    env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = home_dir().unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.config")
    })
}

/// Remove run directories left by dead processes.
pub fn cleanup_stale_dirs(base: &str, my_pid: u32) {
    let entries = match fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if pid == my_pid {
            continue;
        }
        let proc_path = format!("/proc/{pid}");
        if !Path::new(&proc_path).exists() {
            let dir_path = entry.path();
            let _ = fs::remove_dir_all(&dir_path);
        }
    }
}

/// Sync and close a file, ignoring errors.
pub fn sync_and_close(path: &Path) {
    if let Ok(f) = fs::File::open(path) {
        let _ = f.sync_all();
        drop(f);
    }
}

/// Validate that a Unix socket is owned by the current user or root.
/// Returns Ok(()) if valid, Err with reason if not.
pub fn validate_socket_ownership(path: &str) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|e| format!("cannot stat socket {path}: {e}"))?;

    if !metadata.file_type().is_socket() {
        return Err(format!("{path} is not a Unix socket"));
    }

    let socket_uid = metadata.uid();
    let my_uid = unsafe { libc::getuid() };

    if socket_uid != my_uid && socket_uid != 0 {
        return Err(format!(
            "socket {path} is owned by UID {socket_uid}, expected UID {my_uid} or root (0)"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_xdg_dirs() {
        assert!(!xdg_runtime_dir().is_empty());
        assert!(!xdg_state_home().is_empty());
        assert!(!xdg_config_home().is_empty());
    }

    #[test]
    fn test_xdg_runtime_dir_fallback() {
        let orig = env::var("XDG_RUNTIME_DIR").ok();
        env::remove_var("XDG_RUNTIME_DIR");
        let result = xdg_runtime_dir();
        let uid = unsafe { libc::getuid() };
        assert_eq!(result, format!("/run/user/{uid}"));
        if let Some(val) = orig {
            env::set_var("XDG_RUNTIME_DIR", val);
        }
    }

    #[test]
    fn test_validate_socket_nonexistent() {
        assert!(validate_socket_ownership("/nonexistent/socket").is_err());
    }

    #[test]
    fn test_validate_socket_not_a_socket() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("not_a_socket");
        fs::write(&path, "data").unwrap();
        assert!(validate_socket_ownership(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn test_validate_socket_owned_by_user() {
        use std::os::unix::net::UnixListener;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        assert!(validate_socket_ownership(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_sync_and_close() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.log");
        fs::write(&path, "test data").unwrap();
        sync_and_close(&path);
        assert!(path.exists());
    }

    #[test]
    fn test_sync_and_close_nonexistent() {
        let path = Path::new("/nonexistent/path/file.log");
        sync_and_close(path);
    }

    #[test]
    fn test_cleanup_stale_dirs() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        // Create directory for a dead PID (99999999 doesn't exist)
        let stale_dir = base.join("99999999");
        fs::create_dir(&stale_dir).unwrap();
        assert!(stale_dir.exists());

        // Create directory for PID 1 (always exists on Linux)
        let alive_dir = base.join("1");
        fs::create_dir(&alive_dir).unwrap();

        // Create non-numeric entry
        let non_numeric = base.join("not_a_pid");
        fs::create_dir(&non_numeric).unwrap();

        cleanup_stale_dirs(base.to_str().unwrap(), 12345);

        assert!(!stale_dir.exists(), "stale dir should be removed");
        assert!(alive_dir.exists(), "alive dir should remain");
        assert!(non_numeric.exists(), "non-numeric entry should remain");
    }

    #[test]
    fn test_cleanup_stale_dirs_nonexistent_base() {
        cleanup_stale_dirs("/nonexistent_base_12345", 12345);
    }
}
