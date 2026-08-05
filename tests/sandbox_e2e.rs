use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use indoc::indoc;
use predicates::prelude::*;
use tempfile::TempDir;

const MOCK_BL_BIN: &str = r#"for a; do d="$a"; done
[ -d "$d" ] || { echo '[]'; exit 0; }
find "$d" -name ".env" -exec printf '{"File":"%s"}\n' {} + 2>/dev/null | paste -sd, | sed 's/^/[/; s/$/]/'
"#;

/// Mock bwrap that parses its options the way real bubblewrap does
/// (each option consumes a fixed number of following args, regardless of
/// leading dashes) and reports which "program" it would exec inside the sandbox.
const MOCK_BWRAP_BIN: &str = r#"skip=0
for a in "$@"; do
  if [ "$skip" -gt 0 ]; then
    skip=$((skip - 1))
    continue
  fi
  case "$a" in
    --proc|--dev|--tmpfs|--chdir) skip=1 ;;
    --ro-bind|--bind|--setenv) skip=2 ;;
    --*) ;;
    *) printf 'program=%s\n' "$a"; exit 0 ;;
  esac
done
printf 'program=\n'
"#;

fn mock_betterleaks(dir: &Path) -> PathBuf {
    let path = dir.join("bl-mock");
    std::fs::write(&path, format!("#!{}\n{MOCK_BL_BIN}", sh_path())).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn mock_bwrap(dir: &Path) -> PathBuf {
    let path = dir.join("bwrap-mock");
    std::fs::write(&path, format!("#!{}\n{MOCK_BWRAP_BIN}", sh_path())).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn sh_path() -> String {
    let probe = std::process::Command::new("sh")
        .args(["-c", "command -v sh"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| "/bin/sh".to_string());
    probe.trim().to_string()
}

fn base_config(bwrap: &str, bl: &Path, mnt_path: &str) -> String {
    let shell = sh_path();
    format!(
        indoc! {r#"
            agent: opencode
            bwrap_path: {}
            betterleaks_path: {}
            mounts:
              - path: {}
                mode: rw
            agents:
              opencode:
                binary: {}
                lightweight_args: []
        "#},
        bwrap,
        bl.display(),
        mnt_path,
        shell,
    )
}

#[test]
fn test_dry_run_masks_secret_env_files() {
    let tmp = TempDir::new().unwrap();
    let mnt = TempDir::new().unwrap();

    std::fs::write(mnt.path().join(".env"), b"API_KEY=super-secret").unwrap();
    std::fs::write(mnt.path().join("README.md"), b"safe file").unwrap();

    let bl = mock_betterleaks(tmp.path());
    let config_path = tmp.path().join("config.yml");
    std::fs::write(
        &config_path,
        base_config("/usr/bin/bwrap", &bl, &mnt.path().to_string_lossy()),
    )
    .unwrap();

    let secret_path = mnt.path().join(".env").to_string_lossy().to_string();

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", config_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--bind")
                .and(predicate::str::contains("/dev/null"))
                .and(predicate::str::contains(&secret_path)),
        );
}

#[test]
fn test_dry_run_show_policy_skips_masking() {
    let tmp = TempDir::new().unwrap();
    let mnt = TempDir::new().unwrap();

    std::fs::write(mnt.path().join(".env"), b"API_KEY=visible").unwrap();

    let bl = mock_betterleaks(tmp.path());
    let config_path = tmp.path().join("config.yml");
    let config = format!(
        indoc! {r#"
            agent: opencode
            bwrap_path: /usr/bin/bwrap
            betterleaks_path: {}
            mounts:
              - path: {}
                mode: rw
                secrets_policy: show
            agents:
              opencode:
                binary: /bin/sh
                lightweight_args: []
        "#},
        bl.display(),
        mnt.path().display(),
    );
    std::fs::write(&config_path, &config).unwrap();

    let mount_str = mnt.path().to_string_lossy().to_string();

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", config_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(mount_str.as_str())
                .and(predicate::str::contains("--bind /dev/null").not()),
        );
}

// ---------------------------------------------------------------------------
// Tests below require bwrap at runtime.
// ---------------------------------------------------------------------------

fn bwrap_path() -> Option<String> {
    let probe = std::process::Command::new("sh")
        .args(["-c", "command -v bwrap"])
        .output()
        .ok()?;
    if !probe.status.success() {
        return None;
    }
    let path = String::from_utf8(probe.stdout).ok()?;
    let path = path.trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn is_bwrap_sandbox() -> bool {
    let root_src = std::fs::read_to_string("/proc/self/mountinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find(|line| line.split_whitespace().nth(4) == Some("/"))
                .and_then(|line| line.split_whitespace().nth(3).map(str::to_string))
        });
    let is_bwrap_root = matches!(root_src.as_deref(), Some("/newroot" | "/oldroot"));
    if !is_bwrap_root {
        return false;
    }
    let is_userns = std::fs::read_to_string("/proc/self/uid_map")
        .map(|s| s.trim() != "0 4294967295 4294967295")
        .unwrap_or(false);
    is_bwrap_root && is_userns
}

#[test]
fn test_sandbox_masks_secret_files() {
    let bwrap = match bwrap_path() {
        Some(p) => p,
        None => {
            eprintln!("skipping: bwrap not found");
            return;
        }
    };
    if is_bwrap_sandbox() {
        eprintln!("skipping: nested bwrap sandbox not supported");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let mnt = TempDir::new().unwrap();

    std::fs::write(mnt.path().join(".env"), b"API_KEY=should-not-leak").unwrap();

    let bl = mock_betterleaks(tmp.path());
    let config_path = tmp.path().join("config.yml");
    std::fs::write(
        &config_path,
        base_config(&bwrap, &bl, &mnt.path().to_string_lossy()),
    )
    .unwrap();

    let env_path = format!("{}/.env", mnt.path().display());
    let cmd = format!("cat {env_path}");

    let output = Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", config_path.to_str().unwrap(), "--", "-c", &cmd])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("should-not-leak"),
        "secret leaked in stdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("should-not-leak"),
        "secret leaked in stderr:\n{stderr}"
    );
}

#[test]
fn test_sandbox_post_start_gap() {
    let bwrap = match bwrap_path() {
        Some(p) => p,
        None => {
            eprintln!("skipping: bwrap not found");
            return;
        }
    };
    if is_bwrap_sandbox() {
        eprintln!("skipping: nested bwrap sandbox not supported");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let mnt = TempDir::new().unwrap();

    let bl = mock_betterleaks(tmp.path());
    let config_path = tmp.path().join("config.yml");
    std::fs::write(
        &config_path,
        base_config(&bwrap, &bl, &mnt.path().to_string_lossy()),
    )
    .unwrap();

    let mount_str = mnt.path().to_string_lossy().to_string();
    let cmd = format!(
        "echo NEW_SECRET=new_value > {0}/.env && cat {0}/.env",
        mount_str
    );

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", config_path.to_str().unwrap(), "--", "-c", &cmd])
        .assert()
        .success()
        .stdout(predicate::str::contains("NEW_SECRET=new_value"));
}

#[test]
fn test_sandbox_deletion_recreation_gap() {
    let bwrap = match bwrap_path() {
        Some(p) => p,
        None => {
            eprintln!("skipping: bwrap not found");
            return;
        }
    };
    if is_bwrap_sandbox() {
        eprintln!("skipping: nested bwrap sandbox not supported");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let mnt = TempDir::new().unwrap();

    std::fs::write(mnt.path().join(".env"), b"OLD_KEY=old_value").unwrap();

    let bl = mock_betterleaks(tmp.path());
    let config_path = tmp.path().join("config.yml");
    std::fs::write(
        &config_path,
        base_config(&bwrap, &bl, &mnt.path().to_string_lossy()),
    )
    .unwrap();

    let mount_str = mnt.path().to_string_lossy().to_string();
    let cmd = format!(
        "rm {mount_str}/.env 2>/dev/null; echo RECREATED_KEY=recreated > {mount_str}/.env && cat {mount_str}/.env",
    );

    let output = Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", config_path.to_str().unwrap(), "--", "-c", &cmd])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The old secret must not leak even when deletion+recreation is attempted
    assert!(
        !stdout.contains("OLD_KEY"),
        "old secret leaked in stdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("OLD_KEY"),
        "old secret leaked in stderr:\n{stderr}"
    );
}

/// Lightweight mode (`--version`) must emit well-formed bwrap options: every
/// `--ro-bind` must be followed by both SRC and DEST, so bwrap execs the agent
/// binary rather than a misaligned mount path. The mock bwrap reports which
/// program a real bwrap would try to exec.
#[test]
fn test_lightweight_version_execs_agent_binary() {
    let tmp = TempDir::new().unwrap();
    let bwrap = mock_bwrap(tmp.path());

    let config_path = tmp.path().join("config.yml");
    std::fs::write(
        &config_path,
        format!(
            indoc! {r#"
                agent: opencode
                bwrap_path: {}
                betterleaks_path: {}
                agents:
                  opencode:
                    binary: /bin/sh
                    lightweight_args:
                      - --version
            "#},
            bwrap.display(),
            sh_path(),
        ),
    )
    .unwrap();

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", config_path.to_str().unwrap(), "--", "--version"])
        .assert()
        .success()
        .stdout(predicate::str::contains("program=/bin/sh"));
}
