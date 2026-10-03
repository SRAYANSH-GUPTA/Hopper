//! Discovers the skills and custom commands each agent CLI exposes, so the
//! composer's `/` menu can list them for the active provider.
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;
use tokio::task;

use crate::codex::home::{resolve_default_codex_home, resolve_home_dir};
use crate::shared::frontmatter;
use crate::types::WorkspaceEntry;

const MAX_ENTRIES: usize = 500;
const MAX_HEAD_BYTES: u64 = 16 * 1024;
const MAX_COMMAND_DEPTH: usize = 3;
const MAX_DESCRIPTION_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SlashCommandEntry {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) argument_hint: Option<String>,
    /// `"skill"` or `"command"`.
    pub(crate) kind: &'static str,
    /// `"user"`, `"project"` or `"plugin"`.
    pub(crate) scope: &'static str,
    pub(crate) plugin: Option<String>,
    /// `"slash"` (typed as `/name`) or `"mention"` (typed as `$name`, Codex skills).
    pub(crate) invocation: &'static str,
}

/// Home directories the scanners read from. Resolved by the caller so tests
/// can point them at fixtures.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProviderDirs {
    pub(crate) home: Option<PathBuf>,
    pub(crate) claude_home: Option<PathBuf>,
    pub(crate) codex_home: Option<PathBuf>,
}

impl ProviderDirs {
    pub(crate) fn resolve() -> Self {
        let home = resolve_home_dir();
        let claude_home = env::var("CLAUDE_CONFIG_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".claude")));
        Self {
            home,
            claude_home,
            codex_home: resolve_default_codex_home(),
        }
    }
}

#[derive(Default)]
struct Collector {
    entries: Vec<SlashCommandEntry>,
    seen: HashSet<String>,
}

impl Collector {
    fn push(&mut self, entry: SlashCommandEntry) {
        if self.entries.len() >= MAX_ENTRIES || !is_valid_name(&entry.name) {
            return;
        }
        if self.seen.insert(entry.name.to_ascii_lowercase()) {
            self.entries.push(entry);
        }
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'))
}

/// Reads the start of a file; frontmatter always sits at the top.
fn read_head(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_HEAD_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn first_body_line(body: &str) -> Option<String> {
    body.lines()
        .map(|line| line.trim().trim_start_matches('>').trim())
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("```"))
        .map(str::to_string)
}

fn truncate(value: String) -> String {
    if value.chars().count() <= MAX_DESCRIPTION_CHARS {
        return value;
    }
    let mut out: String = value.chars().take(MAX_DESCRIPTION_CHARS - 1).collect();
    out.push('…');
    out
}

struct ParsedFile {
    name: Option<String>,
    description: Option<String>,
    argument_hint: Option<String>,
    user_invocable: bool,
}

fn parse_file(path: &Path) -> Option<ParsedFile> {
    let content = read_head(path)?;
    let (fields, body_offset) =
        frontmatter::parse_frontmatter_fields(&content).unwrap_or_else(|| (Vec::new(), 0));
    let non_empty = |key: &str| {
        frontmatter::field(&fields, key)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let description = non_empty("description").or_else(|| first_body_line(&content[body_offset..]));
    Some(ParsedFile {
        name: non_empty("name"),
        description: description.map(truncate),
        argument_hint: non_empty("argument-hint").or_else(|| non_empty("argument_hint")),
        user_invocable: !matches!(
            frontmatter::field(&fields, "user-invocable").map(str::trim),
            Some("false")
        ),
    })
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}

fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

fn qualified(plugin: Option<&str>, name: &str) -> String {
    match plugin {
        Some(plugin) => format!("{plugin}:{name}"),
        None => name.to_string(),
    }
}

#[derive(Clone, Copy)]
struct Source<'a> {
    scope: &'static str,
    plugin: Option<&'a str>,
    invocation: &'static str,
}

fn push_parsed(
    collector: &mut Collector,
    parsed: ParsedFile,
    fallback_name: &str,
    kind: &'static str,
    source: Source<'_>,
    description_suffix: Option<String>,
) {
    if !parsed.user_invocable {
        return;
    }
    let base = parsed.name.unwrap_or_else(|| fallback_name.to_string());
    let description = match (parsed.description, description_suffix) {
        (Some(description), Some(suffix)) => Some(format!("{description} ({suffix})")),
        (None, Some(suffix)) => Some(format!("({suffix})")),
        (description, None) => description,
    };
    collector.push(SlashCommandEntry {
        name: qualified(source.plugin, &base),
        description,
        argument_hint: parsed.argument_hint,
        kind,
        scope: source.scope,
        plugin: source.plugin.map(str::to_string),
        invocation: source.invocation,
    });
}

/// A single skill directory containing `SKILL.md`.
fn scan_skill_dir(collector: &mut Collector, path: &Path, source: Source<'_>) {
    let Some(dir_name) = file_name(path).map(str::to_string) else {
        return;
    };
    if dir_name.starts_with('.') || !path.is_dir() {
        return;
    }
    let skill_file = path.join("SKILL.md");
    if let Some(parsed) = skill_file.is_file().then(|| parse_file(&skill_file)).flatten() {
        push_parsed(collector, parsed, &dir_name, "skill", source, None);
    }
}

/// Skills laid out as `<dir>/<name>/SKILL.md`.
fn scan_skill_dirs(collector: &mut Collector, dir: &Path, source: Source<'_>) {
    for path in sorted_entries(dir) {
        scan_skill_dir(collector, &path, source);
    }
}

/// Skills stored as single Markdown files, `<dir>/<name>.md`.
fn scan_skill_files(collector: &mut Collector, dir: &Path, source: Source<'_>) {
    for path in sorted_entries(dir) {
        if !path.is_file() || !is_markdown(&path) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string) else {
            continue;
        };
        if let Some(parsed) = parse_file(&path) {
            push_parsed(collector, parsed, &stem, "skill", source, None);
        }
    }
}

/// Custom commands, `<dir>/**/<name>.md`. Subdirectories organize commands and
/// are shown in the description; symlinked directories are not followed.
fn scan_command_dir(collector: &mut Collector, root: &Path, source: Source<'_>) {
    fn walk(collector: &mut Collector, dir: &Path, prefix: &[String], source: Source<'_>) {
        for path in sorted_entries(dir) {
            let Some(name) = file_name(&path).map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let is_real_dir = fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
            if is_real_dir {
                if prefix.len() + 1 < MAX_COMMAND_DEPTH {
                    let mut next = prefix.to_vec();
                    next.push(name);
                    walk(collector, &path, &next, source);
                }
                continue;
            }
            if !path.is_file() || !is_markdown(&path) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if let Some(parsed) = parse_file(&path) {
                let suffix = (!prefix.is_empty()).then(|| prefix.join("/"));
                push_parsed(collector, parsed, stem, "command", source, suffix);
            }
        }
    }
    walk(collector, root, &[], source);
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Extra component paths a plugin manifest declares (a string or an array of
/// strings), resolved inside the plugin. Paths escaping the plugin are ignored.
fn manifest_paths(manifest: Option<&Value>, key: &str, install_path: &Path) -> Vec<PathBuf> {
    let values = match manifest.and_then(|manifest| manifest.get(key)) {
        Some(Value::String(path)) => vec![path.as_str()],
        Some(Value::Array(paths)) => paths.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .map(Path::new)
        .filter(|path| {
            !path.is_absolute()
                && path
                    .components()
                    .all(|component| !matches!(component, std::path::Component::ParentDir))
        })
        .map(|path| install_path.join(path))
        .collect()
}

/// Skills and commands of one plugin: its default `skills/` and `commands/`
/// directories plus any extra paths its `.claude-plugin/plugin.json` declares.
fn scan_plugin(collector: &mut Collector, install_path: &Path, source: Source<'_>) {
    let manifest = read_json(&install_path.join(".claude-plugin").join("plugin.json"));
    scan_skill_dirs(collector, &install_path.join("skills"), source);
    for path in manifest_paths(manifest.as_ref(), "skills", install_path) {
        if path.join("SKILL.md").is_file() {
            scan_skill_dir(collector, &path, source);
        } else {
            scan_skill_dirs(collector, &path, source);
        }
    }
    scan_command_dir(collector, &install_path.join("commands"), source);
    for path in manifest_paths(manifest.as_ref(), "commands", install_path) {
        if path.is_file() && is_markdown(&path) {
            if let (Some(stem), Some(parsed)) = (
                path.file_stem().and_then(|stem| stem.to_str()),
                parse_file(&path),
            ) {
                push_parsed(collector, parsed, stem, "command", source, None);
            }
        } else {
            scan_command_dir(collector, &path, source);
        }
    }
}

/// Skills and commands of installed, enabled Claude Code plugins.
fn scan_claude_plugins(collector: &mut Collector, claude_home: &Path, workspace: Option<&Path>) {
    let Some(registry) = read_json(&claude_home.join("plugins").join("installed_plugins.json")) else {
        return;
    };
    let enabled = read_json(&claude_home.join("settings.json"))
        .and_then(|settings| settings.get("enabledPlugins").cloned())
        .unwrap_or(Value::Null);
    let Some(plugins) = registry.get("plugins").and_then(Value::as_object) else {
        return;
    };
    let mut keys: Vec<&String> = plugins.keys().collect();
    keys.sort();
    for key in keys {
        if enabled.get(key).and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let plugin_name = key.split('@').next().unwrap_or(key.as_str());
        let installs = plugins[key].as_array().cloned().unwrap_or_default();
        let install = installs.iter().find(|install| {
            match install.get("projectPath").and_then(Value::as_str) {
                Some(project) => workspace.is_some_and(|workspace| workspace == Path::new(project)),
                None => true,
            }
        });
        let Some(install_path) = install
            .and_then(|install| install.get("installPath"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
        else {
            continue;
        };
        let source = Source {
            scope: "plugin",
            plugin: Some(plugin_name),
            invocation: "slash",
        };
        scan_plugin(collector, &install_path, source);
    }
}

/// Lists the skills and custom commands `provider` can run as slash commands
/// (or, for Codex skills, as `$` mentions). Earlier sources win name collisions.
pub(crate) fn list_slash_commands(
    provider: &str,
    workspace: Option<&Path>,
    dirs: &ProviderDirs,
) -> Vec<SlashCommandEntry> {
    let mut collector = Collector::default();
    let slash = |scope| Source {
        scope,
        plugin: None,
        invocation: "slash",
    };
    match provider {
        "claude" => {
            if let Some(claude_home) = &dirs.claude_home {
                scan_skill_dirs(&mut collector, &claude_home.join("skills"), slash("user"));
                scan_command_dir(&mut collector, &claude_home.join("commands"), slash("user"));
            }
            if let Some(workspace) = workspace {
                let project = workspace.join(".claude");
                scan_skill_dirs(&mut collector, &project.join("skills"), slash("project"));
                scan_command_dir(&mut collector, &project.join("commands"), slash("project"));
            }
            if let Some(claude_home) = &dirs.claude_home {
                scan_claude_plugins(&mut collector, claude_home, workspace);
            }
        }
        "antigravity" => {
            if let Some(home) = &dirs.home {
                let skills = home.join(".gemini").join("antigravity-cli").join("skills");
                scan_skill_files(&mut collector, &skills, slash("user"));
                scan_skill_dirs(&mut collector, &skills, slash("user"));
            }
        }
        _ => {
            let mention = |scope| Source {
                scope,
                plugin: None,
                invocation: "mention",
            };
            if let Some(codex_home) = &dirs.codex_home {
                scan_skill_dirs(&mut collector, &codex_home.join("skills"), mention("user"));
            }
            if let Some(workspace) = workspace {
                scan_skill_dirs(
                    &mut collector,
                    &workspace.join(".agents").join("skills"),
                    mention("project"),
                );
            }
        }
    }
    collector.entries
}

pub(crate) async fn slash_commands_list_core(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: Option<String>,
    provider: String,
) -> Result<Vec<SlashCommandEntry>, String> {
    let workspace_path = match workspace_id {
        Some(id) => workspaces
            .lock()
            .await
            .get(&id)
            .map(|entry| PathBuf::from(&entry.path)),
        None => None,
    };
    task::spawn_blocking(move || {
        list_slash_commands(&provider, workspace_path.as_deref(), &ProviderDirs::resolve())
    })
    .await
    .map_err(|_| "slash command discovery failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = env::temp_dir().join(format!("hopper-slash-{prefix}-{nonce}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, content).expect("write file");
    }

    fn names(entries: &[SlashCommandEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn lists_claude_skills_commands_and_plugins() {
        let root = temp_dir("claude");
        let claude_home = root.join("claude");
        let workspace = root.join("ws");
        write(
            &claude_home.join("skills/graphify/SKILL.md"),
            "---\nname: graphify\ndescription: Build a knowledge graph\nargument-hint: <path>\n---\n# graphify\n",
        );
        write(
            &claude_home.join("skills/hidden/SKILL.md"),
            "---\nname: hidden\nuser-invocable: false\n---\n",
        );
        write(&claude_home.join("skills/.system/SKILL.md"), "---\nname: system\n---\n");
        write(&claude_home.join("skills/no-skill-file/README.md"), "nothing");
        write(&claude_home.join("commands/deploy.md"), "Deploy the app to staging.\n");
        write(&claude_home.join("commands/frontend/component.md"), "---\ndescription: Make a component\n---\n");
        write(&workspace.join(".claude/skills/graphify/SKILL.md"), "---\nname: graphify\n---\n");
        write(&workspace.join(".claude/commands/test-all.md"), "---\ndescription: Run tests\n---\n");
        let plugin_dir = root.join("plugins/design");
        let disabled_dir = root.join("plugins/off");
        write(&plugin_dir.join("skills/frontend-design/SKILL.md"), "---\ndescription: Design UI\n---\n");
        write(&plugin_dir.join("commands/review.md"), "Review the design.\n");
        write(&disabled_dir.join("skills/off-skill/SKILL.md"), "---\n---\n");
        write(
            &claude_home.join("plugins/installed_plugins.json"),
            &serde_json::json!({
                "version": 2,
                "plugins": {
                    "design@official": [{ "scope": "user", "installPath": plugin_dir }],
                    "off@official": [{ "scope": "user", "installPath": disabled_dir }],
                }
            })
            .to_string(),
        );
        write(
            &claude_home.join("settings.json"),
            r#"{ "enabledPlugins": { "off@official": false } }"#,
        );

        let dirs = ProviderDirs {
            claude_home: Some(claude_home),
            ..ProviderDirs::default()
        };
        let entries = list_slash_commands("claude", Some(&workspace), &dirs);
        assert_eq!(
            names(&entries),
            vec![
                "graphify",
                "deploy",
                "component",
                "test-all",
                "design:frontend-design",
                "design:review",
            ]
        );
        let graphify = &entries[0];
        assert_eq!(graphify.description.as_deref(), Some("Build a knowledge graph"));
        assert_eq!(graphify.argument_hint.as_deref(), Some("<path>"));
        assert_eq!((graphify.kind, graphify.scope, graphify.invocation), ("skill", "user", "slash"));
        assert_eq!(entries[1].description.as_deref(), Some("Deploy the app to staging."));
        assert_eq!(entries[2].description.as_deref(), Some("Make a component (frontend)"));
        assert_eq!(entries[3].scope, "project");
        assert_eq!(entries[4].plugin.as_deref(), Some("design"));
        assert_eq!(entries[5].kind, "command");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_plugin_manifest_skill_and_command_paths() {
        let root = temp_dir("manifest");
        let claude_home = root.join("claude");
        let plugin_dir = root.join("plugins/ui");
        write(
            &plugin_dir.join(".claude-plugin/plugin.json"),
            r#"{ "name": "ui", "skills": ["./.claude/skills/ui-pro", "../escape"], "commands": "./extra/run.md" }"#,
        );
        write(&plugin_dir.join(".claude/skills/ui-pro/SKILL.md"), "---\nname: ui-pro\n---\n");
        write(&plugin_dir.join(".claude/skills/other/SKILL.md"), "---\nname: other\n---\n");
        write(&plugin_dir.join("extra/run.md"), "> Run the thing\n");
        write(&root.join("plugins/escape/SKILL.md"), "---\nname: escape\n---\n");
        write(
            &claude_home.join("plugins/installed_plugins.json"),
            &serde_json::json!({ "plugins": { "ui@market": [{ "installPath": plugin_dir }] } }).to_string(),
        );
        let dirs = ProviderDirs {
            claude_home: Some(claude_home),
            ..ProviderDirs::default()
        };
        let entries = list_slash_commands("claude", None, &dirs);
        assert_eq!(names(&entries), vec!["ui:ui-pro", "ui:run"]);
        assert_eq!(entries[1].description.as_deref(), Some("Run the thing"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_codex_skills_as_mentions() {
        let root = temp_dir("codex");
        let codex_home = root.join("codex");
        let workspace = root.join("ws");
        write(&codex_home.join("skills/devops-helper/SKILL.md"), "---\ndescription: CI help\n---\n");
        write(&workspace.join(".agents/skills/local/SKILL.md"), "---\n---\nLocal skill body\n");
        let dirs = ProviderDirs {
            codex_home: Some(codex_home),
            ..ProviderDirs::default()
        };
        let entries = list_slash_commands("codex", Some(&workspace), &dirs);
        assert_eq!(names(&entries), vec!["devops-helper", "local"]);
        assert!(entries.iter().all(|entry| entry.invocation == "mention"));
        assert_eq!(entries[1].description.as_deref(), Some("Local skill body"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_antigravity_skill_files_and_dirs() {
        let root = temp_dir("agy");
        let skills = root.join(".gemini/antigravity-cli/skills");
        write(&skills.join("devops-helper.md"), "---\ndescription: CI help\n---\n");
        write(&skills.join("folder-skill/SKILL.md"), "---\nname: folder-skill\n---\n");
        let dirs = ProviderDirs {
            home: Some(root.clone()),
            ..ProviderDirs::default()
        };
        let entries = list_slash_commands("antigravity", None, &dirs);
        assert_eq!(names(&entries), vec!["devops-helper", "folder-skill"]);
        assert!(entries.iter().all(|entry| entry.invocation == "slash"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_directories_yield_nothing() {
        let dirs = ProviderDirs {
            home: Some(PathBuf::from("/nonexistent/hopper")),
            claude_home: Some(PathBuf::from("/nonexistent/hopper/.claude")),
            codex_home: Some(PathBuf::from("/nonexistent/hopper/.codex")),
        };
        for provider in ["claude", "codex", "antigravity"] {
            assert!(list_slash_commands(provider, None, &dirs).is_empty());
        }
    }
}
