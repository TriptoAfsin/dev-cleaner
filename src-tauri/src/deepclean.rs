//! System junk › Deep clean: things Windows keeps forever. Superseded driver
//! packages in the driver store, Windows Installer files no installed product
//! uses, an oversized pagefile, and the component store. Everything chosen
//! runs in ONE elevated script, so one UAC prompt.
//!
//! Safety rules:
//! - Drivers are deleted with `pnputil /delete-driver` and never `/force`, so
//!   a package a device still uses is refused by Windows itself. The newest
//!   version of each driver is never offered.
//! - Installer files are moved to a quarantine folder, never deleted, and
//!   only when the registry read that lists the files still in use worked.
//! - The pagefile is resized, never removed.
//! - Under the e2e fake home nothing real is read or run.

use crate::util::{self, run_capture, which};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type R<T> = Result<T, String>;

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;
const QUARANTINE_DAYS: u64 = 30;

/// Run a PowerShell script and return its output.
pub(crate) fn powershell(script: &str, secs: u64) -> Option<String> {
    let ps = which("powershell")?;
    run_capture(&ps, &["-NoProfile", "-NonInteractive", "-Command", script], Duration::from_secs(secs))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn system_root() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
}

fn system_drive() -> String {
    std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
}

// ---------------------------------------------------------------- types

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DriverPkg {
    /// oemNN.inf, what pnputil deletes.
    pub published: String,
    pub version: String,
    /// mm/dd/yyyy as the INF writes it.
    pub date: String,
    pub size: u64,
    /// The newest of its group; never offered.
    pub keep: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DriverGroup {
    pub key: String,
    pub original: String,
    pub provider: String,
    pub class: String,
    /// Newest first.
    pub versions: Vec<DriverPkg>,
    /// Bytes the older versions take.
    pub reclaim: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstallerFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallerInfo {
    /// The registry list of files in use was read. Without it nothing is offered.
    pub checked: bool,
    pub total_files: u64,
    pub total_size: u64,
    pub orphans: Vec<InstallerFile>,
    pub orphan_size: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PageFileStat {
    pub path: String,
    pub allocated: u64,
    pub current: u64,
    pub peak: u64,
    /// Initial and maximum size in bytes; 0/0 = Windows picks the size.
    pub initial: u64,
    pub maximum: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PageOption {
    /// "fixed" or "system".
    pub mode: String,
    pub gb: u32,
    pub label: String,
    pub detail: String,
    /// Bytes freed on the system drive after the restart.
    pub saves: u64,
    pub recommended: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PagefileInfo {
    pub files: Vec<PageFileStat>,
    /// Windows manages every pagefile (the "Automatically manage" box).
    pub automatic: bool,
    pub ram: u64,
    pub commit_peak: u64,
    /// The pagefile on the system drive, if any.
    pub system: Option<PageFileStat>,
    pub oversized: bool,
    pub reason: String,
    pub options: Vec<PageOption>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineBatch {
    pub id: String,
    pub created: u64,
    pub files: u64,
    pub size: u64,
    pub path: String,
    /// Older than 30 days: offered for deletion.
    pub old: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeepScan {
    /// Deep clean exists on Windows only.
    pub supported: bool,
    /// Test sandbox: nothing real was read.
    pub dry_run: bool,
    pub drivers: Vec<DriverGroup>,
    pub drivers_read: bool,
    pub installer: InstallerInfo,
    pub pagefile: PagefileInfo,
    pub quarantine: Vec<QuarantineBatch>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageChange {
    pub mode: String,
    #[serde(default)]
    pub gb: u32,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeepPlan {
    #[serde(default)]
    pub drivers: Vec<String>,
    #[serde(default)]
    pub installers: Vec<String>,
    #[serde(default)]
    pub pagefile: Option<PageChange>,
    #[serde(default)]
    pub component: bool,
    #[serde(default)]
    pub reset_base: bool,
    #[serde(default)]
    pub delete_quarantine: Vec<String>,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeepResult {
    /// The elevated script ran (false = cancelled at the prompt or failed to start).
    pub ran: bool,
    pub dry_run: bool,
    pub drivers_removed: Vec<String>,
    /// Asked for but still there: Windows refused (a device uses it).
    pub drivers_refused: Vec<String>,
    pub freed: u64,
    pub quarantined: u64,
    pub quarantine_id: Option<String>,
    pub pagefile_set: bool,
    pub restart_needed: bool,
    pub notes: Vec<String>,
    pub scan: DeepScan,
}

// ---------------------------------------------------------------- drivers

/// One `pnputil /enum-drivers` entry, read without trusting the labels (they
/// are translated): the published name is the oemNN.inf value, the original
/// name the other .inf value, provider and class the 3rd and 4th values, and
/// the version the value that looks like "date version".
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PnpEntry {
    pub published: String,
    pub original: String,
    pub provider: String,
    pub class: String,
    pub version: String,
    pub date: String,
}

fn is_oem(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    l.starts_with("oem") && l.ends_with(".inf") && l[3..l.len() - 4].chars().all(|c| c.is_ascii_digit()) && l.len() > 7
}

/// "04/20/2022 22.20.0.0" → (date, version). Any date separator, any order.
fn split_date_version(v: &str) -> Option<(String, String)> {
    let mut parts = v.split_whitespace();
    let date = parts.next()?;
    let ver = parts.next()?;
    let date_ok = date.chars().filter(|c| c.is_ascii_digit()).count() >= 6 && date.chars().any(|c| !c.is_ascii_digit());
    let ver_ok = ver.contains('.') && ver.chars().all(|c| c.is_ascii_digit() || c == '.');
    (date_ok && ver_ok).then(|| (date.to_string(), ver.to_string()))
}

pub(crate) fn parse_pnputil(text: &str) -> Vec<PnpEntry> {
    let mut out = vec![];
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let values: Vec<&str> = block
            .lines()
            .filter_map(|l| l.split_once(':').map(|(_, v)| v.trim()))
            .collect();
        let Some(pi) = values.iter().position(|v| is_oem(v)) else { continue };
        let rest = &values[pi + 1..];
        let Some(oi) = rest.iter().position(|v| v.to_ascii_lowercase().ends_with(".inf")) else { continue };
        let Some((date, version)) = values.iter().find_map(|v| split_date_version(v)) else { continue };
        let after = &rest[oi + 1..];
        out.push(PnpEntry {
            published: values[pi].to_ascii_lowercase(),
            original: rest[oi].to_ascii_lowercase(),
            provider: after.first().map(|s| s.to_string()).unwrap_or_default(),
            class: after.get(1).map(|s| s.to_string()).unwrap_or_default(),
            version,
            date,
        });
    }
    out
}

/// `DriverVer = 12/26/2024,1.0.0.3` from an INF's text (UTF-8 or UTF-16).
pub(crate) fn inf_driver_ver(text: &str) -> Option<(String, String)> {
    for line in text.lines() {
        let l = line.trim();
        let Some((k, v)) = l.split_once('=') else { continue };
        if !k.trim().eq_ignore_ascii_case("DriverVer") {
            continue;
        }
        let v = v.split(';').next().unwrap_or("").trim();
        let (d, ver) = v.split_once(',')?;
        return Some((d.trim().to_string(), ver.trim().to_string()));
    }
    None
}

/// Sort key: version numbers, then the INF date (mm/dd/yyyy) as yyyymmdd.
fn ver_key(version: &str, date: &str) -> (Vec<u64>, u64) {
    let v = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let n: Vec<u64> = date.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    let d = match n.as_slice() {
        [m, d, y] if *y > 999 => y * 10_000 + m * 100 + d,
        [y, m, d] if *y > 999 => y * 10_000 + m * 100 + d,
        _ => 0,
    };
    (v, d)
}

fn read_text(p: &Path) -> Option<String> {
    std::fs::read(p).ok().map(|b| util::decode(&b))
}

fn scan_drivers() -> (bool, Vec<DriverGroup>) {
    let Some(pnp) = which("pnputil") else { return (false, vec![]) };
    let Some(text) = run_capture(&pnp, &["/enum-drivers"], Duration::from_secs(60)) else { return (false, vec![]) };
    let entries = parse_pnputil(&text);
    if entries.is_empty() {
        return (false, vec![]);
    }
    let root = system_root();
    let inf_dir = root.join("INF");
    let repo = root.join("System32").join("DriverStore").join("FileRepository");
    // Repository folders by original INF name.
    let mut folders: HashMap<String, Vec<PathBuf>> = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(&repo) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if let Some(idx) = name.find(".inf_") {
                folders.entry(name[..idx + 4].to_string()).or_default().push(e.path());
            }
        }
    }
    let mut groups: HashMap<String, DriverGroup> = HashMap::new();
    for e in &entries {
        let key = format!("{}|{}|{}", e.original, e.provider.to_ascii_lowercase(), e.class.to_ascii_lowercase());
        groups.entry(key.clone()).or_insert_with(|| DriverGroup {
            key,
            original: e.original.clone(),
            provider: e.provider.clone(),
            class: e.class.clone(),
            versions: vec![],
            reclaim: 0,
        });
    }
    // Only groups with more than one version matter; size those.
    let mut used_folders: HashSet<PathBuf> = HashSet::new();
    for e in &entries {
        let key = format!("{}|{}|{}", e.original, e.provider.to_ascii_lowercase(), e.class.to_ascii_lowercase());
        let multi = entries.iter().filter(|x| x.original == e.original && x.provider == e.provider && x.class == e.class).count() > 1;
        let oem_bytes = std::fs::read(inf_dir.join(&e.published)).ok();
        let (date, version) = oem_bytes
            .as_ref()
            .and_then(|b| inf_driver_ver(&util::decode(b)))
            .unwrap_or_else(|| (e.date.clone(), e.version.clone()));
        let mut size = 0;
        if multi {
            // The repository folder whose INF is byte-for-byte the oemNN.inf.
            if let (Some(bytes), Some(cands)) = (oem_bytes.as_ref(), folders.get(&e.original)) {
                let hit = cands.iter().find(|f| !used_folders.contains(*f) && std::fs::read(f.join(&e.original)).ok().as_ref() == Some(bytes));
                let hit = hit.or_else(|| {
                    cands.iter().find(|f| {
                        !used_folders.contains(*f)
                            && read_text(&f.join(&e.original)).and_then(|t| inf_driver_ver(&t)) == Some((date.clone(), version.clone()))
                    })
                });
                if let Some(f) = hit {
                    used_folders.insert(f.clone());
                    size = util::dir_size(f).0;
                }
            }
        }
        if let Some(g) = groups.get_mut(&key) {
            g.versions.push(DriverPkg { published: e.published.clone(), version, date, size, keep: false });
        }
    }
    let mut out: Vec<DriverGroup> = groups
        .into_values()
        .filter(|g| g.versions.len() > 1)
        .map(|mut g| {
            g.versions.sort_by_key(|v| std::cmp::Reverse(ver_key(&v.version, &v.date)));
            g.versions[0].keep = true;
            g.reclaim = g.versions.iter().filter(|v| !v.keep).map(|v| v.size).sum();
            g
        })
        .collect();
    out.sort_by(|a, b| b.reclaim.cmp(&a.reclaim).then(a.original.cmp(&b.original)));
    (true, out)
}

// ---------------------------------------------------------------- installer

/// LocalPackage paths from `reg query … /s /v LocalPackage` output, lowercased.
pub(crate) fn parse_local_packages(text: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("LocalPackage")?.trim_start();
            let rest = rest.strip_prefix("REG_SZ")?.trim();
            (!rest.is_empty()).then(|| rest.to_ascii_lowercase())
        })
        .collect()
}

/// Installer files not named by any LocalPackage.
pub(crate) fn orphans_of(files: &[InstallerFile], used: &HashSet<String>) -> Vec<InstallerFile> {
    files.iter().filter(|f| !used.contains(&f.path.to_ascii_lowercase())).cloned().collect()
}

fn scan_installer() -> InstallerInfo {
    let dir = system_root().join("Installer");
    let mut files = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let l = name.to_ascii_lowercase();
            if !(l.ends_with(".msi") || l.ends_with(".msp")) {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            if !m.is_file() {
                continue;
            }
            let modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            files.push(InstallerFile { path: e.path().to_string_lossy().to_string(), name, size: m.len(), modified });
        }
    }
    let total_files = files.len() as u64;
    let total_size = files.iter().map(|f| f.size).sum();
    let reg = which("reg");
    let text = reg.and_then(|r| {
        run_capture(&r, &["query", r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData", "/s", "/v", "LocalPackage"], Duration::from_secs(60))
    });
    let used = text.map(|t| parse_local_packages(&t)).unwrap_or_default();
    // Conservative: no registry list, no orphans. A machine with installer
    // files always has products registered.
    if used.is_empty() {
        return InstallerInfo { checked: false, total_files, total_size, orphans: vec![], orphan_size: 0 };
    }
    let mut orphans = orphans_of(&files, &used);
    orphans.sort_by_key(|f| std::cmp::Reverse(f.size));
    let orphan_size = orphans.iter().map(|f| f.size).sum();
    InstallerInfo { checked: true, total_files, total_size, orphans, orphan_size }
}

// ---------------------------------------------------------------- quarantine

fn quarantine_root() -> PathBuf {
    PathBuf::from(format!("{}\\", system_drive())).join("DevCleaner Quarantine").join("Installer")
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    created: u64,
    files: Vec<ManifestFile>,
}

#[derive(Serialize, Deserialize)]
struct ManifestFile {
    from: String,
    name: String,
    size: u64,
}

fn list_quarantine() -> Vec<QuarantineBatch> {
    let root = quarantine_root();
    let Ok(rd) = std::fs::read_dir(&root) else { return vec![] };
    let now = now_secs();
    let mut v: Vec<QuarantineBatch> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            let m: Manifest = serde_json::from_str(&read_text(&e.path().join("manifest.json"))?).ok()?;
            let present: Vec<&ManifestFile> = m.files.iter().filter(|f| e.path().join(&f.name).is_file()).collect();
            Some(QuarantineBatch {
                id,
                created: m.created,
                files: present.len() as u64,
                size: present.iter().map(|f| f.size).sum(),
                path: e.path().to_string_lossy().to_string(),
                old: now.saturating_sub(m.created) > QUARANTINE_DAYS * 86_400,
            })
        })
        .filter(|b| b.files > 0)
        .collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.created));
    v
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------- pagefile

const PAGE_PROBE: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$u = @(Get-CimInstance Win32_PageFileUsage | ForEach-Object { @{ path = $_.Name; a = [int64]$_.AllocatedBaseSize; c = [int64]$_.CurrentUsage; p = [int64]$_.PeakUsage } })
$s = @(Get-CimInstance Win32_PageFileSetting | ForEach-Object { @{ path = $_.Name; i = [int64]$_.InitialSize; m = [int64]$_.MaximumSize } })
$auto = (Get-CimInstance Win32_ComputerSystem).AutomaticManagedPagefile
@{ u = $u; s = $s; auto = [bool]$auto } | ConvertTo-Json -Compress -Depth 4
"#;

#[cfg(windows)]
fn commit_peak_and_ram() -> (u64, u64) {
    use windows_sys::Win32::System::ProcessStatus::{K32GetPerformanceInfo, PERFORMANCE_INFORMATION};
    // SAFETY: a zeroed struct of the right size, filled by the call.
    unsafe {
        let mut pi: PERFORMANCE_INFORMATION = std::mem::zeroed();
        pi.cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
        if K32GetPerformanceInfo(&mut pi, pi.cb) == 0 {
            return (0, 0);
        }
        let page = pi.PageSize as u64;
        (pi.CommitPeak as u64 * page, pi.PhysicalTotal as u64 * page)
    }
}

#[cfg(not(windows))]
fn commit_peak_and_ram() -> (u64, u64) {
    (0, 0)
}

pub(crate) fn parse_page_probe(json: &str, ram: u64, commit_peak: u64) -> PagefileInfo {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return PagefileInfo::default() };
    let settings: HashMap<String, (u64, u64)> = v["s"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| (s["path"].as_str().unwrap_or("").to_ascii_lowercase(), (s["i"].as_u64().unwrap_or(0) * MB, s["m"].as_u64().unwrap_or(0) * MB)))
                .collect()
        })
        .unwrap_or_default();
    let files: Vec<PageFileStat> = v["u"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|u| {
                    let path = u["path"].as_str().unwrap_or("").to_string();
                    let (initial, maximum) = settings.get(&path.to_ascii_lowercase()).copied().unwrap_or((0, 0));
                    PageFileStat {
                        allocated: u["a"].as_u64().unwrap_or(0) * MB,
                        current: u["c"].as_u64().unwrap_or(0) * MB,
                        peak: u["p"].as_u64().unwrap_or(0) * MB,
                        initial,
                        maximum,
                        path,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let automatic = v["auto"].as_bool().unwrap_or(false);
    advise(files, automatic, ram, commit_peak)
}

/// Is the system drive's pagefile far bigger than this PC uses, and what to
/// set instead. Keeps room for every byte of commit ever reached plus a
/// quarter, and never goes below 8 GB (enough for a kernel crash dump on
/// most machines).
pub(crate) fn advise(files: Vec<PageFileStat>, automatic: bool, ram: u64, commit_peak: u64) -> PagefileInfo {
    let drive = system_drive().to_ascii_lowercase();
    let system = files.iter().find(|f| f.path.to_ascii_lowercase().starts_with(&drive)).cloned();
    let mut info = PagefileInfo { files: files.clone(), automatic, ram, commit_peak, system: system.clone(), ..Default::default() };
    let Some(sys) = system else {
        info.reason = "No pagefile on the system drive. Nothing to change here.".into();
        return info;
    };
    let other: u64 = files.iter().filter(|f| f.path != sys.path).map(|f| f.allocated).sum();
    // Commit limit = RAM + every pagefile. Keep 25 % headroom over the peak.
    let need_commit = commit_peak + commit_peak / 4;
    let need_page = need_commit.saturating_sub(ram + other);
    let rec_gb = (sys.peak * 2).max(need_page).div_ceil(GB).max(8) as u32;
    let rec = rec_gb as u64 * GB;
    let fixed = sys.initial > 0 && !automatic;
    info.oversized = sys.allocated > 16 * GB && sys.allocated > rec * 2;
    let mut opts = vec![];
    if info.oversized {
        opts.push(PageOption {
            mode: "fixed".into(),
            gb: rec_gb,
            label: format!("Fixed {rec_gb} GB"),
            detail: "recommended for this PC".into(),
            saves: sys.allocated.saturating_sub(rec),
            recommended: true,
        });
        if rec_gb < 16 {
            opts.push(PageOption {
                mode: "fixed".into(),
                gb: 16,
                label: "Fixed 16 GB".into(),
                detail: "more headroom".into(),
                saves: sys.allocated.saturating_sub(16 * GB),
                recommended: false,
            });
        }
    }
    if fixed {
        opts.push(PageOption {
            mode: "system".into(),
            gb: 0,
            label: "System-managed".into(),
            detail: "Windows picks the size".into(),
            saves: 0,
            recommended: !info.oversized,
        });
    }
    info.reason = if info.oversized {
        format!(
            "{} allocated, but the most it has held is {}. Committed memory peaked at {} of {} RAM.",
            human(sys.allocated),
            human(sys.peak),
            human(commit_peak),
            human(ram)
        )
    } else {
        format!("{} allocated, peak use {}. That's a sensible size; nothing to change.", human(sys.allocated), human(sys.peak))
    };
    info.options = opts;
    info
}

fn human(b: u64) -> String {
    if b >= GB {
        format!("{:.1} GB", b as f64 / GB as f64)
    } else {
        format!("{} MB", b / MB)
    }
}

fn scan_pagefile() -> PagefileInfo {
    let (peak, ram) = commit_peak_and_ram();
    let Some(out) = powershell(PAGE_PROBE, 20) else { return PagefileInfo::default() };
    parse_page_probe(out.lines().last().unwrap_or(""), ram, peak)
}

// ---------------------------------------------------------------- scan / run

fn scan_all() -> DeepScan {
    if !cfg!(windows) {
        return DeepScan::default();
    }
    if util::fake_home().is_some() {
        return DeepScan { supported: true, dry_run: true, ..Default::default() };
    }
    let ((drivers_read, drivers), (installer, (pagefile, quarantine))) =
        rayon::join(scan_drivers, || rayon::join(scan_installer, || rayon::join(scan_pagefile, list_quarantine)));
    DeepScan { supported: true, dry_run: false, drivers, drivers_read, installer, pagefile, quarantine }
}

fn ps_q(s: &str) -> String {
    crate::junk::ps_quote(s)
}

/// The one elevated script for a plan. Pure, so it can be tested.
pub(crate) fn build_script(plan: &DeepPlan, scan: &DeepScan, batch_id: &str, batch_dir: &Path) -> (String, u64) {
    use base64::Engine;
    let mut lines = vec!["$ErrorActionPreference = 'Continue'".to_string()];
    let known_drivers: HashSet<&str> = scan.drivers.iter().flat_map(|g| g.versions.iter().filter(|v| !v.keep)).map(|v| v.published.as_str()).collect();
    for d in &plan.drivers {
        let d = d.to_ascii_lowercase();
        // Only packages this scan offered, and never /force.
        if is_oem(&d) && known_drivers.contains(d.as_str()) {
            lines.push(format!("pnputil.exe /delete-driver {d}"));
        }
    }
    let mut moved = 0u64;
    let orphans: HashMap<String, &InstallerFile> = scan.installer.orphans.iter().map(|f| (f.path.to_ascii_lowercase(), f)).collect();
    let chosen: Vec<&InstallerFile> = plan.installers.iter().filter_map(|p| orphans.get(&p.to_ascii_lowercase()).copied()).collect();
    if scan.installer.checked && !chosen.is_empty() {
        let manifest = Manifest {
            created: now_secs(),
            files: chosen.iter().map(|f| ManifestFile { from: f.path.clone(), name: f.name.clone(), size: f.size }).collect(),
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec_pretty(&manifest).unwrap_or_default());
        let dir = batch_dir.to_string_lossy().to_string();
        lines.push(format!("New-Item -ItemType Directory -Force -Path {} | Out-Null", ps_q(&dir)));
        lines.push(format!(
            "[IO.File]::WriteAllBytes({}, [Convert]::FromBase64String('{b64}'))",
            ps_q(&batch_dir.join("manifest.json").to_string_lossy())
        ));
        for f in &chosen {
            lines.push(format!(
                "Move-Item -LiteralPath {} -Destination {} -Force",
                ps_q(&f.path),
                ps_q(&batch_dir.join(&f.name).to_string_lossy())
            ));
            moved += 1;
        }
        let _ = batch_id;
    }
    for id in &plan.delete_quarantine {
        if safe_id(id) && scan.quarantine.iter().any(|q| &q.id == id && q.old) {
            lines.push(format!("Remove-Item -LiteralPath {} -Recurse -Force", ps_q(&quarantine_root().join(id).to_string_lossy())));
        }
    }
    if let (Some(pc), Some(sys)) = (&plan.pagefile, &scan.pagefile.system) {
        match pc.mode.as_str() {
            "system" => {
                lines.push("$cs = Get-CimInstance Win32_ComputerSystem; Set-CimInstance -InputObject $cs -Property @{ AutomaticManagedPagefile = $true }".into());
            }
            "fixed" if pc.gb >= 4 => {
                let mb = pc.gb as u64 * 1024;
                let name = sys.path.clone();
                lines.push("$cs = Get-CimInstance Win32_ComputerSystem; if ($cs.AutomaticManagedPagefile) { Set-CimInstance -InputObject $cs -Property @{ AutomaticManagedPagefile = $false } }".into());
                lines.push(format!(
                    "$pf = Get-CimInstance Win32_PageFileSetting | Where-Object {{ $_.Name -ieq {n} }}; if ($pf) {{ Set-CimInstance -InputObject $pf -Property @{{ InitialSize = [uint32]{mb}; MaximumSize = [uint32]{mb} }} }} else {{ New-CimInstance -ClassName Win32_PageFileSetting -Property @{{ Name = {n}; InitialSize = [uint32]{mb}; MaximumSize = [uint32]{mb} }} | Out-Null }}",
                    n = ps_q(&name)
                ));
            }
            _ => {}
        }
    }
    if plan.component {
        lines.push(if plan.reset_base {
            "Dism.exe /Online /Cleanup-Image /StartComponentCleanup /ResetBase".into()
        } else {
            "Dism.exe /Online /Cleanup-Image /StartComponentCleanup".into()
        });
    }
    lines.push("exit 0".into());
    (lines.join("\n"), moved)
}

fn run_plan(plan: DeepPlan) -> DeepResult {
    if util::fake_home().is_some() || !cfg!(windows) {
        return DeepResult { dry_run: true, notes: vec!["Test sandbox: nothing was run.".into()], scan: scan_all(), ..Default::default() };
    }
    let before = scan_all();
    let id = now_secs().to_string();
    let batch_dir = quarantine_root().join(&id);
    let (script, moved) = build_script(&plan, &before, &id, &batch_dir);
    let tasks = script.lines().count().saturating_sub(2);
    if tasks == 0 {
        return DeepResult { notes: vec!["Nothing selected.".into()], scan: before, ..Default::default() };
    }
    let ran = crate::junk::run_elevated(&script);
    let after = scan_all();
    let mut res = DeepResult { ran, ..Default::default() };
    if !ran {
        res.notes.push("Cancelled at the Windows prompt, or the script could not start.".into());
    }
    // Drivers: which asked-for packages are really gone.
    // A package whose group drops to one version leaves the groups, so ask
    // pnputil for the full list again.
    let still: HashSet<String> = which("pnputil")
        .and_then(|p| run_capture(&p, &["/enum-drivers"], Duration::from_secs(60)))
        .map(|t| parse_pnputil(&t).into_iter().map(|e| e.published).collect())
        .unwrap_or_else(|| plan.drivers.iter().map(|d| d.to_ascii_lowercase()).collect());
    let sizes: HashMap<String, u64> = before.drivers.iter().flat_map(|g| g.versions.iter()).map(|v| (v.published.clone(), v.size)).collect();
    for d in &plan.drivers {
        let d = d.to_ascii_lowercase();
        if !sizes.contains_key(&d) {
            continue;
        }
        let gone = !still.contains(&d);
        if gone {
            res.freed += sizes.get(&d).copied().unwrap_or(0);
            res.drivers_removed.push(d);
        } else {
            res.drivers_refused.push(d);
        }
    }
    if !res.drivers_refused.is_empty() && ran {
        res.notes.push(format!("Windows kept {} driver package(s) a device still uses.", res.drivers_refused.len()));
    }
    if moved > 0 {
        let batch = list_quarantine().into_iter().find(|b| b.id == id);
        if let Some(b) = batch {
            res.quarantined = b.files;
            res.freed += b.size;
            res.quarantine_id = Some(id);
        }
    }
    if let Some(pc) = &plan.pagefile {
        res.pagefile_set = ran;
        res.restart_needed = ran;
        if ran {
            res.notes.push(match pc.mode.as_str() {
                "system" => "Pagefile set to system-managed. It changes after a restart.".into(),
                _ => format!("Pagefile set to {} GB. The space comes back after a restart.", pc.gb),
            });
        }
    }
    for id in &plan.delete_quarantine {
        if let Some(b) = before.quarantine.iter().find(|q| &q.id == id) {
            if ran && !quarantine_root().join(id).exists() {
                res.freed += b.size;
            }
        }
    }
    if plan.component && ran {
        res.notes.push(if plan.reset_base { "Component store cleaned and its base reset." } else { "Component store cleaned." }.into());
    }
    res.scan = after;
    res
}

fn restore(id: &str) -> R<QuarantineResult> {
    if !safe_id(id) {
        return Err("Unknown quarantine batch.".into());
    }
    if util::fake_home().is_some() {
        return Ok(QuarantineResult { restored: 0, dry_run: true });
    }
    let dir = quarantine_root().join(id);
    let m: Manifest = serde_json::from_str(&read_text(&dir.join("manifest.json")).ok_or("This batch has no manifest.")?).map_err(|e| e.to_string())?;
    let mut lines = vec!["$ErrorActionPreference = 'Continue'".to_string()];
    let inst = system_root().join("Installer").to_string_lossy().to_ascii_lowercase();
    let mut n = 0;
    for f in &m.files {
        // Only back into the Installer folder it came from.
        if !f.from.to_ascii_lowercase().starts_with(&inst) || f.name.contains(['\\', '/']) {
            continue;
        }
        let src = dir.join(&f.name);
        if src.is_file() {
            lines.push(format!("Move-Item -LiteralPath {} -Destination {} -Force", ps_q(&src.to_string_lossy()), ps_q(&f.from)));
            n += 1;
        }
    }
    if n == 0 {
        return Err("Nothing left to restore in this batch.".into());
    }
    lines.push(format!("if (-not (Get-ChildItem -LiteralPath {d} -Exclude manifest.json)) {{ Remove-Item -LiteralPath {d} -Recurse -Force }}", d = ps_q(&dir.to_string_lossy())));
    lines.push("exit 0".into());
    if !crate::junk::run_elevated(&lines.join("\n")) {
        return Err("Cancelled at the Windows prompt.".into());
    }
    let restored = m.files.iter().filter(|f| Path::new(&f.from).is_file()).count() as u64;
    Ok(QuarantineResult { restored, dry_run: false })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineResult {
    pub restored: u64,
    pub dry_run: bool,
}

// ---------------------------------------------------------------- commands

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn deep_scan() -> R<DeepScan> {
    blocking(scan_all).await
}

#[tauri::command]
pub async fn deep_run(plan: DeepPlan) -> R<DeepResult> {
    blocking(move || run_plan(plan)).await
}

#[tauri::command]
pub async fn quarantine_list() -> R<Vec<QuarantineBatch>> {
    blocking(|| if util::fake_home().is_some() { vec![] } else { list_quarantine() }).await
}

#[tauri::command]
pub async fn quarantine_restore(id: String) -> R<QuarantineResult> {
    blocking(move || restore(&id)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNP: &str = "Microsoft PnP Utility\r\n\r\nPublished Name:     oem1.inf\r\nOriginal Name:      amdafd.inf\r\nProvider Name:      AMD\r\nClass Name:         System\r\nClass GUID:         {4d36e97d-e325-11ce-bfc1-08002be10318}\r\nDriver Version:     04/20/2022 22.20.0.0\r\nSigner Name:        Microsoft Windows Hardware Compatibility Publisher\r\n\r\nNom publié :     oem13.inf\r\nNom d'origine :      amdappcompat.inf\r\nFournisseur :      Advanced Micro Devices, Inc.\r\nClasse :         System\r\nGUID :         {4d36e97d-e325-11ce-bfc1-08002be10318}\r\nVersion :     26.12.2024 1.0.0.3\r\nSignataire :        Microsoft\r\n\r\nPublished Name:     oem69.inf\r\nOriginal Name:      amdappcompat.inf\r\nProvider Name:      Advanced Micro Devices, Inc.\r\nClass Name:         System\r\nDriver Version:     09/09/2025 1.0.0.3\r\n";

    #[test]
    fn parses_pnputil_without_trusting_labels() {
        let e = parse_pnputil(PNP);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].published, "oem1.inf");
        assert_eq!(e[0].original, "amdafd.inf");
        assert_eq!(e[0].provider, "AMD");
        assert_eq!(e[0].version, "22.20.0.0");
        assert_eq!(e[1].published, "oem13.inf");
        assert_eq!(e[1].original, "amdappcompat.inf");
        assert_eq!(e[1].provider, "Advanced Micro Devices, Inc.");
        assert_eq!(e[1].class, "System");
        assert_eq!(e[2].date, "09/09/2025");
    }

    #[test]
    fn reads_driver_ver_from_an_inf() {
        let inf = "[Version]\r\nSignature=\"$WINDOWS NT$\"\r\n  driverver = 12/26/2024, 1.0.0.3 ; comment\r\n";
        assert_eq!(inf_driver_ver(inf), Some(("12/26/2024".into(), "1.0.0.3".into())));
        assert_eq!(inf_driver_ver("[Version]\nClass=System"), None);
    }

    #[test]
    fn newest_version_then_date_wins() {
        assert!(ver_key("32.0.15.6094", "01/01/2024") > ver_key("31.0.15.5222", "12/31/2025"));
        assert!(ver_key("1.0.0.3", "09/09/2025") > ver_key("1.0.0.3", "12/26/2024"));
    }

    #[test]
    fn local_packages_and_orphans() {
        let reg = "\r\nHKEY_LOCAL_MACHINE\\...\\Patches\\2D00\r\n    LocalPackage    REG_SZ    C:\\WINDOWS\\Installer\\25f9f02.msp\r\n\r\nHKEY...\\InstallProperties\r\n    LocalPackage    REG_SZ    C:\\Windows\\Installer\\1a2b.msi\r\n\r\nEnd of search: 2 match(es) found.\r\n";
        let used = parse_local_packages(reg);
        assert_eq!(used.len(), 2);
        let f = |p: &str| InstallerFile { path: p.into(), name: p.rsplit('\\').next().unwrap().into(), size: 10, modified: 0 };
        let files = vec![f(r"C:\Windows\Installer\25F9F02.msp"), f(r"C:\Windows\Installer\1a2b.msi"), f(r"C:\Windows\Installer\dead.msi")];
        let o = orphans_of(&files, &used);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].name, "dead.msi");
    }

    #[test]
    fn advises_a_smaller_pagefile_only_when_oversized() {
        let pf = |a: u64, p: u64, i: u64| PageFileStat { path: format!("{}\\pagefile.sys", system_drive()), allocated: a * GB, current: 0, peak: p * MB, initial: i, maximum: i };
        let big = advise(vec![pf(49, 254, 0)], false, 64 * GB, 40 * GB);
        assert!(big.oversized);
        assert_eq!(big.options[0].gb, 8);
        assert!(big.options[0].recommended);
        // Commit near RAM: the recommendation grows to cover it.
        let tight = advise(vec![pf(49, 254, 0)], false, 16 * GB, 30 * GB);
        assert!(tight.options[0].gb >= 22);
        let fine = advise(vec![pf(8, 2000, 0)], false, 32 * GB, 20 * GB);
        assert!(!fine.oversized);
        assert!(fine.options.is_empty());
    }

    #[test]
    fn script_only_runs_offered_items_and_never_forces() {
        let scan = DeepScan {
            supported: true,
            drivers: vec![DriverGroup {
                key: "k".into(),
                original: "a.inf".into(),
                provider: "p".into(),
                class: "c".into(),
                versions: vec![
                    DriverPkg { published: "oem2.inf".into(), version: "2".into(), date: "".into(), size: 5, keep: true },
                    DriverPkg { published: "oem1.inf".into(), version: "1".into(), date: "".into(), size: 5, keep: false },
                ],
                reclaim: 5,
            }],
            installer: InstallerInfo {
                checked: true,
                orphans: vec![InstallerFile { path: r"C:\Windows\Installer\x.msi".into(), name: "x.msi".into(), size: 1, modified: 0 }],
                ..Default::default()
            },
            ..Default::default()
        };
        let plan = DeepPlan {
            drivers: vec!["oem1.inf".into(), "oem2.inf".into(), "oem9.inf; rm".into()],
            installers: vec![r"c:\windows\installer\X.MSI".into(), r"C:\Windows\System32\kernel32.dll".into()],
            component: true,
            reset_base: true,
            ..Default::default()
        };
        let (s, moved) = build_script(&plan, &scan, "1", Path::new(r"C:\DevCleaner Quarantine\Installer\1"));
        assert!(s.contains("pnputil.exe /delete-driver oem1.inf"));
        assert!(!s.contains("oem2.inf"), "the kept version is never deleted");
        assert!(!s.contains("oem9"));
        assert!(!s.contains("/force"));
        assert_eq!(moved, 1);
        assert!(!s.contains("kernel32"));
        assert!(s.contains("/ResetBase"));
    }

    /// Read-only: prints what deep clean and Responsiveness see on this PC.
    #[test]
    #[ignore]
    fn probe_this_machine() {
        let s = scan_all();
        println!("drivers_read={} groups={}", s.drivers_read, s.drivers.len());
        for g in s.drivers.iter().take(8) {
            println!("  {} {} reclaim={}MB {:?}", g.original, g.provider, g.reclaim / MB, g.versions.iter().map(|v| (&v.published, &v.version, &v.date, v.size / MB, v.keep)).collect::<Vec<_>>());
        }
        println!("installer checked={} total={} orphans={} {}MB", s.installer.checked, s.installer.total_files, s.installer.orphans.len(), s.installer.orphan_size / MB);
        println!("pagefile {:#?}", s.pagefile);
        println!("quarantine {:?}", s.quarantine);
        println!("{}", crate::tune::tests_overview());
    }
}
