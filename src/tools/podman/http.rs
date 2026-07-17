use std::path::{Path, PathBuf};

/// Extract a version prefix (e.g. "v5") from a URL path.
pub fn extract_api_version(path: &str) -> String {
    let base = path.trim_start_matches('/');
    let first = base.split('/').next().unwrap_or("");
    if first.len() > 1 && first.starts_with('v') {
        if let Some(&c) = first.as_bytes().get(1) {
            if c.is_ascii_digit() {
                return first.to_string();
            }
        }
    }
    String::new()
}

/// Report whether the request is a container create operation.
pub fn is_create_op(method: &str, path: &str) -> bool {
    if method != "POST" {
        return false;
    }
    let mut base = path.trim_start_matches('/');
    if let Some(ver) = extract_api_version(path).into() {
        let ver: String = ver;
        base = base
            .strip_prefix(&ver)
            .unwrap_or(base)
            .trim_start_matches('/');
    }
    base == "containers/create" || base.ends_with("/containers/create")
}

/// Normalize an absolute path by resolving `.` and `..` segments.
/// Panics in debug builds if given a relative path.
pub fn normalize_absolute_path(path: &str) -> String {
    assert!(path.starts_with('/'), "expected absolute path, got: {path}");
    std::path::PathBuf::from(path)
        .clean()
        .to_string_lossy()
        .to_string()
}

trait PathClean {
    fn clean(&self) -> PathBuf;
}

impl PathClean for Path {
    fn clean(&self) -> PathBuf {
        let mut components = Vec::new();
        for comp in self.components() {
            match comp {
                std::path::Component::ParentDir => {
                    if matches!(components.last(), Some(std::path::Component::Normal(_))) {
                        components.pop();
                    }
                }
                std::path::Component::Normal(_) | std::path::Component::RootDir => {
                    components.push(comp);
                }
                _ => {}
            }
        }
        components.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_api_version() {
        assert_eq!(extract_api_version("/v5/containers/create"), "v5");
        assert_eq!(
            extract_api_version("/v1.25/libpod/containers/create"),
            "v1.25"
        );
        assert_eq!(extract_api_version("/containers/create"), "");
        assert_eq!(extract_api_version("/foo/bar"), "");
    }

    #[test]
    fn test_is_create_op() {
        assert!(is_create_op("POST", "/containers/create"));
        assert!(is_create_op("POST", "/v5/containers/create"));
        assert!(is_create_op("POST", "/v5/libpod/containers/create"));
        assert!(!is_create_op("GET", "/containers/create"));
        assert!(!is_create_op("POST", "/containers/list"));
    }

    #[test]
    fn test_normalize_absolute_path() {
        assert_eq!(normalize_absolute_path("/a/b/../c"), "/a/c");
        assert_eq!(normalize_absolute_path("/a/./b"), "/a/b");
        assert_eq!(normalize_absolute_path("/a/b/../../c"), "/c");
        assert_eq!(normalize_absolute_path("/a/b/../../../c"), "/c");
        assert_eq!(normalize_absolute_path("/a/b/c"), "/a/b/c");
        assert_eq!(normalize_absolute_path("/"), "/");
    }

    #[test]
    fn test_normalize_absolute_path_trailing_slash() {
        assert_eq!(normalize_absolute_path("/a/b/c/"), "/a/b/c");
        assert_eq!(normalize_absolute_path("/a/b/../"), "/a");
        assert_eq!(normalize_absolute_path("/a/./"), "/a");
    }

    #[test]
    #[should_panic(expected = "expected absolute path")]
    fn test_normalize_absolute_path_rejects_relative() {
        normalize_absolute_path("a/b/../c");
    }
}
