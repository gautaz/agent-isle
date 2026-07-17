pub mod podman;

use anyhow::Result;

use crate::capability_sources::CapabilitySource;

/// A pluggable tool that extends the sandbox with capabilities.
///
/// Tools provide mounts/env via [`capabilities()`](Tool::capabilities) **before**
/// starting. [`start()`](Tool::start) only launches the tool and returns an
/// optional shutdown hook — never mounts or env.
pub trait Tool: Send {
    /// Identifier matching the config key under `tools:` (e.g. "podman").
    fn id(&self) -> &str;

    /// The tool's capability source, if available.
    ///
    /// Returns `Some(self as &dyn CapabilitySource)` when the tool is enabled
    /// and its underlying resource exists (e.g. podman socket found).
    /// Returns `None` when the tool should not contribute mounts or env.
    fn capabilities(&self) -> Option<&dyn CapabilitySource>;

    /// Start the tool (e.g. launch a podman proxy) and return an optional shutdown hook.
    fn start(&mut self, secret_files: &[String]) -> Result<Option<Box<dyn FnOnce()>>>;
}

/// Return all tools compiled into this binary, configured from the tools config mapping.
pub fn registered_tools(tools_config: &serde_yml::Mapping, rundir: &str) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    #[cfg(feature = "podman")]
    {
        let config = tools_config.get("podman").cloned().unwrap_or_default();
        tools.push(Box::new(podman::PodmanTool::new(config, rundir)));
    }
    tools
}

/// Return IDs of all tools compiled into this binary.
pub fn list_compiled_tool_ids() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut ids = Vec::new();
    #[cfg(feature = "podman")]
    {
        ids.push("podman");
    }
    ids
}

/// Validate that config keys under `tools:` match compiled-in tool IDs.
pub fn validate_tool_config(tools_config: &serde_yml::Mapping, known_ids: &[&str]) -> Result<()> {
    for key in tools_config.keys() {
        match key.as_str() {
            Some(name) => {
                if !known_ids.contains(&name) {
                    anyhow::bail!(
                        "tool \"{name}\" is not available\n  \
                         try: cargo build --features {name}"
                    );
                }
            }
            None => {
                anyhow::bail!("invalid tool config key (expected a string tool name)");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tool_config_empty() {
        let tools_config = serde_yml::Mapping::new();
        let known_ids = vec!["podman"];
        assert!(validate_tool_config(&tools_config, &known_ids).is_ok());
    }

    #[test]
    fn test_validate_tool_config_known() {
        let mut tools_config = serde_yml::Mapping::new();
        tools_config.insert(
            serde_yml::Value::String("podman".to_string()),
            serde_yml::Value::Bool(true),
        );
        let known_ids = vec!["podman"];
        assert!(validate_tool_config(&tools_config, &known_ids).is_ok());
    }

    #[test]
    fn test_validate_tool_config_unknown() {
        let mut tools_config = serde_yml::Mapping::new();
        tools_config.insert(
            serde_yml::Value::String("docker".to_string()),
            serde_yml::Value::Bool(true),
        );
        let known_ids = vec!["podman"];
        let err = validate_tool_config(&tools_config, &known_ids)
            .unwrap_err()
            .to_string();
        assert!(err.contains("docker"));
        assert!(err.contains("not available"));
    }

    #[test]
    fn test_validate_tool_config_non_string_key() {
        let mut tools_config = serde_yml::Mapping::new();
        tools_config.insert(serde_yml::Value::Bool(false), serde_yml::Value::Bool(true));
        let known_ids: Vec<&str> = vec![];
        let err = validate_tool_config(&tools_config, &known_ids)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid tool config key"));
    }

    #[cfg(feature = "podman")]
    #[test]
    fn test_registered_tools_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let tools = registered_tools(&serde_yml::Mapping::new(), dir.path().to_str().unwrap());
        assert!(!tools.is_empty());
    }
}
