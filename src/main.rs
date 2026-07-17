mod load_config;
mod run;

use std::env;
use std::path::Path;
use std::process;

use clap::{CommandFactory, FromArgMatches, Parser};

use agent_isle::{config, logging, platform, tools, util};

#[derive(Parser)]
#[command(
    name = "agent-isle",
    about = "Run AI coding agents in a sandboxed environment",
    version
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

fn parse_args() -> (Option<String>, Option<String>, bool, Vec<String>) {
    let real_args: Vec<String> = env::args().collect();
    let (symlink_agent, symlink_args) = util::detect_symlink_mode(&real_args);
    if let Some(name) = symlink_agent {
        return (Some(name), None, false, symlink_args);
    }
    let presets = config::list_presets().join(", ");
    let tool_ids = tools::list_compiled_tool_ids();
    let tools_str = if tool_ids.is_empty() {
        String::new()
    } else {
        format!("\nBundled tools: {}", tool_ids.join(", "))
    };
    let cmd = Cli::command().after_help(format!("Bundled agent presets: {presets}{tools_str}"));
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    (cli.agent, cli.config, cli.dry_run, cli.args)
}

fn load_and_validate(
    config_path: Option<&str>,
    agent_name: Option<&str>,
) -> (config::Config, config::AgentConfig) {
    let mut cfg = match load_config::load_config(config_path, agent_name) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("config: {e}");
            process::exit(1);
        }
    };
    if cfg.agent.is_empty() {
        tracing::error!("no agent specified");
        tracing::error!("  use --agent <name>, set \"agent:\" in config, or create a symlink");
        tracing::error!("  available presets: {}", config::list_presets().join(", "));
        process::exit(1);
    }
    let agent_name = cfg.agent.clone();
    if let Err(e) = config::apply_preset(&agent_name, &mut cfg) {
        tracing::error!("config: {e}");
        process::exit(1);
    }
    if let Err(e) = config::validate(&cfg) {
        tracing::error!("config: {e}");
        process::exit(1);
    }
    let known_ids = tools::list_compiled_tool_ids();
    if let Err(e) = tools::validate_tool_config(&cfg.tools, &known_ids) {
        tracing::error!("{e}");
        process::exit(1);
    }
    let agent = match cfg.agents.remove(&cfg.agent) {
        Some(a) => a,
        None => {
            tracing::error!("agent {:?} not found", cfg.agent);
            process::exit(1);
        }
    };
    (cfg, agent)
}

fn main() {
    let (agent_name, config_path, dry_run, agent_args) = parse_args();
    let state_home = util::xdg_state_home();
    let my_pid = std::process::id();
    let log_path = logging::init_logging(my_pid, &state_home)
        .unwrap_or_else(|| Path::new("/dev/null").to_path_buf());
    let (cfg, agent) = load_and_validate(config_path.as_deref(), agent_name.as_deref());
    let os_cfg = platform::detect();
    if agent.is_lightweight_op(&agent_args) {
        tracing::info!("running in lightweight mode");
        process::exit(run::run_cmd_bare(
            &cfg.bwrap_path,
            &agent,
            os_cfg.as_ref(),
            &agent_args,
        ));
    }
    process::exit(run::run(
        &cfg,
        agent,
        os_cfg.as_ref(),
        &agent_args,
        dry_run,
        &log_path,
    ));
}
