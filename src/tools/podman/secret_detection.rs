use super::http::normalize_absolute_path;
use super::types::Mount;

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

/// Return bind mount sources that contain secrets.
pub fn find_secret_binds(binds: &[String], secrets: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for b in binds {
        let source = b.split_once(':').map(|(s, _)| s).unwrap_or(b);
        if contains_secret(source, secrets) {
            found.push(source.to_string());
        }
    }
    found
}

/// Return mount sources that contain secrets.
pub fn find_secret_mounts(mounts: &[Mount], secrets: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for m in mounts {
        if m.mount_type == "bind" && !m.source.is_empty() && contains_secret(&m.source, secrets) {
            found.push(m.source.clone());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::podman::types::Mount;

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
    fn test_find_secret_binds() {
        let secrets = vec![
            "/home/user/.ssh/id_rsa".to_string(),
            "/home/user/.env".to_string(),
        ];
        let binds = vec![
            "/home/user/Documents:/mnt/docs".to_string(),
            "/home/user/.ssh:/mnt/ssh".to_string(),
            "/home/user/.env:/mnt/env".to_string(),
        ];
        let result = find_secret_binds(&binds, &secrets);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"/home/user/.ssh".to_string()));
        assert!(result.contains(&"/home/user/.env".to_string()));
    }

    #[test]
    fn test_find_secret_binds_clean() {
        let secrets = vec!["/home/user/.env".to_string()];
        let binds = vec!["/data:/mnt/data".to_string()];
        assert_eq!(find_secret_binds(&binds, &secrets), Vec::<String>::new());
    }

    #[test]
    fn test_find_secret_mounts() {
        let secrets = vec![
            "/home/user/.env".to_string(),
            "/home/user/.ssh/id_rsa".to_string(),
        ];
        let mounts = vec![
            Mount {
                mount_type: "bind".into(),
                source: "/home/user/.env".into(),
                target: "/app/.env".into(),
            },
            Mount {
                mount_type: "bind".into(),
                source: "/home/user/.ssh/id_rsa".into(),
                target: "/app/.ssh".into(),
            },
            Mount {
                mount_type: "volume".into(),
                source: "".into(),
                target: "/data".into(),
            },
        ];
        let result = find_secret_mounts(&mounts, &secrets);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"/home/user/.env".to_string()));
        assert!(result.contains(&"/home/user/.ssh/id_rsa".to_string()));
    }

    #[test]
    fn test_find_secret_mounts_no_bind() {
        let secrets = vec!["/home/user/.env".to_string()];
        let mounts = vec![Mount {
            mount_type: "volume".into(),
            source: "".into(),
            target: "/data".into(),
        }];
        assert_eq!(find_secret_mounts(&mounts, &secrets), Vec::<String>::new());
    }
}
