use assert_cmd::Command;
use indoc::indoc;
use predicates::prelude::PredicateBooleanExt;

#[test]
fn test_cli_help() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Run AI coding agents"))
        .stderr(predicates::str::is_empty());
}

#[test]
fn test_cli_help_with_flags() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--agent", "opencode", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Run AI coding agents"))
        .stderr(predicates::str::is_empty());
}

#[test]
fn test_cli_version() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")))
        .stderr(predicates::str::is_empty());
}

#[test]
fn test_cli_version_with_flags() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--agent", "opencode", "--version"])
        .assert()
        .success()
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")))
        .stderr(predicates::str::is_empty());
}

#[test]
fn test_cli_help_over_version() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--help", "--version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Run AI coding agents"))
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")).not())
        .stderr(predicates::str::is_empty());
}

#[test]
fn test_cli_no_agent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(
        &path,
        indoc! {"\
            bwrap_path: /usr/bin/bwrap
            betterleaks_path: /usr/bin/betterleaks"},
    )
    .unwrap();

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("error: no agent specified"));
}

#[test]
fn test_cli_agent_flag() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--agent", "opencode", "--dry-run", "--help"])
        .assert()
        .success();
}

#[test]
fn test_cli_config_flag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(
        &path,
        indoc! {"\
            agent: opencode
            bwrap_path: /usr/bin/bwrap
            betterleaks_path: /usr/bin/betterleaks"},
    )
    .unwrap();

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", path.to_str().unwrap(), "--dry-run", "--help"])
        .assert()
        .success();
}

#[test]
fn test_cli_dry_run() {
    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--agent", "opencode", "--dry-run", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--help"));
}

#[test]
fn test_cli_invalid_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.yml");
    std::fs::write(&path, "not: valid: yaml: [[[[").unwrap();

    Command::cargo_bin("agent-isle")
        .unwrap()
        .args(["--config", path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("error:"));
}
