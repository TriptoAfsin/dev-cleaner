//! Which processes keep files open, so a clean that had to leave files
//! behind can say why ("held by chrome, Code"). Windows asks the Restart
//! Manager (the API behind "this file is open in another program"); Linux
//! reads /proc/*/fd; macOS asks lsof.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Holder {
    pub name: String,
    pub pid: u32,
    /// Files of the set this process holds, when the OS can tell (0 = unknown).
    pub files: u32,
}

/// Files under `path` that are still there: what a failed removal left.
pub fn leftovers(path: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        match std::fs::symlink_metadata(&d) {
            Ok(m) if m.is_dir() => {
                if let Ok(rd) = std::fs::read_dir(&d) {
                    stack.extend(rd.flatten().map(|e| e.path()));
                }
            }
            Ok(_) => out.push(d),
            Err(_) => {}
        }
    }
    out
}

/// Processes holding any of these files, busiest first.
pub fn holders(files: &[PathBuf]) -> Vec<Holder> {
    if files.is_empty() {
        return vec![];
    }
    let mut v = os_holders(files);
    v.sort_by(|a, b| b.files.cmp(&a.files).then(a.name.cmp(&b.name)));
    v
}

#[cfg(windows)]
fn os_holders(files: &[PathBuf]) -> Vec<Holder> {
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY, RM_PROCESS_INFO,
    };
    let mut out: Vec<Holder> = vec![];
    // The Restart Manager answers for a whole set; ask per batch so a few
    // hundred files stay quick.
    for batch in files.chunks(200) {
        let wide: Vec<Vec<u16>> = batch.iter().map(|p| p.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect()).collect();
        let ptrs: Vec<*const u16> = wide.iter().map(|w| w.as_ptr()).collect();
        let mut session = 0u32;
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        // SAFETY: plain Restart Manager calls; every buffer is ours and sized
        // as the API documents, and the session is always ended.
        unsafe {
            if RmStartSession(&mut session, 0, key.as_mut_ptr()) != 0 {
                continue;
            }
            if RmRegisterResources(session, ptrs.len() as u32, ptrs.as_ptr(), 0, std::ptr::null(), 0, std::ptr::null()) == 0 {
                let mut needed = 0u32;
                let mut count = 0u32;
                let mut reasons = 0u32;
                let mut infos: Vec<RM_PROCESS_INFO> = vec![];
                for _ in 0..3 {
                    count = infos.len() as u32;
                    let r = RmGetList(session, &mut needed, &mut count, if infos.is_empty() { std::ptr::null_mut() } else { infos.as_mut_ptr() }, &mut reasons);
                    if r == 234 {
                        // ERROR_MORE_DATA
                        infos = vec![std::mem::zeroed(); needed as usize + 4];
                        continue;
                    }
                    if r != 0 {
                        count = 0;
                    }
                    break;
                }
                for info in infos.iter().take(count as usize) {
                    let len = info.strAppName.iter().position(|c| *c == 0).unwrap_or(info.strAppName.len());
                    let name = String::from_utf16_lossy(&info.strAppName[..len]);
                    let pid = info.Process.dwProcessId;
                    if !out.iter().any(|h| h.pid == pid) {
                        out.push(Holder { name, pid, files: 0 });
                    }
                }
            }
            RmEndSession(session);
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn os_holders(files: &[PathBuf]) -> Vec<Holder> {
    use std::collections::{HashMap, HashSet};
    let wanted: HashSet<PathBuf> = files.iter().filter_map(|p| std::fs::canonicalize(p).ok()).collect();
    let mut counts: HashMap<u32, u32> = HashMap::new();
    if let Ok(procs) = std::fs::read_dir("/proc") {
        for p in procs.flatten() {
            let Ok(pid) = p.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
            for fd in fds.flatten() {
                if std::fs::read_link(fd.path()).is_ok_and(|t| wanted.contains(&t)) {
                    *counts.entry(pid).or_default() += 1;
                }
            }
        }
    }
    counts
        .into_iter()
        .map(|(pid, n)| Holder {
            name: std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|s| s.trim().to_string()).unwrap_or_default(),
            pid,
            files: n,
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn os_holders(files: &[PathBuf]) -> Vec<Holder> {
    let mut args: Vec<String> = vec!["-F".into(), "pc".into()];
    args.extend(files.iter().take(200).map(|p| p.to_string_lossy().to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(out) = crate::util::run_capture(Path::new("/usr/sbin/lsof"), &refs, std::time::Duration::from_secs(15)) else {
        return vec![];
    };
    parse_lsof(&out)
}

/// `lsof -F pc` output: "p<pid>" then "c<command>" per process, then file
/// lines ("f..."), each counted as one file held.
#[cfg(any(target_os = "macos", test))]
fn parse_lsof(out: &str) -> Vec<Holder> {
    let mut v: Vec<Holder> = vec![];
    for line in out.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            if let Ok(pid) = pid.parse() {
                v.push(Holder { name: String::new(), pid, files: 0 });
            }
        } else if let Some(cmd) = line.strip_prefix('c') {
            if let Some(h) = v.last_mut() {
                h.name = cmd.to_string();
            }
        } else if line.starts_with('f') {
            if let Some(h) = v.last_mut() {
                h.files += 1;
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsof_field_output() {
        let v = parse_lsof("p123\ncGoogle Chrome\nf12\nf13\np77\ncnode\nf4\n");
        assert_eq!(v, vec![
            Holder { name: "Google Chrome".into(), pid: 123, files: 2 },
            Holder { name: "node".into(), pid: 77, files: 1 },
        ]);
    }

    #[test]
    fn finds_leftover_files_in_nested_folders() {
        let root = std::env::temp_dir().join(format!("dc-left-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/x.tmp"), b"x").unwrap();
        std::fs::write(root.join("y.tmp"), b"y").unwrap();
        assert_eq!(leftovers(&root, 10).len(), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn restart_manager_names_the_process_holding_a_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let p = std::env::temp_dir().join(format!("dc-held-{}.lock", std::process::id()));
        // Open without FILE_SHARE_DELETE, as an app holding a temp file would.
        let f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(&p).unwrap();
        let held = holders(std::slice::from_ref(&p));
        drop(f);
        let _ = std::fs::remove_file(&p);
        assert!(held.iter().any(|h| h.pid == std::process::id()), "this test process holds the file: {held:?}");
    }
}
