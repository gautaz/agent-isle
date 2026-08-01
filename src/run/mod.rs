use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process;

use agent_isle::{
    capability_sources, config, platform, sandbox, secrets, tools, user_profile, util,
};

mod launch;
mod setup;

type SourceEnv = HashMap<String, config::EnvValue>;
type ShutdownHook = (usize, Box<dyn FnOnce()>);
type SourceResult = (
    Vec<sandbox::Mount>,
    SourceEnv,
    Vec<Box<dyn tools::Tool>>,
    String,
    String,
);

fn expand_sandbox_config(
    agent: config::AgentConfig,
    cfg: &config::Config,
    setup: &setup::RuntimeSetup,
    log_path: &Path,
) -> Result<config::SandboxConfig, i32> {
    let raw_chdir = if !agent.chdir.is_empty() {
        agent.chdir.clone()
    } else if !cfg.chdir.is_empty() {
        cfg.chdir.clone()
    } else {
        "/".to_string()
    };
    config::expand_vars(
        config::SandboxConfig {
            agent,
            chdir: raw_chdir,
            mounts: cfg.mounts.iter().map(|m| m.to_mount()).collect(),
            env: cfg.env.clone(),
        },
        &config::TemplateVars {
            home: setup.home.clone(),
            user: setup.user.clone(),
            cwd: setup.pwd.clone(),
            xdg_runtime: setup.xdg_runtime.clone(),
            xdg_state: setup.state_home.clone(),
            log_path: log_path.to_string_lossy().to_string(),
        },
    )
    .map_err(|e| {
        tracing::error!(error = %e, "template expansion failed");
        1
    })
}

fn collect_sources<'a>(
    sb_config: &'a config::SandboxConfig,
    os_cfg: &'a dyn platform::OSConfig,
    cfg: &'a config::Config,
    home: &str,
    xdg_runtime: &str,
    rundir: &str,
) -> SourceResult {
    let agent_binary = sb_config.agent.binary.clone();
    let chdir = sb_config.chdir.clone();
    let agent_source = config::AgentSource::new(&sb_config.agent);
    let platform_source = platform::PlatformSource { os_cfg };
    let config_source = config::ConfigSource::new(config::SandboxConfig {
        agent: sb_config.agent.clone(),
        chdir: sb_config.chdir.clone(),
        mounts: sb_config.mounts.clone(),
        env: sb_config.env.clone(),
    });
    let compiled_tools = tools::registered_tools(&cfg.tools, rundir);
    let tool_caps: Vec<&dyn capability_sources::CapabilitySource> = compiled_tools
        .iter()
        .filter_map(|t| t.capabilities())
        .collect();
    let user_profile_source =
        user_profile::UserProfileSource::new(cfg.path_secrets_policy, home, xdg_runtime);
    let all_sources: Vec<&dyn capability_sources::CapabilitySource> = [
        vec![
            &platform_source as &dyn capability_sources::CapabilitySource,
            &agent_source,
        ],
        tool_caps,
        vec![&user_profile_source, &config_source],
    ]
    .concat();
    let sources_mounts = capability_sources::collect_mounts(&all_sources);
    let sources_env = capability_sources::collect_env(&all_sources);
    (
        sources_mounts,
        sources_env,
        compiled_tools,
        agent_binary,
        chdir,
    )
}

fn start_tools(
    compiled_tools: &mut [Box<dyn tools::Tool>],
    secret_files: &[String],
) -> Result<Vec<ShutdownHook>, i32> {
    let mut shutdown_hooks: Vec<(usize, Box<dyn FnOnce()>)> = Vec::new();
    for (i, tool) in compiled_tools.iter_mut().enumerate() {
        match tool.start(secret_files) {
            Ok(Some(hook)) => {
                shutdown_hooks.push((i, hook));
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(tool = tool.id(), error = %e, "tool start failed");
                return Err(1);
            }
        }
    }
    Ok(shutdown_hooks)
}

pub(crate) fn run(
    cfg: &config::Config,
    agent: config::AgentConfig,
    os_cfg: &dyn platform::OSConfig,
    agent_args: &[String],
    dry_run: bool,
    log_path: &Path,
) -> i32 {
    let setup = match setup::setup_runtime(dry_run) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let _guard = scopeguard::guard((), |_| {
        util::cleanup_stale_dirs(&setup.rundir_base, setup.my_pid);
        util::sync_and_close(log_path);
        let _ = fs::remove_dir_all(&setup.rundir);
    });
    let sb_config = match expand_sandbox_config(agent, cfg, &setup, log_path) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (sources_mounts, sources_env, mut compiled_tools, agent_binary, chdir) = collect_sources(
        &sb_config,
        os_cfg,
        cfg,
        &setup.home,
        &setup.xdg_runtime,
        &setup.rundir,
    );
    let secret_files = match scan_secrets(&sources_mounts, &cfg.betterleaks_path) {
        Ok(files) => files,
        Err(e) => {
            tracing::error!("{e}");
            return 1;
        }
    };
    tracing::info!(
        count = secret_files.len(),
        "secret files detected in mounts"
    );
    let shutdown_hooks = match start_tools(&mut compiled_tools, &secret_files) {
        Ok(hooks) => hooks,
        Err(code) => return code,
    };
    let secrets_mounts = os_cfg.secret_mounts(&secret_files);
    tracing::info!(count = secret_files.len(), "secret files masked");
    let all_mounts = [sources_mounts, secrets_mounts].concat();
    launch::launch_sandbox(launch::LaunchConfig {
        bwrap_path: &cfg.bwrap_path,
        mounts: &all_mounts,
        env: &sources_env,
        chdir: &chdir,
        agent_binary,
        agent_args,
        shutdown_hooks,
        dry_run,
    })
}

pub(crate) fn run_cmd_bare(
    bwrap_path: &str,
    agent: &config::AgentConfig,
    os_cfg: &dyn platform::OSConfig,
    args: &[String],
) -> i32 {
    let mut bwrap_args = sandbox::build_minimal_args(os_cfg);
    bwrap_args.push(agent.binary.clone());
    bwrap_args.extend(args.iter().cloned());

    let status = process::Command::new(bwrap_path)
        .args(&bwrap_args)
        .stdin(process::Stdio::inherit())
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .status();

    match status {
        Ok(s) => {
            let code = s.code().unwrap_or_else(|| {
                tracing::warn!("bwrap exited with signal");
                1
            });
            code
        }
        Err(e) => {
            tracing::error!("failed to launch bwrap: {e}");
            1
        }
    }
}

/// Scan mount paths for secrets using betterleaks.
fn scan_secrets(mounts: &[sandbox::Mount], betterleaks: &str) -> Result<Vec<String>, String> {
    use sandbox::SecretsPolicy;
    let paths: Vec<&str> = mounts
        .iter()
        .filter(|m| m.secrets_policy == SecretsPolicy::Mask)
        .map(|m| m.target.as_str())
        .collect();
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    secrets::run_betterleaks_on_paths(&paths, betterleaks).map_err(|e| format!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_agent(binary: &str) -> config::AgentConfig {
        config::AgentConfig {
            binary: binary.to_string(),
            lightweight_args: vec![],
            ..Default::default()
        }
    }

    fn mock_betterleaks() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("betterleaks");
        let shell = which_bin("sh");
        std::fs::write(&script, format!("#!{shell}\necho '[]'")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, script.to_str().unwrap().to_string())
    }

    fn which_bin(name: &str) -> String {
        for shell in &["sh", "/bin/sh"] {
            let o = match std::process::Command::new(shell)
                .args(["-c", &format!("command -v {name}")])
                .output()
            {
                Ok(o) => o,
                Err(_) => continue,
            };
            if !o.status.success() {
                continue;
            }
            if let Ok(path) = String::from_utf8(o.stdout) {
                let path = path.trim().to_string();
                if !path.is_empty() {
                    return path;
                }
            }
        }
        format!("/usr/bin/{name}")
    }

    fn which_true() -> String {
        which_bin("true")
    }
    fn which_false() -> String {
        which_bin("false")
    }

    fn cfg_with_bwrap(bwrap: &str) -> (tempfile::TempDir, config::Config, config::AgentConfig) {
        let (bl_dir, bl_path) = mock_betterleaks();
        let mut cfg = config::default();
        cfg.agent = "opencode".to_string();
        cfg.bwrap_path = bwrap.to_string();
        cfg.betterleaks_path = bl_path;
        cfg.agents
            .insert("opencode".to_string(), mock_agent("/test/bin/opencode"));
        cfg.tools = serde_yml::Mapping::from_iter([(
            serde_yml::Value::String("podman".to_string()),
            serde_yml::Value::Mapping(serde_yml::Mapping::from_iter([(
                serde_yml::Value::String("enabled".to_string()),
                serde_yml::Value::Bool(false),
            )])),
        )]);
        let agent = cfg.agents.remove("opencode").unwrap();
        let runtime_dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", runtime_dir.path());
        (bl_dir, cfg, agent)
    }

    #[test]
    fn test_run_dry_run() {
        let (_bl_dir, cfg, agent) = cfg_with_bwrap("/test/bin/bwrap");
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            true,
            Path::new("/dev/null"),
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_betterleaks_failure() {
        let mut cfg = config::default();
        cfg.agent = "opencode".to_string();
        cfg.bwrap_path = "/test/bin/bwrap".to_string();
        cfg.betterleaks_path = "/nonexistent/betterleaks".to_string();
        cfg.mounts = vec![config::MountConfig::new("/some/path")];
        cfg.agents
            .insert("opencode".to_string(), mock_agent("/test/bin/opencode"));
        let agent = cfg.agents.remove("opencode").unwrap();
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            true,
            Path::new("/dev/null"),
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_bwrap_success() {
        let (_bl_dir, cfg, agent) = cfg_with_bwrap(&which_true());
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Path::new("/dev/null"),
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_bwrap_failure() {
        let (_bl_dir, cfg, agent) = cfg_with_bwrap(&which_false());
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Path::new("/dev/null"),
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_bwrap_not_found() {
        let (_bl_dir, cfg, agent) = cfg_with_bwrap("/nonexistent/bwrap");
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Path::new("/dev/null"),
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_cmd_bare_success() {
        let agent = mock_agent("/test/bin/opencode");
        let os_cfg = platform::detect();
        let code = run_cmd_bare(&which_true(), &agent, os_cfg.as_ref(), &["--help".into()]);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_cmd_bare_failure() {
        let agent = mock_agent("/test/bin/opencode");
        let os_cfg = platform::detect();
        let code = run_cmd_bare(&which_false(), &agent, os_cfg.as_ref(), &[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_with_podman_proxy() {
        let dir = tempfile::tempdir().unwrap();
        let xdg_runtime = dir.path().to_str().unwrap();
        let podman_socket = format!("{xdg_runtime}/podman/podman.sock");
        std::fs::create_dir_all(format!("{xdg_runtime}/podman")).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(&podman_socket).unwrap();

        let (_bl_dir, mut cfg, agent) = cfg_with_bwrap(&which_true());
        let tools_map = serde_yml::Mapping::from_iter([(
            serde_yml::Value::String("podman".to_string()),
            serde_yml::Value::Mapping(serde_yml::Mapping::from_iter([(
                serde_yml::Value::String("enabled".to_string()),
                serde_yml::Value::Bool(true),
            )])),
        )]);
        cfg.tools = tools_map;

        let os_cfg = platform::detect();

        // Override xdg_runtime for this test.
        let old_xdg = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("XDG_RUNTIME_DIR", xdg_runtime);
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Path::new("/dev/null"),
        );
        match old_xdg {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_unknown_template_variable() {
        let (_bl_dir, mut cfg, agent) = cfg_with_bwrap(&which_true());
        cfg.mounts = vec![config::MountConfig::new("{unknown_var}/data")];
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Path::new("/dev/null"),
        );
        assert_eq!(code, 1);
    }
}
