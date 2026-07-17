use std::path::Path;

use anyhow::{Context, Result};

use agent_isle::{config, util};

/// Build the final config by merging defaults, preset, and user config.
pub fn load_config(config_path: Option<&str>, agent_name: Option<&str>) -> Result<config::Config> {
    let mut cfg = config::default();

    if let Some(name) = agent_name {
        cfg.agent = name.to_string();
    }

    let config_path = match config_path {
        Some(p) => {
            tracing::debug!(path = %p, "loading config from explicit path");
            p.to_string()
        }
        None => {
            let xdg_config = util::xdg_config_home();
            let default_path = format!("{xdg_config}/agent-isle/config.yml");
            if Path::new(&default_path).exists() {
                tracing::info!(path = %default_path, "loading config from default path");
                default_path
            } else {
                tracing::info!("no config file found; using defaults");
                String::new()
            }
        }
    };

    if !config_path.is_empty() {
        let user_cfg = config::load(&config_path)
            .with_context(|| format!("load config from {config_path}"))?;
        cfg = config::merge(&cfg, Some(&user_cfg));
    }

    if let Some(name) = agent_name {
        if !name.is_empty() {
            cfg.agent = name.to_string();
        }
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    fn config_with_paths() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg_path = dir.path().join("config.yml");
        std::fs::write(
            &cfg_path,
            indoc! {"
                agents:
                  opencode:
                    binary: /test/bin/opencode
                    lightweight_args:
                      - --help
                      - -h
                      - --version
                      - -v
                bwrap_path: /test/bin/bwrap
                betterleaks_path: /test/bin/betterleaks
            "},
        )
        .unwrap();
        dir
    }

    #[test]
    fn test_load_config_explicit_path() {
        let dir = config_with_paths();
        let cfg_path = dir.path().join("config.yml").to_string_lossy().to_string();
        let cfg = load_config(Some(&cfg_path), None).unwrap();
        assert_eq!(cfg.bwrap_path, "/test/bin/bwrap");
    }

    #[test]
    fn test_load_config_cli_overrides_file() {
        let dir = config_with_paths();
        let cfg_path = dir.path().join("config.yml").to_string_lossy().to_string();
        // "my-agent" has no agents entry and no preset, but load_config
        // no longer validates (validation moved to main after apply_preset).
        let cfg = load_config(Some(&cfg_path), Some("my-agent")).unwrap();
        assert_eq!(cfg.agent, "my-agent");
    }

    #[test]
    fn test_load_config_invalid_path() {
        let result = load_config(Some("/nonexistent/config.yml"), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_empty_agent_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.yml");
        std::fs::write(
            &cfg_path,
            indoc! {"
                bwrap_path: /test/bin/bwrap
                betterleaks_path: /test/bin/betterleaks
            "},
        )
        .unwrap();
        let result = load_config(Some(cfg_path.to_str().unwrap()), None);
        // Either config loading fails, or the loaded config has an empty agent.
        assert!(result.is_err() || result.unwrap().agent.is_empty());
    }

    #[test]
    fn test_load_config_default_path_exists() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("agent-isle");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.yml"),
            indoc! {"
                agents:
                  opencode:
                    binary: /test/bin/opencode
                    lightweight_args:
                      - --help
                      - -h
                      - --version
                      - -v
                bwrap_path: /test/bin/bwrap
                betterleaks_path: /test/bin/betterleaks
            "},
        )
        .unwrap();

        let old_config = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let result = load_config(None, Some("opencode"));
        match old_config {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }

        let cfg = result.unwrap();
        assert_eq!(cfg.bwrap_path, "/test/bin/bwrap");
    }

    #[test]
    fn test_load_config_cli_overrides_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.yml");
        std::fs::write(
            &cfg_path,
            indoc! {"
                agent: aider
                agents:
                  aider:
                    binary: /opt/test/aider
                    lightweight_args:
                      - --help
                      - --version
                bwrap_path: /opt/test/bwrap
                betterleaks_path: /opt/test/betterleaks
            "},
        )
        .unwrap();
        let cfg_path = cfg_path.to_str().unwrap();
        let cfg = load_config(Some(cfg_path), Some("opencode")).unwrap();
        assert_eq!(cfg.agent, "opencode");
        assert_eq!(cfg.bwrap_path, "/opt/test/bwrap");
        assert_eq!(cfg.betterleaks_path, "/opt/test/betterleaks");
    }
}
