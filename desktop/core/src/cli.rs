//! Detects installed `claude` CLIs (used by the JetBrains integration), e.g. those older than the tested version.

use crate::{git, paths, update};

/// Oldest version verified to resolve project settings from the main worktree root (docs/feasibility.md).
pub const MINIMUM_TESTED_VERSION: &str = "2.1.288";

fn candidates() -> Vec<std::path::PathBuf> {
    let home = paths::home_dir();
    if cfg!(windows) {
        vec![home.join(r".local\bin\claude.exe")]
    } else {
        vec![
            home.join(".local/bin/claude"),
            home.join(".claude/local/claude"),
            "/opt/homebrew/bin/claude".into(),
            "/usr/local/bin/claude".into(),
        ]
    }
}

/// `(path, version)` of every installed CLI.
fn installations() -> Vec<(String, String)> {
    candidates()
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|path| {
            let output = git::command(&path.to_string_lossy()).arg("--version").stdin(std::process::Stdio::null()).output().ok()?;
            let version = String::from_utf8_lossy(&output.stdout).split_whitespace().next()?.to_string();
            Some((path.to_string_lossy().into_owned(), version))
        })
        .collect()
}

/// `(path, version)` of every installed CLI older than `minimum`.
pub fn outdated_installations(minimum: &str) -> Vec<(String, String)> {
    installations().into_iter().filter(|(_, version)| update::is_older(version, minimum)).collect()
}

/// `(path, version)` of the CLI the native installer set up, else of the first one found.
pub fn installed() -> Option<(String, String)> {
    installations().into_iter().next()
}
