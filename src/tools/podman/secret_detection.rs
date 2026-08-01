use std::path::Path;

use super::http::normalize_absolute_path;
use super::types::SandboxMount;

/// Report whether mount_source overlaps with any secret path.
pub fn contains_secret(mount_source: &str, secrets: &[String]) -> bool {
    let clean = normalize_absolute_path(mount_source);
    for s in secrets {
        let cs = normalize_absolute_path(s);
        if cs == clean
            || clean.starts_with(&format!("{cs}/"))
            || cs.starts_with(&format!("{clean}/"))
        {
            return true;
        }
    }
    false
}

/// Canonicalize a host path, resolving symlinks and `.`/`..` segments.
/// Falls back to textual normalization when the path cannot be resolved.
pub fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| normalize_absolute_path(path))
}

/// Report whether a host path currently exists, following symlinks.
pub fn exists(path: &str) -> bool {
    Path::new(path).metadata().is_ok()
}

/// Parse a podman bind spec (`/host:/dest[:opts]`) into `(source, read_only)`.
/// Returns None for named volumes or other non-absolute sources.
pub fn parse_bind_spec(spec: &str) -> Option<(String, bool)> {
    let mut parts = spec.splitn(3, ':');
    let source = parts.next()?.to_string();
    if !source.starts_with('/') {
        return None;
    }
    let read_only = parts
        .nth(1)
        .is_some_and(|opts| opts.split(',').any(|o| o.eq_ignore_ascii_case("ro")));
    Some((source, read_only))
}

/// Return the read-only flag of the most restrictive sandbox mount covering
/// `source`. None if the source is outside the authorized sandbox surface.
pub fn authorized_by_sandbox(source: &str, allowed: &[SandboxMount]) -> Option<bool> {
    let mut covered = false;
    let mut read_only = false;
    for m in allowed {
        if source == m.host || source.starts_with(&format!("{}/", m.host)) {
            covered = true;
            read_only |= m.read_only;
        }
    }
    covered.then_some(read_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed_mounts() -> Vec<SandboxMount> {
        vec![
            SandboxMount {
                host: "/home/user/project".into(),
                read_only: false,
            },
            SandboxMount {
                host: "/home/user/config".into(),
                read_only: true,
            },
        ]
    }

    #[test]
    fn test_contains_secret() {
        let secrets = vec![
            "/home/user/.ssh/id_rsa".to_string(),
            "/home/user/.env".to_string(),
        ];
        assert!(contains_secret("/home/user/.ssh/id_rsa", &secrets));
        assert!(!contains_secret("/home/user/.ssh/keys/old", &secrets));
        assert!(contains_secret("/home/user/.ssh", &secrets));
        assert!(!contains_secret("/home/user/Documents/file.txt", &secrets));
        assert!(contains_secret("/home/user/../user/.ssh/id_rsa", &secrets));
    }

    #[test]
    fn test_canonical_fallback_normalizes() {
        assert_eq!(canonical("/a/b/../c"), "/a/c");
        assert_eq!(canonical("/a/./b"), "/a/b");
    }

    #[test]
    fn test_canonical_resolves_existing_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        assert_eq!(canonical(&path.to_string_lossy()), path.to_string_lossy());
    }

    #[test]
    fn test_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(exists(&dir.path().to_string_lossy()));
        assert!(!exists("/definitely/not/a/real/path/xyz"));
    }

    #[test]
    fn test_parse_bind_spec() {
        assert_eq!(
            parse_bind_spec("/home/user/project:/mnt/project:ro"),
            Some(("/home/user/project".to_string(), true))
        );
        assert_eq!(
            parse_bind_spec("/home/user/project:/mnt/project"),
            Some(("/home/user/project".to_string(), false))
        );
        assert_eq!(
            parse_bind_spec("/home/user/project:/mnt/project:rw,Z"),
            Some(("/home/user/project".to_string(), false))
        );
        assert_eq!(
            parse_bind_spec("/home/user/project:/mnt/project:z,ro"),
            Some(("/home/user/project".to_string(), true))
        );
        assert_eq!(
            parse_bind_spec("/home/user/project"),
            Some(("/home/user/project".to_string(), false))
        );
        assert_eq!(parse_bind_spec("mydata:/mnt/data"), None);
    }

    #[test]
    fn test_authorized_equal_and_descendant() {
        let allowed = allowed_mounts();
        assert_eq!(
            authorized_by_sandbox("/home/user/project", &allowed),
            Some(false)
        );
        assert_eq!(
            authorized_by_sandbox("/home/user/project/sub/dir", &allowed),
            Some(false)
        );
        assert_eq!(
            authorized_by_sandbox("/home/user/config", &allowed),
            Some(true)
        );
        assert_eq!(
            authorized_by_sandbox("/home/user/config/app.conf", &allowed),
            Some(true)
        );
    }

    #[test]
    fn test_authorized_outside_sandbox() {
        let allowed = allowed_mounts();
        assert_eq!(authorized_by_sandbox("/home/user/.ssh", &allowed), None);
        assert_eq!(authorized_by_sandbox("/etc/passwd", &allowed), None);
        assert_eq!(authorized_by_sandbox("/home/user", &allowed), None);
    }

    #[test]
    fn test_authorized_most_restrictive_wins() {
        let allowed = vec![
            SandboxMount {
                host: "/home/user/project".into(),
                read_only: true,
            },
            SandboxMount {
                host: "/home/user/project/data".into(),
                read_only: false,
            },
        ];
        assert_eq!(
            authorized_by_sandbox("/home/user/project/data", &allowed),
            Some(true)
        );
    }
}
