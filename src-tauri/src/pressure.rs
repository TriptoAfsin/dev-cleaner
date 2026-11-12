//! Memory pressure: is the machine short of memory, and who is making it so.
//!
//! A background thread samples every two seconds (five on macOS, where it
//! has to run tools): committed memory against its limit, free RAM, how many
//! pages per second are read back from disk (hard faults), the compressed
//! store and the page file in use. The last 8 hours stay in memory; a
//! one-line summary per minute goes to `pressure.jsonl` in the data folder
//! and is kept for 7 days, so "slow spells" survive a restart.
//!
//! Every 30 seconds it also records how much memory each app (processes
//! grouped by name) holds, for the last 2 hours: that is "who is growing".
//!
//! How the level is decided (see [`level`]):
//! - High: committed memory at 90% of the commit limit or more, under 5% of
//!   RAM available, or (once under 20% of RAM is available or commit is at
//!   75%) more than 300 pages/s read back from disk on average over the last
//!   30 seconds. Paging with plenty of RAM free is file reads, not pressure. The OS's own signal (Linux PSI "some avg10"
//!   at 10% or more, macOS pressure level critical) also counts.
//! - Moderate: commit at 75% or more, under 12% available, more than 60
//!   pages/s on average, PSI at 2% or more, or macOS level "warn".
//! - Low otherwise.

use crate::monitor::{classify, Monitor, Verdict};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RING_SECS: u64 = 8 * 3600;
const KEEP_SECS: u64 = 7 * 24 * 3600;
const GROWTH_SECS: u64 = 2 * 3600;
const GROUP_EVERY: u64 = 30;
const MAX_POINTS: usize = 600;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    #[default]
    Low,
    Moderate,
    High,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PressureSample {
    /// Unix seconds.
    pub t: u64,
    pub commit_used: u64,
    pub commit_limit: u64,
    pub mem_available: u64,
    pub mem_total: u64,
    pub pages_in_per_sec: f64,
    pub compressed: Option<u64>,
    pub pagefile_used: Option<u64>,
    pub level: Level,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Spell {
    pub start: u64,
    pub end: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PressureHistory {
    pub samples: Vec<PressureSample>,
    /// Oldest data point in the window (or now when there is none yet).
    pub since: u64,
    pub slow_spells: Vec<Spell>,
    pub peak_commit_pct: f64,
    pub peak_pages_in: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Grower {
    pub name: String,
    pub pids: Vec<u32>,
    pub memory: u64,
    /// Bytes gained since the smallest value in the window.
    pub growth: u64,
    /// Memory over the window, oldest first, at most 40 points.
    pub series: Vec<u64>,
    pub verdict: Verdict,
    pub reason: String,
    pub protected: bool,
}

/// One minute, summarised: the worst value of each figure.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct Minute {
    t: u64,
    /// Max committed bytes.
    cu: u64,
    cl: u64,
    /// Min available bytes.
    av: u64,
    mt: u64,
    /// Max and average pages/s read from disk.
    pm: f64,
    pa: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cz: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pf: Option<u64>,
    lv: Level,
}

impl Minute {
    fn slow(&self) -> bool {
        self.lv == Level::High
    }
    fn sample(&self) -> PressureSample {
        PressureSample {
            t: self.t,
            commit_used: self.cu,
            commit_limit: self.cl,
            mem_available: self.av,
            mem_total: self.mt,
            pages_in_per_sec: self.pm,
            compressed: self.cz,
            pagefile_used: self.pf,
            level: self.lv,
        }
    }
}

#[derive(Default)]
struct Acc {
    minute: Option<Minute>,
    n: u32,
}

impl Acc {
    /// Fold a sample in; returns the finished minute when it rolls over.
    fn add(&mut self, s: &PressureSample) -> Option<Minute> {
        let start = s.t - s.t % 60;
        let mut done = None;
        if self.minute.as_ref().is_some_and(|m| m.t != start) {
            done = self.minute.take();
            self.n = 0;
        }
        let m = self.minute.get_or_insert_with(|| Minute { t: start, av: u64::MAX, ..Default::default() });
        self.n += 1;
        m.cu = m.cu.max(s.commit_used);
        m.cl = m.cl.max(s.commit_limit);
        m.av = m.av.min(s.mem_available);
        m.mt = m.mt.max(s.mem_total);
        m.pm = m.pm.max(s.pages_in_per_sec);
        m.pa += (s.pages_in_per_sec - m.pa) / self.n as f64;
        m.cz = max_opt(m.cz, s.compressed);
        m.pf = max_opt(m.pf, s.pagefile_used);
        m.lv = m.lv.max(s.level);
        done
    }
}

fn max_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

#[derive(Clone)]
struct GroupSnap {
    memory: u64,
    pids: Vec<u32>,
}

#[derive(Default)]
struct State {
    ring: VecDeque<PressureSample>,
    minutes: Vec<Minute>,
    acc: Acc,
    file: Option<PathBuf>,
    groups: VecDeque<(u64, HashMap<String, GroupSnap>)>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ------------------------------------------------------------- the level

/// The pressure level from the figures above. `os` is the OS's own signal
/// when it has one (Linux PSI, macOS memorystatus).
pub fn level(commit_pct: f64, avail_pct: f64, paging_avg: f64, os: Option<Level>) -> Level {
    // Pages read from disk also count file reads (memory-mapped files, app
    // launches, a disk scan), which happen with plenty of RAM free. Paging
    // only means memory pressure once RAM is getting short.
    let tight = avail_pct < 20.0 || commit_pct >= 75.0;
    let ours = if commit_pct >= 90.0 || avail_pct < 5.0 || (tight && paging_avg > 300.0) {
        Level::High
    } else if commit_pct >= 75.0 || avail_pct < 12.0 || (tight && paging_avg > 60.0) {
        Level::Moderate
    } else {
        Level::Low
    };
    ours.max(os.unwrap_or_default())
}

// ------------------------------------------------------------- readers

/// What one read of the OS gives.
#[derive(Default, Debug, Clone)]
struct Raw {
    commit_used: u64,
    commit_limit: u64,
    avail: u64,
    total: u64,
    pages_in: f64,
    pagefile_used: Option<u64>,
    os_level: Option<Level>,
}

#[cfg(windows)]
mod os {
    use super::Raw;
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
    };
    use windows_sys::Win32::System::ProcessStatus::{K32GetPerformanceInfo, PERFORMANCE_INFORMATION};

    pub struct Reader {
        query: isize,
        pages: isize,
        pagefile: isize,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    impl Reader {
        pub fn new() -> Self {
            let mut r = Reader { query: 0, pages: 0, pagefile: 0 };
            // SAFETY: out-pointers to locals; counter paths are NUL-terminated.
            unsafe {
                if PdhOpenQueryW(std::ptr::null(), 0, &mut r.query) == 0 {
                    let p = wide(r"\Memory\Pages Input/sec");
                    if PdhAddEnglishCounterW(r.query, p.as_ptr(), 0, &mut r.pages) != 0 {
                        r.pages = 0;
                    }
                    let f = wide(r"\Paging File(_Total)\% Usage");
                    if PdhAddEnglishCounterW(r.query, f.as_ptr(), 0, &mut r.pagefile) != 0 {
                        r.pagefile = 0;
                    }
                    // Rates need two collections; the first one primes them.
                    PdhCollectQueryData(r.query);
                } else {
                    r.query = 0;
                }
            }
            r
        }

        fn value(&self, counter: isize) -> Option<f64> {
            if counter == 0 {
                return None;
            }
            // SAFETY: a counter handle from this query and a struct we own.
            unsafe {
                let mut v: PDH_FMT_COUNTERVALUE = std::mem::zeroed();
                (PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut v) == 0).then_some(v.Anonymous.doubleValue)
            }
        }

        pub fn read(&mut self) -> Raw {
            // SAFETY: a correctly sized struct we own.
            let mut pi: PERFORMANCE_INFORMATION = unsafe { std::mem::zeroed() };
            pi.cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
            let ok = unsafe { K32GetPerformanceInfo(&mut pi, pi.cb) } != 0;
            if self.query != 0 {
                // SAFETY: the query handle opened in new().
                unsafe { PdhCollectQueryData(self.query) };
            }
            if !ok {
                return Raw::default();
            }
            let page = pi.PageSize as u64;
            let total = pi.PhysicalTotal as u64 * page;
            let limit = pi.CommitLimit as u64 * page;
            let pagefile_size = limit.saturating_sub(total);
            Raw {
                commit_used: pi.CommitTotal as u64 * page,
                commit_limit: limit,
                avail: pi.PhysicalAvailable as u64 * page,
                total,
                pages_in: self.value(self.pages).unwrap_or(0.0).max(0.0),
                pagefile_used: self.value(self.pagefile).filter(|_| pagefile_size > 0).map(|pct| (pct.clamp(0.0, 100.0) / 100.0 * pagefile_size as f64) as u64),
                os_level: None,
            }
        }
    }

    // The PDH query lives as long as the sampler thread.
    unsafe impl Send for Reader {}
}

#[cfg(target_os = "linux")]
mod os {
    use super::{parse_meminfo, Level, Raw};
    use std::time::Instant;

    pub struct Reader {
        last: Option<(u64, Instant)>,
    }

    impl Reader {
        pub fn new() -> Self {
            Reader { last: None }
        }
        pub fn read(&mut self) -> Raw {
            let m = parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default());
            let faults = std::fs::read_to_string("/proc/vmstat")
                .ok()
                .and_then(|s| s.lines().find_map(|l| l.strip_prefix("pgmajfault ").and_then(|v| v.trim().parse::<u64>().ok())));
            let now = Instant::now();
            let pages_in = match (faults, self.last) {
                (Some(f), Some((p, at))) => f.saturating_sub(p) as f64 / now.duration_since(at).as_secs_f64().max(0.1),
                _ => 0.0,
            };
            if let Some(f) = faults {
                self.last = Some((f, now));
            }
            let psi = std::fs::read_to_string("/proc/pressure/memory").ok().and_then(|s| super::parse_psi_some_avg10(&s));
            let os_level = psi.map(|p| if p >= 10.0 { Level::High } else if p >= 2.0 { Level::Moderate } else { Level::Low });
            let swap_used = m.swap_total.saturating_sub(m.swap_free);
            Raw {
                commit_used: m.committed,
                commit_limit: m.commit_limit,
                avail: m.available,
                total: m.total,
                pages_in,
                pagefile_used: (m.swap_total > 0).then_some(swap_used),
                os_level,
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::{parse_swapusage, parse_vm_stat_pageins, Level, Raw};
    use std::path::Path;
    use std::time::{Duration, Instant};

    pub struct Reader {
        sys: sysinfo::System,
        last: Option<(u64, Instant)>,
    }

    fn run(p: &str, args: &[&str]) -> Option<String> {
        crate::util::run_capture(Path::new(p), args, Duration::from_secs(4))
    }

    impl Reader {
        pub fn new() -> Self {
            Reader { sys: sysinfo::System::new(), last: None }
        }
        pub fn read(&mut self) -> Raw {
            self.sys.refresh_memory();
            let total = self.sys.total_memory();
            let avail = self.sys.available_memory();
            let (swap_total, swap_used) = run("/usr/sbin/sysctl", &["-n", "vm.swapusage"]).and_then(|s| parse_swapusage(&s)).unwrap_or((0, 0));
            let now = Instant::now();
            let pageins = run("/usr/bin/vm_stat", &[]).and_then(|s| parse_vm_stat_pageins(&s));
            let pages_in = match (pageins, self.last) {
                (Some(p), Some((q, at))) => p.saturating_sub(q) as f64 / now.duration_since(at).as_secs_f64().max(0.1),
                _ => 0.0,
            };
            if let Some(p) = pageins {
                self.last = Some((p, now));
            }
            let os_level = run("/usr/sbin/sysctl", &["-n", "kern.memorystatus_vm_pressure_level"]).and_then(|s| s.trim().parse::<u32>().ok()).map(|n| match n {
                4 => Level::High,
                2 => Level::Moderate,
                _ => Level::Low,
            });
            // macOS has no commit limit; RAM plus swap in use stands in for it.
            Raw {
                commit_used: total.saturating_sub(avail) + swap_used,
                commit_limit: total + swap_total,
                avail,
                total,
                pages_in,
                pagefile_used: (swap_total > 0).then_some(swap_used),
                os_level,
            }
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub cached: u64,
    pub buffers: u64,
    pub reclaimable: u64,
    pub free: u64,
}

/// Parse `/proc/meminfo` (kB values) into bytes. Also used for the WSL VM.
pub(crate) fn parse_meminfo(text: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let kb: u64 = v.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let b = kb * 1024;
        match k.trim() {
            "MemTotal" => m.total = b,
            "MemAvailable" => m.available = b,
            "MemFree" => m.free = b,
            "Committed_AS" => m.committed = b,
            "CommitLimit" => m.commit_limit = b,
            "SwapTotal" => m.swap_total = b,
            "SwapFree" => m.swap_free = b,
            "Cached" => m.cached = b,
            "Buffers" => m.buffers = b,
            "SReclaimable" => m.reclaimable = b,
            _ => {}
        }
    }
    m
}

/// `some avg10=1.23 avg60=...` from /proc/pressure/memory.
#[allow(dead_code)]
fn parse_psi_some_avg10(text: &str) -> Option<f64> {
    let line = text.lines().find(|l| l.starts_with("some"))?;
    line.split_whitespace().find_map(|w| w.strip_prefix("avg10=")).and_then(|v| v.parse().ok())
}

/// `total = 2048.00M  used = 1024.50M  free = ...` → (total, used) bytes.
#[allow(dead_code)]
fn parse_swapusage(text: &str) -> Option<(u64, u64)> {
    let num = |key: &str| -> Option<u64> {
        let rest = text.split(key).nth(1)?.trim_start().trim_start_matches('=').trim_start();
        let tok = rest.split_whitespace().next()?;
        let (n, unit) = tok.split_at(tok.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(tok.len()));
        let v: f64 = n.parse().ok()?;
        let mul = match unit {
            "K" => 1024.0,
            "M" => 1024.0 * 1024.0,
            "G" => 1024.0 * 1024.0 * 1024.0,
            _ => 1.0,
        };
        Some((v * mul) as u64)
    };
    Some((num("total")?, num("used")?))
}

/// The cumulative "Pageins" count from `vm_stat`.
#[allow(dead_code)]
fn parse_vm_stat_pageins(text: &str) -> Option<u64> {
    text.lines().find_map(|l| l.strip_prefix("Pageins:").map(|v| v.trim().trim_end_matches('.')).and_then(|v| v.parse().ok()))
}

fn to_sample(raw: &Raw, t: u64, compressed: Option<u64>, paging_avg: f64) -> PressureSample {
    let commit_pct = if raw.commit_limit > 0 { raw.commit_used as f64 / raw.commit_limit as f64 * 100.0 } else { 0.0 };
    let avail_pct = if raw.total > 0 { raw.avail as f64 / raw.total as f64 * 100.0 } else { 100.0 };
    PressureSample {
        t,
        commit_used: raw.commit_used,
        commit_limit: raw.commit_limit,
        mem_available: raw.avail,
        mem_total: raw.total,
        pages_in_per_sec: raw.pages_in,
        compressed,
        pagefile_used: raw.pagefile_used,
        level: level(commit_pct, avail_pct, paging_avg, raw.os_level),
    }
}

// ------------------------------------------------------------- persistence

fn load_minutes(path: &PathBuf, now: u64) -> Vec<Minute> {
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    let all: Vec<Minute> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let keep: Vec<Minute> = all.iter().filter(|m| m.t + KEEP_SECS >= now).cloned().collect();
    if keep.len() != all.len() || text.lines().count() != all.len() {
        // Rewrite without the old (or unreadable) lines.
        let body: String = keep.iter().filter_map(|m| serde_json::to_string(m).ok()).map(|l| l + "\n").collect();
        let _ = std::fs::write(path, body);
    }
    keep
}

fn append_minute(path: &PathBuf, m: &Minute) {
    if let (Ok(line), Ok(mut f)) = (serde_json::to_string(m), std::fs::OpenOptions::new().create(true).append(true).open(path)) {
        let _ = writeln!(f, "{line}");
    }
}

// ------------------------------------------------------------- sampler

/// Start the background sampler. Called once from setup.
pub fn start(app: &tauri::AppHandle) {
    use tauri::Manager;
    let file = crate::winstate::data_dir(app).map(|d| d.join("pressure.jsonl"));
    let monitor: Option<Arc<Monitor>> = app.try_state::<Arc<Monitor>>().map(|s| Arc::clone(&s));
    std::thread::Builder::new()
        .name("pressure".into())
        .spawn(move || run(file, monitor))
        .ok();
}

fn run(file: Option<PathBuf>, monitor: Option<Arc<Monitor>>) {
    if let Some(f) = &file {
        let minutes = load_minutes(f, now());
        if let Ok(mut s) = state().lock() {
            s.minutes = minutes;
            s.file = file.clone();
        }
    }
    let step = if cfg!(target_os = "macos") { 5 } else { 2 };
    let mut reader = os::Reader::new();
    let mut procs = sysinfo::System::new();
    let mut last_groups = 0u64;
    loop {
        let raw = reader.read();
        let t = now();
        let compressed = monitor.as_ref().map(|m| m.compressed.load(std::sync::atomic::Ordering::Relaxed)).filter(|n| *n > 0);
        if raw.commit_limit > 0 {
            record(raw, t, compressed);
        }
        if t >= last_groups + GROUP_EVERY {
            last_groups = t;
            let snap = group_snapshot(&mut procs);
            if let Ok(mut s) = state().lock() {
                s.groups.push_back((t, snap));
                while s.groups.front().is_some_and(|(at, _)| at + GROWTH_SECS < t) {
                    s.groups.pop_front();
                }
            }
        }
        std::thread::sleep(Duration::from_secs(step));
    }
}

fn record(raw: Raw, t: u64, compressed: Option<u64>) {
    let Ok(mut s) = state().lock() else { return };
    // Paging averaged over the last ~30 seconds, this read included.
    let recent: Vec<f64> = s.ring.iter().rev().take_while(|x| x.t + 30 > t).map(|x| x.pages_in_per_sec).collect();
    let avg = (recent.iter().sum::<f64>() + raw.pages_in) / (recent.len() + 1) as f64;
    let sample = to_sample(&raw, t, compressed, avg);
    if let Some(done) = s.acc.add(&sample) {
        if let Some(f) = s.file.clone() {
            append_minute(&f, &done);
        }
        s.minutes.push(done);
        let cut = t.saturating_sub(KEEP_SECS);
        if s.minutes.first().is_some_and(|m| m.t < cut) {
            s.minutes.retain(|m| m.t >= cut);
        }
    }
    s.ring.push_back(sample);
    while s.ring.front().is_some_and(|x| x.t + RING_SECS < t) {
        s.ring.pop_front();
    }
}

fn display_name(raw: &str) -> String {
    raw.strip_suffix(".exe").or_else(|| raw.strip_suffix(".EXE")).unwrap_or(raw).to_string()
}

fn group_snapshot(sys: &mut sysinfo::System) -> HashMap<String, GroupSnap> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory());
    let mut out: HashMap<String, GroupSnap> = HashMap::new();
    for (pid, p) in sys.processes() {
        let raw = p.name().to_string_lossy();
        if raw.is_empty() {
            continue;
        }
        let g = out.entry(display_name(&raw)).or_insert(GroupSnap { memory: 0, pids: vec![] });
        g.memory += p.memory();
        g.pids.push(pid.as_u32());
    }
    out
}

// ------------------------------------------------------------- queries

fn spells(minutes: &[Minute]) -> Vec<Spell> {
    let mut out: Vec<Spell> = vec![];
    for m in minutes.iter().filter(|m| m.slow()) {
        match out.last_mut() {
            Some(s) if m.t <= s.end + 60 => s.end = m.t + 60,
            _ => out.push(Spell { start: m.t, end: m.t + 60 }),
        }
    }
    out
}

/// At most `max` points: each bucket keeps its highest commit and its peak paging.
fn downsample(samples: Vec<PressureSample>, max: usize) -> Vec<PressureSample> {
    if samples.len() <= max || max == 0 {
        return samples;
    }
    let per = samples.len().div_ceil(max);
    samples
        .chunks(per)
        .filter_map(|c| {
            let mut best = c.iter().max_by_key(|s| s.commit_used)?.clone();
            best.pages_in_per_sec = c.iter().map(|s| s.pages_in_per_sec).fold(0.0, f64::max);
            best.level = c.iter().map(|s| s.level).max().unwrap_or_default();
            best.t = c.last().map(|s| s.t).unwrap_or(best.t);
            Some(best)
        })
        .collect()
}

fn history_of(ring: &VecDeque<PressureSample>, minutes: &[Minute], current: Option<&Minute>, hours: f64, now: u64) -> PressureHistory {
    let from = now.saturating_sub((hours.max(0.0) * 3600.0) as u64);
    let ring_first = ring.front().map(|s| s.t).unwrap_or(u64::MAX);
    let mut samples: Vec<PressureSample> = minutes.iter().filter(|m| m.t >= from && m.t + 60 <= ring_first).map(Minute::sample).collect();
    samples.extend(ring.iter().filter(|s| s.t >= from).cloned());
    let in_window: Vec<Minute> = minutes.iter().filter(|m| m.t + 60 > from).cloned().chain(current.cloned()).collect();
    let peak_commit_pct = samples
        .iter()
        .filter(|s| s.commit_limit > 0)
        .map(|s| s.commit_used as f64 / s.commit_limit as f64 * 100.0)
        .fold(0.0, f64::max);
    // Paging only matters while memory is short (see `level`): file reads
    // with RAM to spare would otherwise set the peak.
    let peak_pages_in = samples.iter().filter(|s| s.level != Level::Low).map(|s| s.pages_in_per_sec).fold(0.0, f64::max);
    let since = samples.first().map(|s| s.t).unwrap_or(now);
    PressureHistory { samples: downsample(samples, MAX_POINTS), since, slow_spells: spells(&in_window), peak_commit_pct, peak_pages_in }
}

fn growers_of(groups: &VecDeque<(u64, HashMap<String, GroupSnap>)>) -> Vec<Grower> {
    let Some((_, latest)) = groups.back() else { return vec![] };
    let mut out: Vec<Grower> = latest
        .iter()
        .filter(|(_, g)| g.memory >= 64 * 1024 * 1024)
        .map(|(name, g)| {
            let full: Vec<u64> = groups.iter().filter_map(|(_, snap)| snap.get(name).map(|x| x.memory)).collect();
            let min = full.iter().copied().min().unwrap_or(g.memory);
            let per = full.len().div_ceil(40).max(1);
            let series: Vec<u64> = full.chunks(per).map(|c| *c.last().unwrap_or(&0)).collect();
            let (verdict, reason) = classify(name, g.pids.first().copied().unwrap_or(0).max(5));
            Grower {
                name: name.clone(),
                pids: g.pids.clone(),
                memory: g.memory,
                growth: g.memory.saturating_sub(min),
                series,
                verdict,
                reason: reason.to_string(),
                protected: verdict == Verdict::System,
            }
        })
        .collect();
    out.sort_by(|a, b| b.growth.cmp(&a.growth).then(b.memory.cmp(&a.memory)));
    out.truncate(8);
    out
}

#[tauri::command]
pub fn pressure_now() -> PressureSample {
    if let Some(s) = state().lock().ok().and_then(|s| s.ring.back().cloned()) {
        return s;
    }
    // Before the sampler's first read: one read now (no paging rate yet).
    let raw = os::Reader::new().read();
    to_sample(&raw, now(), None, 0.0)
}

#[tauri::command]
pub fn pressure_history(hours: f64) -> PressureHistory {
    let Ok(s) = state().lock() else { return PressureHistory::default() };
    history_of(&s.ring, &s.minutes, s.acc.minute.as_ref(), hours, now())
}

#[tauri::command]
pub fn memory_growth() -> Vec<Grower> {
    state().lock().map(|s| growers_of(&s.groups)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_documented_thresholds() {
        assert_eq!(level(40.0, 50.0, 0.0, None), Level::Low);
        assert_eq!(level(76.0, 50.0, 0.0, None), Level::Moderate);
        assert_eq!(level(40.0, 10.0, 0.0, None), Level::Moderate);
        assert_eq!(level(40.0, 50.0, 80.0, None), Level::Low, "paging with RAM to spare is file reads");
        assert_eq!(level(40.0, 15.0, 80.0, None), Level::Moderate);
        assert_eq!(level(91.0, 50.0, 0.0, None), Level::High);
        assert_eq!(level(40.0, 4.0, 0.0, None), Level::High);
        assert_eq!(level(40.0, 15.0, 400.0, None), Level::High);
        assert_eq!(level(40.0, 50.0, 0.0, Some(Level::High)), Level::High, "the OS signal counts");
    }

    #[test]
    fn parses_meminfo_in_bytes() {
        let m = parse_meminfo("MemTotal:       16000000 kB\nMemFree:  100 kB\nMemAvailable:    8000000 kB\nCached:  2000 kB\nCommitted_AS:   12000000 kB\nCommitLimit:    24000000 kB\nSwapTotal: 4 kB\nSwapFree: 1 kB\nSReclaimable: 10 kB\n");
        assert_eq!(m.total, 16_000_000 * 1024);
        assert_eq!(m.available, 8_000_000 * 1024);
        assert_eq!(m.committed, 12_000_000 * 1024);
        assert_eq!(m.commit_limit, 24_000_000 * 1024);
        assert_eq!(m.cached, 2000 * 1024);
        assert_eq!(m.swap_total - m.swap_free, 3 * 1024);
    }

    #[test]
    fn parses_psi_swapusage_and_vm_stat() {
        assert_eq!(parse_psi_some_avg10("some avg10=3.50 avg60=1.00 avg300=0.20 total=1\nfull avg10=0.00"), Some(3.5));
        assert_eq!(parse_swapusage("total = 2048.00M  used = 1024.00M  free = 1024.00M  (encrypted)"), Some((2048 * 1024 * 1024, 1024 * 1024 * 1024)));
        assert_eq!(parse_vm_stat_pageins("Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 1.\nPageins:                            123456.\n"), Some(123456));
    }

    fn sample(t: u64, used: u64, pages: f64, lv: Level) -> PressureSample {
        PressureSample { t, commit_used: used, commit_limit: 100, mem_available: 50, mem_total: 100, pages_in_per_sec: pages, compressed: None, pagefile_used: None, level: lv }
    }

    #[test]
    fn minutes_keep_the_worst_values_and_roll_over() {
        let mut acc = Acc::default();
        assert!(acc.add(&sample(120, 40, 10.0, Level::Low)).is_none());
        assert!(acc.add(&sample(150, 60, 30.0, Level::Moderate)).is_none());
        let m = acc.add(&sample(181, 10, 0.0, Level::Low)).expect("rolled over");
        assert_eq!((m.t, m.cu, m.pm, m.pa, m.lv), (120, 60, 30.0, 20.0, Level::Moderate));
        let line = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Minute>(&line).unwrap(), m, "round-trips through the file");
    }

    #[test]
    fn slow_minutes_merge_into_spells() {
        let m = |t, lv, pa| Minute { t, lv, pa, ..Default::default() };
        let mins = vec![m(0, Level::High, 0.0), m(60, Level::High, 150.0), m(120, Level::Low, 0.0), m(300, Level::High, 0.0)];
        let s = spells(&mins);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].start, s[0].end), (0, 120));
        assert_eq!((s[1].start, s[1].end), (300, 360));
    }

    #[test]
    fn history_downsamples_and_keeps_peaks() {
        let ring: VecDeque<PressureSample> = (0..3000).map(|i| sample(10_000 + i * 2, if i == 1234 { 99 } else { 50 }, if i == 2000 { 500.0 } else { 1.0 }, if i == 2000 { Level::Moderate } else { Level::Low })).collect();
        let h = history_of(&ring, &[], None, 8.0, 16_000);
        assert!(h.samples.len() <= MAX_POINTS);
        assert!(h.samples.iter().any(|s| s.commit_used == 99), "peak commit survives");
        assert!(h.samples.iter().any(|s| s.pages_in_per_sec == 500.0), "peak paging survives");
        assert_eq!(h.peak_pages_in, 500.0);
        assert!((h.peak_commit_pct - 99.0).abs() < 1e-9);
        assert_eq!(h.since, 10_000);
    }

    #[test]
    fn growers_measure_growth_from_the_window_low() {
        let mb = 1024 * 1024;
        let snap = |m: u64| HashMap::from([("Code".to_string(), GroupSnap { memory: m * mb, pids: vec![10] })]);
        let groups: VecDeque<_> = vec![(0, snap(500)), (30, snap(400)), (60, snap(900))].into();
        let g = growers_of(&groups);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].growth, 500 * mb);
        assert_eq!(g[0].series.len(), 3);
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_reader_reads_sane_numbers() {
        let mut r = os::Reader::new();
        std::thread::sleep(Duration::from_millis(300));
        let raw = r.read();
        assert!(raw.total > 0 && raw.commit_limit >= raw.total, "{raw:?}");
        assert!(raw.commit_used > 0 && raw.avail <= raw.total, "{raw:?}");
        assert!(raw.pages_in >= 0.0);
    }
}
