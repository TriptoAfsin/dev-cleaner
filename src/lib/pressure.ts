// Memory pressure, who is growing, orphaned dev processes, the WSL VM and
// Docker containers. Backend: src-tauri/src/pressure.rs and orphans.rs.
import { invoke } from "@tauri-apps/api/core"
import type { Verdict } from "./api"

export type PressureLevel = "low" | "moderate" | "high"
export type PressureSample = {
  /** Unix seconds. */
  t: number
  commitUsed: number
  commitLimit: number
  memAvailable: number
  memTotal: number
  pagesInPerSec: number
  compressed: number | null
  pagefileUsed: number | null
  level: PressureLevel
}
export type PressureHistory = {
  /** Downsampled to at most ~600 points. */
  samples: PressureSample[]
  since: number
  slowSpells: { start: number; end: number }[]
  peakCommitPct: number
  peakPagesIn: number
}
export type Grower = {
  name: string
  pids: number[]
  memory: number
  growth: number
  series: number[]
  verdict: Verdict
  reason: string
  protected: boolean
}

export type OrphanGroup = "mcp" | "lsp" | "dev" | "other"
export type Orphan = {
  pid: number
  name: string
  group: OrphanGroup
  memory: number
  startedAt: number
  cmd: string
  why: string
  verdict: Verdict
  reason: string
  protected: boolean
}

export type WslDistroState = { name: string; running: boolean; version: string; default: boolean }
export type LinuxMem = { total: number; used: number; cache: number; free: number }
export type WslConfig = {
  path: string
  exists: boolean
  memory: string | null
  processors: string | null
  swap: string | null
  autoMemoryReclaim: string | null
  sparseVhd: string | null
  raw: string
}
export type WslStatus = {
  available: boolean
  running: boolean
  distros: WslDistroState[]
  vmMemory: number | null
  linux: LinuxMem | null
  config: WslConfig
}

export type Container = {
  id: string
  name: string
  image: string
  status: string
  state: string
  memory: number | null
  memoryLimit: number | null
  cpu: number | null
}
export type Containers = { available: boolean; running: boolean; error: string | null; items: Container[] }

export const pressureApi = {
  now: () => invoke<PressureSample>("pressure_now"),
  history: (hours: number) => invoke<PressureHistory>("pressure_history", { hours }),
  growth: () => invoke<Grower[]>("memory_growth"),
  orphans: () => invoke<Orphan[]>("list_orphans"),
  wslStatus: () => invoke<WslStatus>("wsl_status"),
  wslReclaim: () => invoke<WslStatus>("wsl_reclaim"),
  wslShutdown: () => invoke<WslStatus>("wsl_shutdown"),
  containers: () => invoke<Containers>("list_containers"),
  stopContainers: (ids: string[]) => invoke<number>("stop_containers", { ids }),
}

export const ORPHAN_GROUPS: Record<OrphanGroup, string> = {
  mcp: "Claude Code / MCP servers",
  lsp: "Language servers",
  dev: "Dev servers",
  other: "Other dev runtimes",
}

/** The settings Dev Cleaner suggests for ~/.wslconfig. */
export const WSL_SUGGESTED = { autoMemoryReclaim: "dropCache", sparseVhd: "true" }

/** What is missing from the user's .wslconfig, as diff lines. */
export function wslconfigDiff(c: WslConfig | null | undefined): { kind: "same" | "add" | "del"; text: string }[] {
  const lines: { kind: "same" | "add" | "del"; text: string }[] = []
  if (c?.memory) lines.push({ kind: "same", text: `memory=${c.memory}` })
  const amr = c?.autoMemoryReclaim
  // gradual also hands memory back, just more slowly; either is fine.
  if (amr && ["dropcache", "gradual"].includes(amr.toLowerCase())) lines.push({ kind: "same", text: `autoMemoryReclaim=${amr}` })
  else {
    if (amr) lines.push({ kind: "del", text: `autoMemoryReclaim=${amr}` })
    lines.push({ kind: "add", text: "autoMemoryReclaim=dropCache" })
  }
  const sv = c?.sparseVhd
  if (sv && sv.toLowerCase() === "true") lines.push({ kind: "same", text: `sparseVhd=${sv}` })
  else {
    if (sv) lines.push({ kind: "del", text: `sparseVhd=${sv}` })
    lines.push({ kind: "add", text: "sparseVhd=true" })
  }
  return lines
}

export const LEVEL_TEXT: Record<PressureLevel, string> = { low: "Low", moderate: "Moderate", high: "High" }
export const LEVEL_COLOR: Record<PressureLevel, string> = { low: "var(--ok)", moderate: "var(--warn)", high: "var(--danger)" }
