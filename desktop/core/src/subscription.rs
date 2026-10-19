//! Claude subscription (Pro, Max, Team, Enterprise) through Claude Code's own claude.ai login. KeyZapper reads the login
//! only to show the plan and its usage limits, like Claude Code's `/usage` (`GET /api/oauth/usage`, no public API).
//! The token is never refreshed, stored or written: Claude Code renews it while it runs, so its login stays untouched.

use crate::{gateway, l, paths};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// The parts of Claude Code's login KeyZapper uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Login {
    pub access_token: String,
    /// Unix milliseconds.
    pub expires_at: Option<i64>,
    /// e.g. "Max 20×", "Pro".
    pub plan: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    /// `session` (5-hour window), `weekly` (all models) or `model` (weekly window of `model`).
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Share used, 0–100.
    pub percent: f64,
    /// ISO 8601, as Anthropic reports it.
    pub resets_at: Option<String>,
}

/// What the app shows for the subscription.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub logged_in: bool,
    pub plan: Option<String>,
    pub limits: Vec<UsageLimit>,
    /// Unix seconds when `limits` were fetched; they are kept while the login cannot be used.
    pub checked_at: Option<i64>,
    /// Why the limits are missing or not current.
    pub problem: Option<String>,
}

/// `CLAUDE_CONFIG_DIR` or `~/.claude`, like Claude Code.
fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| paths::home_dir().join(".claude"))
}

/// Claude Code's login: in the macOS keychain (item "Claude Code-credentials", written by `security`, so reading it with
/// `security` does not prompt), elsewhere in `<config dir>/.credentials.json`.
pub fn read_login() -> Option<Login> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .stderr(std::process::Stdio::null())
            .output();
        if let Some(login) = output.ok().filter(|o| o.status.success()).and_then(|o| parse_login(&String::from_utf8_lossy(&o.stdout))) {
            return Some(login);
        }
    }
    parse_login(&std::fs::read_to_string(config_dir().join(".credentials.json")).ok()?)
}

/// `{"claudeAiOauth": {"accessToken": …, "expiresAt": <ms>, "subscriptionType": "max", "rateLimitTier": "default_claude_max_20x"}}`
pub fn parse_login(text: &str) -> Option<Login> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let oauth = value.get("claudeAiOauth")?;
    let access_token = oauth.get("accessToken")?.as_str().filter(|t| !t.is_empty())?.to_string();
    let text = |key: &str| oauth.get(key).and_then(|v| v.as_str());
    Some(Login {
        access_token,
        expires_at: oauth.get("expiresAt").and_then(|v| v.as_i64()),
        plan: plan_name(text("subscriptionType"), text("rateLimitTier")),
    })
}

/// "Max 20×" from `max` and `default_claude_max_20x`, "Pro" from `pro`.
pub fn plan_name(subscription_type: Option<&str>, rate_limit_tier: Option<&str>) -> Option<String> {
    let kind = subscription_type.map(str::trim).filter(|t| !t.is_empty())?;
    let mut name: String = kind.chars().take(1).flat_map(char::to_uppercase).chain(kind.chars().skip(1)).collect();
    let multiplier = rate_limit_tier.and_then(|t| t.rsplit('_').next()).and_then(|t| t.strip_suffix('x')).filter(|n| n.parse::<u32>().is_ok());
    if let Some(n) = multiplier {
        name = format!("{name} {n}×");
    }
    Some(name)
}

/// The limits of a `/api/oauth/usage` reply: the `limits` list (`kind` `session`, `weekly_all` or `weekly_scoped` with
/// `scope.model.display_name`, `percent`, `resets_at`) or, from older replies, the windows `five_hour`, `seven_day`,
/// `seven_day_opus` and `seven_day_sonnet` (`utilization`, `resets_at`). Unknown kinds are left out.
pub fn parse_usage(body: &str) -> Option<Vec<UsageLimit>> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let object = value.as_object()?;
    let resets_at = |entry: &serde_json::Value| entry.get("resets_at").and_then(|v| v.as_str()).map(str::to_string);
    let limit = |kind: &str, model: Option<String>, percent: Option<f64>, entry: &serde_json::Value| {
        Some(UsageLimit { kind: kind.into(), model, percent: percent?.clamp(0.0, 100.0), resets_at: resets_at(entry) })
    };
    let listed: Vec<UsageLimit> = object
        .get("limits")
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let percent = entry.get("percent").and_then(|v| v.as_f64());
            match entry.get("kind")?.as_str()? {
                "session" => limit("session", None, percent, entry),
                "weekly_all" => limit("weekly", None, percent, entry),
                "weekly_scoped" => {
                    let model = entry.pointer("/scope/model/display_name").and_then(|v| v.as_str())?.to_string();
                    limit("model", Some(model), percent, entry)
                }
                _ => None,
            }
        })
        .collect();
    if !listed.is_empty() {
        return Some(listed);
    }
    let windows = [("five_hour", "session", None), ("seven_day", "weekly", None), ("seven_day_opus", "model", Some("Opus")), ("seven_day_sonnet", "model", Some("Sonnet"))];
    Some(
        windows
            .into_iter()
            .filter_map(|(key, kind, model)| {
                let entry = object.get(key).filter(|e| e.is_object())?;
                limit(kind, model.map(str::to_string), entry.get("utilization").and_then(|v| v.as_f64()), entry)
            })
            .collect(),
    )
}

fn fetch(token: &str, timeout: Duration) -> Result<Vec<UsageLimit>, String> {
    let response = gateway::agent(timeout)
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", OAUTH_BETA)
        .header("Accept", "application/json")
        .call();
    let mut response = response.map_err(|e| l!("Claude nicht erreichbar: %@", e))?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().unwrap_or_default();
    match status {
        200 => parse_usage(&body).ok_or_else(|| l!("Claude hat den Verbrauch in einem unbekannten Format gemeldet.")),
        401 | 403 => Err(l!("Claude lehnt die Anmeldung ab. In Claude Code „/login“ ausführen.")),
        429 => Err(l!("Zu viele Abfragen bei Claude. Später erneut versuchen.")),
        other => Err(l!("Unerwartete Antwort von Claude (HTTP %@).", other)),
    }
}

/// Reads the login and fetches the limits. Limits of `previous` are kept (with a note) while the login has expired or
/// the request fails, so the last known state stays visible.
pub fn status(previous: Option<&Status>, timeout: Duration) -> Status {
    let Some(login) = read_login() else { return Status::default() };
    let now = crate::oidc::unix_now();
    let kept = |problem: String| Status {
        logged_in: true,
        plan: login.plan.clone(),
        limits: previous.map(|p| p.limits.clone()).unwrap_or_default(),
        checked_at: previous.and_then(|p| p.checked_at),
        problem: Some(problem),
    };
    if login.expires_at.is_some_and(|ms| ms / 1000 <= now) {
        return kept(l!("Die Claude-Anmeldung wird erneuert, sobald Claude Code läuft."));
    }
    match fetch(&login.access_token, timeout) {
        Ok(limits) => Status { logged_in: true, plan: login.plan, limits, checked_at: Some(now), problem: None },
        Err(problem) => kept(problem),
    }
}
