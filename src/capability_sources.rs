use std::collections::HashMap;

use crate::config::EnvValue;
use crate::sandbox::Mount;

/// A source that provides capabilities (mounts and env vars) to the sandbox.
///
/// Every source implements both methods. Sources that don't provide
/// a dimension return an empty collection.
pub trait CapabilitySource {
    fn mounts(&self) -> Vec<Mount>;
    fn env(&self) -> HashMap<String, EnvValue>;
}

/// Collect mounts from all sources.
pub fn collect_mounts(sources: &[&dyn CapabilitySource]) -> Vec<Mount> {
    sources.iter().flat_map(|s| s.mounts()).collect()
}

/// Collect environment variables from all sources.
pub fn collect_env(sources: &[&dyn CapabilitySource]) -> HashMap<String, EnvValue> {
    let mut env = HashMap::new();
    for s in sources {
        env.extend(s.env());
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EnvValue;
    use crate::sandbox::Mount;

    // Mock source that provides fixed mounts and env.
    struct MockSource {
        mounts: Vec<Mount>,
        env: HashMap<String, EnvValue>,
    }

    impl CapabilitySource for MockSource {
        fn mounts(&self) -> Vec<Mount> {
            self.mounts.clone()
        }
        fn env(&self) -> HashMap<String, EnvValue> {
            self.env.clone()
        }
    }

    #[test]
    fn test_collect_mounts_ordering() {
        let a = MockSource {
            mounts: vec![Mount::ro("/b", "/b")],
            env: HashMap::new(),
        };
        let b = MockSource {
            mounts: vec![Mount::ro("/a", "/a")],
            env: HashMap::new(),
        };
        let mounts = collect_mounts(&[&a, &b]);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].host, "/b");
        assert_eq!(mounts[1].host, "/a");
    }

    #[test]
    fn test_collect_env_last_wins() {
        let a = MockSource {
            mounts: vec![],
            env: [("KEY".to_string(), EnvValue::Static("a".to_string()))].into(),
        };
        let b = MockSource {
            mounts: vec![],
            env: [("KEY".to_string(), EnvValue::Static("b".to_string()))].into(),
        };
        let env = collect_env(&[&a, &b]);
        assert_eq!(env.get("KEY").unwrap().resolve().unwrap(), "b");
    }

    #[test]
    fn test_collect_mounts_empty() {
        let mounts = collect_mounts(&[]);
        assert!(mounts.is_empty());
    }

    #[test]
    fn test_collect_env_empty() {
        let env = collect_env(&[]);
        assert!(env.is_empty());
    }
}
