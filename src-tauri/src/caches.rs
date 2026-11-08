//! Package-manager and tool caches: where they live, what they are, how big
//! they are, and how to clean them. A tool's own clean command is preferred;
//! deleting the folder is the fallback.

use crate::util::{self, dir_size, home};
use rayon::prelude::*;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
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

/// Sub-folders of `dir` whose names pass `keep`, each joined with `tail`.
pub(crate) fn children(dir: &std::path::Path, keep: impl Fn(&str) -> bool, tail: &[&str]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .flat_map(|e| tail.iter().map(move |t| e.path().join(t)).collect::<Vec<_>>())
        .collect()
}

#[derive(Clone)]
pub(crate) enum How {
    /// Run the tool's own command; delete the folders if it is missing or fails.
    Tool { tool: &'static str, args: &'static [&'static str] },
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
    Admin { days: Option<u32>, pre: &'static str, cmd: &'static str, post: &'static str },
    /// A command that measures nothing up front (qlmanage, flatpak).
    Command { tool: &'static str, args: &'static [&'static str] },
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
/// below are only a starting point.
fn reported(id: &str, drives: Option<&[String]>) -> Vec<PathBuf> {
    let ask: &[(&str, &[&str])] = match id {
        "npm" => &[("npm", &["config", "get", "cache"])],
        "pnpm" => &[("pnpm", &["store", "path"])],
        "yarn" => &[("yarn", &["cache", "dir"])],
        "bun" => &[("bun", &["pm", "cache"])],
        "pip" => &[("pip", &["cache", "dir"])],
        "uv" => &[("uv", &["cache", "dir"])],
        "go-build" => &[("go", &["env", "GOCACHE"])],
        "go-mod" => &[("go", &["env", "GOMODCACHE"])],
        _ => &[],
    };
    let mut out = vec![];
    for (tool, args) in ask {
        let Some(exe) = which(tool) else { continue };
        let Some(text) = util::run_capture(&exe, args, Duration::from_secs(8)) else { continue };
        let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        let p = PathBuf::from(line.trim_end_matches(['/', '\\']));
        if p.is_absolute() && p.is_dir() {
            // pnpm and npm print the versioned folder (store\v10, _cacache's
            // parent); keep pnpm's store root so every version is counted.
            let p = if id == "pnpm" && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('v')) {
                p.parent().map(PathBuf::from).unwrap_or(p)
            } else {
                p
            };
            out.push(p);
        }
    }
    // pnpm keeps one store per drive (hard links cannot cross drives), at the
    // drive root: T:\.pnpm-store. Look on every drive.
    if id == "pnpm" && cfg!(windows) && util::fake_home().is_none() {
        for d in sysinfo::Disks::new_with_refreshed_list().list() {
            let mount = d.mount_point().to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase();
            if drives.is_some_and(|ds| !ds.iter().any(|x| x.trim_end_matches(['\\', '/']).to_ascii_lowercase() == mount)) {
                continue;
            }
            let p = d.mount_point().join(".pnpm-store");
            if p.is_dir() {
                out.push(p);
            }
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

/// `drives` limits per-drive locations (pnpm stores) to the drives chosen in
/// Settings; None means every drive.
/// The drives chosen in Settings for the last scan. A clean uses the same
/// set, so it never reaches a drive the scan didn't show.
static LAST_DRIVES: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

pub(crate) fn chosen_drives() -> Option<Vec<String>> {
    LAST_DRIVES.lock().ok().and_then(|d| d.clone())
}

/// `walk` re-walks the chosen drives for per-drive junk; without it the last
/// walk's results are reused (a clean right after a scan).
fn targets(drives: Option<&[String]>) -> Vec<Target> {
    targets_with(drives, false)
}

fn targets_with(drives: Option<&[String]>, walk: bool) -> Vec<Target> {
    let remembered = chosen_drives();
    let drives = drives.or(remembered.as_deref());
    let mut t = base_targets();
    for x in t.iter_mut().filter(|x| matches!(x.id, "temp" | "crashdumps")) {
        x.junk = true;
    }
    t.extend(crate::junk::targets(drives, walk));
    if util::fake_home().is_none() {
        let extra: Vec<Vec<PathBuf>> = t.par_iter().map(|x| reported(x.id, drives)).collect();
        for (x, e) in t.iter_mut().zip(extra) {
            x.paths.extend(e);
            x.paths = dedupe(std::mem::take(&mut x.paths));
        }
    }
    t
}

fn base_targets() -> Vec<Target> {
    let h = home();
    let local = util::local_dir();
    let cache = util::cache_dir();
    let win = cfg!(windows);
    let pick = |w: PathBuf, m: PathBuf| if win { w } else { m };
    let tool = |tool, args| How::Tool { tool, args };
    let mut t = vec![
        Target { id: "npm", name: "npm cache", eco: "Node", risk: Risk::Safe,
            about: "Package tarballs npm keeps after every install. Nothing in your projects points here; the next install downloads what it needs.",
            paths: vec![pick(local.join("npm-cache"), h.join(".npm"))],
            junk: false, how: tool("npm", &["cache", "clean", "--force"]) },
        Target { id: "pnpm", name: "pnpm store", eco: "Node", risk: Risk::Safe,
            about: "pnpm's content-addressed store. Pruning removes packages no project links to any more.",
            // Windows %LOCALAPPDATA%\pnpm\store, Linux ~/.local/share/pnpm/store, macOS ~/Library/pnpm/store.
            paths: vec![local.join("pnpm").join("store"), h.join("Library/pnpm/store")],
            junk: false, how: tool("pnpm", &["store", "prune"]) },
        Target { id: "yarn", name: "Yarn cache", eco: "Node", risk: Risk::Safe,
            about: "Downloaded package archives for Yarn classic and Yarn Berry's global cache.",
            paths: vec![pick(local.join("Yarn").join("Cache"), cache.join("Yarn")), h.join(".yarn").join("berry").join("cache")],
            junk: false, how: tool("yarn", &["cache", "clean"]) },
        Target { id: "bun", name: "Bun install cache", eco: "Bun", risk: Risk::Safe,
            about: "Packages Bun has downloaded. Bun fetches them again on the next install.",
            paths: vec![h.join(".bun").join("install").join("cache")],
            junk: false, how: tool("bun", &["pm", "cache", "rm"]) },
        Target { id: "deno", name: "Deno cache", eco: "Deno", risk: Risk::Safe,
            about: "Remote modules and compiled code Deno has cached.",
            paths: vec![pick(local.join("deno"), cache.join("deno"))],
            junk: false, how: How::Delete },
        Target { id: "node-gyp", name: "node-gyp headers", eco: "Node", risk: Risk::Safe,
            about: "Node headers downloaded to build native addons. Fetched again when an addon is compiled.",
            paths: vec![pick(local.join("node-gyp").join("Cache"), cache.join("node-gyp"))],
            junk: false, how: How::Delete },
        Target { id: "cargo", name: "Cargo registry", eco: "Rust", risk: Risk::Safe,
            about: "Downloaded crate archives and their unpacked sources. Cargo restores what a build needs.",
            paths: vec![h.join(".cargo").join("registry").join("cache"), h.join(".cargo").join("registry").join("src")],
            junk: false, how: How::Delete },
        Target { id: "cargo-git", name: "Cargo git checkouts", eco: "Rust", risk: Risk::Safe,
            about: "Git dependencies Cargo has cloned. Cloned again on the next build that uses them.",
            paths: vec![h.join(".cargo").join("git")],
            junk: false, how: How::Delete },
        Target { id: "pip", name: "pip cache", eco: "Python", risk: Risk::Safe,
            about: "Wheels and downloads pip keeps between installs.",
            paths: vec![pick(local.join("pip").join("Cache"), cache.join("pip"))],
            junk: false, how: tool("pip", &["cache", "purge"]) },
        Target { id: "uv", name: "uv cache", eco: "Python", risk: Risk::Safe,
            about: "uv's package cache. Virtual envs that hard-link from it keep working.",
            paths: vec![pick(local.join("uv").join("cache"), h.join(".cache").join("uv"))],
            junk: false, how: tool("uv", &["cache", "clean"]) },
        Target { id: "poetry", name: "Poetry cache", eco: "Python", risk: Risk::Safe,
            about: "Packages and metadata Poetry has downloaded.",
            paths: vec![pick(local.join("pypoetry").join("Cache"), cache.join("pypoetry"))],
            junk: false, how: How::Delete },
        Target { id: "conda", name: "Conda packages", eco: "Python", risk: Risk::Slow,
            about: "Package tarballs and unused packages Conda keeps. conda clean removes only what no environment uses.",
            paths: vec![h.join("miniconda3").join("pkgs"), h.join("anaconda3").join("pkgs"), h.join("miniforge3").join("pkgs")],
            junk: false, how: tool("conda", &["clean", "--all", "-y"]) },
        Target { id: "go-build", name: "Go build cache", eco: "Go", risk: Risk::Safe,
            about: "Compiled packages Go reuses between builds. The next build is slower once.",
            paths: vec![pick(local.join("go-build"), cache.join("go-build"))],
            junk: false, how: tool("go", &["clean", "-cache"]) },
        Target { id: "go-mod", name: "Go module cache", eco: "Go", risk: Risk::Slow,
            about: "Every module version Go has downloaded. Downloaded again by the next build that needs it.",
            paths: vec![h.join("go").join("pkg").join("mod")],
            junk: false, how: tool("go", &["clean", "-modcache"]) },
        Target { id: "gradle", name: "Gradle caches", eco: "Java", risk: Risk::Slow,
            about: "Dependencies and build outputs Gradle caches. Stop Gradle daemons first or some files stay in use.",
            paths: vec![h.join(".gradle").join("caches")],
            junk: false, how: How::Delete },
        Target { id: "maven", name: "Maven repository", eco: "Java", risk: Risk::Slow,
            about: "Your local Maven repository. Anything you installed locally with mvn install is lost too.",
            paths: vec![h.join(".m2").join("repository")],
            junk: false, how: How::Delete },
        Target { id: "nuget", name: "NuGet packages", eco: ".NET", risk: Risk::Slow,
            about: "Global NuGet package folders and HTTP cache.",
            paths: vec![h.join(".nuget").join("packages")],
            junk: false, how: tool("dotnet", &["nuget", "locals", "all", "--clear"]) },
        Target { id: "playwright", name: "Playwright browsers", eco: "Testing", risk: Risk::Slow,
            about: "Browser builds Playwright downloaded for tests. npx playwright install fetches them again.",
            paths: vec![pick(local.join("ms-playwright"), cache.join("ms-playwright"))],
            junk: false, how: How::Delete },
        Target { id: "docker", name: "Docker unused data", eco: "Docker", risk: Risk::Slow,
            about: "Stopped containers, images no container uses, unused networks and build cache. Running containers and all volumes are kept; images are pulled again when needed.",
            paths: vec![],
            junk: false, how: How::Docker },
    ];
    // Editors: their own HTTP, GPU and extension-download caches. Files in use
    // while the editor runs are skipped.
    let editor_cache = |app: &str| -> Vec<PathBuf> {
        let base = util::roaming_dir().join(app);
        ["Cache", "CachedData", "CachedExtensionVSIXs", "Code Cache", "GPUCache", "logs"].iter().map(|d| base.join(d)).collect()
    };
    let jetbrains = if win {
        children(&local.join("JetBrains"), |n| n != "Toolbox" && n.chars().any(|c| c.is_ascii_digit()), &["caches", "index", "log"])
    } else {
        children(&cache.join("JetBrains"), |n| n.chars().any(|c| c.is_ascii_digit()), &["caches", "index"])
    };
    t.extend([
        Target { id: "vscode-cache", name: "VS Code caches", eco: "Editors", risk: Risk::Safe,
            about: "VS Code's web cache, compiled-code cache, downloaded extension packages and logs. Settings and extensions are untouched. Close VS Code first for the most space.",
            paths: editor_cache("Code"), junk: false, how: How::Delete },
        Target { id: "cursor-cache", name: "Cursor caches", eco: "Editors", risk: Risk::Safe,
            about: "Cursor's web cache, compiled-code cache, downloaded extension packages and logs. Settings and chats are untouched.",
            paths: editor_cache("Cursor"), junk: false, how: How::Delete },
        Target { id: "jetbrains", name: "JetBrains IDE caches", eco: "Editors", risk: Risk::Slow,
            about: "Caches, indexes and logs of every JetBrains IDE version. The IDE re-indexes on next open. Settings, plugins and Local History are kept.",
            paths: jetbrains, junk: false, how: How::Delete },
        Target { id: "cypress", name: "Cypress binaries", eco: "Testing", risk: Risk::Slow,
            about: "Cypress app binaries for every version you have installed. npm install downloads the one a project needs.",
            paths: vec![pick(local.join("Cypress").join("Cache"), cache.join("Cypress"))], junk: false, how: How::Delete },
        Target { id: "puppeteer", name: "Puppeteer browsers", eco: "Testing", risk: Risk::Slow,
            about: "Chrome for Testing builds Puppeteer downloaded. Fetched again on the next install.",
            paths: vec![h.join(".cache").join("puppeteer")], junk: false, how: How::Delete },
        Target { id: "electron", name: "Electron downloads", eco: "Desktop", risk: Risk::Slow,
            about: "Electron and electron-builder downloads (runtimes, NSIS, code-sign tools). Fetched again by the next install or build.",
            paths: vec![pick(local.join("electron").join("Cache"), cache.join("electron")), pick(local.join("electron-builder").join("Cache"), cache.join("electron-builder")), h.join(".electron")],
            junk: false, how: How::Delete },
        Target { id: "composer", name: "Composer cache", eco: "PHP", risk: Risk::Safe,
            about: "Package archives and repository metadata Composer caches.",
            paths: vec![pick(local.join("Composer"), cache.join("composer"))], junk: false, how: tool("composer", &["clear-cache"]) },
        Target { id: "pub", name: "Dart / Flutter pub cache", eco: "Dart", risk: Risk::Slow,
            about: "Every Dart and Flutter package version pub has downloaded. Also removes globally activated packages; reactivate them with dart pub global activate.",
            paths: vec![pick(local.join("Pub").join("Cache"), h.join(".pub-cache"))], junk: false, how: How::Delete },
        Target { id: "coursier", name: "Coursier / Ivy cache", eco: "Scala", risk: Risk::Slow,
            about: "Artifacts downloaded by sbt, Mill and scala-cli through Coursier and Ivy.",
            paths: vec![pick(local.join("Coursier").join("Cache"), cache.join("Coursier")), h.join(".coursier").join("cache"), h.join(".ivy2").join("cache")],
            junk: false, how: How::Delete },
        Target { id: "zig", name: "Zig global cache", eco: "Zig", risk: Risk::Safe,
            about: "Zig's global build and package cache. Rebuilt by the next zig build.",
            paths: vec![pick(local.join("zig"), cache.join("zig")), h.join(".cache").join("zig")], junk: false, how: How::Delete },
        Target { id: "vcpkg", name: "vcpkg binary cache", eco: "C/C++", risk: Risk::Slow,
            about: "Prebuilt C/C++ libraries vcpkg reuses. Without them, packages build from source again.",
            paths: vec![pick(local.join("vcpkg").join("archives"), h.join(".cache").join("vcpkg").join("archives"))], junk: false, how: How::Delete },
        Target { id: "conan", name: "Conan package cache", eco: "C/C++", risk: Risk::Slow,
            about: "Conan 2's package storage. Profiles and remotes in ~/.conan2 are kept.",
            paths: vec![h.join(".conan2").join("p")], junk: false, how: How::Delete },
        Target { id: "ccache", name: "ccache / sccache", eco: "C/C++", risk: Risk::Safe,
            about: "Compiler output caches. The next build is slower once.",
            paths: vec![h.join(".ccache"), pick(local.join("ccache"), cache.join("ccache")), pick(local.join("Mozilla").join("sccache"), cache.join("Mozilla.sccache"))],
            junk: false, how: How::Delete },
        Target { id: "rustup-tmp", name: "rustup downloads", eco: "Rust", risk: Risk::Safe,
            about: "Leftover toolchain downloads and temp files. Installed toolchains are kept.",
            paths: vec![h.join(".rustup").join("downloads"), h.join(".rustup").join("tmp")], junk: false, how: How::Delete },
        Target { id: "pre-commit", name: "pre-commit environments", eco: "Python", risk: Risk::Safe,
            about: "Hook environments pre-commit builds. Rebuilt the next time hooks run.",
            paths: vec![h.join(".cache").join("pre-commit")], junk: false, how: How::Delete },
    ]);
    t.extend(ai_targets(&h, &local, &cache, win));
    if win {
        t.push(Target { id: "scoop", name: "Scoop download cache", eco: "System", risk: Risk::Safe,
            about: "Installers Scoop downloaded. Installed apps are kept.",
            paths: vec![h.join("scoop").join("cache")], junk: false, how: How::Delete });
        t.push(Target { id: "temp", name: "Temp files", eco: "System", risk: Risk::Safe,
            about: "Your user temp folder. Only items older than a day are removed, and anything in use is skipped.",
            paths: vec![util::temp_dir()],
            junk: false, how: How::OlderThan(24) });
        t.push(Target { id: "crashdumps", name: "Crash dumps", eco: "System", risk: Risk::Safe,
            about: "Memory dumps written when apps crashed. Only useful when reporting that crash.",
            paths: vec![local.join("CrashDumps")],
            junk: false, how: How::Delete });
    } else if cfg!(target_os = "macos") {
        t.push(Target { id: "xcode", name: "Xcode DerivedData", eco: "Apple", risk: Risk::Safe,
            about: "Build products and indexes for every Xcode project. Rebuilt on the next build.",
            paths: vec![h.join("Library/Developer/Xcode/DerivedData")],
            junk: false, how: How::Delete });
        t.push(Target { id: "cocoapods", name: "CocoaPods cache", eco: "Apple", risk: Risk::Slow,
            about: "Pod specs and sources CocoaPods caches. pod install fetches them again.",
            paths: vec![cache.join("CocoaPods")], junk: false, how: tool("pod", &["cache", "clean", "--all"]) });
        t.push(Target { id: "simulator", name: "Simulator caches", eco: "Apple", risk: Risk::Safe,
            about: "Caches for iOS simulators. Your simulator devices are kept.",
            paths: vec![h.join("Library/Developer/CoreSimulator/Caches")],
            junk: false, how: How::Delete });
        t.push(Target { id: "homebrew", name: "Homebrew cache", eco: "System", risk: Risk::Safe,
            about: "Downloaded bottles and source archives. brew cleanup removes the old ones.",
            paths: vec![cache.join("Homebrew")],
            junk: false, how: tool("brew", &["cleanup", "-s", "--prune=all"]) });
    } else {
        // Linux
        t.push(Target { id: "temp", name: "Temp files", eco: "System", risk: Risk::Safe,
            about: "Your files in /tmp. Only items older than a day are removed, and anything in use is skipped.",
            paths: vec![util::temp_dir()],
            junk: false, how: How::OlderThan(24) });
        t.push(Target { id: "homebrew", name: "Homebrew cache", eco: "System", risk: Risk::Safe,
            about: "Downloaded bottles and source archives from Homebrew on Linux. brew cleanup removes the old ones.",
            paths: vec![cache.join("Homebrew")],
            junk: false, how: tool("brew", &["cleanup", "-s", "--prune=all"]) });
    }
    t
}

/// A cache tool on PATH. Under the e2e fake home, none: the real `npm cache
/// clean` would empty the real cache, so the suite exercises the delete path.
pub(crate) fn which(name: &str) -> Option<std::path::PathBuf> {
    if util::fake_home().is_some() {
        return None;
    }
    util::which(name)
}

/// AI and machine-learning caches: things rebuilt or re-downloaded on demand.
/// Downloaded models live on the AI models page instead, and the Hugging Face
/// folder is cleaned by named sub-folder only, because it also holds the
/// `token` credential.
fn ai_targets(h: &std::path::Path, local: &std::path::Path, cache: &std::path::Path, win: bool) -> Vec<Target> {
    let env_dir = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|_| util::fake_home().is_none());
    let hf = env_dir("HF_HOME").unwrap_or_else(|| h.join(".cache").join("huggingface"));
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let tmp = util::temp_dir();
    let mut gpu = vec![];
    if win {
        let nv = local.join("NVIDIA");
        let amd = local.join("AMD");
        gpu.extend([nv.join("DXCache"), nv.join("GLCache"), local.join("NVIDIA Corporation").join("NV_Cache"), util::roaming_dir().join("NVIDIA").join("ComputeCache")]);
        gpu.extend(["DxCache", "DxcCache", "VkCache", "GLCache"].iter().map(|d| amd.join(d)));
        gpu.push(local.join("D3DSCache"));
        if let Some(parent) = local.parent() {
            gpu.push(parent.join("LocalLow").join("Intel").join("ShaderCache"));
        }
    }
    vec![
        Target { id: "hf-datasets", name: "Hugging Face datasets cache", eco: "AI / ML", risk: Risk::Slow,
            about: "Arrow files and map() results from load_dataset. Rebuilt from the Hub the next time a script loads the dataset. Your token and models are not touched.",
            paths: vec![env_dir("HF_DATASETS_CACHE").unwrap_or_else(|| hf.join("datasets"))], junk: false, how: How::Delete },
        Target { id: "hf-misc", name: "Hugging Face assets & legacy cache", eco: "AI / ML", risk: Risk::Safe,
            about: "Library assets, remote-code modules, the xet shard cache and the pre-2022 transformers cache that newer versions no longer read.",
            paths: vec![hf.join("assets"), hf.join("modules"), hf.join("xet"), hf.join("transformers")], junk: false, how: How::Delete },
        Target { id: "torch-compile", name: "torch.compile / Triton / vLLM caches", eco: "AI / ML", risk: Risk::Safe,
            about: "Kernels compiled by torch.compile (Inductor), Triton, torch C++ extensions (DeepSpeed ops) and vLLM. Recompiled on the next run; only the first run is slower.",
            paths: vec![
                env_dir("TORCHINDUCTOR_CACHE_DIR").unwrap_or_else(|| tmp.join(format!("torchinductor_{user}"))),
                env_dir("TRITON_CACHE_DIR").unwrap_or_else(|| h.join(".triton").join("cache")),
                if win { local.join("torch_extensions") } else { cache.join("torch_extensions") },
                h.join(".cache").join("torch_extensions"),
                env_dir("VLLM_CACHE_ROOT").unwrap_or_else(|| h.join(".cache").join("vllm")).join("torch_compile_cache"),
            ],
            junk: false, how: How::Delete },
        Target { id: "torch-hub", name: "PyTorch hub & checkpoints", eco: "AI / ML", risk: Risk::Slow,
            about: "Pretrained weights fetched by torch.hub, torchvision and older timm. Downloaded again on first use.",
            paths: vec![env_dir("TORCH_HOME").unwrap_or_else(|| h.join(".cache").join("torch")).join("hub")], junk: false, how: How::Delete },
        Target { id: "keras", name: "Keras / TF Hub downloads", eco: "AI / ML", risk: Risk::Slow,
            about: "Datasets and weights from keras.utils.get_file and keras.applications, and TensorFlow Hub modules. Downloaded again on demand.",
            paths: vec![h.join(".keras").join("datasets"), h.join(".keras").join("models"), env_dir("TFHUB_CACHE_DIR").unwrap_or_else(|| tmp.join("tfhub_modules"))],
            junk: false, how: How::Delete },
        Target { id: "tfds-downloads", name: "TensorFlow Datasets downloads", eco: "AI / ML", risk: Risk::Slow,
            about: "Raw archives TFDS downloaded before preparing datasets. Prepared datasets and files you placed in downloads/manual are kept.",
            paths: vec![h.join("tensorflow_datasets").join("downloads").join("extracted")], junk: false, how: How::Delete },
        Target { id: "wandb-cache", name: "Weights & Biases cache", eco: "AI / ML", risk: Risk::Safe,
            about: "Artifact and media cache W&B keeps locally. Runs on wandb.ai and local run folders are not touched.",
            paths: vec![env_dir("WANDB_CACHE_DIR").unwrap_or_else(|| h.join(".cache").join("wandb"))], junk: false, how: How::Delete },
        Target { id: "gradio", name: "Gradio temp files", eco: "AI / ML", risk: Risk::Safe,
            about: "Uploads and outputs Gradio apps wrote to temp. Only items older than a day are removed.",
            paths: vec![env_dir("GRADIO_TEMP_DIR").unwrap_or_else(|| tmp.join("gradio"))], junk: false, how: How::OlderThan(24) },
        Target { id: "gpu-shaders", name: "GPU shader caches", eco: "System", risk: Risk::Safe,
            about: "Compiled shaders and CUDA kernels cached by NVIDIA, AMD, Intel and DirectX. Rebuilt as apps run; the first launch after is a little slower. Close games and GPU apps first.",
            paths: gpu, junk: false, how: How::Delete },
    ]
}

fn method(how: &How) -> String {
    match how {
        How::Tool { tool, args } => {
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
    items
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanEvent {
    pub id: String,
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
        let before: Vec<u64> = admin.iter().map(|t| measure(t).size).collect();
        for t in &admin {
            emit(&CleanEvent { id: t.id.into(), phase: "running".into(), freed: 0, skipped: 0, note: "Waiting for admin approval".into(), ..Default::default() });
        }
        let ok = crate::junk::run_admin(&admin);
        for (t, b) in admin.iter().zip(before) {
            let freed = b.saturating_sub(measure(t).size);
            let note = if ok { "Cleaned as admin" } else { "Admin approval was cancelled or failed" };
            let ev = CleanEvent { id: t.id.into(), phase: if ok { "done" } else { "failed" }.into(), freed, skipped: 0, note: note.into(), ..Default::default() };
            emit(&ev);
            out.push(ev);
        }
    }

    for t in picked.into_iter().filter(|t| !matches!(t.how, How::Admin { .. })) {
        let id = t.id.to_string();
        emit(&CleanEvent { id: id.clone(), phase: "running".into(), freed: 0, skipped: 0, note: method(&t.how), ..Default::default() });
        let before = measure(t).size;
        let mut cleared = Cleared::default();
        let mut note: String;
        match &t.how {
            How::Tool { tool, args } => {
                let ran = which(tool).is_some_and(|p| {
                    if t.id == "pnpm" {
                        // One prune per store; each drive has its own.
                        let stores: Vec<&PathBuf> = t.paths.iter().filter(|s| s.is_dir()).collect();
                        !stores.is_empty()
                            && stores.iter().all(|s| {
                                let dir = s.to_string_lossy().to_string();
                                util::run_ok(&p, &["store", "prune", "--store-dir", &dir], Duration::from_secs(600))
                            })
                    } else {
                        util::run_ok(&p, args, Duration::from_secs(600))
                    }
                });
                if ran {
                    note = format!("Ran {tool} {}", args.join(" "));
                } else {
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
        let after = measure(t).size;
        let freed = before.saturating_sub(after);
        let (kept, in_use, in_use_sample, held_by) = explain(cleared);
        if in_use > 0 {
            note = format!("{note} · {in_use} in use, left alone");
        }
        let ev = CleanEvent { id, phase: "done".into(), freed, skipped: kept + in_use, note, kept, in_use, in_use_sample, held_by };
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
}
