//! Agent instruction files: scans a project folder locally, then asks a gateway model for an assessment,
//! model recommendations and an `AGENTS.md` that follows the chosen goals (cost efficiency, quality, …).

use crate::git;
use crate::models::{suggest_models, Profile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

pub const FILE_NAME: &str = "AGENTS.md";

/// Files beyond this are not counted (huge monorepos).
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 12;

/// Dependencies, caches and build output; skipped when the folder is no git checkout (git honours .gitignore).
const SKIPPED_DIRS: [&str; 22] = [
    "node_modules", "target", "dist", "build", ".build", "out", ".next", ".nuxt", "vendor", ".venv", "venv",
    "__pycache__", ".gradle", ".idea", ".vscode", "DerivedData", "Pods", "coverage", ".git", ".cache", ".tox", "obj",
];

/// Instruction files of Claude Code, Codex, Gemini, Cursor and Copilot, relative to the root.
const AGENT_FILES: [&str; 6] = ["AGENTS.md", "CLAUDE.md", ".claude/CLAUDE.md", "GEMINI.md", ".cursorrules", ".github/copilot-instructions.md"];

const MANIFESTS: [&str; 24] = [
    "package.json", "Cargo.toml", "pyproject.toml", "requirements.txt", "setup.py", "go.mod", "pom.xml", "build.gradle",
    "build.gradle.kts", "settings.gradle.kts", "Package.swift", "Gemfile", "composer.json", "Makefile", "justfile",
    "Taskfile.yml", "Dockerfile", "docker-compose.yml", "tauri.conf.json", "deno.json", "tsconfig.json", "mix.exs",
    "pubspec.yaml", "CMakeLists.txt",
];

/// What the local scan found; shown before the analysis and summarised for the model.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoScan {
    pub root: String,
    /// True when the file list came from git (tracked and untracked, without ignored files).
    pub git: bool,
    pub file_count: usize,
    /// True when the file list was cut at `MAX_FILES`.
    pub truncated: bool,
    /// (language, files), most files first.
    pub languages: Vec<(String, usize)>,
    /// Build manifests up to two folders deep, relative to the root.
    pub manifests: Vec<String>,
    /// Agent instruction files that exist, relative to the root.
    pub agent_files: Vec<String>,
    pub has_agents_file: bool,
    #[serde(skip)]
    files: Vec<String>,
}

pub fn scan(root: &str) -> RepoScan {
    let listed = git::run(&["ls-files", "-z", "--cached", "--others", "--exclude-standard"], root);
    let git = listed.is_some();
    let mut files: Vec<String> = match listed {
        Some(out) => out.split('\0').filter(|f| !f.is_empty()).map(String::from).collect(),
        None => walk(Path::new(root)),
    };
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);

    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for file in &files {
        let extension = Path::new(file).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if let Some(language) = language(&extension) {
            *counts.entry(language).or_default() += 1;
        }
    }
    let mut languages: Vec<(String, usize)> = counts.into_iter().map(|(l, n)| (l.to_string(), n)).collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut manifests: Vec<&String> = files
        .iter()
        .filter(|f| f.matches('/').count() <= 2)
        .filter(|f| {
            let name = f.rsplit('/').next().unwrap_or(f);
            MANIFESTS.contains(&name) || name.ends_with(".csproj") || name.ends_with(".sln")
        })
        .collect();
    manifests.sort_by_key(|f| (f.matches('/').count(), f.to_string()));
    let manifests: Vec<String> = manifests.into_iter().take(12).cloned().collect();

    let agent_files: Vec<String> = AGENT_FILES.iter().filter(|f| Path::new(root).join(f).is_file()).map(|f| f.to_string()).collect();
    RepoScan {
        root: root.to_string(),
        git,
        file_count: files.len(),
        truncated,
        languages,
        manifests,
        has_agents_file: agent_files.iter().any(|f| f == FILE_NAME),
        agent_files,
        files,
    }
}

/// Files below `root` without symlinks, hidden folders (except `.github`) and `SKIPPED_DIRS`.
fn walk(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if depth < MAX_DEPTH && !SKIPPED_DIRS.contains(&name.as_str()) && (!name.starts_with('.') || name == ".github") {
                    stack.push((entry.path(), depth + 1));
                }
            } else if kind.is_file() {
                if let Ok(relative) = entry.path().strip_prefix(root) {
                    files.push(relative.to_string_lossy().replace('\\', "/"));
                }
                if files.len() > MAX_FILES {
                    return files;
                }
            }
        }
    }
    files.sort();
    files
}

fn language(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "swift" => "Swift",
        "kt" | "kts" => "Kotlin",
        "java" => "Java",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "cs" => "C#",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "C++",
        "m" | "mm" => "Objective-C",
        "scala" => "Scala",
        "dart" => "Dart",
        "ex" | "exs" => "Elixir",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        "tf" => "Terraform",
        "lua" => "Lua",
        _ => return None,
    })
}

/// Folders with their file counts (two levels) and the files at the root.
fn tree(files: &[String]) -> String {
    let mut folders: BTreeMap<&str, (usize, BTreeMap<&str, usize>)> = BTreeMap::new();
    let mut root_files = Vec::new();
    for file in files {
        let parts: Vec<&str> = file.split('/').collect();
        if parts.len() == 1 {
            root_files.push(file.as_str());
            continue;
        }
        let entry = folders.entry(parts[0]).or_default();
        entry.0 += 1;
        if parts.len() > 2 {
            *entry.1.entry(parts[1]).or_default() += 1;
        }
    }
    let mut lines = Vec::new();
    for (folder, (count, children)) in folders.iter().take(40) {
        lines.push(format!("{folder}/ ({count} files)"));
        let mut children: Vec<_> = children.iter().collect();
        children.sort_by(|a, b| b.1.cmp(a.1));
        lines.extend(children.into_iter().take(8).map(|(child, n)| format!("  {folder}/{child}/ ({n} files)")));
    }
    if !root_files.is_empty() {
        lines.push(format!("Files at the root: {}", root_files.into_iter().take(40).collect::<Vec<_>>().join(", ")));
    }
    lines.join("\n")
}

/// Start of a text file, None for binary or unreadable files.
fn excerpt(path: &Path, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(limit as u64 * 4).read_to_end(&mut bytes).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut cut: String = text.chars().take(limit).collect();
    if cut.len() < text.len() || std::fs::metadata(path).is_ok_and(|m| m.len() > bytes.len() as u64) {
        cut.push_str("\n[…]");
    }
    Some(cut)
}

/// Summary of the repository for the model: structure, README, manifests, CI and existing agent files.
pub fn context(scan: &RepoScan) -> String {
    let root = Path::new(&scan.root);
    let languages = scan.languages.iter().take(12).map(|(l, n)| format!("{l} {n}")).collect::<Vec<_>>().join(", ");
    let mut out = format!(
        "Repository: {}\nFiles: {}{} ({})\nLanguages (files): {}\n\nFolder structure:\n{}\n",
        crate::paths::folder_name(&scan.root),
        scan.file_count,
        if scan.truncated { "+" } else { "" },
        if scan.git { "git, without ignored files" } else { "no git repository" },
        if languages.is_empty() { "unknown".into() } else { languages },
        tree(&scan.files),
    );
    let mut include = |title: String, relative: &str, limit: usize| {
        if let Some(text) = excerpt(&root.join(relative), limit) {
            out.push_str(&format!("\n=== {title} ===\n{}\n", text.trim_end()));
        }
    };
    if let Some(readme) = scan.files.iter().find(|f| !f.contains('/') && f.to_lowercase().starts_with("readme")) {
        include(format!("{readme} (excerpt)"), readme, 4000);
    }
    for manifest in scan.manifests.iter().take(8) {
        include(manifest.clone(), manifest, 3000);
    }
    let workflows: Vec<&String> = scan.files.iter().filter(|f| f.starts_with(".github/workflows/")).collect();
    for workflow in workflows.iter().take(2) {
        include(format!("CI workflow {workflow}"), workflow, 2000);
    }
    for file in &scan.agent_files {
        include(format!("Existing {file}"), file, 8000);
    }
    out
}

/// What the generated file should optimise for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Goal {
    Cost,
    Quality,
    Speed,
    Security,
    Conventions,
}

impl Goal {
    fn instruction(self) -> &'static str {
        match self {
            Goal::Cost => "Cost efficiency: minimal tokens, tool calls and sub-agents; smallest correct diffs; targeted search instead of broad exploration; smallest relevant test first; cheapest capable model per task, stronger models only for hard reasoning.",
            Goal::Quality => "Quality and correctness: understand before changing, verify with tests and builds, cover edge cases, review the diff before finishing; stronger models where mistakes are expensive.",
            Goal::Speed => "Speed: short answers, act without unnecessary questions when the request is clear, run independent steps in parallel, no long explanations.",
            Goal::Security => "Security: never read, print or commit secrets and credentials, no destructive or irreversible commands without confirmation, careful with dependencies and untrusted input, flag security-relevant changes.",
            Goal::Conventions => "Team conventions: follow the code style, commit message, branch and pull request conventions visible in the repository; keep tests and documentation in step with code changes.",
        }
    }
}

/// Choices from the dialog.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub goals: Vec<Goal>,
    /// `de` or `en`: language of the generated file.
    pub language: String,
    /// Gateway model that runs the analysis; empty picks one.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub notes: String,
}

/// The model for the analysis: the chosen one, else the profile's Opus, Sonnet, default or Haiku model, else the gateway's best.
pub fn analysis_model(chosen: &str, profile: &Profile, available: &[String]) -> Option<String> {
    if !chosen.trim().is_empty() {
        return Some(chosen.trim().to_string());
    }
    let configured = [profile.opus_model.as_deref(), profile.sonnet_model.as_deref(), Some(profile.model_alias.as_str()), profile.haiku_model.as_deref()];
    if let Some(model) = configured.into_iter().flatten().map(str::trim).find(|m| !m.is_empty()) {
        return Some(model.to_string());
    }
    let (opus, sonnet, haiku) = suggest_models(available);
    opus.or(sonnet).or(haiku).or_else(|| available.first().cloned())
}

pub const SYSTEM_PROMPT: &str = "You configure AI coding agents such as Claude Code, Codex, Cursor and GitHub Copilot. You write AGENTS.md files: \
the instruction file an agent loads into its context at the start of every session in a repository. Every line is paid for in every session, \
so the file must be short (usually 40 to 120 lines), concrete and specific to this repository: real commands, real paths, real conventions. \
Leave out generic advice agents follow anyway, and never invent commands, paths, tools or conventions the repository summary does not show. \
Never include secrets, keys or personal data.";

pub fn prompt(context: &str, options: &Options, profile: &Profile, available: &[String]) -> String {
    let language = if options.language == "de" { "German" } else { "English" };
    let goals = if options.goals.is_empty() {
        "- Balanced: correct results at reasonable cost.".to_string()
    } else {
        options.goals.iter().map(|g| format!("- {}", g.instruction())).collect::<Vec<_>>().join("\n")
    };
    let tiers = [
        ("opus", profile.opus_model.as_deref()),
        ("sonnet", profile.sonnet_model.as_deref()),
        ("haiku", profile.haiku_model.as_deref()),
        ("default (ANTHROPIC_MODEL)", Some(profile.model_alias.as_str())),
    ];
    let configured: Vec<(&str, &str)> = tiers.into_iter().filter_map(|(tier, m)| Some((tier, m?.trim())).filter(|(_, m)| !m.is_empty())).collect();
    let tier_lines = if configured.is_empty() {
        "- none configured; Claude Code uses its built-in defaults".to_string()
    } else {
        configured.iter().map(|(tier, m)| format!("- {tier}: {m}")).collect::<Vec<_>>().join("\n")
    };
    let others: Vec<&str> = available.iter().map(String::as_str).filter(|m| !configured.iter().any(|(_, c)| c == m)).take(40).collect();
    let others = if others.is_empty() { "unknown".to_string() } else { others.join(", ") };
    let notes = match options.notes.trim() {
        "" => String::new(),
        notes => format!("\n<notes_from_the_team>\n{notes}\n</notes_from_the_team>\n"),
    };
    format!(
        "<repository>\n{context}\n</repository>\n\n<goals>\n{goals}\n</goals>\n\n<models>\n\
Model tiers configured for this project (Claude Code maps its opus, sonnet and haiku aliases to them):\n{tier_lines}\n\
Further models the project key may use on the gateway: {others}\n\
Rough relative price per token: Sonnet costs about half of Opus, Haiku 5 to 40 times less than Opus depending on the generation. \
Opus is strongest at hard reasoning, architecture, debugging and security; Sonnet handles most implementation work; \
Haiku suits search, summaries, simple edits and sub-agents.\n</models>\n{notes}\n\
Tasks:\n\
1. Assess the repository: kind of project, stack, size, complexity, risky areas and the typical agent tasks here.\n\
2. Decide which of the available models fits which kind of work in this repository (for example exploration and search, routine edits, \
feature work, debugging, architecture or security review), weighing the goals. Start with the recommended default model for everyday work. \
Use only model names listed under models; if none are listed, use the tier names opus, sonnet and haiku.\n\
3. Write the AGENTS.md in {language}: the project in one or two lines, build, test and lint commands taken from the manifests, folders to ignore \
(generated output, dependencies, caches), conventions visible in the repository, rules that implement the goals, and a short model section \
based on step 2. If an existing AGENTS.md or CLAUDE.md is included, keep its still valid project-specific content and improve it instead of starting over.\n\n\
Reply in exactly this format and nothing else:\n\
<assessment>\nThree to six sentences in {language}.\n</assessment>\n\
<recommendations>\nOne line per recommendation: task | model | tier (opus, sonnet, haiku or other) | short reason in {language}\n</recommendations>\n\
<agents_md>\nThe complete file content as Markdown.\n</agents_md>"
    )
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Recommendation {
    pub task: String,
    pub model: String,
    pub tier: String,
    pub reason: String,
}

/// The model's answer, shown for review before anything is written.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub assessment: String,
    pub recommendations: Vec<Recommendation>,
    /// Suggested `AGENTS.md` content.
    pub content: String,
    /// Model that ran the analysis.
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn section<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    Some(rest.rfind(&format!("</{tag}>")).map_or(rest, |end| &rest[..end]).trim())
}

/// Parses the reply; None when it holds no `AGENTS.md`.
pub fn parse_reply(text: &str) -> Option<Proposal> {
    let mut content = section(text, "agents_md").filter(|c| !c.is_empty())?.to_string();
    // Some models wrap the file in a Markdown code fence.
    if content.starts_with("```") && content.ends_with("```") && content.len() > 6 {
        let inner = &content[content.find('\n').unwrap_or(3)..content.len() - 3];
        content = inner.trim().to_string();
    }
    let recommendations = section(text, "recommendations")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*']).trim().trim_matches('|');
            let parts: Vec<&str> = line.splitn(4, '|').map(str::trim).collect();
            let [task, model, tier, reason] = parts[..] else { return None };
            let separator = parts.iter().all(|p| p.chars().all(|c| matches!(c, '-' | ':' | ' ')));
            (!separator && !task.eq_ignore_ascii_case("task")).then(|| Recommendation {
                task: task.into(),
                model: model.trim_matches('`').into(),
                tier: tier.to_lowercase(),
                reason: reason.into(),
            })
        })
        .collect();
    Some(Proposal {
        assessment: section(text, "assessment").unwrap_or_default().to_string(),
        recommendations,
        content,
        ..Default::default()
    })
}
