//! Claude for Microsoft 365 (the Claude add-in for Excel, Word and PowerPoint) behind the company gateway with Entra SSO.
//! KeyZapper takes Anthropic's manifest template, sets a company add-in ID, appends the Entra parameters to both
//! task pane URL slots and registers the result as a sideloaded add-in: a `<id>.manifest.xml` in each app's `wef`
//! folder (macOS), a value under `HKCU\SOFTWARE\Microsoft\Office\16.0\Wef\Developer` (Windows).
//!
//! - Sideloading is Microsoft's test route. The supported production route is central deployment of the same manifest
//!   with the same add-in ID.
//! - No secret ever goes into the manifest, and KeyZapper never reads or writes the add-in's own storage.
//! - `apply_*` and `remove_*` are the only places that touch a container, by writing or deleting, never by reading.
//!   Callers decide through the fingerprint whether to call them. Only KeyZapper's own file or value is ever removed.
//! - The sync (`office_profile` → `plan_sync` → `SyncJob::run`) compares what the profile asks for with what KeyZapper
//!   recorded in its own state (`OfficeApplied`); the containers are never read to find out.

use crate::models::Profile;
use crate::{l, paths};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Anthropic's manifest template.
pub const TEMPLATE_URL: &str = "https://pivot.claude.ai/manifest.xml";
/// Add-in ID of the marketplace version. A company add-in must not reuse it.
pub const MARKETPLACE_ADDIN_ID: &str = "4e2fd29d-dad0-40b3-af3c-a16e347a5ddc";
/// Where Office on Windows looks for sideloaded manifests (below `HKEY_CURRENT_USER`).
pub const WEF_DEVELOPER_KEY: &str = r"SOFTWARE\Microsoft\Office\16.0\Wef\Developer";

const TEMPLATE_ORIGIN: &str = "https://pivot.claude.ai/";
/// The real template is a few KB; anything bigger is not Anthropic's manifest.
const MAX_TEMPLATE_BYTES: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeApp {
    Excel,
    Word,
    PowerPoint,
}

impl OfficeApp {
    pub const ALL: [OfficeApp; 3] = [OfficeApp::Excel, OfficeApp::Word, OfficeApp::PowerPoint];

    pub fn as_str(self) -> &'static str {
        match self {
            OfficeApp::Excel => "excel",
            OfficeApp::Word => "word",
            OfficeApp::PowerPoint => "powerpoint",
        }
    }

    /// Name of the app's sandbox container below `~/Library/Containers`.
    pub fn bundle_id(self) -> &'static str {
        match self {
            OfficeApp::Excel => "com.microsoft.Excel",
            OfficeApp::Word => "com.microsoft.Word",
            OfficeApp::PowerPoint => "com.microsoft.Powerpoint",
        }
    }

    /// App bundle below `/Applications`.
    pub fn app_bundle(self) -> &'static str {
        match self {
            OfficeApp::Excel => "Microsoft Excel.app",
            OfficeApp::Word => "Microsoft Word.app",
            OfficeApp::PowerPoint => "Microsoft PowerPoint.app",
        }
    }
}

/// The Office part of a company profile. A field missing in a stored object (an older or hand-edited state file) takes its
/// empty default instead of making the whole state unreadable; `problems()` then reports the profile as unusable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OfficeClient {
    pub enabled: bool,
    /// Company add-in ID (lowercase GUID), not the marketplace one.
    pub addin_id: String,
    /// Gateway route of the Office add-in.
    pub gateway_url: String,
    /// Entra application (client) ID of the add-in's app registration.
    pub entra_client_id: String,
    /// `<application ID URI>/<scope>`, e.g. `api://<client id>/access_as_user`.
    pub entra_scope: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled_features: Vec<String>,
}

/// A GUID in the canonical hyphenated form; `lowercase` demands lowercase hex digits.
fn is_guid(text: &str, lowercase: bool) -> bool {
    text.len() == 36 && uuid::Uuid::parse_str(text).is_ok() && (!lowercase || !text.chars().any(|c| c.is_ascii_uppercase()))
}

/// The gateway URL points to this machine.
fn is_local(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.');
            domain == "localhost" || domain.ends_with(".localhost")
        }
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

impl OfficeClient {
    /// What keeps this profile from being used; empty means usable.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !is_guid(&self.addin_id, true) {
            problems.push(l!("Die Add-in-ID muss eine GUID in Kleinbuchstaben sein."));
        } else if self.addin_id == MARKETPLACE_ADDIN_ID {
            problems.push(l!("Die Add-in-ID darf nicht die des Marketplace-Add-ins sein."));
        }
        let gateway = url::Url::parse(&self.gateway_url).ok().filter(|u| u.scheme() == "https");
        match gateway.as_ref().and_then(|u| u.host().map(|host| (u, host))) {
            None => problems.push(l!("Die Gateway-URL muss eine https-URL mit Host sein.")),
            Some((_, host)) if is_local(&host) => problems.push(l!("Die Gateway-URL darf nicht auf diesen Rechner zeigen.")),
            Some((u, _)) if !u.username().is_empty() || u.password().is_some() => problems.push(l!("Die Gateway-URL darf keine Zugangsdaten enthalten.")),
            Some(_) => {}
        }
        if !is_guid(&self.entra_client_id, false) {
            problems.push(l!("Die Entra-Client-ID muss eine GUID sein."));
        }
        if self.entra_scope.is_empty() || self.entra_scope.chars().any(char::is_whitespace) {
            problems.push(l!("Der Entra-Scope darf nicht leer sein und keine Leerzeichen enthalten."));
        }
        for feature in &self.disabled_features {
            let valid = feature.starts_with(|c: char| c.is_ascii_lowercase()) && feature.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '.');
            if !valid {
                problems.push(l!("Ungültige Funktion in der Liste der deaktivierten Funktionen: %@", feature));
            }
        }
        problems
    }
}

/// The query parameters the add-in reads, in a fixed order. The values never contain a secret.
pub fn parameters(office: &OfficeClient) -> Vec<(&'static str, String)> {
    let mut parameters = vec![
        ("gateway_url", office.gateway_url.clone()),
        ("gateway_api_format", "anthropic".to_string()),
        ("entra_sso", "1".to_string()),
        ("graph_client_id", office.entra_client_id.clone()),
        ("entra_scope", office.entra_scope.clone()),
        ("gateway_auth_source", "entra".to_string()),
    ];
    if !office.disabled_features.is_empty() {
        parameters.push(("disabled_features", office.disabled_features.join(",")));
    }
    parameters
}

/// The value of the first `<SourceLocation DefaultValue="…"` of the template.
fn source_location(template: &str) -> Option<&str> {
    let tag = &template[template.find("<SourceLocation")?..];
    let tag = &tag[..tag.find('>')?];
    let value = &tag[tag.find("DefaultValue=\"")? + "DefaultValue=\"".len()..];
    Some(&value[..value.find('"')?])
}

/// Anthropic's `template` with the company add-in ID and the Entra parameters in both task pane URL slots.
/// Fails (and so writes nothing) for an unusable profile or a template that does not look like Anthropic's.
pub fn manifest(template: &str, office: &OfficeClient) -> Result<String, String> {
    let problems = office.problems();
    if !problems.is_empty() {
        return Err(problems.join(" "));
    }
    let unknown = || l!("Die Manifest-Vorlage von Claude hat ein unbekanntes Format.");
    let base = source_location(template).filter(|base| base.starts_with(TEMPLATE_ORIGIN) && !base.contains('#')).ok_or_else(unknown)?;
    let id = template.find("<Id>").map(|i| i + "<Id>".len()).and_then(|from| Some((from, from + template[from..].find("</Id>")?)));
    let (from, to) = id.ok_or_else(unknown)?;
    let template = format!("{}{}{}", &template[..from], office.addin_id, &template[to..]);
    let query = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(parameters(office)).finish().replace('&', "&amp;");
    let separator = if base.contains('?') { "&amp;" } else { "?" };
    let slot = format!("DefaultValue=\"{base}\"");
    if template.matches(&slot).count() < 2 {
        return Err(unknown());
    }
    Ok(template.replace(&slot, &format!("DefaultValue=\"{base}{separator}{query}\"")))
}

/// SHA-256 of the manifest (hex), to see whether what is registered is what would be written.
pub fn fingerprint(manifest: &str) -> String {
    Sha256::digest(manifest.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The office apps installed below `applications_dir` (`/Applications`).
pub fn installed_apps(applications_dir: &Path) -> Vec<OfficeApp> {
    OfficeApp::ALL.into_iter().filter(|app| applications_dir.join(app.app_bundle()).is_dir()).collect()
}

/// File name of KeyZapper's manifest. Only a company add-in ID (lowercase GUID, not the marketplace one) qualifies, so
/// the ID cannot carry path syntax and never names a manifest that is not KeyZapper's.
fn file_name(addin_id: &str) -> std::io::Result<String> {
    if !is_guid(addin_id, true) || addin_id == MARKETPLACE_ADDIN_ID {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid add-in ID"));
    }
    Ok(format!("{addin_id}.manifest.xml"))
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn wef_file(containers_dir: &Path, app: OfficeApp, file_name: &str) -> PathBuf {
    containers_dir.join(app.bundle_id()).join("Data/Documents/wef").join(file_name)
}

/// Runs `step` for every app, even after a failure (a denied prompt for one app must not leave the others half done),
/// and returns the first error.
fn for_each_app(apps: &[OfficeApp], step: impl Fn(OfficeApp) -> std::io::Result<()>) -> std::io::Result<()> {
    let mut first = Ok(());
    for app in apps {
        let result = step(*app);
        if first.is_ok() {
            first = result;
        }
    }
    first
}

/// Sideloads `manifest` in the `wef` folder of each of `apps` (`containers_dir` is `~/Library/Containers`).
/// Takes effect when the app starts. Writes to every app even if one fails; the first error is returned.
pub fn apply_mac(containers_dir: &Path, apps: &[OfficeApp], addin_id: &str, manifest: &str) -> std::io::Result<()> {
    let name = file_name(addin_id)?;
    for_each_app(apps, |app| paths::write_atomic(&wef_file(containers_dir, app, &name), manifest.as_bytes(), false))
}

/// Removes KeyZapper's manifest from every app; other files in `wef` stay. Tries every app even if one fails; the first
/// error is returned.
pub fn remove_mac(containers_dir: &Path, addin_id: &str) -> std::io::Result<()> {
    let name = file_name(addin_id)?;
    for_each_app(&OfficeApp::ALL, |app| remove_file(&wef_file(containers_dir, app, &name)))
}

/// Writes `manifest` to `manifest_dir` (`%LOCALAPPDATA%\KeyZapper\office`) and registers its absolute path as the
/// value `addin_id` under `HKCU\<registry_key>`. Takes effect when the app starts.
#[cfg(windows)]
pub fn apply_windows(manifest_dir: &Path, registry_key: &str, addin_id: &str, manifest: &str) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    let file = manifest_dir.join(file_name(addin_id)?);
    paths::write_atomic(&file, manifest.as_bytes(), false)?;
    let (key, _) = winreg::RegKey::predef(HKEY_CURRENT_USER).create_subkey(registry_key)?;
    key.set_value(addin_id, &std::path::absolute(&file)?.to_string_lossy().into_owned())
}

/// Removes KeyZapper's manifest file and its registry value; other values stay. Does both even if one fails; the first
/// error is returned.
#[cfg(windows)]
pub fn remove_windows(manifest_dir: &Path, registry_key: &str, addin_id: &str) -> std::io::Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    let file = remove_file(&manifest_dir.join(file_name(addin_id)?));
    let value = match winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(registry_key, KEY_SET_VALUE) {
        Ok(key) => key.delete_value(addin_id),
        Err(e) => Err(e),
    };
    file.and(match value {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    })
}

// MARK: Template

/// Where the last good copy of Anthropic's template is kept below `data_dir`, for working offline.
pub fn template_file_in(data_dir: &Path) -> PathBuf {
    data_dir.join("office-manifest-template.xml")
}

/// Anthropic's current template from `TEMPLATE_URL`.
pub fn fetch_template(timeout: Duration) -> Result<String, String> {
    fetch_template_from(TEMPLATE_URL, timeout)
}

/// The text behind `url`; anything but a plain 200 is an error.
pub fn fetch_template_from(url: &str, timeout: Duration) -> Result<String, String> {
    let mut response = crate::gateway::agent(timeout).get(url).call().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    response.body_mut().with_config().limit(MAX_TEMPLATE_BYTES).read_to_string().map_err(|e| e.to_string())
}

/// The manifest for `office` from the template just `fetched` (`None` = not tried), else from the copy stored in
/// `template_file`. A fetched template is only used and stored if it yields a manifest, so a proxy's login page or a
/// changed layout neither gets written to Office nor replaces the good copy.
pub fn manifest_from(template_file: &Path, fetched: Option<Result<String, String>>, office: &OfficeClient) -> Result<String, String> {
    if let Some(Ok(template)) = &fetched {
        let xml = manifest(template, office)?;
        // A copy that cannot be stored only costs the offline fallback.
        let _ = paths::write_atomic(template_file, template.as_bytes(), false);
        return Ok(xml);
    }
    match (std::fs::read_to_string(template_file), fetched) {
        (Ok(stored), _) => manifest(&stored, office),
        (Err(_), Some(Err(reason))) => Err(l!("Die Manifest-Vorlage von Claude konnte nicht geladen werden (%@) und es ist keine gespeichert.", reason)),
        (Err(_), _) => Err(l!("Es ist noch keine Manifest-Vorlage von Claude gespeichert.")),
    }
}

// MARK: Sync

/// The profile Claude in Office is set up from: the first one with Office switched on, those IT defines (`managed_ids`)
/// before the user's own, otherwise in list order.
pub fn office_profile<'a>(profiles: &'a [Profile], managed_ids: &[String]) -> Option<&'a Profile> {
    let enabled = |p: &&Profile| p.office.as_ref().is_some_and(|o| o.enabled);
    profiles.iter().filter(enabled).find(|p| managed_ids.contains(&p.id)).or_else(|| profiles.iter().find(enabled))
}

/// What the Office profile asks for on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesiredOffice {
    pub profile_id: String,
    pub addin_id: String,
    /// `fingerprint` of the manifest that would be written.
    pub fingerprint: String,
    /// `OfficeApp::as_str` of the apps the manifest goes to.
    pub apps: Vec<String>,
}

impl DesiredOffice {
    pub fn new(profile_id: &str, office: &OfficeClient, manifest: &str, apps: &[OfficeApp]) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            addin_id: office.addin_id.clone(),
            fingerprint: fingerprint(manifest),
            apps: apps.iter().map(|app| app.as_str().to_string()).collect(),
        }
    }
}

/// What KeyZapper registered last. Kept in `state.json`; the containers themselves are never read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeApplied {
    pub profile_id: String,
    pub addin_id: String,
    pub fingerprint: String,
    pub apps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncAction {
    Nothing,
    /// Write the manifest (again).
    Apply,
    /// The add-in ID changed: remove the old manifest, write the new one.
    Replace { old_addin_id: String },
    /// Nothing is wanted any more.
    Remove { addin_id: String },
}

/// Touches the containers only if what is wanted differs from what was registered: another add-in ID, another manifest
/// (profile change, new template version) or another set of installed apps.
pub fn plan_sync(desired: Option<&DesiredOffice>, applied: Option<&OfficeApplied>) -> SyncAction {
    match (desired, applied) {
        (None, None) => SyncAction::Nothing,
        (None, Some(applied)) => SyncAction::Remove { addin_id: applied.addin_id.clone() },
        (Some(_), None) => SyncAction::Apply,
        (Some(desired), Some(applied)) if desired.addin_id != applied.addin_id => SyncAction::Replace { old_addin_id: applied.addin_id.clone() },
        (Some(desired), Some(applied)) if desired.fingerprint == applied.fingerprint && desired.apps == applied.apps => SyncAction::Nothing,
        (Some(_), Some(_)) => SyncAction::Apply,
    }
}

/// Like `plan_sync`, but for a run the user asked for (settings change, refresh) after a registration failed: the
/// manifest is written again although the record says it is current.
pub fn plan_retry(desired: Option<&DesiredOffice>, applied: Option<&OfficeApplied>) -> SyncAction {
    match plan_sync(desired, applied) {
        SyncAction::Nothing if desired.is_some() => SyncAction::Apply,
        action => action,
    }
}

/// Where Office on this machine reads sideloaded manifests, and where the installed apps are found.
#[derive(Clone, Debug)]
pub struct Sideload {
    /// macOS: `~/Library/Containers`.
    pub containers_dir: PathBuf,
    /// macOS: `/Applications`.
    pub applications_dir: PathBuf,
    /// Windows: where the manifest files go (`%LOCALAPPDATA%\KeyZapper\office`).
    pub manifest_dir: PathBuf,
    /// Windows: the registry key below `HKCU` that lists them.
    pub registry_key: String,
}

impl Sideload {
    pub fn system() -> Self {
        Self {
            containers_dir: paths::home_dir().join("Library/Containers"),
            applications_dir: PathBuf::from("/Applications"),
            manifest_dir: paths::data_dir().join("office"),
            registry_key: WEF_DEVELOPER_KEY.to_string(),
        }
    }

    /// The apps a manifest is registered for: on macOS those installed below `/Applications` that were started once (their
    /// container folder exists; KeyZapper only checks that it is there and never creates it), on Windows all three (one
    /// registry value serves every app).
    pub fn apps(&self) -> Vec<OfficeApp> {
        if cfg!(windows) {
            return OfficeApp::ALL.to_vec();
        }
        installed_apps(&self.applications_dir).into_iter().filter(|app| self.containers_dir.join(app.bundle_id()).is_dir()).collect()
    }

    #[cfg(not(windows))]
    pub fn apply(&self, apps: &[OfficeApp], addin_id: &str, manifest: &str) -> std::io::Result<()> {
        apply_mac(&self.containers_dir, apps, addin_id, manifest)
    }

    #[cfg(windows)]
    pub fn apply(&self, _apps: &[OfficeApp], addin_id: &str, manifest: &str) -> std::io::Result<()> {
        apply_windows(&self.manifest_dir, &self.registry_key, addin_id, manifest)
    }

    #[cfg(not(windows))]
    pub fn remove(&self, addin_id: &str) -> std::io::Result<()> {
        remove_mac(&self.containers_dir, addin_id)
    }

    #[cfg(windows)]
    pub fn remove(&self, addin_id: &str) -> std::io::Result<()> {
        remove_windows(&self.manifest_dir, &self.registry_key, addin_id)
    }
}

/// What a `SyncJob` leaves behind: the new record and the error to show, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub applied: Option<OfficeApplied>,
    pub error: Option<String>,
}

/// One decided sync run. It carries everything the I/O needs, so the caller can decide under its lock and run it outside.
#[derive(Clone, Debug)]
pub struct SyncJob {
    pub action: SyncAction,
    pub desired: Option<DesiredOffice>,
    /// The manifest text for `Apply` and `Replace`.
    pub manifest: String,
    pub apps: Vec<OfficeApp>,
    /// The record before the run.
    pub previous: Option<OfficeApplied>,
}

impl SyncJob {
    /// Writes, replaces or removes and says what to record. The record always names what could be on disk:
    /// - Before anything is written, `checkpoint` gets the intended record (target add-in ID, fingerprint, apps) with an
    ///   error saying that the run is not finished, and must save it (true = saved). Only then are the manifests written, so
    ///   a crash in between cannot leave manifests the record does not know. The final outcome replaces that error. If the
    ///   record cannot be saved, nothing is written.
    /// - A registration that failed for some apps stays recorded (intended apps and fingerprint), so that deactivating
    ///   removes everything it did write. The error is returned for the status; a run without a request of the user does
    ///   not try it again, because the record says it is current.
    /// - A removal that failed keeps the record, so the next run removes again. A removal needs no checkpoint: until it is
    ///   recorded, the old record still names the files, and removing again does no harm.
    /// - A replacement removes the old manifest first and keeps the old record until that worked; if it did not, it stops
    ///   there, so no manifest ends up in a folder the record no longer names.
    pub fn run(&self, sideload: &Sideload, checkpoint: &mut dyn FnMut(&Outcome) -> bool) -> Outcome {
        let kept = |e: std::io::Error| Outcome { applied: self.previous.clone(), error: Some(l!("Claude in Office konnte nicht entfernt werden: %@", e)) };
        match &self.action {
            SyncAction::Nothing => Outcome { applied: self.previous.clone(), error: None },
            SyncAction::Remove { addin_id } => match sideload.remove(addin_id) {
                Ok(()) => Outcome { applied: None, error: None },
                Err(e) => kept(e),
            },
            SyncAction::Replace { old_addin_id } => match sideload.remove(old_addin_id) {
                Ok(()) => self.apply(sideload, checkpoint),
                Err(e) => kept(e),
            },
            SyncAction::Apply => self.apply(sideload, checkpoint),
        }
    }

    fn apply(&self, sideload: &Sideload, checkpoint: &mut dyn FnMut(&Outcome) -> bool) -> Outcome {
        let Some(desired) = &self.desired else { return Outcome { applied: self.previous.clone(), error: None } };
        let applied = OfficeApplied {
            profile_id: desired.profile_id.clone(),
            addin_id: desired.addin_id.clone(),
            fingerprint: desired.fingerprint.clone(),
            apps: desired.apps.clone(),
        };
        let pending = Outcome { applied: Some(applied.clone()), error: Some(l!("Die letzte Einrichtung von Claude in Office wurde nicht abgeschlossen.")) };
        if !checkpoint(&pending) {
            return Outcome { applied: self.previous.clone(), error: Some(l!("Der Zustand konnte nicht gespeichert werden; Claude in Office wurde nicht eingerichtet.")) };
        }
        let error = sideload.apply(&self.apps, &desired.addin_id, &self.manifest).err().map(|e| l!("Claude in Office konnte nicht eingerichtet werden: %@", e));
        Outcome { applied: Some(applied), error }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OfficeState {
    Off,
    Applied,
    Error,
}

/// What the app shows about Claude in Office.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OfficeStatus {
    pub state: OfficeState,
    /// The apps the manifest is registered for.
    pub apps: Vec<String>,
    pub message: Option<String>,
}

/// `applied` is the record, `error` why Office cannot be set up or fully worked (it also keeps the record's apps visible).
pub fn status(applied: Option<&OfficeApplied>, error: Option<&str>) -> OfficeStatus {
    let apps = applied.map(|a| a.apps.clone()).unwrap_or_default();
    let state = match (applied, error) {
        (_, Some(_)) => OfficeState::Error,
        (Some(_), None) => OfficeState::Applied,
        (None, None) => OfficeState::Off,
    };
    OfficeStatus { state, apps, message: error.map(String::from) }
}
