//! "What's new" after an update: highlights per release and the last version whose highlights were shown.

use crate::l;
use crate::update::compare_versions;
use serde::Serialize;
use std::cmp::Ordering;

const SEEN_FILE: &str = "seen-version";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Release {
    pub version: String,
    pub items: Vec<String>,
}

/// Highlights per release, newest first (German source strings, translated by `l!`).
fn notes() -> Vec<(&'static str, Vec<String>)> {
    vec![
        (
            "1.5.0",
            vec![
                l!("Claude-Abo als Profiltyp: Projekte nutzen dein Claude Pro, Max oder Team über die Anmeldung von Claude Code – ohne Gateway und Key."),
                l!("Ein Klick aufs Symbol in der Menüleiste bzw. im Infobereich zeigt den Verbrauch auf einen Blick: Limits des Claude-Abos und Budgets aller Keys. Rechtsklick öffnet das bisherige Menü."),
            ],
        ),
        (
            "1.4.0",
            vec![
                l!("Neues Design: Seitenleiste, Budgetbalken mit Ampel, Projektstatus auf einen Blick mit „Alle reparieren“ – und Projektordner einfach auf ein Profil ziehen."),
                l!("Nach dem Schließen läuft KeyZapper in der Menüleiste bzw. im Infobereich weiter: Budgets im Blick und das Profil je Projekt mit einem Klick wechseln."),
                l!("MCP-Server aus Vorlagen wie GitHub, Notion oder Sentry anlegen und je Projekt per Schalter einschalten. Unter Windows starten npx-Server automatisch über cmd /c."),
                l!("MCP-Server einrichten und je Projekt auswählen: KeyZapper schreibt sie in die .mcp.json des Repositorys und schaltet sie frei, Claude Code lädt sie dort automatisch."),
                l!("Updates installieren sich nach Bestätigung mit Administratorrechten selbst, danach startet KeyZapper automatisch neu."),
                l!("Nach einem Update zeigt KeyZapper einmal, was neu ist."),
            ],
        ),
        (
            "1.3.0",
            vec![
                l!("Für Agenten optimieren: Das ✨-Symbol am Projekt analysiert das Repository per KI und erzeugt eine AGENTS.md nach deinen Zielen, z. B. Kosteneffizienz – samt Empfehlung, welches Modell sich wofür eignet."),
                l!("Symbole zeigen beim Überfahren ihren Namen oder einen kurzen Hilfetext."),
                l!("Updates werden vor der Installation anhand ihrer Signatur geprüft."),
                l!("Gateways, an die Keys gehen, müssen https verwenden (http nur für localhost)."),
            ],
        ),
    ]
}

fn without_suffix(version: &str) -> &str {
    version.split('-').next().unwrap_or(version)
}

/// Highlights after updating from `previous` to `current`, newest first: the releases after the previous one up to the
/// current one, compared without beta suffix (the betas of a release share its notes). Without a previous version
/// (updated from a release before this dialog existed) the newest up to the current one. At most three releases.
pub fn since(previous: Option<&str>, current: &str) -> Vec<Release> {
    let current = without_suffix(current);
    notes()
        .into_iter()
        .filter(|(version, _)| compare_versions(version, current) != Ordering::Greater)
        .filter(|(version, _)| previous.is_none_or(|p| compare_versions(version, without_suffix(p)) == Ordering::Greater))
        .take(3)
        .map(|(version, items)| Release { version: version.to_string(), items })
        .collect()
}

pub fn seen_version() -> Option<String> {
    let text = std::fs::read_to_string(crate::paths::data_dir().join(SEEN_FILE)).ok()?;
    Some(text.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn mark_seen(version: &str) -> std::io::Result<()> {
    crate::paths::write_atomic(&crate::paths::data_dir().join(SEEN_FILE), version.as_bytes(), false)
}
