# R&D: deeper disk cleanup and memory pressure for dev workloads

Status: research, nothing implemented yet · 2026-11-12

Two questions:

1. **Disk.** What fills a dev machine's system drive beyond the dev caches Dev Cleaner already handles, and what can be removed safely? That covers installer leftovers, Windows Update, old drivers, and their macOS/Linux equivalents.
2. **Memory.** Running VS Code, Claude Code and Docker for hours fills RAM. Windows compresses pages, then pages them out, and the machine gets sluggish. What can Dev Cleaner do about that, and which "RAM cleaner" tricks don't help?

---

## 1. Measured on the reference machine (Windows 11, 62 GB RAM, C: 423 GB)

C: has 326 GB used and 97 GB free. These were the largest items:

| Item | Size | Already covered? | What it is |
|---|---:|---|---|
| `C:\Users\…\CrossDevice\<phone>` | **54.7 GB** | No | Phone Link / Cross Device photo sync: 15k files (41 GB jpg, 12 GB mp4) going back to Mar 2024. User data, not a cache. |
| `C:\pagefile.sys` | **49.3 GB** | No (info only) | Fixed size, not system-managed. Current use is **206 MB** and peak is **230 MB**. A second pagefile on D: is 9 GB. |
| `C:\Program Files (x86)\Steam` + Epic + Ubisoft + ProgramData game data | ~29 GB | No | Installed games. Can be moved to another drive, not cleaned. |
| `C:\Windows\WinSxS` | 15.6 GB (apparent) | Partly (DISM StartComponentCleanup) | Real size is smaller because of hard links. `/ResetBase` frees more but stops installed updates from being uninstalled. |
| `C:\Windows\Installer` | 6.9 GB (304 msi/msp) | No | MSI/MSP cache. Some of these files are usually orphaned (their product was uninstalled). |
| `DriverStore\FileRepository` | 5.9 GB | No | 88 packages. 20 drivers have more than one version and 29 packages are surplus. The surplus is **≈3.7 GB**: 2.7 GB of old NVIDIA display drivers and 0.9 GB of AMD OpenCL. |
| `AppData\Local\Programs` | 11.5 GB | No (apps) | Per-user installed apps. Only their updater caches are fair game. |
| `~/.cache`, `.lmstudio`, `.unsloth`, `.claude` | 28 GB | Mostly yes | Existing Disk / AI pages. |
| npm-cache, Edge, Chrome | 10.6 GB | Yes | Existing. |
| `C:\ProgramData\Package Cache` | 1.6 GB | No | Visual Studio / .NET / VC++ bundle cache. Needed to repair or uninstall. **Don't delete.** |
| NVIDIA installer and downloader leftovers | ~1–1.3 GB | No | `Program Files\NVIDIA Corporation\Installer2`, `ProgramData\NVIDIA Corporation\Downloader`. |
| `C:\Windows\Logs` | 0.9 GB | Partly (CBS/DISM logs) | |

Memory snapshot at the same moment (idle-ish afternoon, Docker stopped):

| Process group | Processes | Private memory |
|---|---:|---:|
| VS Code (`Code`) | 53 | 7.9 GB |
| Chrome | 39 | 5.2 GB |
| Claude Code (`claude`) | 6 | 4.8 GB |
| Edge | 43 | 4.7 GB |
| node (MCP servers, dev servers, tsserver…) | 11 | 2.0 GB |
| hex | 1 | 1.8 GB |
| Memory Compression | 1 | 2.4 GB working set |

Commit charge was 48.6 GB out of a 119.8 GB limit. WSL is capped at 24 GB with `autoMemoryReclaim=gradual`. Three browsers together use 11.7 GB, more than VS Code and Claude Code combined.

---

## 2. Impact table: what to build

Ranked by expected benefit for a typical dev machine, weighed against risk and effort. The Win/Mac/Linux columns show where each feature applies.

| # | Feature | Kind | Typical gain | Risk | Effort | Win | Mac | Linux |
|---|---|---|---|---|---|:-:|:-:|:-:|
| 1 | **Space analyzer** (treemap / top folders per drive, with "what is this" labels) | Disk | Finds the 50 GB surprises (here: CrossDevice, pagefile) | None (read-only) | M | ✅ | ✅ | ✅ |
| 2 | **Memory pressure monitor + "who is growing"** | RAM | Explains the sluggishness and names the cause | None | M | ✅ | ✅ | ✅ |
| 3 | **Orphaned dev processes** (MCP servers, tsserver, dev servers, eslint_d, language servers whose parent editor/agent is gone) | RAM | 0.5–4 GB after long sessions | Low | S–M | ✅ | ✅ | ✅ |
| 4 | **Docker / WSL VM memory**: show VM size vs. actual use, reclaim cache, `.wslconfig` advisor, idle-container list | RAM | 2–16 GB | Low | M | ✅ | ✅ | ~ |
| 5 | **Pagefile / swap advisor** (oversized fixed pagefile, hiberfil, swap location) | Disk | Here 45+ GB | Low (setting change, reboot) | S | ✅ | ~ | ✅ |
| 6 | **Old driver packages** (`pnputil`) | Disk | 1–6 GB (GPU drivers ≈1 GB each) | Low (in-use drivers refuse deletion) | S | ✅ | — | — |
| 7 | **Docker/WSL disk**: `docker system df` + prune, VHDX/Docker.raw compaction, sparse VHD | Disk | 5–100 GB | Low–Med | M | ✅ | ✅ | ✅ |
| 8 | **VS Code memory breakdown** (per window → extension host, tsserver, language servers) + settings advisor | RAM | 1–4 GB | None (advice) / Low (restart host) | M | ✅ | ✅ | ✅ |
| 9 | **Orphaned Windows Installer files** | Disk | 1–10 GB | **Medium**: deleting a needed .msi breaks repair/uninstall, so quarantine, don't delete | M | ✅ | — | — |
| 10 | **Component store `/ResetBase`** + reserved storage info | Disk | 1–5 GB | Medium (can't uninstall updates) | S | ✅ | — | — |
| 11 | **Vendor installer leftovers** (NVIDIA/AMD/Intel downloader, Installer2, `C:\AMD`, `C:\Intel`, Office ClickToRun cache) | Disk | 0.5–3 GB | Low | S | ✅ | — | — |
| 12 | **Old kernels / OS installers** (Linux old kernels; macOS `Install macOS *.app`, `/Library/Updates`) | Disk | 0.5–15 GB | Low–Med | S | — | ✅ | ✅ |
| 13 | **Phone/device backups and sync folders** (CrossDevice, iOS `MobileSync/Backup`, Android Studio AVDs) | Disk | 5–100 GB | None if info-only, since it's user data | S | ✅ | ✅ | ✅ |
| 14 | **Background memory priority** ("Focus mode": low memory priority + EcoQoS for Slack, idle Code windows, Docker) | RAM | Smoother foreground; doesn't free memory | Low (reversible, per process) | M | ✅ | ~ | ✅ |
| 15 | **System Restore / shadow storage** size and cap | Disk | 2–20 GB | Medium | S | ✅ | — | — |
| 16 | Standby-list purge (RAMMap / ISLC style) | RAM | Cosmetic | Low | S | ✅ | — | — |

Recommendation: build 1–6 first. Items 1 and 2 answer "where did my disk/RAM go"; 3–6 fix the problems they show.

---

## 3. Disk: detail and safety rules

### 3.1 Space analyzer (#1)
- Show the top N folders under each drive, and drill into a folder on click. Every known path gets a label from the definitions (`caches.json` already knows hundreds), plus a new `landmarks.json` for things that aren't caches: CrossDevice, Steam libraries, MobileSync backups, VM disks, pagefile.
- Fast scanning: on Windows, read the NTFS MFT when elevated (WizTree's approach, seconds instead of minutes) and fall back to a parallel `jwalk` walk. On macOS and Linux, a parallel walk with hard links counted once and `st_blocks` used for on-disk size. Sparse files such as `Docker.raw` report a huge apparent size.
- Read-only. Each row's action is "open in Explorer/Finder" or "go to the page that cleans this".

### 3.2 Windows
| Target | How to clean | Safety |
|---|---|---|
| Old driver versions | `pnputil /enum-drivers`, group by Original Name and keep the newest version (by date, then version), then `pnputil /delete-driver oemNN.inf` **without** `/force` | Windows refuses to delete a driver that's in use. Never offer `/force`. Admin batch. |
| Orphaned `C:\Windows\Installer` files | Ask MSI which ones are registered (`MsiEnumProductsEx` / `MsiEnumPatchesEx` → `LocalPackage`), and treat every other .msi/.msp as orphaned | Move them to a quarantine folder on another drive. Delete only after N days or on a second confirm. Never touch `$PatchCache$` by hand. |
| Component store | Existing `StartComponentCleanup`, plus optional `/ResetBase` (off by default, with the warning) and `/AnalyzeComponentStore` to show the real reclaimable size first | `/ResetBase` can't be undone. |
| Pagefile | Detect a fixed pagefile much larger than peak usage (`Win32_PageFileUsage.PeakUsage`). Advise system-managed, or a size like 1× RAM on a small drive | Only advise; changing it needs a reboot. Never delete the file. |
| Hibernation | `powercfg /h /type reduced` (keeps Fast Startup at about 20% of RAM) or `/h off` on desktops | Turning it off disables Fast Startup and hibernate. |
| Vendor leftovers | NVIDIA `Installer2`, `Downloader`, `C:\NVIDIA`, `C:\AMD`, `C:\Intel`, ProgramData `Package Cache` (**info only**) | Paths that are only extracted installer payloads. |
| Shadow storage | `vssadmin list shadowstorage`, then offer to resize to a cap | Deleting restore points is irreversible. Never pre-selected. |
| Reserved storage | `DISM /Online /Get-ReservedStorageState`, info only | Leave it alone. Updates fail without it on small drives. |

### 3.3 macOS
- `~/Library/Application Support/MobileSync/Backup` (iPhone backups, often 10–100 GB): info + open in Finder.
- `/Applications/Install macOS *.app` and `/Library/Updates`: delete with a confirm.
- `~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw`: show the real size (sparse), suggest `docker system prune` and lowering the disk limit.
- Swap files in `/private/var/vm`: info only. They shrink after a reboot.
- `brew cleanup -s --prune=all`, and the Xcode Archives and iOS DeviceSupport items Dev Cleaner already has.

### 3.4 Linux
- Old kernels: `apt autoremove --purge` (Debian/Ubuntu), `dnf remove --oldinstallonly` (Fedora), keeping the running kernel and one fallback.
- Core dumps: `/var/lib/systemd/coredump`, `coredumpctl`.
- `/var/lib/docker` (overlay2, build cache): `docker system df` → `docker builder prune`, `docker image prune -a`.
- Trash: `~/.local/share/Trash`. Already covered.

### 3.5 Docker and WSL disks (#7, all OSes)
- `docker system df -v`: images, dangling layers, build cache, stopped containers, volumes (volumes are **never** pre-selected).
- WSL VHDX: `wsl --manage <distro> --set-sparse true` (WSL 2.0+), or compact offline with `wsl --shutdown` then `Optimize-VHD` (needs Hyper-V) or diskpart `compact vdisk`. Show how much space the virtual disk has allocated compared with how much the VM uses.

---

## 4. Memory: why the machine gets sluggish

What happens, in order:
1. Committed memory grows: more VS Code windows, extension hosts, tsserver, Claude Code sessions with their MCP servers, browser tabs, and the Docker/WSL VM.
2. When free and standby memory run out, Windows **compresses** pages it hasn't used lately (the `Memory Compression` process). That's cheap and mostly fine.
3. When compression isn't enough, it **writes pages to the pagefile**. When you switch back to a window, its pages are read back from disk (*hard faults*). That's the lag.

So the fix is to **use less committed memory**, or to keep the foreground app's pages in RAM. **Trimming working sets (what "Free memory" does today) doesn't fix it.** It pushes pages onto the standby/modified lists and into compression. The app then faults them back in, which can make the next switch *slower*. The UI should say this honestly and point people at the features below.

### 4.1 Memory pressure monitor (#2)
A single gauge that shows when the machine is actually short of memory, rather than just that RAM looks full:
- **Windows:** commit charge / commit limit, compressed memory (`Memory Compression` working set, or the `SystemCompressedMemory` performance counter), `Memory\Pages Input/sec` (hard faults read from disk) and pagefile usage. Paging is happening when pages input/sec stays high (>~100/s for a minute).
- **macOS:** `kern.memorystatus_vm_pressure_level` (normal/warn/critical), `vm.swapusage`, compressor size from `vm_stat`.
- **Linux:** PSI, `/proc/pressure/memory` (`some avg10`, `full avg10`). It's the best signal on any OS, plus zram/zswap stats.
- **"Who is growing":** sample private memory per app group every minute and show the slope ("VS Code extension host +1.8 GB in 3 h", "claude (session started 09:12) 2.1 GB"). This catches leaks and runaway long sessions.
- Optional notification when pressure stays high for N minutes, naming the top grower.

### 4.2 Orphaned dev processes (#3)
Long sessions leave behind processes nobody is using:
- MCP servers (`node`, `npx`, `uvx`, `python`, `bun`) whose parent `claude`/`codex`/`cursor` process has exited.
- `tsserver`, `eslint_d`, `rust-analyzer`, `gopls`, Pyright, Java LS (often 1 GB+) from closed VS Code windows.
- Dev servers (`vite`, `next dev`, `webpack`) whose terminal is gone. The Ports page sees half of this already.

Detection: the process tree (parent PID dead, or parent is `explorer`/`init`/`launchd`), plus command-line patterns kept in `processes.json`. Show them as "Orphaned · Safe to end". This fits the existing verdict model.

### 4.3 Docker and WSL VM memory (#4)
- **Windows:** the `VmmemWSL` / `vmmem` working set compared with what the VM reports (`wsl -d <distro> free -b`). The difference is Linux page cache the VM is holding onto.
  - "Reclaim now": `wsl -d <distro> -u root sh -c "sync; echo 1 > /proc/sys/vm/drop_caches"`.
  - `.wslconfig` advisor: `memory=` cap (suggest about 50% of RAM), `autoMemoryReclaim=gradual` (or `dropCache`), `sparseVhd=true`, and a sensible `vmIdleTimeout`. Show the current file and a diff, write it only on confirm, and say that `wsl --shutdown` applies it.
- **macOS:** Docker Desktop / OrbStack / Colima VM memory setting compared with what the VM actually uses.
- **All OSes:** `docker stats --no-stream`, a list of containers by memory with uptime, a "stop" action, and a flag on containers with no `mem_limit`.

### 4.4 VS Code (#8)
- Group `Code` processes by window: the `--type=` argument gives renderer, extensionHost, utility, and `tsserver` (whose arguments include the project path). Show "window *dev-cleaner*: 2.3 GB (tsserver 1.1 GB)".
- Advisor (read the settings, suggest changes, write nothing without asking):
  - `files.watcherExclude` / `search.exclude` for `node_modules`, `target`, `.venv`, `dist`
  - `typescript.tsserver.maxTsServerMemory`
  - `extensions.experimental.affinity` and disabling heavy extensions per workspace
- Actions: none on processes besides end/restart, because a restarted extension host comes back clean.

### 4.5 Background memory priority, "Focus mode" (#14)
Windows lets us lower a process's **memory priority** (`SetProcessInformation(ProcessMemoryPriority, MEMORY_PRIORITY_LOW)`) and enable **EcoQoS** (`ProcessPowerThrottling`). Pages from low-priority processes are trimmed and paged out first, so the app in front stays responsive. It frees nothing, but it chooses *who* pays when memory runs short. It's per process, reversible, and lost when the process restarts. On Linux, the equivalent is `systemd-run --user --scope -p MemoryHigh=…` or cgroup v2 `memory.high`. macOS has no supported way to do this.

### 4.6 What not to build
| Idea | Why not |
|---|---|
| Disabling memory compression (`Disable-MMAgent -mc`) | Swaps a cheap CPU cost for disk I/O and makes things slower. |
| Disabling the pagefile | Commit limit falls to RAM size, so apps crash instead of slowing down. |
| Scheduled "RAM boost" / periodic trimming | Causes the hard faults it claims to prevent. |
| Standby purge as a headline feature (#16) | Standby memory counts as available. Purging it just throws away disk cache. Keep it as a hidden power-user button at most. |
| `purge` / `drop_caches` presented as a fix | Same as above. Keep the existing button, but describe it honestly. |

---

## 5. Advice for this machine (no code needed)

1. **CrossDevice (55 GB):** in Settings → Bluetooth & devices → Mobile devices, stop photo sync, or move the folder to D:/K:. Biggest single win.
2. **Pagefile (49 GB on C:, peak use 230 MB):** set it to system-managed, or about 8–16 GB. Reboot. Gains ~35–45 GB.
3. **Old drivers (≈3.7 GB):** `pnputil /enum-drivers`, then delete the older `nv_dispi` and `amdocl` packages without `/force`.
4. **Memory:** three browsers use 11.7 GB. Turn on Edge sleeping tabs and Chrome Memory Saver, and use one browser for dev. Close idle VS Code windows. Each one carries its own extension host and tsserver. Restart long Claude Code sessions to drop their MCP servers.
5. WSL config is already good (24 GB cap, gradual reclaim, sparse VHD, swap on K:).
