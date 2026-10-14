//! MCP servers per project: `<root>/.mcp.json` (git-excluded like the local settings) plus `enabledMcpjsonServers`
//! in `.claude/settings.local.json`, so Claude Code loads them without asking. Reverts exactly what it wrote.

use crate::errors::{Error, Result, SettingsConflict};
use crate::models::McpServer;
use crate::settings::{self, LOCAL_SETTINGS_PATH};
use crate::{git, l};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const MCP_PATH: &str = ".mcp.json";
pub const GIT_EXCLUDE_LINE: &str = "/.mcp.json";
const ENABLED_KEY: &str = "enabledMcpjsonServers";

pub struct McpApplyResult {
    /// The entries written, by server name.
    pub managed: BTreeMap<String, Value>,
    pub git_exclude_entry: Option<String>,
    pub changed: bool,
}

fn servers_of(file: &serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    file.get("mcpServers").and_then(Value::as_object).cloned().unwrap_or_default()
}

fn set_servers(file: &mut serde_json::Map<String, Value>, servers: serde_json::Map<String, Value>) {
    if servers.is_empty() {
        file.remove("mcpServers");
    } else {
        file.insert("mcpServers".into(), Value::Object(servers));
    }
}

/// Rewrites `enabledMcpjsonServers` in the local settings: `remove` names go, `add` names come (deduplicated).
fn update_enabled(root: &str, remove: &[String], add: &[String]) -> Result<bool> {
    settings::mutate_settings(&Path::new(root).join(LOCAL_SETTINGS_PATH), |s| {
        let mut enabled: Vec<String> = s
            .get(ENABLED_KEY)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        enabled.retain(|n| !remove.contains(n) || add.contains(n));
        for name in add {
            if !enabled.contains(name) {
                enabled.push(name.clone());
            }
        }
        if enabled.is_empty() {
            s.remove(ENABLED_KEY);
        } else {
            s.insert(ENABLED_KEY.into(), enabled.into());
        }
        Ok(())
    })
}

/// Writes `servers` into the project's `.mcp.json` and enables them in the local settings. Entries the app wrote
/// before (`previous`) but that are no longer wanted are removed if still unchanged; foreign entries stay.
pub fn apply(root: &str, servers: &[McpServer], previous: &BTreeMap<String, Value>, previous_exclude: Option<&str>) -> Result<McpApplyResult> {
    if !Path::new(root).is_dir() {
        return Err(Error::FolderNotFound(root.to_string()));
    }
    let desired: BTreeMap<String, Value> = servers.iter().map(|s| (s.name.clone(), s.definition())).collect();
    if !desired.is_empty() && git::status(&["ls-files", "--error-unmatch", MCP_PATH], root) == 0 {
        return Err(Error::SettingsConflicts(vec![SettingsConflict::blocking(l!(
            "%@ ist im Git-Repository eingecheckt und würde geteilt. Server dort bitte manuell eintragen.", MCP_PATH
        ))]));
    }
    let names: Vec<String> = desired.keys().cloned().collect();
    let previous_names: Vec<String> = previous.keys().cloned().collect();
    let changed = settings::with_lock(|| {
        let mut changed = settings::mutate_settings(&Path::new(root).join(MCP_PATH), |file| {
            let mut current = servers_of(file);
            let conflicts: Vec<SettingsConflict> = desired
                .iter()
                .filter(|(name, def)| current.get(*name).is_some_and(|c| c != *def && Some(c) != previous.get(*name)))
                .map(|(name, _)| SettingsConflict::blocking(l!("%@: Server „%@“ ist dort bereits anders definiert. Bitte manuell entfernen oder umbenennen.", MCP_PATH, name)))
                .collect();
            if !conflicts.is_empty() {
                return Err(Error::SettingsConflicts(conflicts));
            }
            for (name, def) in &desired {
                current.insert(name.clone(), def.clone());
            }
            for (name, def) in previous {
                if !desired.contains_key(name) && current.get(name) == Some(def) {
                    current.remove(name);
                }
            }
            set_servers(file, current);
            Ok(())
        })?;
        changed |= update_enabled(root, &previous_names, &names)?;
        Ok(changed)
    })?;
    let git_exclude_entry = if desired.is_empty() {
        if let Some(line) = previous_exclude {
            settings::remove_git_exclude(root, line)?;
        }
        None
    } else {
        settings::ensure_git_excluded(root, MCP_PATH, GIT_EXCLUDE_LINE)?.or_else(|| previous_exclude.map(str::to_string))
    };
    Ok(McpApplyResult { managed: desired, git_exclude_entry, changed })
}

/// Removes only entries still equal to what the app wrote. Returns the names left in place because they were
/// changed by someone else in the meantime.
pub fn revert(root: &str, managed: &BTreeMap<String, Value>, exclude: Option<&str>) -> Result<Vec<String>> {
    let mut kept = Vec::new();
    if managed.is_empty() && exclude.is_none() || !Path::new(root).is_dir() {
        return Ok(kept);
    }
    let names: Vec<String> = managed.keys().cloned().collect();
    settings::with_lock(|| {
        settings::mutate_settings(&Path::new(root).join(MCP_PATH), |file| {
            let mut current = servers_of(file);
            for (name, def) in managed {
                if current.get(name) == Some(def) {
                    current.remove(name);
                } else if current.contains_key(name) {
                    kept.push(name.clone());
                }
            }
            set_servers(file, current);
            Ok(())
        })?;
        update_enabled(root, &names, &[])
    })?;
    if let Some(line) = exclude {
        settings::remove_git_exclude(root, line)?;
    }
    Ok(kept)
}

/// True if every entry the app wrote is still in `.mcp.json` unchanged.
pub fn is_active(root: &str, managed: &BTreeMap<String, Value>) -> bool {
    if managed.is_empty() {
        return true;
    }
    let Ok(Some(file)) = settings::read_json(&Path::new(root).join(MCP_PATH).to_string_lossy()) else { return false };
    let current = servers_of(&file);
    managed.iter().all(|(name, def)| current.get(name) == Some(def))
}
