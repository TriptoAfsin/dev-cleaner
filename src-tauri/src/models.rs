//! Finds AI models downloaded for local use: Ollama, the Hugging Face hub
//! cache, LM Studio, ComfyUI / Stable Diffusion WebUI and friends, and loose
//! .gguf / .safetensors / .ckpt files. Every one can be downloaded again, but
//! they are often the biggest files on a developer machine.

use crate::util::{self, dir_size, remove_tree};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub static CANCEL: AtomicBool = AtomicBool::new(false);

const MB: u64 = 1024 * 1024;
const MAX_DEPTH: u32 = 8;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    /// Short machine key for the icon and the filter chips: "ollama",
    /// "huggingface", "hf-dataset", "lmstudio", "comfyui", "loose", ...
    pub kind: String,
    /// Human label, e.g. "ComfyUI · checkpoints" or "Loose file".
    pub source: String,
    /// "GGUF", "safetensors", "PyTorch", ...
    pub format: String,
    pub path: String,
    pub size: u64,
    pub files: u64,
    /// Seconds since epoch.
    pub modified: u64,
    pub accessed: Option<u64>,
    pub deletable: bool,
    pub note: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEvent {
    pub id: String,
    pub ok: bool,
    pub freed: u64,
    pub error: Option<String>,
}

// ---------------------------------------------------------------- locations

fn env_dir(var: &str) -> Option<PathBuf> {
    // Under the e2e fake home, never look at the real machine's overrides.
    if util::fake_home().is_some() {
        return None;
    }
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn ollama_dir() -> PathBuf {
    env_dir("OLLAMA_MODELS").unwrap_or_else(|| util::home().join(".ollama").join("models"))
}

fn hf_hub_dir() -> PathBuf {
    env_dir("HF_HUB_CACHE")
        .or_else(|| env_dir("HF_HOME").map(|h| h.join("hub")))
        .unwrap_or_else(|| util::home().join(".cache").join("huggingface").join("hub"))
}

fn lmstudio_roots() -> Vec<PathBuf> {
    let h = util::home();
    vec![h.join(".lmstudio").join("models"), h.join(".cache").join("lm-studio").join("models")]
}

/// Folders apps keep models in (definitions/models.json). Each is walked for
/// model files.
fn app_roots() -> Vec<(PathBuf, &'static str, &'static str)> {
    crate::defs::get()
        .models
        .app_roots
        .iter()
        .filter_map(|r| Some((crate::defs::expand(&r.path)?, r.app.as_str(), r.label.as_str())))
        .collect()
}

/// Everyday folders people drop downloaded models into.
fn loose_roots() -> Vec<PathBuf> {
    crate::defs::get().models.loose_roots.iter().filter_map(|t| crate::defs::expand(t)).collect()
}

/// Comparable form of a path: forward slashes, no trailing slash, and
/// lower-case where the file system usually ignores case.
fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/').to_string();
    if cfg!(any(windows, target_os = "macos")) {
        s.to_lowercase()
    } else {
        s
    }
}

fn same(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => norm(a) == norm(b),
    }
}

fn is_real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
}

fn secs(t: std::io::Result<SystemTime>) -> u64 {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

fn now() -> u64 {
    secs(Ok(SystemTime::now()))
}

// ---------------------------------------------------------------- matching

/// Smallest size a file with this extension must have to count as a model,
/// or None when the extension is not a model format. `.bin`, `.pb` and
/// `.npz` are common for other things too, so they need to be much bigger.
fn min_size(ext: &str) -> Option<u64> {
    match ext.to_ascii_lowercase().as_str() {
        "gguf" | "ggml" | "safetensors" | "ckpt" | "pt" | "pth" | "onnx" | "tflite" | "mlmodel" | "h5" | "keras"
        | "mlpackage" => Some(50 * MB),
        "bin" | "pb" | "npz" => Some(200 * MB),
        _ => None,
    }
}

fn ext_of(name: &str) -> Option<&str> {
    name.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.is_empty())
}

fn is_model_file(name: &str, size: u64) -> bool {
    ext_of(name).and_then(min_size).is_some_and(|min| size >= min)
}

fn format_label(ext: &str) -> String {
    match ext.to_ascii_lowercase().as_str() {
        "gguf" => "GGUF",
        "ggml" => "GGML",
        "safetensors" => "safetensors",
        "ckpt" => "ckpt",
        "pt" | "pth" => "PyTorch",
        "onnx" => "ONNX",
        "tflite" => "TFLite",
        "mlmodel" | "mlpackage" => "Core ML",
        "h5" | "keras" => "Keras",
        "pb" => "TensorFlow",
        "npz" => "NumPy",
        "bin" => "bin",
        "parquet" => "Parquet",
        "arrow" => "Arrow",
        "jsonl" | "json" => "JSON",
        "csv" => "CSV",
        "" => "",
        other => return other.to_string(),
    }
    .to_string()
}

/// `model-00001-of-00004.safetensors` → ("model", "00004", "safetensors").
fn shard_parts(name: &str) -> Option<(String, String, String)> {
    let (stem, ext) = name.rsplit_once('.')?;
    let (left, total) = stem.rsplit_once("-of-")?;
    let (base, idx) = left.rsplit_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(total) || !digits(idx) || base.is_empty() || min_size(ext).is_none() {
        return None;
    }
    Some((base.to_string(), total.to_string(), ext.to_string()))
}

/// `models--meta-llama--Llama-3.1-8B` → ("models", "meta-llama/Llama-3.1-8B").
fn parse_hf_dir(name: &str) -> Option<(&'static str, String)> {
    let (kind, rest) = if let Some(r) = name.strip_prefix("models--") {
        ("models", r)
    } else {
        ("datasets", name.strip_prefix("datasets--")?)
    };
    if rest.is_empty() {
        return None;
    }
    Some((kind, rest.replacen("--", "/", 1)))
}

/// Which app a model file belongs to, judged from the folders around it.
fn app_label(path: &Path) -> Option<(&'static str, String)> {
    let parts: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    let after_models = |from: usize| -> Option<String> {
        let i = parts[from..].iter().position(|p| p == "models")? + from;
        // The folder after "models", unless that is the file itself.
        (i + 2 < parts.len()).then(|| {
            path.components().nth(i + 1).map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default()
        })
    };
    let with_sub = |kind: &'static str, label: &str, i: usize| {
        let s = match after_models(i) {
            Some(sub) => format!("{label} · {sub}"),
            None => label.to_string(),
        };
        Some((kind, s))
    };
    for (i, p) in parts.iter().enumerate() {
        let prev = if i > 0 { parts[i - 1].as_str() } else { "" };
        match p.as_str() {
            "comfyui" | "comfyui_windows_portable" => return with_sub("comfyui", "ComfyUI", i),
            s if s.starts_with("stable-diffusion-webui") || s.starts_with("sd-webui") || s == "webui_forge" => {
                return with_sub("a1111", "Stable Diffusion WebUI", i)
            }
            "gpt4all" => return Some(("gpt4all", "GPT4All".into())),
            "llama.cpp" => return Some(("llamacpp", "llama.cpp".into())),
            "lm-studio" | ".lmstudio" => return Some(("lmstudio", "LM Studio".into())),
            "modelscope" => return Some(("modelscope", "ModelScope".into())),
            "whisper" if prev == ".cache" => return Some(("whisper", "Whisper".into())),
            "torch" if prev == ".cache" => return Some(("torch", "PyTorch hub".into())),
            "jan" if parts.get(i + 1).is_some_and(|n| n == "models" || n == "data") => {
                return Some(("jan", "Jan".into()))
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------- Ollama

struct OllamaRow {
    name: String,
    size: u64,
    modified: u64,
}

fn parse_size(num: &str, unit: &str) -> Option<u64> {
    let n: f64 = num.parse().ok()?;
    let mult = match unit.to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KIB" => 1024.0,
        "MIB" => 1024.0 * 1024.0,
        "GIB" => 1024.0 * 1024.0 * 1024.0,
        "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * mult) as u64)
}

/// "3 weeks ago", "About an hour ago", "Less than a second ago" → epoch secs.
fn parse_ago(text: &str, now: u64) -> u64 {
    let t = text.trim().to_lowercase();
    if t.starts_with("less than") || t == "now" || t == "just now" {
        return now;
    }
    if t == "yesterday" {
        return now.saturating_sub(86_400);
    }
    let t = t.trim_start_matches("about ").trim_end_matches(" ago");
    let mut it = t.split_whitespace();
    let (Some(n), Some(unit)) = (it.next(), it.next()) else { return 0 };
    let n: u64 = match n {
        "a" | "an" | "one" => 1,
        s => match s.parse() {
            Ok(v) => v,
            Err(_) => return 0,
        },
    };
    let unit = unit.trim_end_matches('s');
    let s = match unit {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86_400,
        "week" => 7 * 86_400,
        "month" => 30 * 86_400,
        "year" => 365 * 86_400,
        _ => return 0,
    };
    now.saturating_sub(n * s)
}

/// Parse `ollama list`: NAME  ID  SIZE  MODIFIED, size as "4.7 GB".
fn parse_ollama_list(text: &str, now: u64) -> Vec<OllamaRow> {
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return vec![] };
    if !header.trim_start().to_uppercase().starts_with("NAME") {
        return vec![];
    }
    lines
        .filter_map(|line| {
            let toks: Vec<&str> = line.split_whitespace().collect();
            if toks.len() < 4 {
                return None;
            }
            let name = toks[0].to_string();
            // "4.7 GB" is two tokens; tolerate "4.7GB" too.
            let (size, rest) = match parse_size(toks[2], toks[3]) {
                Some(s) => (s, &toks[4..]),
                None => {
                    let t = toks[2];
                    let split = t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len());
                    (parse_size(&t[..split], &t[split..]).unwrap_or(0), &toks[3..])
                }
            };
            Some(OllamaRow { name, size, modified: parse_ago(&rest.join(" "), now) })
        })
        .collect()
}

fn ollama_exe() -> Option<PathBuf> {
    // The e2e suite must never see or remove the machine's real models.
    if util::fake_home().is_some() {
        return None;
    }
    util::which("ollama")
}

fn ollama_list(exe: &Path) -> Option<Vec<OllamaRow>> {
    let out = util::run_capture(exe, &["list"], Duration::from_secs(20))?;
    if !out.trim_start().to_uppercase().starts_with("NAME") {
        return None;
    }
    Some(parse_ollama_list(&out, now()))
}

fn scan_ollama(found: &(dyn Fn(&Model) + Sync)) -> Vec<Model> {
    let dir = ollama_dir();
    let path = dir.to_string_lossy().to_string();
    let exe = ollama_exe();
    if let Some(rows) = exe.as_deref().and_then(ollama_list) {
        let out: Vec<Model> = rows
            .into_iter()
            .map(|r| Model {
                id: format!("ollama:{}", r.name),
                name: r.name,
                kind: "ollama".into(),
                source: "Ollama".into(),
                format: "GGUF".into(),
                path: path.clone(),
                size: r.size,
                files: 1,
                modified: r.modified,
                accessed: None,
                deletable: true,
                note: "Removed with ollama rm. Layers shared with other models are kept.".into(),
            })
            .collect();
        out.iter().for_each(found);
        return out;
    }
    if !is_real_dir(&dir) {
        return vec![];
    }
    let (size, files) = dir_size(&dir);
    if size == 0 {
        return vec![];
    }
    let installed = exe.is_some();
    let m = Model {
        id: format!("ollama-dir:{path}"),
        name: "Ollama models".into(),
        kind: "ollama".into(),
        source: "Ollama".into(),
        format: "GGUF".into(),
        path,
        size,
        files,
        modified: secs(std::fs::metadata(&dir).and_then(|m| m.modified())),
        accessed: None,
        deletable: !installed,
        note: if installed {
            "Ollama is installed but not answering. Start it to list and remove models one by one.".into()
        } else {
            "Ollama isn't on PATH, so this is the whole models folder.".into()
        },
    };
    found(&m);
    vec![m]
}

// ---------------------------------------------------------------- folders

struct TreeInfo {
    modified: u64,
    accessed: u64,
    /// Extension of the largest file, following file symlinks for the size
    /// (Hugging Face snapshots link to blobs).
    largest_ext: String,
}

fn tree_info(dir: &Path) -> TreeInfo {
    fn go(dir: &Path, depth: u32, acc: &mut (u64, u64, u64, String)) {
        if depth > 12 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                go(&p, depth + 1, acc);
                continue;
            }
            let Ok(lm) = std::fs::symlink_metadata(&p) else { continue };
            acc.0 = acc.0.max(secs(lm.modified()));
            acc.1 = acc.1.max(secs(lm.accessed()));
            let size = if ft.is_symlink() { std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) } else { lm.len() };
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(ext) = ext_of(&name) {
                let known = min_size(ext).is_some() || matches!(ext, "parquet" | "arrow" | "jsonl" | "csv");
                if known && size > acc.2 {
                    acc.2 = size;
                    acc.3 = ext.to_ascii_lowercase();
                }
            }
        }
    }
    let mut acc = (0, 0, 0, String::new());
    go(dir, 0, &mut acc);
    if acc.0 == 0 {
        acc.0 = secs(std::fs::metadata(dir).and_then(|m| m.modified()));
    }
    TreeInfo { modified: acc.0, accessed: acc.1, largest_ext: acc.3 }
}

fn subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .collect()
}

fn folder_model(id: String, name: String, kind: &str, source: &str, path: &Path, note: &str) -> Model {
    let (size, files) = dir_size(path);
    let info = tree_info(path);
    Model {
        id,
        name,
        kind: kind.into(),
        source: source.into(),
        format: format_label(&info.largest_ext),
        path: path.to_string_lossy().to_string(),
        size,
        files,
        modified: info.modified,
        accessed: (info.accessed > 0).then_some(info.accessed),
        deletable: true,
        note: note.into(),
    }
}

fn scan_hf(found: &(dyn Fn(&Model) + Sync)) -> Vec<Model> {
    let hub = hf_hub_dir();
    subdirs(&hub)
        .par_iter()
        .filter_map(|(name, path)| {
            if CANCEL.load(Ordering::Relaxed) {
                return None;
            }
            let (kind, repo) = parse_hf_dir(name)?;
            let (k, src, note) = if kind == "models" {
                ("huggingface", "Hugging Face", "Downloaded again the next time transformers, diffusers or the hub asks for it.")
            } else {
                ("hf-dataset", "Hugging Face datasets", "Downloaded again the next time the datasets library loads it.")
            };
            let m = folder_model(format!("hf:{}", path.to_string_lossy()), repo, k, src, path, note);
            (m.size > 0).then(|| {
                found(&m);
                m
            })
        })
        .collect()
}

fn scan_lmstudio(found: &(dyn Fn(&Model) + Sync)) -> Vec<Model> {
    let mut models: Vec<(String, PathBuf)> = vec![];
    for root in lmstudio_roots() {
        for (publisher, pdir) in subdirs(&root) {
            for (model, mdir) in subdirs(&pdir) {
                models.push((format!("{publisher}/{model}"), mdir));
            }
        }
    }
    models
        .par_iter()
        .filter_map(|(name, path)| {
            if CANCEL.load(Ordering::Relaxed) {
                return None;
            }
            let m = folder_model(
                format!("lms:{}", path.to_string_lossy()),
                name.clone(),
                "lmstudio",
                "LM Studio",
                path,
                "Download it again from LM Studio's Discover tab.",
            );
            (m.size > 0).then(|| {
                found(&m);
                m
            })
        })
        .collect()
}

// ---------------------------------------------------------------- file walk

/// Folders never worth walking into for loose models.
fn skip_dir(lower: &str) -> bool {
    matches!(
        lower,
        "node_modules"
            | ".git"
            | "appdata"
            | "library"
            | "$recycle.bin"
            | "system volume information"
            | "windows"
            | ".venv"
            | "venv"
            | "site-packages"
            | "dist-packages"
            | ".trash"
            | ".trashes"
            | ".npm"
            | ".cargo"
            | ".rustup"
            | ".pnpm-store"
    ) || lower.starts_with("program files")
}

struct Hit {
    path: PathBuf,
    size: u64,
    pkg: bool,
    kind: &'static str,
    source: String,
}

struct Walk<'a> {
    covered: HashSet<String>,
    seen: AtomicU64,
    progress: &'a (dyn Fn(&Path) + Sync),
}

impl Walk<'_> {
    fn hit(&self, path: PathBuf, size: u64, pkg: bool, root_label: Option<(&'static str, &'static str)>) -> Hit {
        let (kind, source) = app_label(&path)
            .or_else(|| root_label.map(|(k, l)| (k, l.to_string())))
            .unwrap_or(("loose", "Loose file".to_string()));
        Hit { path, size, pkg, kind, source }
    }

    fn walk(&self, dir: &Path, depth: u32, label: Option<(&'static str, &'static str)>) -> Vec<Hit> {
        if CANCEL.load(Ordering::Relaxed) || depth > MAX_DEPTH {
            return vec![];
        }
        if self.seen.fetch_add(1, Ordering::Relaxed) % 150 == 0 {
            (self.progress)(dir);
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
        let mut hits = vec![];
        let mut dirs = vec![];
        for e in rd.flatten() {
            // DirEntry::file_type never follows symlinks.
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            if ft.is_dir() {
                let lower = name.to_lowercase();
                if lower.ends_with(".mlpackage") {
                    let size = dir_size(&path).0;
                    if size >= 50 * MB {
                        hits.push(self.hit(path, size, true, label));
                    }
                } else if !skip_dir(&lower) && !self.covered.contains(&norm(&path)) {
                    dirs.push(path);
                }
            } else if ft.is_file() {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                // Shards are kept whatever their size; the group is checked later.
                if is_model_file(&name, size) || (shard_parts(&name).is_some() && size > 0) {
                    hits.push(self.hit(path, size, false, label));
                }
            }
        }
        hits.extend(dirs.par_iter().flat_map(|d| self.walk(d, depth + 1, label)).collect::<Vec<_>>());
        hits
    }
}

fn file_times(p: &Path) -> (u64, u64) {
    std::fs::symlink_metadata(p).map(|m| (secs(m.modified()), secs(m.accessed()))).unwrap_or((0, 0))
}

/// Turn walk hits into models, folding sharded files into one item.
fn group_hits(hits: Vec<Hit>) -> Vec<Model> {
    let mut out = vec![];
    let mut shards: BTreeMap<(String, String, String, String), Vec<Hit>> = BTreeMap::new();
    for h in hits {
        let name = h.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !h.pkg {
            if let Some((base, total, ext)) = shard_parts(&name) {
                let dir = h.path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                shards.entry((dir, base, total, ext)).or_default().push(h);
                continue;
            }
        }
        if !h.pkg && !is_model_file(&name, h.size) {
            continue;
        }
        let (modified, accessed) = file_times(&h.path);
        let format = format_label(if h.pkg { "mlpackage" } else { ext_of(&name).unwrap_or("") });
        let p = h.path.to_string_lossy().to_string();
        out.push(Model {
            id: format!("{}:{p}", if h.pkg { "pkg" } else { "file" }),
            name,
            kind: h.kind.into(),
            source: h.source,
            format,
            path: p,
            size: h.size,
            files: 1,
            modified,
            accessed: (accessed > 0).then_some(accessed),
            deletable: true,
            note: String::new(),
        });
    }
    for ((dir, base, total, ext), mut group) in shards {
        let size: u64 = group.iter().map(|h| h.size).sum();
        if size < min_size(&ext).unwrap_or(u64::MAX) {
            continue;
        }
        group.sort_by(|a, b| a.path.cmp(&b.path));
        let (mut modified, mut accessed) = (0, 0);
        for h in &group {
            let (m, a) = file_times(&h.path);
            modified = modified.max(m);
            accessed = accessed.max(a);
        }
        let n = group.len();
        let expected = total.trim_start_matches('0').parse::<usize>().unwrap_or(n);
        let first = &group[0];
        out.push(Model {
            id: format!("shards:{dir}|{base}|{total}|{ext}"),
            name: format!("{base}.{ext}"),
            kind: first.kind.into(),
            source: first.source.clone(),
            format: format_label(&ext),
            path: first.path.to_string_lossy().to_string(),
            size,
            files: n as u64,
            modified,
            accessed: (accessed > 0).then_some(accessed),
            deletable: true,
            note: if n == expected { format!("{n} shards") } else { format!("{n} of {expected} shards present") },
        });
    }
    out
}

// ---------------------------------------------------------------- scan

pub fn scan(app: &AppHandle, extra_roots: &[String]) -> Vec<Model> {
    CANCEL.store(false, Ordering::Relaxed);
    let found = |m: &Model| {
        let _ = app.emit("model-found", m);
    };
    let progress = |p: &Path| {
        let _ = app.emit("model-walk", p.to_string_lossy());
    };
    scan_with(extra_roots, &found, &progress)
}

fn scan_with(extra_roots: &[String], found: &(dyn Fn(&Model) + Sync), progress: &(dyn Fn(&Path) + Sync)) -> Vec<Model> {
    let mut out = vec![];

    progress(&ollama_dir());
    out.extend(scan_ollama(found));
    if !CANCEL.load(Ordering::Relaxed) {
        progress(&hf_hub_dir());
        out.extend(scan_hf(found));
    }
    if !CANCEL.load(Ordering::Relaxed) {
        progress(&lmstudio_roots()[0]);
        out.extend(scan_lmstudio(found));
    }

    // Everything above is listed as whole items; the file walk must not list
    // its files again, nor walk the app folders twice.
    let apps = app_roots();
    let mut covered: HashSet<String> = [ollama_dir(), hf_hub_dir()].iter().map(|p| norm(p)).collect();
    covered.extend(lmstudio_roots().iter().map(|p| norm(p)));
    covered.extend(apps.iter().map(|(p, _, _)| norm(p)));

    let mut roots: Vec<(PathBuf, Option<(&'static str, &'static str)>)> =
        apps.iter().map(|(p, k, l)| (p.clone(), Some((*k, *l)))).collect();
    let mut loose: Vec<PathBuf> = loose_roots();
    loose.extend(extra_roots.iter().filter(|s| !s.trim().is_empty()).map(PathBuf::from));
    // Drop roots inside another root so nothing is walked twice.
    let keys: Vec<String> = loose.iter().map(|p| norm(p)).collect();
    let mut kept: HashSet<String> = HashSet::new();
    for (p, k) in loose.iter().zip(&keys) {
        let inside = keys.iter().any(|o| o != k && k.starts_with(&format!("{o}/")));
        if !inside && kept.insert(k.clone()) {
            roots.push((p.clone(), None));
        }
    }

    let walk = Walk { covered, seen: AtomicU64::new(0), progress };
    let hits: Vec<Hit> = roots
        .par_iter()
        .filter(|(p, _)| is_real_dir(p))
        .flat_map(|(p, label)| walk.walk(p, 0, *label))
        .collect();
    let files = group_hits(hits);
    files.iter().for_each(found);
    out.extend(files);

    let mut ids = HashSet::new();
    out.retain(|m| ids.insert(m.id.clone()));
    out.sort_by_key(|i| std::cmp::Reverse(i.size));
    out
}

// ---------------------------------------------------------------- delete

#[derive(Debug)]
enum Target {
    Ollama { exe: PathBuf, name: String, size: u64 },
    Tree(PathBuf),
    Files(Vec<PathBuf>),
}

/// Check an id again from scratch: only things a scan would list can ever be
/// deleted through this module.
fn validate(id: &str) -> Result<Target, String> {
    let (scheme, rest) = id.split_once(':').ok_or("Unknown model id")?;
    match scheme {
        "ollama" => {
            let exe = ollama_exe().ok_or("Ollama isn't on PATH")?;
            let rows = ollama_list(&exe).ok_or("Ollama isn't answering; start it and try again")?;
            let row = rows.into_iter().find(|r| r.name == rest).ok_or("Ollama doesn't list this model")?;
            Ok(Target::Ollama { exe, name: row.name, size: row.size })
        }
        "ollama-dir" => {
            let p = PathBuf::from(rest);
            if ollama_exe().is_some() {
                return Err("Ollama is installed; remove models one by one with Ollama running".into());
            }
            if is_real_dir(&p) && same(&p, &ollama_dir()) {
                Ok(Target::Tree(p))
            } else {
                Err("Not the Ollama models folder".into())
            }
        }
        "hf" => {
            let p = PathBuf::from(rest);
            let ok = is_real_dir(&p)
                && p.file_name().and_then(|n| n.to_str()).and_then(parse_hf_dir).is_some()
                && p.parent().is_some_and(|parent| same(parent, &hf_hub_dir()));
            if ok {
                Ok(Target::Tree(p))
            } else {
                Err("Not a Hugging Face hub repo folder".into())
            }
        }
        "lms" => {
            let p = PathBuf::from(rest);
            let ok = is_real_dir(&p)
                && p.parent().and_then(Path::parent).is_some_and(|gp| lmstudio_roots().iter().any(|r| same(gp, r)));
            if ok {
                Ok(Target::Tree(p))
            } else {
                Err("Not an LM Studio model folder".into())
            }
        }
        "pkg" => {
            let p = PathBuf::from(rest);
            let named = p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().ends_with(".mlpackage"));
            if named && is_real_dir(&p) && dir_size(&p).0 >= 50 * MB {
                Ok(Target::Tree(p))
            } else {
                Err("Not a Core ML package".into())
            }
        }
        "file" => {
            let p = PathBuf::from(rest);
            let meta = std::fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if meta.is_file() && is_model_file(&name, meta.len()) {
                Ok(Target::Files(vec![p]))
            } else {
                Err("Not a model file".into())
            }
        }
        "shards" => {
            let mut it = rest.rsplitn(4, '|');
            let (Some(ext), Some(total), Some(base), Some(dir)) = (it.next(), it.next(), it.next(), it.next()) else {
                return Err("Unknown model id".into());
            };
            let want = (base.to_string(), total.to_string(), ext.to_string());
            let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
            let mut files = vec![];
            let mut size = 0;
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if shard_parts(&name).as_ref() == Some(&want) && e.file_type().is_ok_and(|t| t.is_file()) {
                    size += e.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push(e.path());
                }
            }
            if !files.is_empty() && size >= min_size(ext).unwrap_or(u64::MAX) {
                Ok(Target::Files(files))
            } else {
                Err("No model shards found".into())
            }
        }
        _ => Err("Unknown model id".into()),
    }
}

fn remove_file(p: &Path) -> std::io::Result<()> {
    std::fs::remove_file(p).or_else(|first| {
        let meta = std::fs::symlink_metadata(p)?;
        let mut perm = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        let _ = std::fs::set_permissions(p, perm);
        std::fs::remove_file(p).map_err(|_| first)
    })
}

fn delete_one(id: &str) -> DeleteEvent {
    let fail = |freed, e: String| DeleteEvent { id: id.to_string(), ok: false, freed, error: Some(e) };
    match validate(id) {
        Err(e) => fail(0, e),
        Ok(Target::Ollama { exe, name, size }) => {
            if util::run_ok(&exe, &["rm", &name], Duration::from_secs(120)) {
                DeleteEvent { id: id.into(), ok: true, freed: size, error: None }
            } else {
                fail(0, format!("ollama rm {name} failed"))
            }
        }
        Ok(Target::Tree(p)) => {
            let (size, _) = dir_size(&p);
            match remove_tree(&p) {
                Ok(()) => DeleteEvent { id: id.into(), ok: true, freed: size, error: None },
                Err(e) => fail(size.saturating_sub(dir_size(&p).0), e.to_string()),
            }
        }
        Ok(Target::Files(files)) => {
            let mut freed = 0;
            let mut err = None;
            for f in files {
                let size = std::fs::symlink_metadata(&f).map(|m| m.len()).unwrap_or(0);
                match remove_file(&f) {
                    Ok(()) => freed += size,
                    Err(e) => err = Some(e.to_string()),
                }
            }
            match err {
                None => DeleteEvent { id: id.into(), ok: true, freed, error: None },
                Some(e) => fail(freed, e),
            }
        }
    }
}

pub fn delete(app: &AppHandle, ids: &[String]) -> Vec<DeleteEvent> {
    ids.par_iter()
        .map(|id| {
            let ev = delete_one(id);
            let _ = app.emit("model-deleted", &ev);
            ev
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dc-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A sparse file of the given size, so tests don't write gigabytes.
    fn sized(p: &Path, size: u64) {
        let f = std::fs::File::create(p).unwrap();
        f.set_len(size).unwrap();
    }

    #[test]
    fn parses_hf_dir_names() {
        assert_eq!(parse_hf_dir("models--meta-llama--Llama-3.1-8B"), Some(("models", "meta-llama/Llama-3.1-8B".into())));
        assert_eq!(parse_hf_dir("datasets--squad--plain_text"), Some(("datasets", "squad/plain_text".into())));
        assert_eq!(parse_hf_dir("models--gpt2"), Some(("models", "gpt2".into())));
        assert_eq!(parse_hf_dir(".locks"), None);
        assert_eq!(parse_hf_dir("models--"), None);
    }

    #[test]
    fn filters_by_extension_and_size() {
        assert!(is_model_file("llama.Q4_K_M.gguf", 4_000 * MB));
        assert!(is_model_file("sd_xl.SAFETENSORS", 60 * MB));
        assert!(!is_model_file("tiny.gguf", 10 * MB));
        assert!(!is_model_file("notes.txt", 5_000 * MB));
        assert!(!is_model_file("firmware.bin", 100 * MB));
        assert!(is_model_file("pytorch_model.bin", 300 * MB));
        assert!(!is_model_file("weights.npz", 150 * MB));
        assert!(!is_model_file("noext", 5_000 * MB));
    }

    #[test]
    fn groups_shards_into_one_item() {
        assert_eq!(
            shard_parts("model-00001-of-00004.safetensors"),
            Some(("model".into(), "00004".into(), "safetensors".into()))
        );
        assert_eq!(shard_parts("Llama-3-70B-Q4_K_M-00002-of-00002.gguf").map(|s| s.0), Some("Llama-3-70B-Q4_K_M".into()));
        assert_eq!(shard_parts("model.safetensors"), None);
        assert_eq!(shard_parts("part-00001-of-00004.txt"), None);

        let d = tmp("shards");
        let mk = |name: &str, size| {
            let p = d.join(name);
            sized(&p, size);
            Hit { path: p, size, pkg: false, kind: "loose", source: "Loose file".into() }
        };
        let hits = vec![
            mk("model-00001-of-00003.safetensors", 30 * MB),
            mk("model-00002-of-00003.safetensors", 30 * MB),
            mk("model-00003-of-00003.safetensors", 10 * MB),
            mk("solo.gguf", 80 * MB),
            mk("small.gguf", MB),
        ];
        let models = group_hits(hits);
        assert_eq!(models.len(), 2, "{models:?}");
        let sh = models.iter().find(|m| m.id.starts_with("shards:")).unwrap();
        assert_eq!(sh.name, "model.safetensors");
        assert_eq!(sh.files, 3);
        assert_eq!(sh.size, 70 * MB);
        assert_eq!(sh.note, "3 shards");
        // The id validates back to exactly those three files.
        match validate(&sh.id).unwrap() {
            Target::Files(f) => assert_eq!(f.len(), 3),
            t => panic!("{t:?}"),
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn parses_ollama_list() {
        let now = 1_000_000_000;
        let text = "NAME                       ID              SIZE      MODIFIED\n\
                    llama3.1:8b                46e0c10c039e    4.9 GB    3 weeks ago\n\
                    qwen2.5-coder:1.5b         6d3abb8d2d53    986 MB    About an hour ago\n\
                    nomic-embed-text:latest    0a109f422b47    274 MB    Less than a second ago\n";
        let rows = parse_ollama_list(text, now);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "llama3.1:8b");
        assert_eq!(rows[0].size, 4_900_000_000);
        assert_eq!(rows[0].modified, now - 21 * 86_400);
        assert_eq!(rows[1].size, 986_000_000);
        assert_eq!(rows[1].modified, now - 3600);
        assert_eq!(rows[2].modified, now);
        assert!(parse_ollama_list("Error: could not connect to ollama app", now).is_empty());
    }

    #[test]
    fn labels_files_by_app() {
        let p = Path::new("/x/ComfyUI/models/checkpoints/sdxl.safetensors");
        assert_eq!(app_label(p), Some(("comfyui", "ComfyUI · checkpoints".into())));
        let p = Path::new("/x/stable-diffusion-webui/models/Lora/a.safetensors");
        assert_eq!(app_label(p), Some(("a1111", "Stable Diffusion WebUI · Lora".into())));
        assert_eq!(app_label(Path::new("/x/Downloads/llama.gguf")), None);
    }

    #[test]
    fn delete_refuses_non_model_paths() {
        let d = tmp("refuse");
        let txt = d.join("notes.txt");
        sized(&txt, 300 * MB);
        let small = d.join("tiny.gguf");
        sized(&small, MB);
        let folder = d.join("models--org--name");
        std::fs::create_dir_all(&folder).unwrap();

        assert!(validate(&format!("file:{}", txt.display())).is_err());
        assert!(validate(&format!("file:{}", small.display())).is_err());
        assert!(validate(&format!("file:{}", d.display())).is_err());
        // An HF-looking folder outside the hub cache is refused.
        assert!(validate(&format!("hf:{}", folder.display())).is_err());
        assert!(validate(&format!("lms:{}", d.display())).is_err());
        assert!(validate(&format!("pkg:{}", d.display())).is_err());
        assert!(validate(&format!("ollama-dir:{}", d.display())).is_err());
        assert!(validate("rm -rf:/").is_err());
        assert!(validate("nothing").is_err());

        let ev = delete_one(&format!("file:{}", txt.display()));
        assert!(!ev.ok);
        assert!(txt.exists());

        let big = d.join("real.gguf");
        sized(&big, 60 * MB);
        let ev = delete_one(&format!("file:{}", big.display()));
        assert!(ev.ok, "{:?}", ev.error);
        assert!(!big.exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
