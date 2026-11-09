//! Package-manager and tool caches: where they live, what they are, how big
//! they are, and how to clean them. A tool's own clean command is preferred;
//! deleting the folder is the fallback.

use crate::defs;
use crate::util::{self, dir_size};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    /// Rebuilt on demand, nothing of yours is lost.
    Safe,
    /// Safe, but the next build or install is slow or needs a download.
    Slow,
    /// Yours (Trash, device backups, Xcode archives): never selected for you,
    /// and the confirmation says it can't be undone.
    Data,
}

#[derive(Clone)]
pub(crate) enum How {
    /// Run the tool's own command; delete the folders if it is missing or
    /// fails. With `per_path`, run once per existing path, `{path}` replaced.
    Tool { tool: &'static str, args: Vec<&'static str>, per_path: Vec<&'static str> },
    /// Delete the folders' contents.
    Delete,
    /// Delete entries older than this many hours (temp folders: installers
    /// running right now still need their files).
    OlderThan(u64),
    /// Docker manages its own storage.
    Docker,
    /// Empty the Recycle Bin / Trash through the OS, never by deleting its
    /// folders.
    RecycleBin,
    /// Needs admin rights. Every admin item chosen in one clean runs in a
    /// single elevated script (one UAC / password prompt). `days` deletes
    /// only entries older than that in each path; `pre`/`post` stop and
    /// restart services; `cmd` runs a command (DISM, journalctl, apt-get).
    /// The commands come from the app's own list (junk::admin_action).
    Admin { days: Option<u32>, pre: &'static str, cmd: &'static str, post: &'static str },
    /// A command that measures nothing up front (qlmanage, flatpak).
    Command { tool: &'static str, args: Vec<&'static str> },
}

#[derive(Clone)]
pub(crate) struct Target {
    pub id: &'static str,
    pub name: &'static str,
    pub eco: &'static str,
    pub about: &'static str,
    pub risk: Risk,
    pub paths: Vec<PathBuf>,
    pub how: How,
    /// Listed on the System junk page rather than Disk cleanup.
    pub junk: bool,
    /// Tool queries for where the cache really is.
    pub ask: &'static [defs::Ask],
    /// An engine step to run before cleaning (stop Gradle daemons).
    pub before: Option<&'static str>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheItem {
    pub id: String,
    pub name: String,
    pub eco: String,
    pub about: String,
    pub risk: Risk,
    pub paths: Vec<String>,
    pub size: u64,
    pub files: u64,
    /// What will run, in plain words ("npm cache clean --force" or "Delete folder").
    pub method: String,
    pub found: bool,
    pub junk: bool,
    /// Needs admin rights; cleaned in one elevated batch.
    pub admin: bool,
    /// False when the size can't be known before cleaning (DISM, snapshots).
    pub measured: bool,
}

/// Where each tool says its cache is. Tools move caches with config and env
/// vars (npm `cache`, `PNPM_STORE_DIR`, `GOCACHE`...), so the default paths
/// are only a starting point.
fn reported(t: &Target) -> Vec<PathBuf> {
    let mut out = vec![];
    for a in t.ask {
        let Some(exe) = which(&a.tool) else { continue };
        let args: Vec<&str> = a.args.iter().map(String::as_str).collect();
        let Some(text) = util::run_capture(&exe, &args, Duration::from_secs(8)) else { continue };
        let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        let p = PathBuf::from(line.trim_end_matches(['/', '\\']));
        if p.is_absolute() && p.is_dir() {
            // Some tools print a versioned folder (pnpm's store\v10); keep the
            // root so every version is counted.
            let versioned = p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.len() > 1 && n.starts_with('v') && n[1..].chars().all(|c| c.is_ascii_digit())
            });
            out.push(if versioned { p.parent().map(PathBuf::from).unwrap_or(p) } else { p });
        }
    }
    out
}

/// Drop duplicates and any path inside another listed path, so nothing is
/// counted twice.
fn dedupe(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let key = |p: &PathBuf| p.to_string_lossy().to_ascii_lowercase().replace('/', "\\").trim_end_matches('\\').to_string();
    let mut out: Vec<PathBuf> = vec![];
    for p in paths {
        let k = key(&p);
        if out.iter().any(|o| {
            let ok = key(o);
            k == ok || k.starts_with(&format!("{ok}\\"))
        }) {
            continue;
        }
        out.retain(|o| !key(o).starts_with(&format!("{k}\\")));
        out.push(p);
    }
    out
}

/// The drives chosen in Settings for the last scan. A clean uses the same
/// set, so it never reaches a drive the scan didn't show.
static LAST_DRIVES: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

pub(crate) fn chosen_drives() -> Option<Vec<String>> {
    LAST_DRIVES.lock().ok().and_then(|d| d.clone())
}

/// `drives` limits per-drive locations (pnpm stores, the drive walk) to the
/// drives chosen in Settings; None means every drive. Without `walk` the
/// last walk's results are reused (a clean right after a scan).
fn targets(drives: Option<&[String]>) -> Vec<Target> {
    targets_with(drives, false)
}

fn targets_with(drives: Option<&[String]>, walk: bool) -> Vec<Target> {
    let remembered = chosen_drives();
    let drives = drives.or(remembered.as_deref());
    let fake = util::fake_home().is_some();
    let found = crate::junk::drive_junk(drives, walk);
    let rules: Vec<&'static defs::CacheRule> = defs::get()
        .caches
        .iter()
        .filter(|r| defs::applies(r))
        .filter(|r| !(r.real_only && fake))
        .filter(|r| r.requires.as_deref().is_none_or(|t| which(t).is_some()))
        .collect();
    rules
        .par_iter()
        .filter_map(|r| {
            let mut t = target(r, drives, &found)?;
            if !fake {
                t.paths.extend(reported(&t));
            }
            t.paths = dedupe(std::mem::take(&mut t.paths));
            Some(t)
        })
        .collect()
}

/// A rule as something the engine can measure and clean.
fn target(r: &'static defs::CacheRule, drives: Option<&[String]>, walk: &crate::junk::DriveJunk) -> Option<Target> {
    let strs = |v: &'static [String]| v.iter().map(String::as_str).collect::<Vec<&'static str>>();
    let how = match &r.how {
        defs::HowDef::Delete => How::Delete,
        defs::HowDef::OlderThan { hours } => How::OlderThan(*hours),
        defs::HowDef::Tool { tool, args, per_path_args } => How::Tool { tool, args: strs(args), per_path: strs(per_path_args) },
        defs::HowDef::Command { tool, args } => How::Command { tool, args: strs(args) },
        defs::HowDef::Docker => How::Docker,
        defs::HowDef::RecycleBin => How::RecycleBin,
        defs::HowDef::Admin { days, action } => {
            let (pre, cmd, post) = match action {
                Some(a) => crate::junk::admin_action(a)?,
                None => ("", "", ""),
            };
            How::Admin { days: *days, pre, cmd, post }
        }
    };
    let mut paths = vec![];
    for e in &r.paths {
        expand_entry(e, drives, walk, &mut paths);
    }
    Some(Target { id: &r.id, name: &r.name, eco: &r.eco, about: &r.about, risk: r.risk, paths, how, junk: r.junk, ask: &r.ask, before: r.before.as_deref() })
}

fn expand_entry(e: &defs::PathEntry, drives: Option<&[String]>, walk: &crate::junk::DriveJunk, out: &mut Vec<PathBuf>) {
    use defs::PathEntry;
    match e {
        PathEntry::Template(t) => out.extend(defs::expand(t)),
        PathEntry::PerOs(p) => {
            if let Some(x) = p.pick() {
                expand_entry(x, drives, walk, out);
            }
        }
        PathEntry::Expand(x) => {
            let dir = || defs::expand(&x.dir);
            match x.expand.as_str() {
                "children" => {
                    let Some(d) = dir() else { return };
                    let versioned = x.matching.as_deref() == Some("versioned");
                    let keep = |n: &str| {
                        (!versioned || n.chars().any(|c| c.is_ascii_digit()))
                            && !x.except.iter().any(|e| e == n)
                            && !x.except_prefix.iter().any(|p| n.starts_with(p.as_str()))
                            && (x.contains.is_empty() || x.contains.iter().any(|c| n.contains(c.as_str())))
                    };
                    let Ok(rd) = std::fs::read_dir(&d) else { return };
                    for e in rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
                        if !keep(&e.file_name().to_string_lossy()) {
                            continue;
                        }
                        for tail in &x.tail {
                            out.push(if tail.is_empty() { e.path() } else { e.path().join(tail) });
                        }
                    }
                }
                "old-entries" => out.extend(dir().map(|d| old_entries(&d, x.keep.unwrap_or(1), &x.except, &x.prefix)).unwrap_or_default()),
                "chromium-profiles" => out.extend(dir().map(|d| crate::junk::chromium(&d)).unwrap_or_default()),
                "files" => {
                    let ext: Vec<&str> = x.ext.iter().map(String::as_str).collect();
                    let prefix: Vec<&str> = x.prefix.iter().map(String::as_str).collect();
                    let prefix = (!prefix.is_empty()).then_some(prefix.as_slice());
                    out.extend(dir().map(|d| crate::junk::files_with(&d, &ext, prefix)).unwrap_or_default());
                }
                "drive-walk" => out.extend(match x.find.as_deref() {
                    Some("partials") => walk.partials.clone(),
                    Some("thumbs") => walk.thumbs.clone(),
                    Some("shader-caches") => walk.shader_caches.clone(),
                    _ => vec![],
                }),
                // One folder per drive root (pnpm keeps a store per drive, since
                // hard links can't cross drives). Never under the fake home.
                "drive-roots" if util::fake_home().is_none() => {
                    for d in sysinfo::Disks::new_with_refreshed_list().list() {
                        if !crate::junk::drive_chosen(d.mount_point(), drives) {
                            continue;
                        }
                        for tail in &x.tail {
                            let p = d.mount_point().join(tail);
                            if p.is_dir() {
                                out.push(p);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Files and folders in `dir` other than the newest `keep` (by modified
/// time), for tools that keep every version they ever downloaded. Never a
/// symlink, nor whatever a symlink in `dir` points at: that is the version
/// in use (Codex's `current`), nor the target of a link in the folder above. With `prefix`, only names starting with one.
fn old_entries(dir: &std::path::Path, keep: usize, except: &[String], prefix: &[String]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut live = vec![];
    // Codex keeps `current` beside `releases/`, so links one level up count too.
    if let Some(Ok(up)) = dir.parent().map(std::fs::read_dir) {
        for e in up.flatten() {
            if std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.file_type().is_symlink()) {
                if let Ok(t) = std::fs::canonicalize(e.path()) {
                    live.push(t);
                }
            }
        }
    }
    let mut entries = vec![];
    for e in rd.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
        if meta.file_type().is_symlink() {
            if let Ok(t) = std::fs::canonicalize(e.path()) {
                live.push(t);
            }
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || except.iter().any(|x| *x == name) || (!prefix.is_empty() && !prefix.iter().any(|p| name.starts_with(p.as_str()))) {
            continue;
        }
        entries.push((meta.modified().ok(), e.path()));
    }
    // Version-named entries (22.17.0, v18.20.8, 0.166.0-x86_64…) sort by
    // version: a reinstall or copy can leave an older version with a newer
    // date. Anything else (hash-named builds) sorts by modified time.
    let versions: Vec<Option<Vec<u64>>> = entries.iter().map(|(_, p)| version_of(&p.file_name().unwrap_or_default().to_string_lossy())).collect();
    if versions.iter().all(Option::is_some) {
        let mut v: Vec<_> = entries.into_iter().zip(versions).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        entries = v.into_iter().map(|(e, _)| e).collect();
    } else {
        entries.sort_by(|a, b| b.0.cmp(&a.0));
    }
    entries
        .into_iter()
        .skip(keep)
        .map(|(_, p)| p)
        .filter(|p| std::fs::canonicalize(p).map_or(true, |c| !live.iter().any(|l| *l == c)))
        .collect()
}

/// The leading version in a name: "v22.17.0" → [22, 17, 0],
/// "0.166.0-x86_64-pc-windows-msvc" → [0, 166, 0], "Stable-1a2b" → None.
fn version_of(name: &str) -> Option<Vec<u64>> {
    let rest = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    if rest.len() + 2 < name.len() {
        return None; // a long non-numeric prefix: not a version name
    }
    let head: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let parts: Vec<u64> = head.split('.').filter(|x| !x.is_empty()).map(|x| x.parse().unwrap_or(0)).collect();
    (parts.len() >= 2).then_some(parts)
}

/// A cache tool on PATH. Under the e2e fake home, none: the real `npm cache
/// clean` would empty the real cache, so the suite exercises the delete path.
pub(crate) fn which(name: &str) -> Option<std::path::PathBuf> {
    if util::fake_home().is_some() {
        return None;
    }
    util::which(name)
}

fn method(how: &How) -> String {
    match how {
        How::Tool { tool, args, .. } => {
            if which(tool).is_some() {
                format!("{tool} {}", args.join(" "))
            } else {
                format!("Delete folder ({tool} not found)")
            }
        }
        How::Delete => "Delete folder contents".into(),
        How::OlderThan(h) if h % 24 == 0 => format!("Delete items older than {} days", h / 24),
        How::OlderThan(h) => format!("Delete items older than {h} h"),
        How::Docker => "docker system prune -af".into(),
        How::RecycleBin => {
            if cfg!(windows) {
                "Empty the Recycle Bin".into()
            } else {
                "Empty the Trash".into()
            }
        }
        How::Admin { days, cmd, .. } => {
            let what = if !cmd.is_empty() {
                cmd.to_string()
            } else if let Some(d) = days {
                format!("Delete items older than {d} days")
            } else {
                "Delete folder contents".into()
            };
            format!("{what} (as admin)")
        }
        How::Command { tool, args } => format!("{tool} {}", args.join(" ")),
    }
}

fn measure(t: &Target) -> CacheItem {
    let mut measured = true;
    let (size, files, found) = match &t.how {
        How::Docker => {
            let s = docker_reclaimable();
            (s.unwrap_or(0), 0, s.is_some())
        }
        How::RecycleBin => match crate::junk::recycle_bin_size() {
            Some((bytes, items)) => (bytes, items, true),
            None => (0, 0, false),
        },
        How::Command { tool, .. } if t.paths.is_empty() => {
            measured = false;
            (0, 0, which(tool).is_some())
        }
        How::Admin { cmd, .. } if t.paths.is_empty() => {
            measured = false;
            (0, 0, !cmd.is_empty() && crate::junk::admin_available())
        }
        _ => {
            // Files (thumbnail caches, broken downloads) count as themselves.
            let existing: Vec<&PathBuf> = t.paths.iter().filter(|p| p.exists()).collect();
            let (s, f) = existing.par_iter().map(|p| dir_size(p)).reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            (s, f, !existing.is_empty())
        }
    };
    CacheItem {
        id: t.id.into(),
        name: t.name.into(),
        eco: t.eco.into(),
        about: t.about.into(),
        risk: t.risk,
        paths: t.paths.iter().filter(|p| p.exists()).map(|p| p.to_string_lossy().to_string()).collect(),
        size,
        files,
        method: method(&t.how),
        found,
        junk: t.junk,
        admin: matches!(t.how, How::Admin { .. }),
        measured,
    }
}

/// Measure every target in parallel, emitting each as soon as it is done so
/// the list fills in live.
/// Sizes from the last scan. A clean starts from these instead of measuring
/// every folder again first, which took seconds on big caches.
static LAST_SIZES: std::sync::Mutex<Option<std::collections::HashMap<String, (u64, u64)>>> = std::sync::Mutex::new(None);

/// A live progress line for the item being cleaned, sent several times a
/// second: what is happening and how far it has got.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Tick {
    id: String,
    stage: String,
    bytes: u64,
    files: u64,
    elapsed_ms: u64,
}

/// Sends ticks while an item is being cleaned; stops when dropped.
struct Ticker {
    stage: std::sync::Arc<std::sync::Mutex<String>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: std::time::Instant,
}

impl Ticker {
    fn start(app: &AppHandle, id: &str, stage: &str) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        util::progress::reset();
        let stage = std::sync::Arc::new(std::sync::Mutex::new(stage.to_string()));
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let started = std::time::Instant::now();
        let (app, id, s2, stop2) = (app.clone(), id.to_string(), stage.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let (bytes, files) = util::progress::get();
                let stage = s2.lock().map(|s| s.clone()).unwrap_or_default();
                let _ = app.emit("clean-tick", Tick { id: id.clone(), stage, bytes, files, elapsed_ms: started.elapsed().as_millis() as u64 });
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        Ticker { stage, stop, started }
    }

    fn stage(&self, s: &str) {
        if let Ok(mut g) = self.stage.lock() {
            *g = s.to_string();
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Size before cleaning: the last scan's figure when there is one.
fn size_before(t: &Target) -> u64 {
    LAST_SIZES.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(t.id).map(|s| s.0))).unwrap_or_else(|| measure(t).size)
}

pub fn scan(app: &AppHandle, drives: Option<&[String]>) -> Vec<CacheItem> {
    if let Ok(mut d) = LAST_DRIVES.lock() {
        *d = drives.map(|x| x.to_vec());
    }
    let ts = targets_with(drives, true);
    let _ = app.emit("cache-scan-start", ts.len());
    let mut items: Vec<CacheItem> = ts
        .par_iter()
        .map(|t| {
            let item = measure(t);
            let _ = app.emit("cache-scanned", &item);
            item
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.size));
    if let Ok(mut m) = LAST_SIZES.lock() {
        *m = Some(items.iter().map(|i| (i.id.clone(), (i.size, i.files))).collect());
    }
    items
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanEvent {
    pub id: String,
    /// How long cleaning this item took.
    #[serde(default)]
    pub elapsed_ms: u64,
    /// "running", "done" or "failed".
    pub phase: String,
    pub freed: u64,
    pub skipped: u64,
    pub note: String,
    /// Left alone on purpose: newer than the age limit.
    #[serde(default)]
    pub kept: u64,
    /// Could not be removed, usually because a program has it open.
    #[serde(default)]
    pub in_use: u64,
    /// A few of those paths, to show.
    #[serde(default)]
    pub in_use_sample: Vec<String>,
    /// Programs holding them.
    #[serde(default)]
    pub held_by: Vec<crate::holders::Holder>,
}

/// What clearing one target did.
#[derive(Default)]
struct Cleared {
    kept: u64,
    failed: Vec<PathBuf>,
}

impl Cleared {
    fn add(&mut self, o: Cleared) {
        self.kept += o.kept;
        self.failed.extend(o.failed);
    }
}

/// Delete what is in a path: a folder's contents, or a single file.
fn clear(p: &std::path::Path, age: Option<Duration>) -> Cleared {
    if p.is_dir() {
        let r = util::empty_dir_report(p, age);
        Cleared { kept: r.kept, failed: r.failed }
    } else if p.is_file() {
        let young = age.is_some_and(|a| {
            std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_some_and(|e| e < a)
        });
        if young {
            Cleared { kept: 1, failed: vec![] }
        } else if std::fs::remove_file(p).is_err() {
            Cleared { kept: 0, failed: vec![p.to_path_buf()] }
        } else {
            Cleared::default()
        }
    } else {
        Cleared::default()
    }
}

/// Turn what a clean left behind into counts, sample paths and the
/// programs holding the files.
fn explain(c: Cleared) -> (u64, u64, Vec<String>, Vec<crate::holders::Holder>) {
    let in_use = c.failed.len() as u64;
    let sample = c.failed.iter().take(6).map(|p| p.to_string_lossy().to_string()).collect();
    // A folder that failed holds the files that are actually open.
    let files: Vec<PathBuf> = c.failed.iter().flat_map(|p| crate::holders::leftovers(p, 50)).take(400).collect();
    (c.kept, in_use, sample, crate::holders::holders(&files))
}

pub fn clean(app: &AppHandle, ids: &[String]) -> Vec<CleanEvent> {
    let ts = targets(None);
    let emit = |e: &CleanEvent| {
        let _ = app.emit("clean-progress", e);
    };
    let picked: Vec<&Target> = ids.iter().filter_map(|id| ts.iter().find(|t| t.id == id)).collect();
    let mut out = vec![];

    // Admin items first, all in one elevated run: one prompt, not one each.
    let admin: Vec<&Target> = picked.iter().copied().filter(|t| matches!(t.how, How::Admin { .. })).collect();
    if !admin.is_empty() {
        let before: Vec<u64> = admin.iter().map(|t| size_before(t)).collect();
        for t in &admin {
            emit(&CleanEvent { id: t.id.into(), phase: "running".into(), freed: 0, skipped: 0, note: "Waiting for admin approval".into(), ..Default::default() });
        }
        let ticker = Ticker::start(app, admin[0].id, "Waiting for admin approval, then running as admin");
        let ok = crate::junk::run_admin(&admin);
        ticker.stage("Checking what's left");
        let elapsed_ms = ticker.elapsed_ms();
        for (t, b) in admin.iter().zip(before) {
            let freed = b.saturating_sub(measure(t).size);
            let note = if ok { "Cleaned as admin" } else { "Admin approval was cancelled or failed" };
            let ev = CleanEvent { id: t.id.into(), phase: if ok { "done" } else { "failed" }.into(), freed, skipped: 0, note: note.into(), elapsed_ms, ..Default::default() };
            emit(&ev);
            out.push(ev);
        }
    }

    // Running Gradle daemons keep caches open; stop them once first.
    if picked.iter().any(|t| t.before == Some("stop-gradle")) {
        if let Some(g) = which("gradle") {
            util::run_ok(&g, &["--stop"], Duration::from_secs(60));
        }
    }

    for t in picked.into_iter().filter(|t| !matches!(t.how, How::Admin { .. })) {
        let id = t.id.to_string();
        emit(&CleanEvent { id: id.clone(), phase: "running".into(), freed: 0, skipped: 0, note: method(&t.how), ..Default::default() });
        let ticker = Ticker::start(app, &id, &match &t.how {
            How::Tool { tool, args, .. } | How::Command { tool, args } => format!("Running {tool} {}", args.join(" ")),
            How::Docker => "Running docker system prune -af".into(),
            How::RecycleBin => "Emptying the Recycle Bin".into(),
            _ => "Deleting files".into(),
        });
        let before = size_before(t);
        let mut cleared = Cleared::default();
        let mut note: String;
        match &t.how {
            How::Tool { tool, args, per_path } => {
                let ran = which(tool).is_some_and(|p| {
                    if !per_path.is_empty() {
                        // Once per location (pnpm: each drive has its own store).
                        let dirs: Vec<&PathBuf> = t.paths.iter().filter(|s| s.is_dir()).collect();
                        !dirs.is_empty()
                            && dirs.iter().all(|s| {
                                let dir = s.to_string_lossy().to_string();
                                let a: Vec<&str> = per_path.iter().map(|x| if *x == "{path}" { dir.as_str() } else { x }).collect();
                                util::run_ok(&p, &a, Duration::from_secs(600))
                            })
                    } else {
                        util::run_ok(&p, args, Duration::from_secs(600))
                    }
                });
                if ran {
                    note = format!("Ran {tool} {}", args.join(" "));
                } else {
                    ticker.stage(&format!("{tool} unavailable: deleting files"));
                    // A missing or failing tool falls back to deleting.
                    for p in &t.paths {
                        cleared.add(clear(p, None));
                    }
                    note = format!("{tool} unavailable, deleted the folder");
                }
            }
            How::Delete => {
                for p in &t.paths {
                    cleared.add(clear(p, None));
                }
                note = "Deleted".into();
            }
            How::OlderThan(hours) => {
                for p in &t.paths {
                    cleared.add(clear(p, Some(Duration::from_secs(hours * 3600))));
                }
                note = method(&t.how).replace("Delete", "Removed");
            }
            How::Docker => {
                let ok = which("docker").is_some_and(|p| util::run_ok(&p, &["system", "prune", "-af"], Duration::from_secs(600)));
                note = if ok { "Pruned unused Docker data".into() } else { "Docker is not running".into() };
            }
            How::RecycleBin => {
                note = if crate::junk::empty_recycle_bin() { "Emptied".into() } else { "Could not empty it".into() };
            }
            How::Command { tool, args } => {
                let ok = which(tool).is_some_and(|p| util::run_ok(&p, args, Duration::from_secs(600)));
                note = if ok { format!("Ran {tool} {}", args.join(" ")) } else { format!("{tool} failed or is missing") };
            }
            How::Admin { .. } => unreachable!("admin items run above"),
        }
        ticker.stage("Checking what's left");
        let after = measure(t).size;
        let freed = before.saturating_sub(after);
        if !cleared.failed.is_empty() {
            ticker.stage("Finding which programs hold the files left behind");
        }
        let (kept, in_use, in_use_sample, held_by) = explain(cleared);
        let elapsed_ms = ticker.elapsed_ms();
        drop(ticker);
        if in_use > 0 {
            note = format!("{note} · {in_use} in use, left alone");
        }
        let ev = CleanEvent { id, phase: "done".into(), freed, skipped: kept + in_use, note, kept, in_use, in_use_sample, held_by, elapsed_ms };
        emit(&ev);
        out.push(ev);
    }
    out
}

/// Bytes `docker system prune -af` would free: images, containers and build
/// cache. Volumes are counted by `docker system df` but never pruned here, so
/// they are left out. None if Docker is missing or its daemon is stopped.
fn docker_reclaimable() -> Option<u64> {
    let docker = which("docker")?;
    let out = util::run_capture(&docker, &["system", "df", "--format", "{{.Type}}|{{.Reclaimable}}"], Duration::from_secs(8))?;
    if !out.contains('|') {
        return None;
    }
    Some(sum_reclaimable(&out))
}

fn sum_reclaimable(out: &str) -> u64 {
    out.lines()
        .filter_map(|l| {
            let (kind, rest) = l.split_once('|')?;
            if kind.to_ascii_lowercase().contains("volume") {
                return None;
            }
            parse_size(rest.split_whitespace().next()?)
        })
        .sum()
}

/// "1.234GB" / "512.5MB" / "0B" -> bytes.
fn parse_size(s: &str) -> Option<u64> {
    let idx = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(idx);
    let n: f64 = num.parse().ok()?;
    let mult = match unit.to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" | "KIB" => 1e3,
        "MB" | "MIB" => 1e6,
        "GB" | "GIB" => 1e9,
        "TB" | "TIB" => 1e12,
        _ => return None,
    };
    Some((n * mult) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_sizes() {
        assert_eq!(parse_size("1.5GB"), Some(1_500_000_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("512kB"), Some(512_000));
        assert_eq!(parse_size("n/a"), None);
    }

    #[test]
    fn nested_and_duplicate_paths_are_counted_once() {
        let v = dedupe(vec![
            PathBuf::from(r"C:\Users\a\AppData\Local\Yarn\Cache\v6"),
            PathBuf::from(r"C:\Users\a\AppData\Local\Yarn\Cache"),
            PathBuf::from(r"c:\users\a\appdata\local\yarn\cache"),
            PathBuf::from(r"T:\.pnpm-store"),
        ]);
        assert_eq!(v, vec![PathBuf::from(r"C:\Users\a\AppData\Local\Yarn\Cache"), PathBuf::from(r"T:\.pnpm-store")]);
    }

    #[test]
    fn docker_volumes_are_not_counted() {
        let out = "Images|10.5GB (80%)\nContainers|1GB (100%)\nLocal Volumes|28GB (90%)\nBuild Cache|500MB";
        assert_eq!(sum_reclaimable(out), 12_000_000_000);
    }

    #[test]
    fn every_target_has_a_unique_id() {
        let ts = targets(None);
        let mut ids: Vec<_> = ts.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ts.len());
    }

    use std::time::{Duration, SystemTime};

    fn aged(p: &std::path::Path, days_old: u64) {
        let f = std::fs::File::options().write(true).open(p).or_else(|_| std::fs::File::open(p)).unwrap();
        let _ = f.set_modified(SystemTime::now() - Duration::from_secs(days_old * 86400));
    }

    #[test]
    fn old_entries_keeps_the_newest_and_what_a_symlink_points_at() {
        let d = std::env::temp_dir().join(format!("dc-old-entries-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (name, age) in [("1.0.0", 30), ("1.1.0", 20), ("1.2.0", 10), ("1.3.0", 1)] {
            let p = d.join(name);
            std::fs::write(&p, b"x").unwrap();
            aged(&p, age);
        }
        std::fs::write(d.join(".lock"), b"").unwrap();

        let mut got: Vec<String> = old_entries(&d, 2, &[], &[]).iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        got.sort();
        assert_eq!(got, ["1.0.0", "1.1.0"]);

        // An `except` name and a symlink's target stay even when old.
        let got = old_entries(&d, 1, &["1.0.0".into()], &[]);
        assert!(!got.iter().any(|p| p.ends_with("1.0.0")));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(d.join("1.1.0"), d.join("current")).unwrap();
            let got = old_entries(&d, 1, &[], &[]);
            assert!(!got.iter().any(|p| p.ends_with("1.1.0") || p.ends_with("current")));
            assert!(got.iter().any(|p| p.ends_with("1.0.0")));
        }
        // Version names beat dates: v22 stays even when it is the oldest file.
        let v = d.join("versions");
        std::fs::create_dir_all(&v).unwrap();
        for (name, age) in [("v22.17.0", 30), ("v18.20.8", 2), ("v16.20.2", 1)] {
            std::fs::write(v.join(name), b"x").unwrap();
            aged(&v.join(name), age);
        }
        let got = old_entries(&v, 2, &[], &[]);
        assert_eq!(got.len(), 1);
        assert!(got[0].ends_with("v16.20.2"));
        assert_eq!(version_of("0.166.0-x86_64-pc-windows-msvc"), Some(vec![0, 166, 0]));
        assert_eq!(version_of("13995fba801849b0"), None);
        assert_eq!(version_of("Stable-1a2b3c"), None);
        std::fs::remove_dir_all(&v).unwrap();
        // A prefix narrows the candidates: nvm-windows keeps v18.x beside nvm.exe.
        std::fs::write(d.join("nvm.exe"), b"x").unwrap();
        aged(&d.join("nvm.exe"), 90);
        let got = old_entries(&d, 0, &[], &["1.".into()]);
        assert_eq!(got.len(), if cfg!(unix) { 3 } else { 4 }); // unix: 1.1.0 is behind `current`
        assert!(!got.iter().any(|p| p.ends_with("nvm.exe")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
