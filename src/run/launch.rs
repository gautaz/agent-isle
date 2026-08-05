use std::process;

use agent_isle::sandbox;

use super::{ShutdownHook, SourceEnv};

pub(crate) struct LaunchConfig<'a> {
    pub(crate) bwrap_path: &'a str,
    pub(crate) mounts: &'a [sandbox::Mount],
    pub(crate) env: &'a SourceEnv,
    pub(crate) chdir: &'a str,
    pub(crate) agent_binary: String,
    pub(crate) agent_args: &'a [String],
    pub(crate) shutdown_hooks: Vec<ShutdownHook>,
    pub(crate) dry_run: bool,
}

pub(crate) fn launch_sandbox(params: LaunchConfig<'_>) -> i32 {
    let bwrap_args = sandbox::build_args(sandbox::BuildArgs {
        mounts: params.mounts,
        env: params.env,
        chdir: params.chdir,
    })
    .exec(params.agent_binary, params.agent_args.iter().cloned());
    if params.dry_run {
        println!("{} {}", params.bwrap_path, bwrap_args.join(" "));
        return 0;
    }
    tracing::info!(binary = %params.bwrap_path, "starting bwrap");
    let status = process::Command::new(params.bwrap_path)
        .args(&bwrap_args)
        .stdin(process::Stdio::inherit())
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .status();
    for (_i, hook) in params.shutdown_hooks.into_iter().rev() {
        hook();
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
