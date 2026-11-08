//! Which TCP ports are listening, and which process holds each. A port is
//! freed by ending its process.

use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Ports developers reach for most, checked even when nothing listens on them
/// so the list can say "free".
pub const COMMON: &[u16] = &[
    3000, 3001, 4200, 4321, 5000, 5001, 5173, 5174, 5432, 6006, 6379, 8000, 8080, 8081, 8443, 8888, 9000,
    9229, 27017, 3306, 1420, 11434,
];

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PortState {
    InUse,
    Free,
    System,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PortEntry {
    pub port: u16,
    pub state: PortState,
    pub pid: Option<u32>,
    pub process: Option<String>,
    pub addresses: Vec<String>,
    pub memory: Option<u64>,
    pub exe: Option<String>,
    pub cmd: Option<String>,
    pub hint: Option<String>,
    pub common: bool,
}

struct Listener {
    port: u16,
    addr: String,
    pid: u32,
}

pub fn list(all: bool, extra: &[u16]) -> Vec<PortEntry> {
    let listeners = listeners();
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory().with_exe(sysinfo::UpdateKind::OnlyIfNotSet).with_cmd(sysinfo::UpdateKind::OnlyIfNotSet),
    );

    let mut by_port: BTreeMap<u16, (u32, Vec<String>)> = BTreeMap::new();
    for l in listeners {
        let e = by_port.entry(l.port).or_insert((l.pid, vec![]));
        if !e.1.contains(&l.addr) {
            e.1.push(l.addr);
        }
    }

    let wanted: Vec<u16> = COMMON.iter().chain(extra.iter()).copied().collect();
    let mut out: HashMap<u16, PortEntry> = HashMap::new();
    for (&port, (pid, addrs)) in &by_port {
        let common = wanted.contains(&port);
        if !all && !common {
            continue;
        }
        let p = sys.process(Pid::from_u32(*pid));
        let name = p.map(|p| p.name().to_string_lossy().trim_end_matches(".exe").to_string());
        let system = *pid <= 4
            || name.as_deref().is_some_and(|n| crate::monitor::classify(n, *pid).0 == crate::monitor::Verdict::System);
        let cmd = p.map(|p| {
            p.cmd().iter().skip(1).map(|s| s.to_string_lossy()).collect::<Vec<_>>().join(" ")
        });
        out.insert(
            port,
            PortEntry {
                port,
                state: if system { PortState::System } else { PortState::InUse },
                pid: Some(*pid),
                hint: hint(name.as_deref().unwrap_or(""), cmd.as_deref().unwrap_or(""), port),
                process: name,
                addresses: addrs.clone(),
                memory: p.map(|p| p.memory()),
                exe: p.and_then(|p| p.exe()).map(|e| e.to_string_lossy().to_string()),
                cmd: cmd.filter(|c| !c.is_empty()),
                common,
            },
        );
    }
    for &port in &wanted {
        out.entry(port).or_insert(PortEntry {
            port,
            state: PortState::Free,
            pid: None,
            process: None,
            addresses: vec![],
            memory: None,
            exe: None,
            cmd: None,
            hint: None,
            common: true,
        });
    }
    let mut v: Vec<PortEntry> = out.into_values().collect();
    // In use first, then free, then system; by port within each.
    v.sort_by_key(|e| {
        (
            match e.state {
                PortState::InUse => 0,
                PortState::Free => 1,
                PortState::System => 2,
            },
            e.port,
        )
    });
    v
}

/// A friendly guess at what is behind a port ("vite", "Postgres").
fn hint(name: &str, cmd: &str, port: u16) -> Option<String> {
    let c = cmd.to_ascii_lowercase();
    for (needle, label) in [
        ("vite", "Vite"), ("next", "Next.js"), ("nuxt", "Nuxt"), ("astro", "Astro"), ("webpack", "webpack"),
        ("storybook", "Storybook"), ("tauri", "Tauri"), ("uvicorn", "uvicorn"), ("flask", "Flask"),
        ("manage.py", "Django"), ("rails", "Rails"), ("jupyter", "Jupyter"), ("--inspect", "Node debugger"),
    ] {
        if c.contains(needle) {
            return Some(label.into());
        }
    }
    let n = name.to_ascii_lowercase();
    let by_name = match n.as_str() {
        "postgres" => Some("PostgreSQL"),
        "mysqld" => Some("MySQL"),
        "mongod" => Some("MongoDB"),
        "redis-server" => Some("Redis"),
        "ollama" => Some("Ollama"),
        "com.docker.backend" | "docker desktop" | "wslrelay" => Some("Docker / WSL forward"),
        _ => None,
    };
    by_name.or(match port {
        5432 => Some("PostgreSQL port"),
        6379 => Some("Redis port"),
        27017 => Some("MongoDB port"),
        3306 => Some("MySQL port"),
        11434 => Some("Ollama port"),
        _ => None,
    })
    .map(String::from)
}

#[cfg(windows)]
fn listeners() -> Vec<Listener> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    /// One table for one address family. Retries if it grows between the size
    /// query and the read. u32 elements keep the buffer aligned.
    unsafe fn table(family: u32) -> Option<Vec<u32>> {
        let mut size: u32 = 0;
        GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0);
        for _ in 0..4 {
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u32; (size as usize).div_ceil(4)];
            match GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) {
                0 => return Some(buf),
                122 => continue, // ERROR_INSUFFICIENT_BUFFER
                _ => return None,
            }
        }
        None
    }

    let mut out = vec![];
    // SAFETY: the buffers are laid out by the OS as the table structs below.
    unsafe {
        if let Some(buf) = table(AF_INET as u32) {
            let t = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            for e in std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize) {
                out.push(Listener {
                    port: u16::from_be(e.dwLocalPort as u16),
                    addr: std::net::Ipv4Addr::from(u32::from_be(e.dwLocalAddr)).to_string(),
                    pid: e.dwOwningPid,
                });
            }
        }
        if let Some(buf) = table(AF_INET6 as u32) {
            let t = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
            for e in std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize) {
                out.push(Listener {
                    port: u16::from_be(e.dwLocalPort as u16),
                    addr: std::net::Ipv6Addr::from(e.ucLocalAddr).to_string(),
                    pid: e.dwOwningPid,
                });
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn listeners() -> Vec<Listener> {
    let Some(out) = crate::util::run_capture(
        std::path::Path::new("/usr/sbin/lsof"),
        &["-nP", "-iTCP", "-sTCP:LISTEN"],
        std::time::Duration::from_secs(8),
    ) else {
        return vec![];
    };
    out.lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let pid = parts.get(1)?.parse().ok()?;
            let name = parts.get(8)?;
            let (addr, port) = name.rsplit_once(':')?;
            Some(Listener { port: port.parse().ok()?, addr: addr.trim_matches(['[', ']']).to_string(), pid })
        })
        .collect()
}
