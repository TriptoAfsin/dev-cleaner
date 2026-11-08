import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export type DiskStat = { mount: string; total: number; free: number }
export type LiveStats = {
  cpu: number
  cores: number[]
  memTotal: number
  memUsed: number
  memAvailable: number
  swapTotal: number
  swapUsed: number
  compressed: number | null
  disk: DiskStat | null
  processCount: number
  uptime: number
}

export type Verdict = "safe" | "saveFirst" | "system" | "unknown"
export type ProcGroup = {
  name: string
  pids: number[]
  memory: number
  cpu: number
  verdict: Verdict
  reason: string
  protected: boolean
  exe: string | null
}
export type KillResult = { ended: number; failed: number; freed: number }
export type FreeResult = { before: number; after: number; trimmed: number; note: string }

export type Risk = "safe" | "slow" | "data"
export type CacheItem = {
  id: string
  name: string
  eco: string
  about: string
  risk: Risk
  paths: string[]
  size: number
  files: number
  method: string
  found: boolean
  /** Listed on the System junk page rather than Disk cleanup. */
  junk: boolean
  /** Needs admin rights; cleaned in one elevated batch. */
  admin: boolean
  /** False when the size is only known after cleaning. */
  measured: boolean
}
export type Privileges = {
  os: string
  isAdmin: boolean
  canRelaunch: boolean
  fullDiskAccess: boolean | null
  pkexec: boolean
  inFlatpak: boolean
}
export type Holder = { name: string; pid: number; files: number }
/** Live progress for the item being cleaned. */
export type CleanTick = { id: string; stage: string; bytes: number; files: number; elapsedMs: number }
export type CleanEvent = {
  id: string
  elapsedMs?: number
  phase: "running" | "done" | "failed"
  freed: number
  skipped: number
  note: string
  /** Left alone on purpose: newer than the age limit. */
  kept?: number
  /** Could not be removed, usually open in another program. */
  inUse?: number
  inUseSample?: string[]
  heldBy?: Holder[]
}

export type PortState = "inUse" | "free" | "system"
export type PortEntry = {
  port: number
  state: PortState
  pid: number | null
  process: string | null
  addresses: string[]
  memory: number | null
  exe: string | null
  cmd: string | null
  hint: string | null
  common: boolean
}

export type Tool = { name: string; version: string | null; path: string | null; installed: boolean }
export type PageFile = { path: string; size: number; used: number }
export type Drive = { mount: string; name: string; fs: string; kind: string; total: number; free: number; removable: boolean }
export type Battery = {
  percent: number
  state: string
  health: number
  cycles: number | null
  timeToEmptyMins: number | null
  timeToFullMins: number | null
  vendor: string | null
  model: string | null
}
export type WslDistro = { name: string; state: string; version: string; default: boolean }
export type SystemInfo = {
  osName: string
  osVersion: string
  kernel: string
  hostname: string
  arch: string
  uptime: number
  cpuBrand: string
  cpuCores: number
  cpuThreads: number
  cpuFreqMhz: number
  memTotal: number
  swapTotal: number
  swapUsed: number
  compression: boolean | null
  compressedBytes: number | null
  pageFiles: PageFile[]
  gpus: string[]
  vmMemory: number | null
  powerPlan: string | null
  devDrive: boolean | null
  drives: Drive[]
  battery: Battery | null
  developerMode: boolean | null
  longPaths: boolean | null
  isAdmin: boolean
  wslVersion: string | null
  wsl: WslDistro[]
  runtimes: Tool[]
  editors: Tool[]
  aiTools: Tool[]
}

export type Artifact = {
  path: string
  project: string
  projectPath: string
  kind: string
  label: string
  restore: string
  size: number
  files: number
  modified: number
}
export type DeleteEvent = { path: string; ok: boolean; freed: number; error: string | null }

export type AppInfo = {
  version: string
  os: string
  arch: string
  installedVia: string
  upgradeCommand: string | null
  repo: string
}
export type UpdateInfo = { current: string; latest: string; newer: boolean; url: string; publishedAt: string; notes: string }

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window

export const api = {
  inTauri,
  liveStats: () => invoke<LiveStats>("live_stats"),
  listProcesses: () => invoke<ProcGroup[]>("list_processes"),
  killProcesses: (pids: number[]) => invoke<KillResult>("kill_processes", { pids }),
  freeMemory: () => invoke<FreeResult>("free_memory"),
  scanCaches: (drives?: string[] | null) => invoke<CacheItem[]>("scan_caches", { drives: drives ?? null }),
  cleanCaches: (ids: string[]) => invoke<CleanEvent[]>("clean_caches", { ids }),
  listPorts: (all: boolean, extra: number[]) => invoke<PortEntry[]>("list_ports", { all, extra }),
  systemInfo: () => invoke<SystemInfo>("system_info"),
  scanProjects: (roots: string[], depth?: number) => invoke<Artifact[]>("scan_projects", { roots, depth }),
  listDrives: () => invoke<Drive[]>("list_drives"),
  cancelProjectScan: () => invoke<void>("cancel_project_scan"),
  deleteArtifacts: (paths: string[]) => invoke<DeleteEvent[]>("delete_artifacts", { paths }),
  isAdmin: () => invoke<boolean>("is_admin"),
  relaunchAsAdmin: () => invoke<boolean>("relaunch_as_admin"),
  privilegesInfo: () => invoke<Privileges>("privileges_info"),
  openFullDiskAccess: () => invoke<boolean>("open_full_disk_access"),
  platform: () => invoke<string>("platform"),
  appInfo: () => invoke<AppInfo>("app_info"),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  loadSettings: () => invoke<Record<string, unknown> | null>("load_settings"),
  saveSettings: (settings: unknown) => invoke<void>("save_settings", { settings }),
  saveFile: (path: string, base64: string) => invoke<void>("save_file", { path, base64 }),
  revealWindow: () => invoke<boolean>("reveal_window"),
  settleWindow: () => invoke<void>("settle_window"),
}

export function on<T>(event: string, cb: (payload: T) => void): () => void {
  let un: UnlistenFn | undefined
  let dead = false
  void listen<T>(event, (e) => cb(e.payload)).then((u) => {
    if (dead) u()
    else un = u
  })
  return () => {
    dead = true
    un?.()
  }
}
