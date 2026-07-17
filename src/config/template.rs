use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;

use super::{AgentConfig, EnvValue, MountConfig, SandboxConfig, TemplateVars};

/// Template vars regex: {var}
#[allow(clippy::expect_used)]
static TEMPLATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{(\w+)\}").expect("valid regex"));

/// Expand template variables in a string using the given TemplateVars.
pub fn expand_string(s: &str, v: &TemplateVars) -> Result<String> {
    for caps in TEMPLATE_RE.captures_iter(s) {
        let name = caps.get(1).map_or(s, |m| m.as_str());
        match name {
            "home" | "user" | "cwd" | "xdg_runtime" | "xdg_state" | "log_path" => {}
            unknown => {
                anyhow::bail!("unknown template variable {{{unknown}}} in configuration");
            }
        }
    }
    Ok(TEMPLATE_RE
        .replace_all(s, |caps: &regex::Captures| {
            caps.get(1).map_or_else(
                || s.to_string(),
                |m| match m.as_str() {
                    "home" => v.home.clone(),
                    "user" => v.user.clone(),
                    "cwd" => v.cwd.clone(),
                    "xdg_runtime" => v.xdg_runtime.clone(),
                    "xdg_state" => v.xdg_state.clone(),
                    "log_path" => v.log_path.clone(),
                    _ => m.as_str().to_string(),
                },
            )
        })
        .to_string())
}

/// Expand templates in an EnvValue.
fn expand_env_value(ev: &EnvValue, v: &TemplateVars) -> Result<EnvValue> {
    match ev {
        EnvValue::Static(s) => Ok(EnvValue::Static(expand_string(s, v)?)),
        EnvValue::Command { command } => Ok(EnvValue::Command {
            command: expand_string(command, v)?,
        }),
    }
}

/// Expand templates in mount list and env map.
pub fn expand_mounts(
    mounts: Vec<crate::sandbox::Mount>,
    env: HashMap<String, EnvValue>,
    v: &TemplateVars,
) -> Result<(Vec<crate::sandbox::Mount>, HashMap<String, EnvValue>)> {
    let expanded_mounts = mounts
        .into_iter()
        .map(|m| {
            Ok(crate::sandbox::Mount {
                host: expand_string(&m.host, v)?,
                target: expand_string(&m.target, v)?,
                mode: m.mode,
                secrets_policy: m.secrets_policy,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let expanded_env = env
        .into_iter()
        .map(|(k, val)| Ok((expand_string(&k, v)?, expand_env_value(&val, v)?)))
        .collect::<Result<HashMap<_, _>>>()?;
    Ok((expanded_mounts, expanded_env))
}

/// Expanded agent mounts and environment.
pub type ExpandedAgent = (Vec<MountConfig>, HashMap<String, EnvValue>);

/// Expand templates in agent mount list (Vec<MountConfig>) and env map.
pub fn expand_agent_mounts(
    mounts: Vec<MountConfig>,
    env: HashMap<String, EnvValue>,
    v: &TemplateVars,
) -> Result<ExpandedAgent> {
    let expanded = mounts
        .into_iter()
        .map(|m| {
            Ok(MountConfig {
                path: expand_string(&m.path, v)?,
                target: m.target.map(|t| expand_string(&t, v)).transpose()?,
                mode: m.mode,
                secrets_policy: m.secrets_policy,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let expanded_env = env
        .into_iter()
        .map(|(k, val)| Ok((expand_string(&k, v)?, expand_env_value(&val, v)?)))
        .collect::<Result<HashMap<_, _>>>()?;
    Ok((expanded, expanded_env))
}

/// Expand template variables in config string fields.
pub fn expand_vars(input: SandboxConfig, vars: &TemplateVars) -> Result<SandboxConfig> {
    let (mounts, env) = expand_mounts(input.mounts, input.env, vars)?;
    let chdir = expand_string(&input.chdir, vars)?;

    let agent = AgentConfig {
        binary: expand_string(&input.agent.binary, vars)?,
        chdir: expand_string(&input.agent.chdir, vars)?,
        mounts: input.agent.mounts,
        env: input.agent.env,
        lightweight_args: input.agent.lightweight_args,
    };
    let (agent_mounts, agent_env) = expand_agent_mounts(agent.mounts, agent.env, vars)?;

    let agent = AgentConfig {
        mounts: agent_mounts,
        env: agent_env,
        ..agent
    };

    Ok(SandboxConfig {
        agent,
        chdir,
        mounts,
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MountConfig;
    use crate::config::SandboxConfig;

    fn test_vars() -> TemplateVars {
        TemplateVars {
            home: "/home/testuser".to_string(),
            user: "testuser".to_string(),
            cwd: "/project".to_string(),
            xdg_runtime: "/tmp".to_string(),
            xdg_state: "/home/testuser/.local/state".to_string(),
            log_path: "/tmp/logs/app.log".to_string(),
        }
    }

    #[test]
    fn test_expand_vars() {
        use crate::sandbox::Mount;
        let input = SandboxConfig {
            agent: AgentConfig {
                binary: "opencode".to_string(),
                mounts: vec![MountConfig::new("{home}/.config/app")],
                env: [(
                    "LOG".to_string(),
                    EnvValue::Static("{log_path}".to_string()),
                )]
                .into(),
                ..Default::default()
            },
            chdir: "/project".to_string(),
            mounts: vec![Mount::ro("{home}/.config/common", "{home}/.config/common")],
            env: [(
                "COMMON_LOG".to_string(),
                EnvValue::Static("{log_path}".to_string()),
            )]
            .into(),
        };

        let result = expand_vars(input, &test_vars()).unwrap();

        assert_eq!(result.mounts[0].host, "/home/testuser/.config/common");
        assert_eq!(result.mounts[0].target, "/home/testuser/.config/common");
        assert_eq!(
            result.env.get("COMMON_LOG").unwrap().resolve().unwrap(),
            "/tmp/logs/app.log"
        );
        assert_eq!(
            result.agent.mounts[0],
            MountConfig::new("/home/testuser/.config/app")
        );
        assert_eq!(
            result.agent.env.get("LOG").unwrap().resolve().unwrap(),
            "/tmp/logs/app.log"
        );
    }

    #[test]
    fn test_expand_env_value_command() {
        let result = expand_vars(
            SandboxConfig {
                agent: AgentConfig::default(),
                chdir: "/".to_string(),
                mounts: vec![],
                env: [(
                    "CMD_VAR".to_string(),
                    EnvValue::Command {
                        command: "echo expanded".to_string(),
                    },
                )]
                .into(),
            },
            &test_vars(),
        )
        .unwrap();

        assert_eq!(
            result.env.get("CMD_VAR").unwrap().resolve().unwrap(),
            "expanded"
        );
    }

    #[test]
    fn test_expand_unknown_template_variable() {
        use crate::sandbox::Mount;
        let result = expand_vars(
            SandboxConfig {
                agent: AgentConfig::default(),
                chdir: "/".to_string(),
                mounts: vec![Mount::ro("{unknown_var}/data", "{unknown_var}/data")],
                env: HashMap::new(),
            },
            &test_vars(),
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown template variable"));
    }
}
