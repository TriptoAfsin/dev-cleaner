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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rules<T> {
    rules: Vec<T>,
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
        caches: parse::<Rules<CacheRule>>("caches.json", &read("caches.json")?)?.rules,
        projects: parse("projects.json", &read("projects.json")?)?,
        processes: parse("processes.json", &read("processes.json")?)?,
        ports: parse("ports.json", &read("ports.json")?)?,
        models: parse("models.json", &read("models.json")?)?,
        configs: parse("configs.json", &read("configs.json")?)?,
        startup: parse("startup.json", &read("startup.json")?)?,
        services: parse("services.json", &read("services.json")?)?,
        index,
    };
    check(&defs)?;
    Ok(defs)
}

/// Structural checks: every template expands, every named action and
/// expander exists, ids are unique per OS. (Path and command limits come
/// with signing.)
pub fn check(d: &Defs) -> Result<(), String> {
    let tpl = |t: &str, id: &str| expand_str(t).map(|_| ()).map_err(|e| format!("{id}: {e}"));
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
    let mut seen: HashMap<(&str, &str), ()> = HashMap::new();
    for r in &d.caches {
        for os in &r.os {
            if !["windows", "macos", "linux"].contains(&os.as_str()) {
                return Err(format!("{}: unknown os {os}", r.id));
            }
        }
        for os in ["windows", "macos", "linux"] {
            if (r.os.is_empty() || r.os.iter().any(|o| o == os)) && seen.insert((r.id.as_str(), os), ()).is_some() {
                return Err(format!("{}: listed twice for {os}", r.id));
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
    let builtin_version = parse::<Index>("index.json", BUILTIN[0].1).map(|i| i.version).unwrap_or_default();
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
        }
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
