use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Read a newline-delimited file of secret file paths and return the paths.
pub fn read_secret_paths(path: &str) -> Result<Vec<String>> {
    let data =
        fs::read_to_string(path).with_context(|| format!("read secret paths from {path}"))?;
    Ok(data
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

#[derive(Deserialize)]
struct BetterleaksFinding {
    #[serde(rename = "File")]
    file: String,
}

/// Parse betterleaks JSON output and return deduplicated, sorted file paths.
pub fn parse_betterleaks_output(stdout: &[u8]) -> Result<Vec<String>> {
    let trimmed = std::str::from_utf8(stdout)
        .with_context(|| "betterleaks output is not valid UTF-8")?
        .trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(vec![]);
    }
    let findings: Vec<BetterleaksFinding> =
        serde_json::from_str(trimmed).with_context(|| "parse betterleaks output")?;

    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<String> = findings
        .into_iter()
        .filter(|f| !f.file.is_empty() && seen.insert(f.file.clone()))
        .map(|f| f.file)
        .collect();
    files.sort();
    Ok(files)
}

/// Run betterleaks to detect secret files in the given directory.
pub fn run_betterleaks(dir: &str, tool_path: &str) -> Result<Vec<String>> {
    let output = Command::new(tool_path)
        .args([
            "dir",
            "--no-banner",
            "--no-color",
            "--max-target-megabytes",
            "1",
            "--redact",
            "-l",
            "error",
            "--exit-code",
            "0",
            "-f",
            "json",
            "-r",
            "-",
            dir,
        ])
        .output()
        .with_context(|| format!("execute {tool_path}"))?;

    parse_betterleaks_output(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_read_secret_paths() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secrets.txt");
        fs::write(
            &path,
            "/home/user/.ssh/id_rsa\n/home/user/.env\n\n/home/user/.gnupg\n",
        )
        .unwrap();
        let got = read_secret_paths(path.to_str().unwrap()).unwrap();
        assert_eq!(
            got,
            vec![
                "/home/user/.ssh/id_rsa",
                "/home/user/.env",
                "/home/user/.gnupg"
            ]
        );
    }

    #[test]
    fn test_read_secret_paths_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.txt");
        fs::write(&path, "").unwrap();
        let got = read_secret_paths(path.to_str().unwrap()).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn test_read_secret_paths_missing() {
        let result = read_secret_paths("/nonexistent/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_betterleaks_bad_tool_path() {
        let result = run_betterleaks("/tmp", "/nonexistent/betterleaks");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_betterleaks_empty() {
        let result = parse_betterleaks_output(b"[]");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_betterleaks_null() {
        let result = parse_betterleaks_output(b"null");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_betterleaks_empty_string() {
        let result = parse_betterleaks_output(b"");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_betterleaks_invalid_json() {
        let result = parse_betterleaks_output(b"not-json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_betterleaks_valid() {
        let input = br#"[{"File":"/s1.txt"},{"File":"/s2.txt"}]"#;
        let result = parse_betterleaks_output(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["/s1.txt", "/s2.txt"]);
    }

    #[test]
    fn test_parse_betterleaks_dedup() {
        let input = br#"[{"File":"/dup.txt"},{"File":"/dup.txt"}]"#;
        let result = parse_betterleaks_output(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["/dup.txt"]);
    }

    #[test]
    fn test_parse_betterleaks_empty_file_field() {
        let input = br#"[{"File":"/a.txt"},{"File":""},{"File":"/b.txt"}]"#;
        let result = parse_betterleaks_output(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["/a.txt", "/b.txt"]);
    }
}
