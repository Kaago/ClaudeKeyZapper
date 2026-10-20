//! Claude Desktop with a KeyZapper profile. Claude Desktop's third-party mode reads one configuration file from its
//! configuration library (`Claude-3p/configLibrary/<id>.json`, `_meta.json` names the applied one). KeyZapper keeps
//! one entry there: requests go to the profile's gateway, and Claude Desktop gets the key or SSO token from
//! `keyzapper-helper` (`inferenceCredentialHelper`), just like Claude Code. A configuration IT deploys via MDM profile
//! or registry policy takes precedence over the library.

use crate::models::Profile;
use crate::{json, paths};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// ID of KeyZapper's entry in the configuration library.
pub const ENTRY_ID: &str = "ca2e9fe9-a839-47f6-81cc-7229ac419d96";
const META_FILE: &str = "_meta.json";
const TTL_ENV: &str = "CLAUDE_CODE_API_KEY_HELPER_TTL_MS";
/// Claude Code's default helper TTL; KeyZapper's SSO tokens are refreshed with this margin.
const DEFAULT_TTL_SECS: u64 = 300;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// KeyZapper profile in KeyZapper's entry, if there is one.
    pub profile_id: Option<String>,
    /// KeyZapper's entry is the one Claude Desktop applies.
    pub applied: bool,
    /// IT deploys a Claude Desktop configuration; the library may then be ignored.
    pub managed: bool,
}

/// `~/Library/Application Support/Claude-3p/configLibrary` (macOS), `%LOCALAPPDATA%\Claude-3p\configLibrary` (Windows).
pub fn library_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(local).join(r"Claude-3p\configLibrary");
    }
    #[cfg(target_os = "macos")]
    return paths::home_dir().join("Library/Application Support/Claude-3p/configLibrary");
    #[allow(unreachable_code)]
    paths::home_dir().join(".config/Claude-3p/configLibrary")
}

/// Gateway provider with `keyzapper-helper` as credential helper; the profile's model tiers become the model list
/// (Sonnet first, the default), else Claude Desktop asks the gateway (`/v1/models`).
pub fn configuration(profile: &Profile, helper: &str) -> Value {
    let ttl = profile
        .environment
        .as_ref()
        .and_then(|env| env.get(TTL_ENV))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|ms| (ms / 1000).max(60))
        .unwrap_or(DEFAULT_TTL_SECS);
    let mut config = json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": profile.endpoint,
        "inferenceCredentialKind": "helper-script",
        "inferenceCredentialHelper": helper,
        "inferenceCredentialHelperArgs": ["credential", "--profile", profile.id],
        "inferenceCredentialHelperTtlSec": ttl,
    });
    let tiers = [("sonnet", &profile.sonnet_model), ("opus", &profile.opus_model), ("haiku", &profile.haiku_model)];
    let models: Vec<Value> = tiers
        .into_iter()
        .filter_map(|(tier, model)| Some(json!({"name": model.as_deref().map(str::trim).filter(|m| !m.is_empty())?, "anthropicFamilyTier": tier})))
        .collect();
    if !models.is_empty() {
        config["inferenceModels"] = Value::Array(models);
    }
    config
}

fn read(path: &Path) -> Option<json::Object> {
    json::parse_lenient(&std::fs::read_to_string(path).ok()?)
}

fn write(path: &Path, value: &Value) -> std::io::Result<()> {
    let data = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    paths::write_atomic(path, &data, true)
}

/// Writes KeyZapper's entry for `profile` and makes it the applied configuration. Takes effect when Claude Desktop starts.
pub fn apply(dir: &Path, profile: &Profile, helper: &str) -> std::io::Result<()> {
    write(&dir.join(format!("{ENTRY_ID}.json")), &configuration(profile, helper))?;
    let meta_path = dir.join(META_FILE);
    let mut meta = read(&meta_path).unwrap_or_default();
    let mut entries: Vec<Value> = meta.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
    entries.retain(|e| e.get("id").and_then(Value::as_str) != Some(ENTRY_ID));
    entries.push(json!({"id": ENTRY_ID, "name": format!("KeyZapper – {}", profile.name)}));
    meta.insert("entries".into(), Value::Array(entries));
    meta.insert("appliedId".into(), ENTRY_ID.into());
    write(&meta_path, &Value::Object(meta))
}

/// Removes KeyZapper's entry; Claude Desktop then starts with its normal sign-in again (unless another entry is applied).
pub fn remove(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(format!("{ENTRY_ID}.json"))) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let meta_path = dir.join(META_FILE);
    let Some(mut meta) = read(&meta_path) else { return Ok(()) };
    if let Some(entries) = meta.get_mut("entries").and_then(Value::as_array_mut) {
        entries.retain(|e| e.get("id").and_then(Value::as_str) != Some(ENTRY_ID));
    }
    if meta.get("appliedId").and_then(Value::as_str) == Some(ENTRY_ID) {
        meta.insert("appliedId".into(), Value::Null);
    }
    write(&meta_path, &Value::Object(meta))
}

pub fn status(dir: &Path) -> Status {
    let profile_id = read(&dir.join(format!("{ENTRY_ID}.json"))).and_then(|config| {
        let args = config.get("inferenceCredentialHelperArgs")?.as_array()?;
        let at = args.iter().position(|a| a == "--profile")?;
        args.get(at + 1)?.as_str().map(str::to_string)
    });
    let applied = profile_id.is_some() && read(&dir.join(META_FILE)).and_then(|m| m.get("appliedId")?.as_str().map(str::to_string)).as_deref() == Some(ENTRY_ID);
    Status { profile_id, applied, managed: managed_by_it() }
}

/// An MDM profile (`com.anthropic.claudefordesktop`) or registry policy (`SOFTWARE\Policies\Claude`) is present.
fn managed_by_it() -> bool {
    #[cfg(target_os = "macos")]
    {
        let domain = "com.anthropic.claudefordesktop.plist";
        let user = std::env::var("USER").unwrap_or_default();
        let base = std::path::Path::new("/Library/Managed Preferences");
        return base.join(domain).is_file() || (!user.is_empty() && base.join(user).join(domain).is_file());
    }
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        return [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().any(|hive| {
            winreg::RegKey::predef(hive).open_subkey(r"SOFTWARE\Policies\Claude").is_ok_and(|key| key.enum_values().next().is_some())
        });
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_uses_the_helper_and_the_model_tiers() {
        let mut profile = Profile::new("A1B2C3D4-0000-4000-8000-000000000001".into(), "Team".into(), "https://litellm.firma.example".into(), String::new());
        let config = configuration(&profile, "/Applications/KeyZapper.app/Contents/MacOS/keyzapper-helper");
        assert_eq!(config["inferenceProvider"], "gateway");
        assert_eq!(config["inferenceGatewayBaseUrl"], "https://litellm.firma.example");
        assert_eq!(config["inferenceCredentialKind"], "helper-script");
        assert_eq!(config["inferenceCredentialHelperArgs"], json!(["credential", "--profile", "A1B2C3D4-0000-4000-8000-000000000001"]));
        assert_eq!(config["inferenceCredentialHelperTtlSec"], 300);
        assert!(config.get("inferenceModels").is_none());
        profile.opus_model = Some("eu.opus".into());
        profile.sonnet_model = Some("eu.sonnet".into());
        profile.haiku_model = Some(" ".into());
        profile.environment = Some([(TTL_ENV.to_string(), "900000".to_string())].into());
        let config = configuration(&profile, "helper");
        assert_eq!(config["inferenceModels"], json!([{"name": "eu.sonnet", "anthropicFamilyTier": "sonnet"}, {"name": "eu.opus", "anthropicFamilyTier": "opus"}]));
        assert_eq!(config["inferenceCredentialHelperTtlSec"], 900);
    }

    #[test]
    fn apply_and_remove_keep_other_entries() {
        let dir = std::env::temp_dir().join(format!("keyzapper-claude-desktop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(META_FILE), r#"{"appliedId": "other", "entries": [{"id": "other", "name": "Eigene"}]}"#).unwrap();
        assert_eq!(status(&dir).profile_id, None);
        let profile = Profile::new("A1B2C3D4-0000-4000-8000-000000000001".into(), "Team".into(), "https://litellm.firma.example".into(), String::new());
        apply(&dir, &profile, "helper").unwrap();
        apply(&dir, &profile, "helper").unwrap();
        let meta = read(&dir.join(META_FILE)).unwrap();
        assert_eq!(meta["appliedId"], ENTRY_ID);
        assert_eq!(meta["entries"], json!([{"id": "other", "name": "Eigene"}, {"id": ENTRY_ID, "name": "KeyZapper – Team"}]));
        let current = status(&dir);
        assert_eq!((current.profile_id.as_deref(), current.applied), (Some(profile.id.as_str()), true));
        remove(&dir).unwrap();
        let meta = read(&dir.join(META_FILE)).unwrap();
        assert_eq!((&meta["appliedId"], &meta["entries"]), (&Value::Null, &json!([{"id": "other", "name": "Eigene"}])));
        assert_eq!(status(&dir), Status { managed: status(&dir).managed, ..Status::default() });
        remove(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
