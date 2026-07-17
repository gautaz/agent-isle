use std::env;
use std::fs;
use std::process;

use tracing::info;

use agent_isle::util;

pub(crate) struct RuntimeSetup {
    pub(crate) pwd: String,
    pub(crate) home: String,
    pub(crate) user: String,
    pub(crate) xdg_runtime: String,
    pub(crate) state_home: String,
    pub(crate) rundir: String,
    pub(crate) rundir_base: String,
    pub(crate) my_pid: u32,
}

pub(crate) fn setup_runtime(dry_run: bool) -> Result<RuntimeSetup, i32> {
    let pwd = env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| {
            tracing::warn!("current_dir failed, using '.' as working directory");
            ".".to_string()
        });
    let home = util::home_dir().unwrap_or_else(|e| {
        tracing::error!("{e}");
        process::exit(1);
    });
    let user = util::username().unwrap_or_else(|e| {
        tracing::error!("{e}");
        process::exit(1);
    });
    let xdg_runtime = util::xdg_runtime_dir();
    let state_home = util::xdg_state_home();
    let rundir_base = format!("{xdg_runtime}/agent-isle");
    let my_pid = process::id();
    let rundir = format!("{rundir_base}/{my_pid}");
    if dry_run {
        tracing::debug!("dry-run mode, skipping rundir creation");
    } else if let Err(e) = fs::create_dir_all(&rundir) {
        tracing::error!("failed to create rundir: {e}");
        return Err(1);
    }
    info!("starting agent-isle (pid={my_pid})");
    Ok(RuntimeSetup {
        pwd,
        home,
        user,
        xdg_runtime,
        state_home,
        rundir,
        rundir_base,
        my_pid,
    })
}
