use std::fs;
use std::path::PathBuf;

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Initialize tracing with file + stderr layers.
///
/// Normal mode (file created):
///   - debug/info: log file only
///   - warn/error/fatal: log file + stderr
///
/// Fallback mode (file creation failed):
///   - All levels to stderr
///
/// Returns the log file path on success, or `None` if the log file couldn't be created.
pub fn init_logging(pid: u32, state_home: &str) -> Option<PathBuf> {
    let log_dir = format!("{state_home}/agent-isle/logs");
    let _ = fs::create_dir_all(&log_dir);
    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
    let log_path = PathBuf::from(format!("{log_dir}/{timestamp}_{pid}.log"));

    if let Ok(log_file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let file_layer = fmt::layer().with_writer(log_file).with_ansi(false);
        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .with_filter(LevelFilter::WARN);

        let _ = tracing_subscriber::registry()
            .with(file_layer)
            .with(stderr_layer)
            .try_init()
            .inspect_err(|_| {
                tracing::warn!(
                    "tracing subscriber already initialized; file logging may not be captured"
                )
            });

        Some(log_path)
    } else {
        let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(false);

        let _ = tracing_subscriber::registry().with(stderr_layer).try_init();

        tracing::warn!("failed to create log file at {log_path:?}");
        None
    }
}
