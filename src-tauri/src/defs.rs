//! Definitions: what Dev Cleaner knows about caches, project build folders,
//! processes, ports, model folders and config files, kept as data in
//! `definitions/*.json` instead of code.
//!
//! A copy is compiled into the app. A newer set in the app's data folder
//! (`<data>/definitions`) is used instead when it loads and checks out; if
//! anything about it is wrong, the built-in copy is used and the reason is
//! kept for Settings. The engine (deleting, running tools, elevation, the
//! drive walk) stays in code: definitions can only name the engine's
//! actions, never supply commands of their own.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// The definitions format this app reads.
pub const SCHEMA: u32 = 1;

const BUILTIN: &[(&str, &str)] = &[
    ("index.json", include_str!("../../definitions/index.json")),
    ("caches.json", include_str!("../../definitions/caches.json")),
    ("projects.json", include_str!("../../definitions/projects.json")),
    ("processes.json", include_str!("../../definitions/processes.json")),
    ("ports.json", include_str!("../../definitions/ports.json")),
    ("models.json", include_str!("../../definitions/models.json")),
    ("configs.json", include_str!("../../definitions/configs.json")),
    ("startup.json", include_str!("../../definitions/startup.json")),
    ("services.json", include_str!("../../definitions/services.json")),
];

// ------------------------------------------------------------------- format

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub schema: u32,
    pub version: String,
    #[serde(default)]
    pub min_app: String,
    pub files: Vec<String>,
}

/// One cache, language cache or junk location.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheRule {
    pub id: String,
    pub name: String,
    pub eco: String,
    pub risk: crate::caches::Risk,
    pub about: String,
    #[serde(default)]
    pub paths: Vec<PathEntry>,
    #[serde(default)]
    pub how: HowDef,
    /// Only on these OSes ("windows", "macos", "linux"); all when empty.
    #[serde(default)]
    pub os: Vec<String>,
    /// Listed on System junk rather than Disk cleanup.
    #[serde(default)]
    pub junk: bool,
    /// System locations: never offered under the test fake home.
    #[serde(default)]
    pub real_only: bool,
    /// The oldest app that understands this rule. An older app skips just
    /// this rule and keeps the rest of the set.
    #[serde(default)]
    pub min_app: Option<String>,
    /// Offered only when this tool is on PATH.
    #[serde(default)]
    pub requires: Option<String>,
    /// Ask the tool where its cache really is (it may have been moved).
    #[serde(default)]
    pub ask: Vec<Ask>,
    /// An engine step to run first (see BEFORE_ACTIONS).
    #[serde(default)]
    pub before: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub tool: String,
    pub args: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HowDef {
    #[default]
    Delete,
    OlderThan {
        hours: u64,
    },
    #[serde(rename_all = "camelCase")]
    Tool {
        tool: String,
        args: Vec<String>,
        /// Run once per existing path, `{path}` replaced (one pnpm store per drive).
        #[serde(default)]
        per_path_args: Vec<String>,
    },
    Command {
        tool: String,
        args: Vec<String>,
    },
    Docker,
    RecycleBin,
    Admin {
        #[serde(default)]
        days: Option<u32>,
        /// A named admin action built into the app (see junk::admin_action).
        #[serde(default)]
        action: Option<String>,
    },
}

/// A path: a template, an expander, or a per-OS choice of either.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PathEntry {
    Template(String),
    Expand(Expand),
    PerOs(PerOs),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Expand {
    /// "children", "old-entries", "chromium-profiles", "files", "drive-walk" or "drive-roots".
    pub expand: String,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub tail: Vec<String>,
    /// "versioned": only sub-folders with a digit in the name.
    #[serde(default, rename = "match")]
    pub matching: Option<String>,
    #[serde(default, rename = "except")]
    pub except: Vec<String>,
    #[serde(default)]
    pub except_prefix: Vec<String>,
    /// For children: only names containing one of these.
    #[serde(default)]
    pub contains: Vec<String>,
    /// For old-entries: how many of the newest entries to keep (default 1).
    #[serde(default)]
    pub keep: Option<usize>,
    #[serde(default)]
    pub ext: Vec<String>,
    #[serde(default)]
    pub prefix: Vec<String>,
    /// For drive-walk: "partials", "thumbs" or "shader-caches".
    #[serde(default)]
    pub find: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerOs {
    #[serde(default)]
    pub windows: Option<Box<PathEntry>>,
    #[serde(default)]
    pub macos: Option<Box<PathEntry>>,
    #[serde(default)]
    pub linux: Option<Box<PathEntry>>,
    /// macOS and Linux.
    #[serde(default)]
    pub unix: Option<Box<PathEntry>>,
}

impl PerOs {
    pub fn pick(&self) -> Option<&PathEntry> {
        let specific = match std::env::consts::OS {
            "windows" => &self.windows,
            "macos" => &self.macos,
            _ => &self.linux,
        };
        let fallback = if cfg!(windows) { &None } else { &self.unix };
        specific.as_deref().or(fallback.as_deref())
    }
}

/// A project build or dependency folder.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRule {
    pub names: Vec<String>,
    pub kind: String,
    pub label: String,
    pub restore: String,
    #[serde(default)]
    pub parent: Option<Has>,
    #[serde(default)]
    pub inside: Option<Has>,
    #[serde(default)]
    pub grandparent: Option<Has>,
    #[serde(default)]
    pub parent_name: Option<String>,
}

/// What a folder must contain: any of these files or extensions, and all of
/// these folders.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Has {
    #[serde(default)]
    pub any: Vec<String>,
    #[serde(default)]
    pub any_ext: Vec<String>,
    #[serde(default)]
    pub all_dirs: Vec<String>,
}

impl Has {
    pub fn holds(&self, dir: &Path) -> bool {
        let any_ok = (self.any.is_empty() && self.any_ext.is_empty())
            || self.any.iter().any(|f| dir.join(f).exists())
            || (!self.any_ext.is_empty()
                && std::fs::read_dir(dir).is_ok_and(|rd| {
                    rd.flatten().any(|e| {
                        let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                        self.any_ext.iter().any(|x| n.ends_with(&format!(".{x}")))
                    })
                }));
        any_ok && self.all_dirs.iter().all(|d| dir.join(d).is_dir())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Projects {
    pub rules: Vec<ProjectRule>,
    /// Folders never walked into (lower-case names).
    pub skip: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Processes {
    pub system: HashMap<String, String>,
    pub save_first: HashMap<String, String>,
    pub safe: HashMap<String, String>,
    pub helpers: Helpers,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Helpers {
    pub suffixes: Vec<String>,
    pub contains: Vec<String>,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ports {
    pub common: Vec<u16>,
    pub command_hints: Vec<(String, String)>,
    pub process_hints: HashMap<String, String>,
    pub port_hints: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Models {
    pub app_roots: Vec<AppRoot>,
    pub loose_roots: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppRoot {
    pub path: String,
    pub app: String,
    pub label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configs {
    /// (id, display name), in the order the Configs page lists them.
    pub tools: Vec<(String, String)>,
    pub secrets: HashMap<String, String>,
    pub items: Vec<ConfigDef>,
    pub copy_targets: CopyTargets,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDef {
    pub tool: String,
    pub slug: String,
    pub label: String,
    pub kind: crate::configs::Kind,
    /// Same template on every OS.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub unix: Option<String>,
    /// Why the file is sensitive; "@name" refers to `secrets`.
    #[serde(default)]
    pub secret: Option<String>,
    /// Not selected by default.
    #[serde(default)]
    pub off: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyTargets {
    pub editors: Vec<Endpoint>,
    pub instructions: Vec<Endpoint>,
    pub skills: Vec<Endpoint>,
    pub commands: Vec<Endpoint>,
    pub agents: Vec<Endpoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// Programs that start at sign-in: which to keep, which are fine to turn off.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Startup {
    pub keep: HashMap<String, String>,
    pub optional: HashMap<String, String>,
}

/// Services: which the OS needs, which are developer servers, which are optional.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Services {
    pub protected: HashMap<String, String>,
    pub dev: HashMap<String, String>,
    pub optional: HashMap<String, String>,
}

/// Look a name up in a verdict table: case-insensitive, and a key ending in
/// `*` matches any name that starts with the rest.
pub fn lookup<'a>(table: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    let n = name.to_ascii_lowercase();
    if let Some((_, v)) = table.iter().find(|(k, _)| k.to_ascii_lowercase() == n) {
        return Some(v.as_str());
    }
    table
        .iter()
        .filter_map(|(k, v)| k.strip_suffix('*').map(|p| (p.to_ascii_lowercase(), v)))
        .filter(|(p, _)| n.starts_with(p.as_str()))
        .max_by_key(|(p, _)| p.len())
        .map(|(_, v)| v.as_str())
}

pub struct Defs {
    pub index: Index,
    pub caches: Vec<CacheRule>,
    pub projects: Projects,
    pub processes: Processes,
    pub ports: Ports,
    pub models: Models,
    pub configs: Configs,
    pub startup: Startup,
    pub services: Services,
    /// Cache rules this app skipped: written for a newer Dev Cleaner, or
    /// failing a check. The rest of the set is still used.
    pub skipped: Vec<String>,
}

// ---------------------------------------------------------------- templates

/// Expand `{home}`, `{local}`, `{env:NAME|fallback}`... into a path string.
/// Unknown variables are an error, so a bad rule is caught when the set
/// loads, not when it cleans.
pub fn expand_str(t: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = t;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let mut depth = 1;
        let mut end = None;
        for (i, c) in after.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| format!("unclosed {{ in {t}"))?;
        out.push_str(&var(&after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn var(name: &str) -> Result<String, String> {
    use crate::util;
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    if let Some(e) = name.strip_prefix("env:") {
        let (key, fallback) = e.split_once('|').unwrap_or((e, ""));
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || key.is_empty() {
            return Err(format!("bad variable name {key}"));
        }
        // Under the e2e fake home, never read the real machine's overrides.
        let real = std::env::var(key).ok().filter(|v| !v.is_empty() && util::fake_home().is_none());
        return match real {
            Some(v) => Ok(v),
            None => expand_str(fallback),
        };
    }
    let env_or = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    Ok(match name {
        "home" => s(util::home()),
        "local" => s(util::local_dir()),
        "roaming" => s(util::roaming_dir()),
        "cache" => s(util::cache_dir()),
        "temp" => s(util::temp_dir()),
        "xdgCache" => s(util::home().join(".cache")),
        "xdgData" => s(util::home().join(".local").join("share")),
        "localLow" => s(util::local_dir().parent().map(|p| p.join("LocalLow")).unwrap_or_default()),
        "user" => std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default(),
        "systemRoot" => env_or("SystemRoot", r"C:\Windows"),
        "systemDrive" => env_or("SystemDrive", "C:"),
        "programData" => env_or("ProgramData", r"C:\ProgramData"),
        other => return Err(format!("unknown variable {{{other}}}")),
    })
}

/// A template as a path, with this OS's separators. None when it expands to
/// nothing (an unset variable with an empty fallback).
pub fn expand(t: &str) -> Option<PathBuf> {
    let s = expand_str(t).ok()?;
    if s.trim().is_empty() {
        return None;
    }
    let raw = PathBuf::from(if cfg!(windows) { s.replace('/', "\\") } else { s });
    // Rebuild from components: drops doubled separators ("{temp}/x" where
    // the temp folder already ends in one).
    Some(raw.components().collect())
}

// ------------------------------------------------------------------ loading

/// Named engine steps a cache rule may ask for before cleaning.
pub const BEFORE_ACTIONS: &[&str] = &["stop-gradle"];
pub const EXPANDERS: &[&str] = &["children", "old-entries", "chromium-profiles", "files", "drive-walk", "drive-roots"];
pub const WALK_FINDS: &[&str] = &["partials", "thumbs", "shader-caches"];

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    BuiltIn,
    Folder,
}

pub struct Loaded {
    pub defs: Defs,
    pub source: Source,
    /// Why the folder copy wasn't used, if there is one and it wasn't.
    pub error: Option<String>,
    pub loaded_at: u64,
}

static FOLDER: OnceLock<PathBuf> = OnceLock::new();
static CURRENT: RwLock<Option<&'static Loaded>> = RwLock::new(None);

/// Where a newer set of definitions is kept (downloaded updates, later).
pub fn init(data_dir: &Path) {
    let _ = FOLDER.set(data_dir.join("definitions"));
}

pub fn folder() -> Option<&'static PathBuf> {
    FOLDER.get()
}

/// Parse one file. A top-level "$comment" is allowed and ignored; anything
/// else unknown is an error, so a typo can't silently drop a rule.
fn parse<T: for<'de> Deserialize<'de>>(name: &str, text: &str) -> Result<T, String> {
    let mut v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{name}: {e}"))?;
    if let Some(o) = v.as_object_mut() {
        o.remove("$comment");
    }
    serde_json::from_value(v).map_err(|e| format!("{name}: {e}"))
}

fn parse_set(read: impl Fn(&str) -> Result<String, String>) -> Result<Defs, String> {
    let index: Index = parse("index.json", &read("index.json")?)?;
    if index.schema != SCHEMA {
        return Err(format!("schema {} needs a different version of Dev Cleaner (this one reads schema {SCHEMA})", index.schema));
    }
    if !index.min_app.is_empty() && crate::updates::is_newer(&index.min_app, env!("CARGO_PKG_VERSION")) {
        return Err(format!("needs Dev Cleaner {} or newer", index.min_app));
    }
    let defs = Defs {
        caches: vec![],
        projects: parse("projects.json", &read("projects.json")?)?,
        processes: parse("processes.json", &read("processes.json")?)?,
        ports: parse("ports.json", &read("ports.json")?)?,
        models: parse("models.json", &read("models.json")?)?,
        configs: parse("configs.json", &read("configs.json")?)?,
        startup: parse("startup.json", &read("startup.json")?)?,
        services: parse("services.json", &read("services.json")?)?,
        skipped: vec![],
        index,
    };
    let (caches, skipped) = parse_caches(&read("caches.json")?)?;
    let mut defs = Defs { caches, skipped, ..defs };
    check(&defs)?;
    defs.skipped.sort();
    Ok(defs)
}

/// caches.json, one rule at a time: a rule this app can't read (a field or
/// expander added later, a `minApp` above this version, a failed check) is
/// skipped with the reason instead of failing the whole set. That way a
/// definitions update can add rules for newer apps without cutting older
/// ones off.
fn parse_caches(text: &str) -> Result<(Vec<CacheRule>, Vec<String>), String> {
    let mut v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("caches.json: {e}"))?;
    let rules = v.get_mut("rules").and_then(|r| r.as_array_mut()).map(std::mem::take).ok_or("caches.json: no rules list")?;
    let app = env!("CARGO_PKG_VERSION");
    let (mut ok, mut skipped) = (vec![], vec![]);
    for raw in rules {
        let id = raw.get("id").and_then(|x| x.as_str()).unwrap_or("?").to_string();
        let rule: CacheRule = match serde_json::from_value(raw) {
            Ok(r) => r,
            Err(e) => {
                skipped.push(format!("{id}: {e}"));
                continue;
            }
        };
        if let Some(m) = rule.min_app.as_deref().filter(|m| crate::updates::is_newer(m, app)) {
            skipped.push(format!("{id}: needs Dev Cleaner {m}"));
            continue;
        }
        match check_rule(&rule) {
            Ok(()) => ok.push(rule),
            Err(e) => skipped.push(e),
        }
    }
    Ok((ok, skipped))
}

/// A set's version ("2026.11.09.2") as numbers, for comparing sets.
fn set_version(v: &str) -> Vec<u64> {
    v.split('.').map(|x| x.parse().unwrap_or(0)).collect()
}

/// Structural checks: every template expands, every named action and
/// expander exists, ids are unique per OS. (Path and command limits come
/// with signing.)
fn tpl(t: &str, id: &str) -> Result<(), String> {
    expand_str(t).map(|_| ()).map_err(|e| format!("{id}: {e}"))
}

/// One cache rule: its OS names, templates, expanders and named actions.
fn check_rule(r: &CacheRule) -> Result<(), String> {
    fn entry(e: &PathEntry, id: &str, tpl: &dyn Fn(&str, &str) -> Result<(), String>) -> Result<(), String> {
        match e {
            PathEntry::Template(t) => tpl(t, id),
            PathEntry::Expand(x) => {
                if !EXPANDERS.contains(&x.expand.as_str()) {
                    return Err(format!("{id}: unknown expander {}", x.expand));
                }
                if x.expand == "drive-walk" && !x.find.as_deref().is_some_and(|f| WALK_FINDS.contains(&f)) {
                    return Err(format!("{id}: drive-walk needs find: {}", WALK_FINDS.join(", ")));
                }
                if x.expand != "drive-walk" && x.expand != "drive-roots" {
                    tpl(&x.dir, id)?;
                }
                Ok(())
            }
            PathEntry::PerOs(p) => {
                for x in [&p.windows, &p.macos, &p.linux, &p.unix].into_iter().flatten() {
                    entry(x, id, tpl)?;
                }
                Ok(())
            }
        }
    }
    for os in &r.os {
        if !["windows", "macos", "linux"].contains(&os.as_str()) {
            return Err(format!("{}: unknown os {os}", r.id));
        }
    }
    for p in &r.paths {
        entry(p, &r.id, &tpl)?;
    }
    if let HowDef::Admin { action: Some(a), .. } = &r.how {
        if crate::junk::admin_action(a).is_none() {
            return Err(format!("{}: unknown admin action {a}", r.id));
        }
    }
    if let Some(b) = &r.before {
        if !BEFORE_ACTIONS.contains(&b.as_str()) {
            return Err(format!("{}: unknown step {b}", r.id));
        }
    }
    Ok(())
}

/// Structural checks across the set: unique ids per OS, every template in
/// the other files expands. Each cache rule was checked as it was read.
pub fn check(d: &Defs) -> Result<(), String> {
    let mut seen: HashMap<(&str, &str), ()> = HashMap::new();
    for r in &d.caches {
        for os in ["windows", "macos", "linux"] {
            if (r.os.is_empty() || r.os.iter().any(|o| o == os)) && seen.insert((r.id.as_str(), os), ()).is_some() {
                return Err(format!("{}: listed twice for {os}", r.id));
            }
        }
    }
    for r in &d.models.app_roots {
        tpl(&r.path, &r.app)?;
    }
    for r in &d.models.loose_roots {
        tpl(r, "loose root")?;
    }
    for c in &d.configs.items {
        if let Some(s) = c.secret.as_deref().and_then(|s| s.strip_prefix('@')) {
            if !d.configs.secrets.contains_key(s) {
                return Err(format!("{}:{}: unknown secret note @{s}", c.tool, c.slug));
            }
        }
    }
    Ok(())
}

fn builtin_index() -> Index {
    parse("index.json", BUILTIN[0].1).expect("the built-in index is valid")
}

fn builtin() -> Defs {
    parse_set(|name| BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, t)| t.to_string()).ok_or(format!("{name} missing")))
        .expect("the built-in definitions are valid (checked by tests)")
}

fn from_folder(dir: &Path) -> Option<Result<Defs, String>> {
    if !dir.join("index.json").is_file() {
        return None;
    }
    Some(parse_set(|name| {
        let p = dir.join(name);
        let meta = std::fs::metadata(&p).map_err(|_| format!("{name} is missing"))?;
        if meta.len() > 4 * 1024 * 1024 {
            return Err(format!("{name} is too large"));
        }
        std::fs::read_to_string(&p).map_err(|e| format!("{name}: {e}"))
    }))
}

fn load() -> Loaded {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let found = folder().and_then(|d| from_folder(d));
    match found {
        // After an app update the built-in set may be the newer one: the
        // folder copy only wins when it is newer.
        Some(Ok(defs)) if set_version(&defs.index.version) <= set_version(&builtin_index().version) => {
            Loaded { defs: builtin(), source: Source::BuiltIn, error: None, loaded_at: now }
        }
        Some(Ok(defs)) => Loaded { defs, source: Source::Folder, error: None, loaded_at: now },
        Some(Err(e)) => Loaded { defs: builtin(), source: Source::BuiltIn, error: Some(e), loaded_at: now },
        None => Loaded { defs: builtin(), source: Source::BuiltIn, error: None, loaded_at: now },
    }
}

fn current() -> &'static Loaded {
    if let Some(l) = *CURRENT.read().unwrap_or_else(|e| e.into_inner()) {
        return l;
    }
    let mut w = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if let Some(l) = *w {
        return l;
    }
    // Leaked on purpose: rules hand out &'static strs, and a set is replaced
    // at most a few times a day.
    let l: &'static Loaded = Box::leak(Box::new(load()));
    *w = Some(l);
    l
}

pub fn get() -> &'static Defs {
    &current().defs
}

/// Read the definitions again (the folder copy may have changed).
pub fn reload() -> Info {
    let l: &'static Loaded = Box::leak(Box::new(load()));
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(l);
    info()
}

// ----------------------------------------------------------------- download

const REMOTE: &str = "https://raw.githubusercontent.com/TriptoAfsin/dev-cleaner/main/definitions/";
const MAX_FILE: usize = 4 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fetched {
    /// "updated", "up-to-date" or "needs-app" (the newest set needs a newer
    /// Dev Cleaner; the current set stays).
    pub outcome: String,
    pub version: String,
    pub min_app: String,
    pub info: Info,
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let r = agent.get(url).set("User-Agent", "dev-cleaner").call().map_err(|e| match e {
        ureq::Error::Status(c, _) => format!("GitHub answered with HTTP {c}."),
        ureq::Error::Transport(_) => "Couldn't reach GitHub. Check your connection.".to_string(),
    })?;
    let mut body = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(r.into_reader(), MAX_FILE as u64 + 1), &mut body).map_err(|_| "The download couldn't be read.".to_string())?;
    if body.len() > MAX_FILE {
        return Err("A definitions file is too large.".into());
    }
    Ok(body)
}

/// Fetch the newest definitions from GitHub. A set is kept only when it is
/// newer than the one in use, this app can read it, and it passes every
/// check; it is written to the definitions folder next to the old one and
/// swapped in, then loaded. Any failure leaves the current set in place.
pub fn download() -> Result<Fetched, String> {
    let base = match std::env::var("DEV_CLEANER_DEFS_URL") {
        Ok(u) if !u.is_empty() => if u.ends_with('/') { u } else { format!("{u}/") },
        _ if crate::util::fake_home().is_some() => return Err("Definitions downloads are off in tests".into()),
        _ => REMOTE.to_string(),
    };
    let dir = folder().ok_or("no data folder")?.clone();
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build();
    let index_text = fetch(&agent, &format!("{base}index.json"))?;
    let index: Index = parse("index.json", &index_text)?;
    let have = info();
    let up_to_date = set_version(&index.version) <= set_version(&have.version);
    let needs_app = index.schema != SCHEMA || (!index.min_app.is_empty() && crate::updates::is_newer(&index.min_app, env!("CARGO_PKG_VERSION")));
    let done = |outcome: &str| Fetched { outcome: outcome.into(), version: index.version.clone(), min_app: index.min_app.clone(), info: info() };
    if up_to_date {
        return Ok(done("up-to-date"));
    }
    if needs_app {
        return Ok(done("needs-app"));
    }
    // Only plain file names, never a path.
    let mut files: HashMap<String, String> = HashMap::new();
    files.insert("index.json".into(), index_text);
    for f in &index.files {
        if !f.ends_with(".json") || !f.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') || f.starts_with('.') {
            return Err(format!("Refused a file name in the download: {f}"));
        }
        files.insert(f.clone(), fetch(&agent, &format!("{base}{f}"))?);
    }
    parse_set(|name| files.get(name).cloned().ok_or(format!("{name} is missing from the download")))
        .map_err(|e| format!("The downloaded set didn't pass its checks ({e}). Keeping the current one."))?;

    // Write next to the folder, then swap, so a half-written set is never read.
    let staging = dir.with_extension("download");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    for (name, text) in &files {
        std::fs::write(staging.join(name), text).map_err(|e| e.to_string())?;
    }
    let old = dir.with_extension("old");
    let _ = std::fs::remove_dir_all(&old);
    if dir.exists() {
        std::fs::rename(&dir, &old).map_err(|e| format!("Couldn't replace the definitions folder: {e}"))?;
    }
    if let Err(e) = std::fs::rename(&staging, &dir) {
        let _ = std::fs::rename(&old, &dir);
        return Err(format!("Couldn't replace the definitions folder: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    reload();
    Ok(done("updated"))
}

// --------------------------------------------------------------------- info

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub version: String,
    pub builtin_version: String,
    pub schema: u32,
    pub source: Source,
    /// The folder a newer set is read from, and whether it exists.
    pub folder: Option<String>,
    pub folder_exists: bool,
    pub files: Vec<String>,
    pub loaded_at: u64,
    pub error: Option<String>,
    pub counts: Counts,
    /// Rules this app skipped, with the reason.
    pub skipped: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Rules that apply on this OS.
    pub caches: usize,
    pub junk: usize,
    pub projects: usize,
    pub processes: usize,
    pub ports: usize,
    pub models: usize,
    pub configs: usize,
    pub startup: usize,
    pub services: usize,
}

pub fn applies(r: &CacheRule) -> bool {
    r.os.is_empty() || r.os.iter().any(|o| o == std::env::consts::OS)
}

pub fn info() -> Info {
    let l = current();
    let d = &l.defs;
    let builtin_version = builtin_index().version;
    let here: Vec<&CacheRule> = d.caches.iter().filter(|r| applies(r)).collect();
    let p = &d.processes;
    Info {
        version: d.index.version.clone(),
        builtin_version,
        schema: d.index.schema,
        source: l.source,
        folder: folder().map(|f| f.to_string_lossy().into_owned()),
        folder_exists: folder().is_some_and(|f| f.is_dir()),
        files: d.index.files.clone(),
        loaded_at: l.loaded_at,
        error: l.error.clone(),
        counts: Counts {
            caches: here.iter().filter(|r| !r.junk).count(),
            junk: here.iter().filter(|r| r.junk).count(),
            projects: d.projects.rules.len(),
            processes: p.system.len() + p.save_first.len() + p.safe.len(),
            ports: d.ports.command_hints.len() + d.ports.process_hints.len() + d.ports.port_hints.len(),
            models: d.models.app_roots.len(),
            configs: d.configs.items.len(),
            startup: d.startup.keep.len() + d.startup.optional.len(),
            services: d.services.protected.len() + d.services.dev.len() + d.services.optional.len(),
        },
        skipped: d.skipped.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_definitions_load_and_check() {
        let d = builtin();
        assert!(d.caches.len() > 100);
        assert_eq!(d.index.schema, SCHEMA);
        for f in &d.index.files {
            assert!(BUILTIN.iter().any(|(n, _)| n == f), "{f} is listed but not built in");
        }        // Our own set must be fully readable: nothing skipped.
        assert!(d.skipped.is_empty(), "built-in rules skipped: {:?}", d.skipped);
    }

    #[test]
    fn a_rule_for_a_newer_app_is_skipped_and_the_rest_kept() {
        let text = r#"{"rules": [
            {"id": "ok", "name": "Ok", "eco": "X", "risk": "safe", "about": "", "paths": ["{home}/.ok"]},
            {"id": "future-field", "name": "F", "eco": "X", "risk": "safe", "about": "", "paths": [], "someNewField": 1},
            {"id": "future-expander", "name": "E", "eco": "X", "risk": "safe", "about": "", "paths": [{"expand": "teleport", "dir": "{home}"}]},
            {"id": "future-app", "name": "A", "eco": "X", "risk": "safe", "about": "", "paths": [], "minApp": "99.0.0"},
            {"id": "old-app", "name": "B", "eco": "X", "risk": "safe", "about": "", "paths": [], "minApp": "0.1.0"}
        ]}"#;
        let (ok, skipped) = parse_caches(text).unwrap();
        let ids: Vec<&str> = ok.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["ok", "old-app"]);
        assert_eq!(skipped.len(), 3);
        assert!(skipped.iter().any(|s| s.starts_with("future-app: needs Dev Cleaner 99.0.0")));
        assert!(parse_caches("{ nope").is_err(), "a file that isn't JSON still fails the set");
    }

    #[test]
    fn set_versions_compare_as_numbers() {
        assert!(set_version("2026.11.10.1") > set_version("2026.11.9.9"));
        assert!(set_version("2026.11.09.10") > set_version("2026.11.09.2"));
        assert!(set_version("2026.11.09.2") == set_version("2026.11.09.2"));
    }

    #[test]
    fn lookup_is_case_insensitive_and_takes_the_longest_prefix() {
        let t: HashMap<String, String> = [("MySQL*", "a"), ("mysql80", "b"), ("my*", "c")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(lookup(&t, "MYSQL80"), Some("b"));
        assert_eq!(lookup(&t, "MySQL57"), Some("a"));
        assert_eq!(lookup(&t, "mything"), Some("c"));
        assert_eq!(lookup(&t, "postgres"), None);
    }

    #[test]
    fn templates_expand_and_reject_unknown_variables() {
        assert!(expand_str("{home}/.npm").unwrap().ends_with("/.npm"));
        assert_eq!(expand_str("{env:DC_SURELY_UNSET_VAR|/x/{env:DC_ALSO_UNSET|y}}").unwrap(), "/x/y");
        assert!(expand_str("{nope}/x").is_err());
        assert!(expand_str("{home").is_err());
        assert!(expand("{env:DC_SURELY_UNSET_VAR|}").is_none());
    }

    #[test]
    fn a_broken_folder_copy_falls_back_with_a_reason() {
        let dir = std::env::temp_dir().join(format!("dc-defs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (n, t) in BUILTIN {
            std::fs::write(dir.join(n), t).unwrap();
        }
        assert!(matches!(from_folder(&dir), Some(Ok(_))));
        std::fs::write(dir.join("ports.json"), "{ not json").unwrap();
        assert!(from_folder(&dir).unwrap().is_err());
        std::fs::write(dir.join("index.json"), BUILTIN[0].1.replace("\"schema\": 1", "\"schema\": 99")).unwrap();
        assert!(from_folder(&dir).unwrap().err().unwrap().contains("schema 99"));
        std::fs::remove_dir_all(&dir).unwrap();
        let empty = std::env::temp_dir().join("dc-defs-none");
        assert!(from_folder(&empty).is_none());
    }
}
