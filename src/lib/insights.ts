// Recommendations from what was actually measured. Every insight names the
// number it is based on; nothing here guesses.
import type { CacheItem, LiveStats, ProcGroup, SystemInfo } from "./api"
import { bytes, duration } from "./format"

export type Insight = {
  id: string
  level: "warn" | "tip" | "ok"
  title: string
  detail: string
  /** A shell command that applies the fix, when there is one. */
  command?: string
  /** A section of the app that helps. */
  go?: "disk" | "projects" | "memory" | "processes" | "ports" | "models"
}

export function insights(s: SystemInfo | null, live: LiveStats | null, caches: CacheItem[], procs: ProcGroup[] | null): Insight[] {
  const out: Insight[] = []
  const win = !!s && /windows/i.test(s.osName)
  const reclaim = caches.reduce((n, c) => n + c.size, 0)

  // Disk
  const sysDrive = live?.disk
  if (sysDrive) {
    const used = 1 - sysDrive.free / sysDrive.total
    if (used > 0.9) {
      out.push({
        id: "disk-full",
        level: "warn",
        title: `${sysDrive.mount} is ${Math.round(used * 100)}% full`,
        detail: `Only ${bytes(sysDrive.free)} free. Windows and macOS slow down when the system drive has under 10% free, and builds start failing.${reclaim ? ` ${bytes(reclaim)} of caches can be cleaned.` : ""}`,
        go: "disk",
      })
    } else if (used > 0.8) {
      out.push({ id: "disk-tight", level: "tip", title: `${sysDrive.mount} is ${Math.round(used * 100)}% full`, detail: `${bytes(sysDrive.free)} free.${reclaim ? ` Cleaning caches gives back ${bytes(reclaim)}.` : ""}`, go: "disk" })
    }
  }
  const big = [...caches].sort((a, b) => b.size - a.size)[0]
  if (big && big.size > 10 * 1024 ** 3) {
    out.push({ id: "big-cache", level: "tip", title: `${big.name} holds ${bytes(big.size)}`, detail: `${big.about}`, go: "disk" })
  }
  for (const d of s?.drives ?? []) {
    if (d.removable || !d.total || (sysDrive && d.mount.toLowerCase() === sysDrive.mount.toLowerCase())) continue
    if (d.free / d.total < 0.05 && d.total > 50 * 1024 ** 3) {
      out.push({ id: `drive-${d.mount}`, level: "tip", title: `${d.mount} has ${bytes(d.free)} left`, detail: `${Math.round((1 - d.free / d.total) * 100)}% of ${bytes(d.total, 0)} used. pnpm keeps a separate store on each drive, and project build folders add up: scan it on the Projects page.`, go: "projects" })
    }
  }

  // Memory
  if (live) {
    const used = live.memUsed / live.memTotal
    const swapUsed = live.swapTotal ? live.swapUsed / live.swapTotal : 0
    if (used > 0.9 || (used > 0.8 && swapUsed > 0.5)) {
      const top = (procs ?? []).filter((p) => !p.protected).slice(0, 3)
      out.push({
        id: "mem-pressure",
        level: "warn",
        title: `Memory is ${Math.round(used * 100)}% used`,
        detail: `${bytes(live.memAvailable)} available${live.swapTotal ? `, page file ${Math.round(swapUsed * 100)}% used` : ""}. When RAM runs out the system pages to disk and everything slows.${top.length ? ` Biggest apps: ${top.map((p) => `${p.name} ${bytes(p.memory)}`).join(", ")}.` : ""}`,
        go: "memory",
      })
    }
  }
  if (s?.vmMemory && s.vmMemory > 4 * 1024 ** 3) {
    out.push({
      id: "wsl-mem",
      level: "tip",
      title: `WSL / Docker VM holds ${bytes(s.vmMemory)}`,
      detail: "The Linux VM keeps memory it no longer needs. Shut it down when you're not using it, or cap it with memory= in %USERPROFILE%\\.wslconfig.",
      command: "wsl --shutdown",
    })
  }
  if (win && s?.compression === false) {
    out.push({ id: "compression-off", level: "tip", title: "Memory compression is off", detail: "Turning it on lets Windows keep more in RAM instead of paging to disk. Needs an admin PowerShell and a restart.", command: "Enable-MMAgent -MemoryCompression" })
  }
  if (s && s.memTotal >= 16 * 1024 ** 3 && s.pageFiles.length) {
    const sizes = s.pageFiles.reduce((n, p) => n + p.size, 0)
    const used = s.pageFiles.reduce((n, p) => n + p.used, 0)
    const onSys = s.pageFiles.some((p) => sysDrive && p.path.toLowerCase().startsWith(sysDrive.mount.toLowerCase()))
    if (onSys && sysDrive && sysDrive.free / sysDrive.total < 0.1 && sizes > 16 * 1024 ** 3) {
      out.push({ id: "pagefile", level: "tip", title: `Page file takes ${bytes(sizes, 0)} on a full system drive`, detail: `Only ${bytes(used)} of it is in use. With ${bytes(s.memTotal, 0)} of RAM you can move it to a roomier drive or set a smaller fixed size in System Properties → Advanced → Performance → Virtual memory.` })
    }
  }

  // CPU
  const hog = (procs ?? []).filter((p) => !p.protected).sort((a, b) => b.cpu - a.cpu)[0]
  if (hog && hog.cpu > 40) {
    out.push({ id: "cpu-hog", level: "tip", title: `${hog.name} is using ${hog.cpu.toFixed(0)}% CPU`, detail: `${hog.reason}. If it's a stuck dev server or build, ending it frees the CPU.`, go: "processes" })
  }
  const nodes = (procs ?? []).find((p) => p.name.toLowerCase() === "node")
  if (nodes && nodes.pids.length >= 8) {
    out.push({ id: "node-swarm", level: "tip", title: `${nodes.pids.length} node processes running`, detail: `Together they use ${bytes(nodes.memory)}. Forgotten dev servers often hold ports too.`, go: "ports" })
  }

  // Windows developer settings
  if (win && s) {
    if (s.longPaths === false) out.push({ id: "longpaths", level: "warn", title: "Long paths are disabled", detail: "Deep node_modules trees go past 260 characters, and installs or deletes fail. Needs an admin PowerShell.", command: 'New-ItemProperty -Path "HKLM:\\SYSTEM\\CurrentControlSet\\Control\\FileSystem" -Name LongPathsEnabled -Value 1 -PropertyType DWORD -Force' })
    if (s.devDrive === false) out.push({ id: "devdrive", level: "tip", title: "No Dev Drive", detail: "A Dev Drive (ReFS with Defender in performance mode) makes installs and builds noticeably faster on Windows 11. Create one in Settings → System → Storage → Disks & volumes, then keep repos and package caches on it." })
    if (s.powerPlan && /saver/i.test(s.powerPlan) && !s.battery) out.push({ id: "power", level: "tip", title: `Power plan: ${s.powerPlan}`, detail: "On a desktop, Balanced or High performance keeps the CPU from being held back.", command: "powercfg /setactive SCHEME_BALANCED" })
    if (s.developerMode === false) out.push({ id: "devmode", level: "tip", title: "Developer Mode is off", detail: "Without it, creating symlinks needs admin rights; pnpm and some build tools fall back to slower copies." })
  }
  if (s && s.uptime > 14 * 86400) {
    out.push({ id: "uptime", level: "tip", title: `Up for ${duration(s.uptime)}`, detail: "A restart applies pending updates and clears memory that leaked over time." })
  }
  if (s?.battery && s.battery.health > 0 && s.battery.health < 80) {
    out.push({ id: "battery", level: "tip", title: `Battery holds ${s.battery.health.toFixed(0)}% of its design capacity`, detail: "Below 80% a battery is considered worn; expect shorter runtimes." })
  }

  if (!out.length && s && live) out.push({ id: "ok", level: "ok", title: "Nothing needs attention", detail: "Disk, memory and CPU look healthy." })
  const rank = { warn: 0, tip: 1, ok: 2 }
  return out.sort((a, b) => rank[a.level] - rank[b.level])
}
