//! MCP servers IT publishes centrally: a JSON document at the address of `ManagedMcpServersURL`, in Claude Code's
//! `.mcp.json` format (`{"mcpServers": {"<name>": {"type": "http", "url": …}}}`). If an SSO profile signs in at the
//! same host (gateway or identity provider), its token goes along, so the server can answer per user. The last list
//! fetched is kept in `mcp-catalog.json`, so the servers stay usable offline.

use crate::managed::{derived_mcp_id, is_secure_endpoint};
use crate::models::{McpServer, McpTransport};
use crate::{gateway, l, paths};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

const CACHE_FILE: &str = "mcp-catalog.json";
const MAX_SIZE: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub url: String,
    /// Unix time of the last successful fetch.
    pub fetched_at: i64,
    pub servers: Vec<McpServer>,
    /// Names of entries that were left out because they are incomplete or invalid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FetchError {
    /// 401 or 403: the server wants a (different) sign-in.
    Unauthorized,
    Http(u16),
    Network(String),
    Invalid(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            FetchError::Unauthorized => l!("Der Server verlangt eine Anmeldung. Bitte beim SSO-Profil dieses Servers anmelden."),
            FetchError::Http(status) => l!("Unerwartete Antwort vom Server (HTTP %@).", status),
            FetchError::Network(reason) => l!("Server nicht erreichbar (VPN?): %@", reason),
            FetchError::Invalid(reason) => l!("Die MCP-Liste ist ungültig: %@", reason),
        })
    }
}

/// Reads `{"mcpServers": {…}}`; entries that are incomplete, have an invalid name or a remote URL without https
/// are skipped and returned by name.
pub fn parse(text: &str) -> Result<(Vec<McpServer>, Vec<String>), FetchError> {
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| FetchError::Invalid(e.to_string()))?;
    let entries = value.get("mcpServers").and_then(Value::as_object).ok_or_else(|| FetchError::Invalid(l!("„mcpServers“ fehlt.")))?;
    let (mut servers, mut skipped) = (Vec::new(), Vec::new());
    for (name, definition) in entries {
        match server(name, definition) {
            Some(server) => servers.push(server),
            None => skipped.push(name.clone()),
        }
    }
    Ok((servers, skipped))
}

fn server(name: &str, definition: &Value) -> Option<McpServer> {
    let definition = definition.as_object()?;
    if !McpServer::is_valid_name(name) {
        return None;
    }
    let text = |key: &str| definition.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let pairs = |key: &str| -> BTreeMap<String, String> {
        definition
            .get(key)
            .and_then(Value::as_object)
            .map(|map| map.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
            .unwrap_or_default()
    };
    let transport = match text("type").as_deref() {
        Some("stdio") => McpTransport::Stdio,
        Some("http") => McpTransport::Http,
        Some("sse") => McpTransport::Sse,
        None if definition.contains_key("url") => McpTransport::Http,
        None => McpTransport::Stdio,
        Some(_) => return None,
    };
    let mut server = McpServer::new(derived_mcp_id(name), name.to_string());
    server.transport = transport;
    if transport == McpTransport::Stdio {
        server.command = text("command")?;
        server.args = definition.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        server.env = pairs("env");
    } else {
        server.url = text("url").filter(|url| is_secure_endpoint(url))?;
        server.headers = pairs("headers");
        if let Some(oauth) = definition.get("oauth").and_then(Value::as_object) {
            server.oauth_client_id = oauth.get("clientId").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            server.oauth_callback_port = oauth.get("callbackPort").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0);
            server.oauth_scopes = oauth.get("scopes").and_then(Value::as_str).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
        }
    }
    Some(server)
}

/// Loads the list from `url` (https only, http for localhost), with `token` as bearer token if given.
pub fn fetch(url: &str, token: Option<&str>, timeout: Duration) -> Result<Catalog, FetchError> {
    if !is_secure_endpoint(url) {
        return Err(FetchError::Invalid(l!("Bitte eine vollständige https-URL angeben (http nur für localhost).")));
    }
    let mut request = gateway::agent(timeout).get(url).header("Accept", "application/json").header("User-Agent", "KeyZapper");
    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.call().map_err(|e| FetchError::Network(e.to_string()))?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(FetchError::Unauthorized),
        other => return Err(FetchError::Http(other)),
    }
    let body = response.body_mut().with_config().limit(MAX_SIZE).read_to_string().map_err(|e| FetchError::Network(e.to_string()))?;
    let (servers, skipped) = parse(&body)?;
    Ok(Catalog { url: url.to_string(), fetched_at: crate::oidc::unix_now(), servers, skipped })
}

fn cache_path() -> PathBuf {
    paths::data_dir().join(CACHE_FILE)
}

/// The list last fetched from `url`; None if there is none or it came from another address.
pub fn load_cache(url: &str) -> Option<Catalog> {
    let catalog: Catalog = serde_json::from_slice(&std::fs::read(cache_path()).ok()?).ok()?;
    (catalog.url == url).then_some(catalog)
}

pub fn save_cache(catalog: &Catalog) -> std::io::Result<()> {
    let data = serde_json::to_vec_pretty(catalog).map_err(std::io::Error::other)?;
    paths::write_atomic(&cache_path(), &data, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_mcp_json_format_and_skips_invalid_entries() {
        let text = r#"{"mcpServers": {
            "weave": {"type": "http", "url": "https://weave.firma.example/mcp", "headers": {"X-Tenant": "a"},
                      "oauth": {"clientId": "abc", "callbackPort": 8976, "scopes": "mcp.read  mcp.write"}},
            "fetch": {"command": "uvx", "args": ["mcp-server-fetch"], "env": {"A": "1"}},
            "legacy": {"type": "sse", "url": "https://sse.firma.example/sse"},
            "insecure": {"type": "http", "url": "http://mcp.firma.example"},
            "no command": {"type": "stdio", "command": "x"},
            "empty": {"type": "stdio"},
            "odd": {"type": "websocket", "url": "https://x.example"}
        }}"#;
        let (servers, mut skipped) = parse(text).unwrap();
        skipped.sort();
        assert_eq!(skipped, ["empty", "insecure", "no command", "odd"]);
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["fetch", "legacy", "weave"]);
        let weave = &servers[2];
        assert_eq!(weave.id, derived_mcp_id("weave"));
        assert_eq!(weave.transport, McpTransport::Http);
        assert_eq!((weave.oauth_client_id.as_str(), weave.oauth_callback_port, weave.oauth_scopes.as_str()), ("abc", Some(8976), "mcp.read mcp.write"));
        assert_eq!(weave.definition_for(false)["headers"]["X-Tenant"], "a");
        assert_eq!(servers[0].transport, McpTransport::Stdio);
        assert_eq!(servers[0].args, ["mcp-server-fetch"]);
        assert_eq!(servers[1].transport, McpTransport::Sse);
        assert!(matches!(parse("[]"), Err(FetchError::Invalid(_))));
        assert!(matches!(parse("{"), Err(FetchError::Invalid(_))));
    }

    #[test]
    fn refuses_plain_http_addresses() {
        assert!(matches!(fetch("http://mcp.firma.example/list.json", None, Duration::from_secs(1)), Err(FetchError::Invalid(_))));
    }
}
