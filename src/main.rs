use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use agent_isle::{config, platform, sandbox, secrets, tools, util};

#[derive(Parser)]
#[command(
    name = "agent-isle",
    about = "Run AI coding agents in a sandboxed environment",
    version,
    after_help = "Bundled agent presets: opencode"
)]
struct Cli {
    /// Agent name (selects preset)
    #[arg(short = 'a', long = "agent")]
    agent: Option<String>,

    /// Config file path
    #[arg(short = 'c', long = "config")]
    config: Option<String>,

    /// Print bwrap args without executing
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Arguments forwarded to the agent
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

/// Detect whether the current invocation is a symlink-mode call.
///
/// Returns `(agent_name, args)` for symlink mode, or `(None, [])` for normal mode.
fn detect_symlink_mode(args: &[String]) -> (Option<String>, Vec<String>) {
    let exec_name = args
        .first()
        .map(PathBuf::from)
        .and_then(|p| p.file_name().map(|n| n.to_os_string()))
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let is_symlink = exec_name != "agent-isle" && !exec_name.is_empty();

    if is_symlink {
        (Some(exec_name), args[1..].to_vec())
    } else {
        (None, vec![])
    }
}

fn main() {
    let real_args: Vec<String> = env::args().collect();
    let (symlink_agent, symlink_args) = detect_symlink_mode(&real_args);

    let (agent_name, config_path, dry_run, agent_args) = if let Some(name) = symlink_agent {
        (Some(name), None, false, symlink_args)
    } else {
        let cli = Cli::parse();
        (cli.agent, cli.config, cli.dry_run, cli.args)
    };

    // Load and merge configuration.
    let cfg = match load_config(config_path.as_deref(), agent_name.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: config: {e}");
            process::exit(1);
        }
    };

    // Require agent selection.
    if cfg.agent.is_empty() {
        eprintln!("error: no agent specified");
        eprintln!("  use --agent <name>, set \"agent:\" in config, or create a symlink");
        eprintln!("  available presets: {}", config::list_presets().join(", "));
        process::exit(1);
    }

    let mut cfg = cfg;

    // Apply agent preset if specified.
    {
        let agent_name = cfg.agent.clone();
        if let Err(e) = config::apply_preset(&agent_name, &mut cfg) {
            eprintln!("error: config: {e}");
            process::exit(1);
        }
    }

    // Validate configuration after preset application.
    if let Err(e) = config::validate(&cfg) {
        eprintln!("error: config: {e}");
        process::exit(1);
    }

    // Extract selected agent config for downstream use.
    let agent = match cfg.agents.remove(&cfg.agent) {
        Some(a) => a,
        None => {
            eprintln!("error: agent {:?} not found", cfg.agent);
            process::exit(1);
        }
    };

    // Detect platform.
    let os_cfg = platform::detect();

    // Lightweight mode: run with minimal sandbox for agent-specific flags.
    if agent.is_lightweight_op(&agent_args) {
        process::exit(run_cmd_bare(
            &cfg.bwrap_path,
            &agent,
            os_cfg.as_ref(),
            &agent_args,
        ));
    }

    process::exit(run(
        &cfg,
        agent,
        os_cfg.as_ref(),
        &agent_args,
        dry_run,
        None,
    ));
}

fn run(
    cfg: &config::Config,
    agent: config::AgentConfig,
    os_cfg: &dyn platform::OSConfig,
    agent_args: &[String],
    dry_run: bool,
    runtime_dir: Option<&str>,
) -> i32 {
    let pwd = env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let xdg_runtime = runtime_dir
        .map(|s| s.to_string())
        .unwrap_or_else(util::xdg_runtime_dir);
    let state_home = util::xdg_state_home();

    // Create run directory.
    let rundir_base = format!("{xdg_runtime}/agent-isle");
    let my_pid = process::id();
    let rundir = format!("{rundir_base}/{my_pid}");
    if let Err(e) = fs::create_dir_all(&rundir) {
        eprintln!("failed to create rundir: {e}");
        return 1;
    }

    // Set up log file.
    let log_dir = format!("{state_home}/agent-isle/logs");
    let _ = fs::create_dir_all(&log_dir);
    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
    let log_path = format!("{log_dir}/{timestamp}_{my_pid}.log");

    // Initialize tracing with stderr + file layers.
    let log_file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("failed to create log file: {e}");
            None
        }
    };

    let file_layer = log_file.map(|f| fmt::layer().with_writer(f).with_ansi(false));
    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .try_init();

    info!("starting agent-isle (pid={my_pid})");

    // Single deferred cleanup.
    let log_path_clone = log_path.clone();
    let rundir_clone = rundir.clone();
    let rundir_base_clone = rundir_base.clone();
    let _guard = scopeguard::guard((), |_| {
        util::cleanup_stale_dirs(&rundir_base_clone, my_pid);
        util::sync_and_close(Path::new(&log_path_clone));
        let _ = fs::remove_dir_all(&rundir_clone);
    });

    // Expand template variables in config.
    let home = util::home_dir().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        process::exit(1);
    });
    let user = util::username().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        process::exit(1);
    });
    let sb_config = match config::expand_vars(
        config::SandboxConfig {
            agent,
            mounts: cfg
                .ro_mounts
                .iter()
                .map(|s| sandbox::Mount::ro(s, s))
                .chain(cfg.rw_mounts.iter().map(|s| sandbox::Mount::rw(s, s)))
                .collect(),
            env: cfg.env.clone(),
        },
        &config::TemplateVars {
            home: home.clone(),
            user: user.clone(),
            cwd: pwd.clone(),
            xdg_runtime: xdg_runtime.clone(),
            xdg_state: state_home,
            log_path,
        },
    ) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "template expansion failed");
            return 1;
        }
    };

    // Run secret detection.
    let secret_files = match secrets::run_betterleaks(&pwd, &cfg.betterleaks_path) {
        Ok(files) => files,
        Err(e) => {
            tracing::error!(error = %e, "secret detection failed");
            return 1;
        }
    };
    tracing::info!(count = secret_files.len(), "secret files masked");

    // Convert secret file paths to mask mounts via the platform.
    let secret_mounts = os_cfg.secret_mounts(&secret_files);

    // Set up Podman proxy if available.
    let mut proxy_stop: Option<Box<dyn FnOnce()>> = None;
    let mut proxy_socket_path: Option<String> = None;

    if cfg.tools.podman.enabled == Some(true) {
        let podman_socket = cfg
            .tools
            .podman
            .socket_path
            .clone()
            .unwrap_or_else(|| format!("{xdg_runtime}/podman/podman.sock"));
        if let Err(e) = util::validate_socket_ownership(&podman_socket) {
            tracing::warn!(error = %e, "podman socket ownership check failed");
        } else if Path::new(&podman_socket).exists() {
            let secrets_file = format!("{rundir}/secrets");
            let secrets_content: String = secret_files.iter().map(|s| format!("{s}\n")).collect();
            if fs::write(&secrets_file, &secrets_content).is_err() {
                tracing::error!("failed to write secrets file");
                return 1;
            }

            let proxy_socket = format!("{rundir}/proxy.sock");
            let file_secrets = match secrets::read_secret_paths(&secrets_file) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(error = %e, "failed to read secret paths");
                    return 1;
                }
            };
            match tools::start_proxy(&proxy_socket, &podman_socket, file_secrets) {
                Ok(stop) => {
                    proxy_stop = Some(Box::new(stop));
                    proxy_socket_path = Some(proxy_socket.clone());
                    tracing::info!(socket = %proxy_socket, "podman proxy started");

                    // Wait for proxy socket to appear.
                    for i in 0..20 {
                        if Path::new(&proxy_socket).exists() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        if i == 19 {
                            tracing::warn!("proxy socket did not appear in time");
                        }
                    }
                }
                Err(_e) => {
                    tracing::error!("failed to start proxy");
                    return 1;
                }
            }
        }
    }

    // Assemble all mounts from every module.
    let mut all_mounts: Vec<sandbox::Mount> = Vec::new();
    // Platform mounts (infrastructure, OS-specific)
    all_mounts.extend(os_cfg.mounts(&home, &user));
    // Sandbox mounts (PWD, cache)
    match sandbox::sandbox_mounts(&pwd) {
        Ok(mounts) => all_mounts.extend(mounts),
        Err(e) => {
            tracing::error!(error = %e, "failed to build sandbox mounts");
            return 1;
        }
    }
    // Config mounts
    all_mounts.extend(sb_config.mounts);
    // Agent mounts (convert host=target strings to typed Mount)
    all_mounts.extend(
        sb_config
            .agent
            .ro_mounts
            .iter()
            .map(|s| sandbox::Mount::ro(s, s)),
    );
    all_mounts.extend(
        sb_config
            .agent
            .rw_mounts
            .iter()
            .map(|s| sandbox::Mount::rw(s, s)),
    );
    // Secret mask mounts
    all_mounts.extend(secret_mounts);
    // Proxy bind mount (if active)
    if let Some(ref socket) = proxy_socket_path {
        all_mounts.push(sandbox::Mount::rw(socket, "/tmp/podman-proxy.sock"));
    }

    // Assemble all environment variables from every module.
    let mut all_env: std::collections::HashMap<String, config::EnvValue> = os_cfg
        .env(&xdg_runtime)
        .into_iter()
        .map(|(k, v)| (k, config::EnvValue::Static(v)))
        .collect();
    // Config env
    all_env.extend(sb_config.env);
    // Agent env
    for (k, v) in &sb_config.agent.env {
        all_env.insert(k.clone(), v.clone());
    }
    // CONTAINER_HOST (only if proxy is active)
    if proxy_socket_path.is_some() {
        all_env.insert(
            "CONTAINER_HOST".into(),
            config::EnvValue::Static("unix:///tmp/podman-proxy.sock".into()),
        );
    }

    // Build bwrap args.
    let mut bwrap_args = sandbox::build_args(sandbox::BuildArgs {
        mounts: &all_mounts,
        env: &all_env,
        pwd: &pwd,
    });
    bwrap_args.push(sb_config.agent.binary.clone());
    bwrap_args.extend(agent_args.iter().cloned());

    if dry_run {
        println!("{} {}", cfg.bwrap_path, bwrap_args.join(" "));
        return 0;
    }

    // Start the sandboxed process.
    tracing::info!(binary = %cfg.bwrap_path, "starting bwrap");
    let status = process::Command::new(&cfg.bwrap_path)
        .args(&bwrap_args)
        .stdin(process::Stdio::inherit())
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .status();

    // Stop proxy after bwrap finishes.
    if let Some(stop) = proxy_stop.take() {
        stop();
    }

    match status {
        Ok(s) => {
            tracing::info!(code = s.code().unwrap_or(-1), "bwrap exited");
            s.code().unwrap_or(1)
        }
        Err(e) => {
            tracing::error!("failed to start bwrap: {e}");
            1
        }
    }
}

fn run_cmd_bare(
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
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => 1,
    }
}

/// Build the final config by merging defaults, preset, and user config.
fn load_config(config_path: Option<&str>, agent_name: Option<&str>) -> Result<config::Config> {
    // Start with defaults.
    let mut cfg = config::default();

    // Set agent name from flag (empty means detect from executable).
    if let Some(name) = agent_name {
        cfg.agent = name.to_string();
    }

    // If no config path provided, check default location.
    let config_path = match config_path {
        Some(p) => p.to_string(),
        None => {
            let xdg_config = util::xdg_config_home();
            let default_path = format!("{xdg_config}/agent-isle/config.yml");
            if Path::new(&default_path).exists() {
                default_path
            } else {
                String::new()
            }
        }
    };

    // Load user config file and merge on top.
    if !config_path.is_empty() {
        let user_cfg = config::load(&config_path)
            .with_context(|| format!("load config from {config_path}"))?;
        cfg = config::merge(&cfg, Some(&user_cfg));
    }

    // CLI flag takes precedence over config file.
    if let Some(name) = agent_name {
        if !name.is_empty() {
            cfg.agent = name.to_string();
        }
    }

    // Auto-detect Podman if not explicitly configured.
    if cfg.tools.podman.enabled.is_none() {
        let podman_socket = cfg.tools.podman.socket_path.clone().unwrap_or_else(|| {
            let xdg_runtime = util::xdg_runtime_dir();
            format!("{xdg_runtime}/podman/podman.sock")
        });
        let socket_ok = Path::new(&podman_socket).exists()
            && util::validate_socket_ownership(&podman_socket).is_ok();
        cfg.tools.podman.enabled = Some(socket_ok);
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_detect_symlink_mode_normal() {
        let args = vec![
            "agent-isle".into(),
            "--agent".into(),
            "opencode".into(),
            "--".into(),
            "--help".into(),
        ];
        let (name, remaining) = detect_symlink_mode(&args);
        assert_eq!(name, None);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_detect_symlink_mode_symlink() {
        let args = vec!["opencode".into(), "--help".into(), "foo.rs".into()];
        let (name, remaining) = detect_symlink_mode(&args);
        assert_eq!(name.as_deref(), Some("opencode"));
        assert_eq!(remaining, vec!["--help", "foo.rs"]);
    }

    #[test]
    fn test_detect_symlink_mode_empty() {
        let (name, remaining) = detect_symlink_mode(&[]);
        assert_eq!(name, None);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_detect_symlink_mode_agent_isle_no_args() {
        let args = vec!["agent-isle".into()];
        let (name, remaining) = detect_symlink_mode(&args);
        assert_eq!(name, None);
        assert!(remaining.is_empty());
    }

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
        let result = load_config(None, None);
        // Either config loading fails, or the loaded config has an empty agent.
        assert!(result.is_err() || result.unwrap().agent.is_empty());
    }

    #[test]
    fn test_run_dry_run() {
        let dir = test_rundir();
        let (_bl_dir, cfg, agent) = cfg_with_bwrap("/test/bin/bwrap");
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            true,
            Some(dir.path().to_str().unwrap()),
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_betterleaks_failure() {
        let dir = test_rundir();
        let mut cfg = config::default();
        cfg.agent = "opencode".to_string();
        cfg.bwrap_path = "/test/bin/bwrap".to_string();
        cfg.betterleaks_path = "/nonexistent/betterleaks".to_string();
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
            Some(dir.path().to_str().unwrap()),
        );
        assert_eq!(code, 1);
    }

    fn which_true() -> String {
        std::process::Command::new("which")
            .arg("true")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "/usr/bin/true".to_string())
    }

    fn which_false() -> String {
        std::process::Command::new("which")
            .arg("false")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "/usr/bin/false".to_string())
    }

    fn cfg_with_bwrap(bwrap: &str) -> (tempfile::TempDir, config::Config, config::AgentConfig) {
        let (bl_dir, bl_path) = mock_betterleaks();
        let mut cfg = config::default();
        cfg.agent = "opencode".to_string();
        cfg.bwrap_path = bwrap.to_string();
        cfg.betterleaks_path = bl_path;
        cfg.agents
            .insert("opencode".to_string(), mock_agent("/test/bin/opencode"));
        let agent = cfg.agents.remove("opencode").unwrap();
        (bl_dir, cfg, agent)
    }

    /// Create a unique temp dir for test isolation.
    /// Each test calling `run()` needs a unique rundir to avoid parallel
    /// test interference (all tests share the same PID).
    fn test_rundir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_run_bwrap_success() {
        let dir = test_rundir();
        let (_bl_dir, cfg, agent) = cfg_with_bwrap(&which_true());
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Some(dir.path().to_str().unwrap()),
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_bwrap_failure() {
        let dir = test_rundir();
        let (_bl_dir, cfg, agent) = cfg_with_bwrap(&which_false());
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Some(dir.path().to_str().unwrap()),
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_bwrap_not_found() {
        let dir = test_rundir();
        let (_bl_dir, cfg, agent) = cfg_with_bwrap("/nonexistent/bwrap");
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Some(dir.path().to_str().unwrap()),
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
        std::fs::write(&script, "#!/bin/sh\necho '[]'").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, script.to_str().unwrap().to_string())
    }

    #[test]
    fn test_run_with_podman_proxy() {
        let dir = test_rundir();
        let xdg_runtime = dir.path().to_str().unwrap();
        let podman_socket = format!("{xdg_runtime}/podman/podman.sock");
        std::fs::create_dir_all(format!("{xdg_runtime}/podman")).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(&podman_socket).unwrap();

        let (_bl_dir, mut cfg, agent) = cfg_with_bwrap(&which_true());
        cfg.tools.podman.enabled = Some(true);

        let os_cfg = platform::detect();

        let code = run(&cfg, agent, os_cfg.as_ref(), &[], false, Some(xdg_runtime));
        assert_eq!(code, 0);
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

        let old_config = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", dir.path());
        let result = load_config(None, Some("opencode"));
        match old_config {
            Some(v) => env::set_var("XDG_CONFIG_HOME", v),
            None => env::remove_var("XDG_CONFIG_HOME"),
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

    #[test]
    fn test_run_unknown_template_variable() {
        let dir = test_rundir();
        let (_bl_dir, mut cfg, agent) = cfg_with_bwrap(&which_true());
        cfg.ro_mounts = vec!["{unknown_var}/data".to_string()];
        let os_cfg = platform::detect();
        let code = run(
            &cfg,
            agent,
            os_cfg.as_ref(),
            &[],
            false,
            Some(dir.path().to_str().unwrap()),
        );
        assert_eq!(code, 1);
    }
}
