//! Space analyzer: where a drive's (or a folder's) space went.
//!
//! One parallel walk builds a tree of folders in memory: every folder with
//! its total size, file count and category, plus its biggest files; small
//! files are only summed. The page then asks for slices of that tree
//! (`space_tree`) to draw the treemap, so nothing walks the disk twice.
//!
//! Read-only: nothing here deletes. Items with a known fix name the page
//! that cleans them (or the Windows settings page that does).

use rayon::prelude::*;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

// ------------------------------------------------------------- categories

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Cat {
    /// Rebuildable: caches, temp files, build output, dependencies.
    Cache,
    /// Something with a known, specific fix (pagefile, old drivers, …).
    Fix,
    /// The user's own files.
    User,
    /// Installed software and games.
    Apps,
    /// The OS and whatever the scan could not read.
    System,
}

impl Cat {
    fn idx(self) -> usize {
        self as usize
    }
}

/// Where a fix is done: a page of this app (with a tab), or a Windows
/// settings page.
#[derive(Debug)]
pub enum Action {
    None,
    Page { page: &'static str, tab: Option<&'static str>, label: &'static str },
    Uri { uri: &'static str, label: &'static str },
}

#[derive(Debug)]
pub struct Rule {
    pub about: &'static str,
    pub fix: Option<&'static str>,
    pub action: Action,
}

macro_rules! rule {
    ($name:ident, $about:expr) => {
        static $name: Rule = Rule { about: $about, fix: None, action: Action::None };
    };
    ($name:ident, $about:expr, $fix:expr, $action:expr) => {
        static $name: Rule = Rule { about: $about, fix: Some($fix), action: $action };
    };
}

const DEEP: Option<&str> = Some("deep");
const QUICK: Option<&str> = Some("quick");

rule!(PAGEFILE, "Virtual memory. Windows moves memory pages here when RAM runs short.",
    "If it is far bigger than you ever use, a system-managed or smaller size frees space after a restart. The pagefile advisor shows your peak use.",
    Action::Page { page: "junk", tab: DEEP, label: "Open pagefile advisor" });
rule!(HIBERFIL, "Hibernation file, sized to a share of your RAM so the PC can hibernate.",
    "If you never hibernate, running powercfg /h off in an admin terminal removes it. Fast Startup turns off with it.",
    Action::None);
rule!(SWAPFILE, "Swap space for Store apps. Windows manages it; leave it alone.");
rule!(RECYCLE, "Files you deleted. They keep using space until the Recycle Bin is emptied.",
    "Empty it from System junk.",
    Action::Page { page: "junk", tab: QUICK, label: "Open System junk" });
rule!(TRASH, "Files you deleted. They keep using space until the Trash is emptied.",
    "Empty it from System junk.",
    Action::Page { page: "junk", tab: QUICK, label: "Open System junk" });
rule!(WINDOWS_OLD, "Your previous Windows installation, kept so you can roll back.",
    "Once you don't need to roll back, remove it in Settings › System › Storage › Temporary files.",
    Action::Uri { uri: "ms-settings:storagesense", label: "Open Storage settings" });
rule!(INSTALLER, "Cached .msi and .msp packages Windows needs to repair or uninstall programs.",
    "Packages that no installed program uses can be moved to a quarantine folder from Deep clean.",
    Action::Page { page: "junk", tab: DEEP, label: "Review in Deep clean" });
rule!(DRIVERS, "Every driver version ever installed on this PC.",
    "Older versions of drivers you still have can be removed; the newest of each is always kept.",
    Action::Page { page: "junk", tab: DEEP, label: "Review in Deep clean" });
rule!(WU_DOWNLOAD, "Windows Update downloads that are already installed.",
    "Safe to clear from System junk.",
    Action::Page { page: "junk", tab: QUICK, label: "Open System junk" });
rule!(CROSSDEVICE, "Photos and videos Phone Link copied from your phone. Your files, not a cache.",
    "Stop photo sync in Settings › Bluetooth & devices › Mobile devices, or move this folder to another drive.",
    Action::Uri { uri: "ms-settings:mobile-devices", label: "Open Mobile devices settings" });
rule!(WINSXS, "Windows component store. Its files are hard-linked into System32, so the size shown counts many of them twice. Only DISM may clean it; System junk runs it for you.");
rule!(SYSTEM_VOLUME, "Restore points and shadow copies. Manage them in System Protection.");
rule!(APPDATA, "Per-app settings and caches. Mixed: some is rebuildable, some is settings you want to keep.");
rule!(LIBRARY, "Per-app settings, caches and data. Mixed: some is rebuildable, some you want to keep.");
rule!(BUILD, "Build output or installed dependencies of a project. The next install or build makes it again.",
    "The Projects page finds these across your code folders and removes the ones you pick.",
    Action::Page { page: "projects", tab: None, label: "Open Projects" });
rule!(MODELS, "Downloaded AI models. They can be downloaded again when needed.",
    "Review and remove models on AI / ML.",
    Action::Page { page: "models", tab: None, label: "Open AI / ML" });
rule!(CACHE, "Rebuildable. The tool that made it downloads or regenerates it when needed.",
    "Disk cleanup measures known caches and cleans them with each tool's own command.",
    Action::Page { page: "disk", tab: None, label: "Open Disk cleanup" });
rule!(TEMP, "Temporary files apps left behind.",
    "Old temp files are cleared from System junk.",
    Action::Page { page: "junk", tab: QUICK, label: "Open System junk" });
rule!(UNREAD, "Space the scan could not read: restore points and shadow copies, file system metadata, and folders you don't have rights to. Running Dev Cleaner as admin reads more of it.");

/// Every settings URI a rule can open. `space_open` accepts only these.
const URIS: &[&str] = &["ms-settings:storagesense", "ms-settings:mobile-devices"];

/// Folder names whose contents are rebuildable caches, anywhere.
const CACHE_DIRS: &[&str] = &[
    ".cache", "cache", "caches", "code cache", "gpucache", "shadercache", "grshadercache", "graphitedawncache", "dxcache",
    "d3dscache", "glcache", "nv_cache", "inetcache", "crashdumps", "crashpad", "npm-cache", "_cacache", ".npm", ".pnpm-store",
    "pnpm-cache", ".yarn-cache", "yarn-cache", ".gradle", ".m2", "go-build", ".nuget", "nuget", "pip", "uv", ".bun",
    "service worker", "webcache", "squirreltemp", "package cache-temp",
];
/// Build output and dependency folders inside projects.
const BUILD_DIRS: &[&str] = &[
    "node_modules", ".next", ".nuxt", ".svelte-kit", ".turbo", ".parcel-cache", ".angular", "__pycache__", ".pytest_cache",
    ".mypy_cache", ".ruff_cache", ".dart_tool",
];
const MODEL_DIRS: &[&str] = &[".lmstudio", ".ollama", "huggingface", ".unsloth"];
const USER_DIRS: &[&str] = &[
    "documents", "pictures", "videos", "downloads", "desktop", "music", "movies", "source", "repos", "projects", "code", "dev",
    "saved games", "contacts", "favorites",
];
const APP_DIRS: &[&str] = &["program files", "program files (x86)", "programdata", "xboxgames", "windowsapps", "steamlibrary", "applications"];

/// The category of a folder or file from its path (lowercase components
/// from the drive root, the item's own name last). Unknown items inherit
/// their parent's category; caches and fixes pass their rule down too.
pub fn classify(rel: &[String], parent: Cat, parent_rule: Option<&'static Rule>, is_dir: bool, cargo_sibling: bool) -> (Cat, Option<&'static Rule>) {
    let n = rel.len();
    let Some(last) = rel.last().map(String::as_str) else { return (parent, parent_rule) };
    let at = |i: usize| rel.get(i).map(String::as_str).unwrap_or("");
    let win = cfg!(windows);

    // Drive root.
    if win && n == 1 {
        if !is_dir {
            return match last {
                "pagefile.sys" => (Cat::Fix, Some(&PAGEFILE)),
                "hiberfil.sys" => (Cat::Fix, Some(&HIBERFIL)),
                "swapfile.sys" => (Cat::System, Some(&SWAPFILE)),
                _ => (Cat::System, None),
            };
        }
        return match last {
            "windows" | "recovery" | "$winreagent" | "$windows.~bt" | "$windows.~ws" | "$sysreset" | "boot" | "efi" | "perflogs" | "inetpub" | "intel" | "amd" | "nvidia" | "esd" | "drivers" | "msocache" | "onedrivetemp" => (Cat::System, None),
            "system volume information" => (Cat::System, Some(&SYSTEM_VOLUME)),
            "$recycle.bin" => (Cat::Fix, Some(&RECYCLE)),
            "windows.old" => (Cat::Fix, Some(&WINDOWS_OLD)),
            "users" => (Cat::User, None),
            x if APP_DIRS.contains(&x) => (Cat::Apps, None),
            x if x.starts_with('$') => (Cat::System, None),
            _ => (Cat::User, None),
        };
    }

    if win && at(0) == "windows" && is_dir {
        match (n, last) {
            (2, "winsxs") => return (Cat::System, Some(&WINSXS)),
            (2, "installer") => return (Cat::Fix, Some(&INSTALLER)),
            (2, "temp") => return (Cat::Cache, Some(&TEMP)),
            (2, "logs") => return (Cat::Cache, Some(&TEMP)),
            (3, "driverstore") if at(1) == "system32" => return (Cat::Fix, Some(&DRIVERS)),
            (3, "download") if at(1) == "softwaredistribution" => return (Cat::Fix, Some(&WU_DOWNLOAD)),
            _ => {}
        }
    }

    // Inside a profile: C:\Users\<name>\… on Windows, /Users/<name> on macOS,
    // /home/<name> on Linux.
    let profile = (at(0) == "users" || at(0) == "home") && n >= 3;
    if profile && is_dir {
        let sub = &rel[2..];
        let s = |i: usize| sub.get(i).map(String::as_str).unwrap_or("");
        match sub.len() {
            1 => match s(0) {
                "crossdevice" => return (Cat::Fix, Some(&CROSSDEVICE)),
                "appdata" => return (Cat::User, Some(&APPDATA)),
                "library" => return (Cat::User, Some(&LIBRARY)),
                ".trash" => return (Cat::Fix, Some(&TRASH)),
                x if x.starts_with("onedrive") || USER_DIRS.contains(&x) => return (Cat::User, None),
                _ => {}
            },
            3 if s(0) == "appdata" && s(1) == "local" => match s(2) {
                "programs" | "packages" => return (Cat::Apps, None),
                "temp" => return (Cat::Cache, Some(&TEMP)),
                _ => {}
            },
            2 if s(0) == "library" && s(1) == "caches" => return (Cat::Cache, Some(&CACHE)),
            4 if s(0) == ".local" && s(1) == "share" && s(2) == "trash" && s(3) == "files" => return (Cat::Fix, Some(&TRASH)),
            _ => {}
        }
    }

    if is_dir && parent != Cat::Cache && parent != Cat::Fix {
        if MODEL_DIRS.contains(&last) {
            return (Cat::Cache, Some(&MODELS));
        }
        if BUILD_DIRS.contains(&last) || (last == "target" && cargo_sibling) {
            return (Cat::Cache, Some(&BUILD));
        }
        if last == "temp" || last == "tmp" {
            return (Cat::Cache, Some(&TEMP));
        }
        if CACHE_DIRS.contains(&last) {
            return (Cat::Cache, Some(&CACHE));
        }
        if last == "steamapps" || last == "epic games" || last == "steam" {
            return (Cat::Apps, None);
        }
    }

    if matches!(parent, Cat::Cache | Cat::Fix) {
        return (parent, parent_rule);
    }
    (parent, None)
}

// ------------------------------------------------------------------- tree

#[derive(Debug)]
struct FileLeaf {
    name: String,
    size: u64,
    cat: Cat,
    rule: Option<&'static Rule>,
}

#[derive(Debug)]
struct DirNode {
    name: String,
    size: u64,
    files: u64,
    cat: Cat,
    rule: Option<&'static Rule>,
    /// Bytes by category, this folder and everything inside.
    mix: [u64; 5],
    children: Vec<DirNode>,
    /// The biggest files directly in this folder.
    big: Vec<FileLeaf>,
    /// The other files directly in this folder: bytes and count.
    rest: (u64, u64),
    /// Not a real folder (the "could not read" share of a drive).
    synthetic: bool,
}

impl DirNode {
    fn new(name: String, cat: Cat, rule: Option<&'static Rule>) -> Self {
        Self { name, size: 0, files: 0, cat, rule, mix: [0; 5], children: vec![], big: vec![], rest: (0, 0), synthetic: false }
    }
}

/// Files kept by name in each folder: the biggest few over 1 MB, and
/// every file over 256 MB.
const KEEP_TOP: usize = 12;
const KEEP_MIN: u64 = 1 << 20;
const KEEP_ALWAYS: u64 = 256 << 20;

struct Walk<'a> {
    files: AtomicU64,
    bytes: AtomicU64,
    dirs: AtomicU64,
    unreadable: AtomicU64,
    current: Mutex<String>,
    cancel: &'a AtomicBool,
}

/// Bytes a file takes on disk. Cloud placeholders (OneDrive "online only")
/// take none; sparse files on Unix count their allocated blocks.
fn disk_len(m: &std::fs::Metadata) -> u64 {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_OPEN: u32 = 0x40000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        if m.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0 {
            return 0;
        }
        m.len()
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.blocks() * 512
    }
    #[cfg(not(any(windows, unix)))]
    {
        m.len()
    }
}

/// Pseudo file systems never worth walking.
fn skip_dir(rel: &[String], name: &str) -> bool {
    !cfg!(windows) && rel.is_empty() && matches!(name, "proc" | "sys" | "dev" | "run")
}

fn walk(path: &Path, name: String, rel: Vec<String>, cat: Cat, rule: Option<&'static Rule>, w: &Walk) -> DirNode {
    let mut node = DirNode::new(name, cat, rule);
    if w.cancel.load(Ordering::Relaxed) {
        return node;
    }
    w.dirs.fetch_add(1, Ordering::Relaxed);
    if rel.len() <= 5 {
        if let Ok(mut c) = w.current.lock() {
            *c = path.to_string_lossy().into_owned();
        }
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        w.unreadable.fetch_add(1, Ordering::Relaxed);
        return node;
    };
    let mut subdirs: Vec<(PathBuf, String)> = vec![];
    let mut files: Vec<(String, u64)> = vec![];
    let mut cargo = false;
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        // Symlinks and junctions point at space counted elsewhere.
        if ft.is_symlink() {
            continue;
        }
        let fname = e.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            subdirs.push((e.path(), fname));
        } else {
            // DirEntry metadata comes from the directory listing, so locked
            // files (pagefile.sys) still report their size.
            let size = e.metadata().map(|m| disk_len(&m)).unwrap_or(0);
            if fname.eq_ignore_ascii_case("cargo.toml") {
                cargo = true;
            }
            files.push((fname, size));
        }
    }
    let direct: u64 = files.iter().map(|f| f.1).sum();
    w.files.fetch_add(files.len() as u64, Ordering::Relaxed);
    w.bytes.fetch_add(direct, Ordering::Relaxed);
    node.files = files.len() as u64;
    files.sort_unstable_by_key(|f| std::cmp::Reverse(f.1));
    for (i, (fname, size)) in files.into_iter().enumerate() {
        if (i < KEEP_TOP && size >= KEEP_MIN) || size >= KEEP_ALWAYS {
            let mut r = rel.clone();
            r.push(fname.to_lowercase());
            let (c, ru) = classify(&r, cat, rule, false, cargo);
            node.mix[c.idx()] += size;
            node.big.push(FileLeaf { name: fname, size, cat: c, rule: ru });
        } else {
            node.rest.0 += size;
            node.rest.1 += 1;
        }
    }
    node.mix[cat.idx()] += node.rest.0;

    let kids: Vec<DirNode> = subdirs
        .into_par_iter()
        .filter_map(|(p, n)| {
            let lower = n.to_lowercase();
            if skip_dir(&rel, &lower) {
                return None;
            }
            let mut r = rel.clone();
            r.push(lower);
            let (c, ru) = classify(&r, cat, rule, true, cargo);
            Some(walk(&p, n, r, c, ru, w))
        })
        .collect();
    node.size = direct;
    for k in &kids {
        node.size += k.size;
        node.files += k.files;
        for (m, x) in node.mix.iter_mut().zip(k.mix) {
            *m += x;
        }
    }
    node.children = kids;
    node.children.sort_unstable_by_key(|c| std::cmp::Reverse(c.size));
    node
}

/// Lowercase path components below the root or prefix: `C:\Users\Dev` →
/// `["users", "dev"]`, `/home/dev` → `["home", "dev"]`.
fn rel_of(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect()
}

/// Category of the folder a scan starts in, from its ancestors.
fn root_class(rel: &[String]) -> (Cat, Option<&'static Rule>) {
    let mut cat = if cfg!(windows) { Cat::System } else { Cat::User };
    let mut rule = None;
    for i in 1..=rel.len() {
        (cat, rule) = classify(&rel[..i], cat, rule, true, false);
    }
    (cat, rule)
}

// ------------------------------------------------------------- scan state

struct Scan {
    root: PathBuf,
    node: DirNode,
    summary: ScanSummary,
}

static SCAN: Mutex<Option<Scan>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FixOut {
    pub text: &'static str,
    pub label: Option<&'static str>,
    pub page: Option<&'static str>,
    pub tab: Option<&'static str>,
    pub uri: Option<&'static str>,
}

fn fix_of(rule: Option<&'static Rule>) -> Option<FixOut> {
    let r = rule?;
    let text = r.fix?;
    Some(match r.action {
        Action::None => FixOut { text, label: None, page: None, tab: None, uri: None },
        Action::Page { page, tab, label } => FixOut { text, label: Some(label), page: Some(page), tab, uri: None },
        Action::Uri { uri, label } => FixOut { text, label: Some(label), page: None, tab: None, uri: Some(uri) },
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveUse {
    pub mount: String,
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub cat: Cat,
    pub about: Option<&'static str>,
    pub fix: Option<FixOut>,
    pub is_file: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub root: String,
    pub total: u64,
    pub files: u64,
    pub dirs: u64,
    pub unreadable: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    /// Bytes by category: cache, fix, user, apps, system.
    pub cats: [u64; 5],
    /// Items with a specific fix, biggest first.
    pub findings: Vec<Finding>,
    /// Folders of rebuildable cache found, in total.
    pub cache_total: u64,
    pub drive: Option<DriveUse>,
    pub at: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub files: u64,
    pub bytes: u64,
    pub dirs: u64,
    pub current: String,
    pub elapsed_ms: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NodeOut {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub files: u64,
    /// "dir", "file", "files" (the small files of a folder, summed), "more"
    /// (folded smaller items), or "other" (not a real folder).
    pub kind: &'static str,
    pub cat: Cat,
    pub mix: [u64; 5],
    pub about: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixOut>,
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<NodeOut>>,
}

fn mix_one(cat: Cat, size: u64) -> [u64; 5] {
    let mut m = [0; 5];
    m[cat.idx()] = size;
    m
}

/// A folder and its contents to `depth` levels, biggest first, at most
/// `limit` items per level (the rest folded into one "N more" item).
fn view(node: &DirNode, path: &Path, depth: u32, limit: usize) -> NodeOut {
    let has_children = !node.synthetic && (!node.children.is_empty() || !node.big.is_empty() || node.rest.1 > 0);
    let children = (depth > 0 && has_children).then(|| {
        let mut items: Vec<NodeOut> = vec![];
        for c in &node.children {
            // A folder that is nearly all one subfolder (C:\Users → your
            // profile) shows that subfolder's contents instead, named
            // "Users › you", so the map isn't one flat block.
            let (mut target, mut p, mut name) = (c, path.join(&c.name), c.name.clone());
            while let Some(only) = dominant(target) {
                name = format!("{name} › {}", only.name);
                p = p.join(&only.name);
                target = only;
            }
            let mut v = view(target, &p, depth - 1, (limit / 2).max(6));
            v.name = name;
            items.push(v);
        }
        for f in &node.big {
            items.push(NodeOut {
                path: path.join(&f.name).to_string_lossy().into_owned(),
                name: f.name.clone(),
                size: f.size,
                files: 1,
                kind: "file",
                cat: f.cat,
                mix: mix_one(f.cat, f.size),
                about: f.rule.map(|r| r.about),
                fix: fix_of(f.rule),
                has_children: false,
                children: None,
            });
        }
        if node.rest.1 > 0 && node.rest.0 > 0 {
            let n = node.rest.1;
            items.push(NodeOut {
                path: format!("{}\u{0}files", path.to_string_lossy()),
                name: format!("{n} {}", if n == 1 { "file" } else { "smaller files" }),
                size: node.rest.0,
                files: n,
                kind: "files",
                cat: node.cat,
                mix: mix_one(node.cat, node.rest.0),
                about: node.rule.map(|r| r.about),
                fix: fix_of(node.rule),
                has_children: false,
                children: None,
            });
        }
        items.sort_by_key(|i| std::cmp::Reverse(i.size));
        fold(items, limit, path)
    });
    NodeOut {
        path: path.to_string_lossy().into_owned(),
        name: node.name.clone(),
        size: node.size,
        files: node.files,
        kind: if node.synthetic { "other" } else { "dir" },
        cat: node.cat,
        mix: node.mix,
        about: node.rule.map(|r| r.about),
        fix: fix_of(node.rule),
        has_children,
        children,
    }
}

/// The one subfolder holding at least 90% of a folder, if there is one.
fn dominant(n: &DirNode) -> Option<&DirNode> {
    let c = n.children.first()?;
    (n.size > 0 && !c.synthetic && c.size as f64 >= n.size as f64 * 0.9 && n.children.get(1).is_none_or(|d| d.size < c.size / 4)).then_some(c)
}

/// Keep the first `limit` items; fold the rest into one "N more" item.
fn fold(mut items: Vec<NodeOut>, limit: usize, parent: &Path) -> Vec<NodeOut> {
    if items.len() <= limit + 1 {
        return items;
    }
    let tail = items.split_off(limit);
    let mut mix = [0u64; 5];
    for t in &tail {
        for (m, x) in mix.iter_mut().zip(t.mix) {
            *m += x;
        }
    }
    let cat = [Cat::Cache, Cat::Fix, Cat::User, Cat::Apps, Cat::System][(0..5).max_by_key(|i| mix[*i]).unwrap_or(4)];
    items.push(NodeOut {
        path: format!("{}\u{0}more", parent.to_string_lossy()),
        name: format!("{} more", tail.len()),
        size: tail.iter().map(|t| t.size).sum(),
        files: tail.iter().map(|t| t.files).sum(),
        kind: "more",
        cat,
        mix,
        about: None,
        fix: None,
        has_children: false,
        children: None,
    });
    items
}

/// The folder at `target` inside the tree rooted at `root`.
fn find<'a>(node: &'a DirNode, root: &Path, target: &Path) -> Option<&'a DirNode> {
    let rest = target.strip_prefix(root).ok()?;
    let mut n = node;
    for c in rest.components() {
        let Component::Normal(s) = c else { continue };
        let s = s.to_string_lossy();
        n = n.children.iter().find(|k| if cfg!(windows) { k.name.eq_ignore_ascii_case(&s) } else { k.name == s })?;
    }
    Some(n)
}

/// Items with a specific fix, biggest first (nested ones under a fix are
/// part of it), and the total of rebuildable cache folders.
fn findings(node: &DirNode, path: &Path, out: &mut Vec<Finding>, cache: &mut u64) {
    for c in &node.children {
        let p = path.join(&c.name);
        if c.cat == Cat::Fix && node.cat != Cat::Fix {
            if c.size >= 50 << 20 && c.rule.is_some_and(|r| r.fix.is_some()) {
                out.push(Finding {
                    path: p.to_string_lossy().into_owned(),
                    name: c.name.clone(),
                    size: c.size,
                    cat: c.cat,
                    about: c.rule.map(|r| r.about),
                    fix: fix_of(c.rule),
                    is_file: false,
                });
            }
        } else if c.cat == Cat::Cache && node.cat != Cat::Cache {
            *cache += c.size;
        } else {
            findings(c, &p, out, cache);
        }
    }
    for f in &node.big {
        if f.cat == Cat::Fix && node.cat != Cat::Fix && f.rule.is_some_and(|r| r.fix.is_some()) && f.size >= 50 << 20 {
            out.push(Finding {
                path: path.join(&f.name).to_string_lossy().into_owned(),
                name: f.name.clone(),
                size: f.size,
                cat: f.cat,
                about: f.rule.map(|r| r.about),
                fix: fix_of(f.rule),
                is_file: true,
            });
        }
    }
}

fn drive_of(root: &Path) -> Option<DriveUse> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let key = |s: &str| s.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let r = key(&root.to_string_lossy());
    let d = disks
        .list()
        .iter()
        .filter(|d| {
            let m = key(&d.mount_point().to_string_lossy());
            r == m || r.starts_with(&format!("{m}\\")) || r.starts_with(&format!("{m}/")) || m.is_empty()
        })
        .max_by_key(|d| d.mount_point().as_os_str().len())?;
    let total = d.total_space();
    let free = d.available_space();
    Some(DriveUse { mount: d.mount_point().to_string_lossy().trim_end_matches('\\').to_string(), total, used: total.saturating_sub(free), free })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Walk `root` and keep the tree. The progress callback runs every 150 ms.
fn scan_core(root: &Path, mut on_progress: impl FnMut(Progress)) -> Result<ScanSummary, String> {
    let meta = std::fs::metadata(root).map_err(|e| format!("Can't open {}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    CANCEL.store(false, Ordering::Relaxed);
    let started = Instant::now();
    let rel = rel_of(root);
    let (cat, rule) = root_class(&rel);
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.to_string_lossy().trim_end_matches(['\\', '/']).to_string());
    let w = Walk {
        files: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        dirs: AtomicU64::new(0),
        unreadable: AtomicU64::new(0),
        current: Mutex::new(String::new()),
        cancel: &CANCEL,
    };
    let done = AtomicBool::new(false);
    let mut node = std::thread::scope(|s| {
        let h = s.spawn(|| walk(root, name, rel.clone(), cat, rule, &w));
        while !done.load(Ordering::Relaxed) {
            if h.is_finished() {
                done.store(true, Ordering::Relaxed);
                break;
            }
            on_progress(Progress {
                files: w.files.load(Ordering::Relaxed),
                bytes: w.bytes.load(Ordering::Relaxed),
                dirs: w.dirs.load(Ordering::Relaxed),
                current: w.current.lock().map(|c| c.clone()).unwrap_or_default(),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            std::thread::sleep(Duration::from_millis(150));
        }
        h.join().unwrap_or_else(|_| DirNode::new(String::new(), cat, rule))
    });
    let cancelled = CANCEL.load(Ordering::Relaxed);
    let drive = if crate::util::fake_home().is_some() { None } else { drive_of(root) };

    // A whole drive: the space the walk could not see, as its own item.
    if let Some(d) = &drive {
        let is_root = rel.is_empty() || root.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(&d.mount);
        let gap = d.used.saturating_sub(node.size);
        if is_root && !cancelled && gap > (1u64 << 30).max(d.total / 100) {
            let mut x = DirNode::new("System and unreadable".into(), Cat::System, Some(&UNREAD));
            x.size = gap;
            x.synthetic = true;
            x.mix[Cat::System.idx()] = gap;
            node.size += gap;
            node.mix[Cat::System.idx()] += gap;
            node.children.push(x);
            node.children.sort_unstable_by_key(|c| std::cmp::Reverse(c.size));
        }
    }

    let mut found = vec![];
    let mut cache_total = 0;
    if node.cat == Cat::Cache {
        cache_total = node.size;
    } else {
        findings(&node, root, &mut found, &mut cache_total);
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.size));
    let summary = ScanSummary {
        root: root.to_string_lossy().into_owned(),
        total: node.size,
        files: node.files,
        dirs: w.dirs.load(Ordering::Relaxed),
        unreadable: w.unreadable.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
        cancelled,
        cats: node.mix,
        findings: found,
        cache_total,
        drive,
        at: now_secs(),
    };
    if let Ok(mut s) = SCAN.lock() {
        *s = Some(Scan { root: root.to_path_buf(), node, summary: summary.clone() });
    }
    Ok(summary)
}

/// What to scan for a requested root. Under the e2e fake home only the fake
/// home is ever walked; on macOS and Linux "/" means the home folder.
fn resolve_root(root: &str) -> PathBuf {
    if let Some(f) = crate::util::fake_home() {
        return f;
    }
    let p = PathBuf::from(root);
    if !cfg!(windows) && p == Path::new("/") {
        return crate::util::home();
    }
    if cfg!(windows) && root.len() == 2 && root.ends_with(':') {
        return PathBuf::from(format!("{root}\\"));
    }
    p
}

// --------------------------------------------------------------- commands

type R<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// Walk a drive or folder and keep its tree. Emits `space-progress`.
#[tauri::command]
pub async fn space_scan(app: AppHandle, root: String) -> R<ScanSummary> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("A scan is already running.".into());
    }
    let res = blocking(move || {
        let root = resolve_root(&root);
        scan_core(&root, |p| {
            let _ = app.emit("space-progress", p);
        })
    })
    .await;
    RUNNING.store(false, Ordering::SeqCst);
    res?
}

/// A folder of the last scan with its contents to `depth` levels.
#[tauri::command]
pub async fn space_tree(path: Option<String>, depth: Option<u32>) -> R<NodeOut> {
    blocking(move || {
        let s = SCAN.lock().map_err(|e| e.to_string())?;
        let scan = s.as_ref().ok_or("Nothing scanned yet.")?;
        let target = path.map(PathBuf::from).unwrap_or_else(|| scan.root.clone());
        let node = find(&scan.node, &scan.root, &target).ok_or("That folder is not in the last scan.")?;
        Ok(view(node, &target, depth.unwrap_or(2).min(3), 28))
    })
    .await?
}

/// The last scan's summary, if any (the page shows it again when reopened).
#[tauri::command]
pub async fn space_last() -> R<Option<ScanSummary>> {
    blocking(|| SCAN.lock().ok().and_then(|s| s.as_ref().map(|x| x.summary.clone()))).await
}

#[tauri::command]
pub fn space_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Open a Windows settings page a fix points to. Only the URIs the rules
/// above name are accepted.
#[tauri::command]
pub fn space_open(app: AppHandle, uri: String) -> R<()> {
    use tauri_plugin_opener::OpenerExt;
    if !URIS.contains(&uri.as_str()) {
        return Err("Not a settings page Dev Cleaner opens.".into());
    }
    app.opener().open_url(uri, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(p: &str) -> Vec<String> {
        p.split('/').filter(|s| !s.is_empty()).map(|s| s.to_lowercase()).collect()
    }
    fn class(p: &str, parent: Cat, dir: bool) -> Cat {
        classify(&rel(p), parent, None, dir, false).0
    }

    #[test]
    fn builds_and_caches_are_rebuildable_anywhere() {
        assert_eq!(class("users/dev/code/app/node_modules", Cat::User, true), Cat::Cache);
        assert_eq!(class("users/dev/.cache", Cat::User, true), Cat::Cache);
        assert_eq!(classify(&rel("x/proj/target"), Cat::User, None, true, true).0, Cat::Cache);
        assert_eq!(classify(&rel("x/proj/target"), Cat::User, None, true, false).0, Cat::User, "target without Cargo.toml is not build output");
        let (c, r) = classify(&rel("users/dev/.lmstudio"), Cat::User, None, true, false);
        assert_eq!(c, Cat::Cache);
        assert!(std::ptr::eq(r.unwrap(), &MODELS));
    }

    #[test]
    fn caches_and_fixes_pass_down_their_rule() {
        let (c, r) = classify(&rel("a/node_modules/react"), Cat::Cache, Some(&BUILD), true, false);
        assert_eq!(c, Cat::Cache);
        assert!(std::ptr::eq(r.unwrap(), &BUILD));
        // A "cache" folder inside build output stays part of the build output.
        let (_, r) = classify(&rel("a/node_modules/.cache"), Cat::Cache, Some(&BUILD), true, false);
        assert!(std::ptr::eq(r.unwrap(), &BUILD));
        assert_eq!(class("a/docs/readme", Cat::User, true), Cat::User);
    }

    #[test]
    fn profile_folders() {
        assert_eq!(class("users/dev/documents", Cat::User, true), Cat::User);
        assert_eq!(class("users/dev/appdata/local/programs", Cat::User, true), Cat::Apps);
        assert_eq!(class("users/dev/appdata/local/temp", Cat::User, true), Cat::Cache);
        assert_eq!(class("users/dev/library/caches", Cat::User, true), Cat::Cache);
        assert_eq!(class("users/dev/crossdevice", Cat::User, true), Cat::Fix);
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_root() {
        assert_eq!(class("pagefile.sys", Cat::System, false), Cat::Fix);
        assert_eq!(class("hiberfil.sys", Cat::System, false), Cat::Fix);
        assert_eq!(class("swapfile.sys", Cat::System, false), Cat::System);
        assert_eq!(class("windows", Cat::System, true), Cat::System);
        assert_eq!(class("program files (x86)", Cat::System, true), Cat::Apps);
        assert_eq!(class("$recycle.bin", Cat::System, true), Cat::Fix);
        assert_eq!(class("windows/installer", Cat::System, true), Cat::Fix);
        assert_eq!(class("windows/system32/driverstore", Cat::System, true), Cat::Fix);
        assert_eq!(class("windows/softwaredistribution/download", Cat::System, true), Cat::Fix);
        assert_eq!(class("windows/winsxs", Cat::System, true), Cat::System);
        assert_eq!(class("windows/system32/drivers", Cat::System, true), Cat::System);
        assert_eq!(root_class(&rel("users/dev")).0, Cat::User);
        assert_eq!(root_class(&rel("program files/foo")).0, Cat::Apps);
    }

    fn leaf(name: &str, size: u64) -> NodeOut {
        NodeOut { path: name.into(), name: name.into(), size, files: 1, kind: "file", cat: Cat::User, mix: mix_one(Cat::User, size), about: None, fix: None, has_children: false, children: None }
    }

    #[test]
    fn fold_keeps_the_biggest_and_sums_the_rest() {
        let items: Vec<NodeOut> = (0..10).map(|i| leaf(&format!("f{i}"), 100 - i)).collect();
        let out = fold(items, 4, Path::new("root"));
        assert_eq!(out.len(), 5);
        assert_eq!(out[4].kind, "more");
        assert_eq!(out[4].name, "6 more");
        assert_eq!(out[4].size, (4..10).map(|i| 100 - i).sum::<u64>());
        // One extra item is shown, not folded into "1 more".
        let items: Vec<NodeOut> = (0..5).map(|i| leaf(&format!("f{i}"), 10)).collect();
        assert_eq!(fold(items, 4, Path::new("r")).len(), 5);
    }

    #[test]
    fn scans_a_folder_with_correct_totals() {
        let root = std::env::temp_dir().join(format!("dc-space-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app/node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("app/node_modules/pkg/index.js"), vec![0u8; 3000]).unwrap();
        std::fs::write(root.join("docs/big.bin"), vec![0u8; 2 << 20]).unwrap();
        for i in 0..5 {
            std::fs::write(root.join(format!("docs/s{i}.txt")), vec![0u8; 100]).unwrap();
        }
        let s = scan_core(&root, |_| {}).unwrap();
        assert_eq!(s.files, 7);
        // Unix counts allocated blocks, so allow for rounding up.
        let exact = 3000 + (2 << 20) + 500;
        assert!(s.total >= exact && s.total < exact + 64 * 1024, "total {}", s.total);
        assert!(s.cache_total >= 3000);
        let tree = SCAN.lock().unwrap();
        let t = tree.as_ref().unwrap();
        let docs = find(&t.node, &t.root, &root.join("docs")).unwrap();
        assert_eq!(docs.big.len(), 1, "the 2 MB file is kept by name");
        assert_eq!(docs.rest.1, 5, "small files are only counted");
        let v = view(&t.node, &t.root, 2, 28);
        let kids = v.children.unwrap();
        assert_eq!(kids[0].name, "docs");
        assert!(kids[0].children.as_ref().unwrap().iter().any(|k| k.kind == "files" && k.files == 5));
        drop(tree);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `DC_SPACE_ROOT=C: cargo test space::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real() {
        let root = resolve_root(&std::env::var("DC_SPACE_ROOT").unwrap_or_else(|_| "C:".into()));
        let s = scan_core(&root, |_| {}).unwrap();
        println!("{} files, {} dirs, {} unreadable, {:.1} GB in {} ms", s.files, s.dirs, s.unreadable, s.total as f64 / 1e9, s.elapsed_ms);
        println!("cats {:?} cache {:.1} GB drive {:?}", s.cats.map(|c| c / 1_000_000_000), s.cache_total as f64 / 1e9, s.drive.as_ref().map(|d| d.used / 1_000_000_000));
        for f in &s.findings {
            println!("  {} {:.1} GB", f.path, f.size as f64 / 1e9);
        }
        let t = SCAN.lock().unwrap();
        let t = t.as_ref().unwrap();
        for c in view(&t.node, &t.root, 1, 28).children.unwrap() {
            println!("  {:<40} {:>8.1} GB {:?} {}", c.name, c.size as f64 / 1e9, c.cat, c.kind);
        }
    }

    /// Dumps a scan and tree slices as JSON for a UI preview (DC_SPACE_DUMP=file).
    #[test]
    #[ignore]
    fn dump() {
        let root = resolve_root(&std::env::var("DC_SPACE_ROOT").unwrap_or_else(|_| "C:".into()));
        let s = scan_core(&root, |_| {}).unwrap();
        let t = SCAN.lock().unwrap();
        let t = t.as_ref().unwrap();
        let mut trees = serde_json::Map::new();
        let top = view(&t.node, &t.root, 2, 28);
        let mut paths = vec![top.path.clone()];
        for c in top.children.iter().flatten() {
            if c.kind == "dir" {
                paths.push(c.path.clone());
                for g in c.children.iter().flatten() {
                    if g.kind == "dir" {
                        paths.push(g.path.clone());
                    }
                }
            }
        }
        for p in paths {
            if let Some(n) = find(&t.node, &t.root, Path::new(&p)) {
                trees.insert(p.clone(), serde_json::to_value(view(n, Path::new(&p), 2, 28)).unwrap());
            }
        }
        let out = serde_json::json!({ "summary": s, "trees": trees });
        std::fs::write(std::env::var("DC_SPACE_DUMP").unwrap(), serde_json::to_string(&out).unwrap()).unwrap();
    }
}
