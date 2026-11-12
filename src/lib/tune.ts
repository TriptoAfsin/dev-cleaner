// Deep clean (System junk) and Responsiveness: types and backend calls.
import { invoke } from "@tauri-apps/api/core"

// ------------------------------------------------------------ deep clean

export type DriverPkg = { published: string; version: string; date: string; size: number; keep: boolean }
export type DriverGroup = { key: string; original: string; provider: string; class: string; versions: DriverPkg[]; reclaim: number }
export type InstallerFile = { path: string; name: string; size: number; modified: number }
export type InstallerInfo = { checked: boolean; totalFiles: number; totalSize: number; orphans: InstallerFile[]; orphanSize: number }
export type PageFileStat = { path: string; allocated: number; current: number; peak: number; initial: number; maximum: number }
export type PageOption = { mode: "fixed" | "system"; gb: number; label: string; detail: string; saves: number; recommended: boolean }
export type PagefileInfo = {
  files: PageFileStat[]
  automatic: boolean
  ram: number
  commitPeak: number
  system: PageFileStat | null
  oversized: boolean
  reason: string
  options: PageOption[]
}
export type QuarantineBatch = { id: string; created: number; files: number; size: number; path: string; old: boolean }
export type DeepScan = {
  supported: boolean
  dryRun: boolean
  drivers: DriverGroup[]
  driversRead: boolean
  installer: InstallerInfo
  pagefile: PagefileInfo
  quarantine: QuarantineBatch[]
}
export type DeepPlan = {
  drivers: string[]
  installers: string[]
  pagefile: { mode: "fixed" | "system"; gb: number } | null
  component: boolean
  resetBase: boolean
  deleteQuarantine: string[]
}
export type DeepResult = {
  ran: boolean
  dryRun: boolean
  driversRemoved: string[]
  driversRefused: string[]
  freed: number
  quarantined: number
  quarantineId: string | null
  pagefileSet: boolean
  restartNeeded: boolean
  notes: string[]
  scan: DeepScan
}
export type QuarantineResult = { restored: number; dryRun: boolean }

// ------------------------------------------------------------ responsiveness

export type TuneState = "applied" | "available" | "na"
export type TuneItem = {
  id: string
  title: string
  evidence: string
  impact: 1 | 2 | 3
  admin: boolean
  note: string
  state: TuneState
  naReason: string | null
  detail: string
  code: string
  link: string | null
}
export type TuneOverview = { items: TuneItem[]; good: string[]; startupEnabled: number | null; dryRun: boolean }
export type JournalEntry = {
  id: string
  tweak: string
  title: string
  summary: string
  appliedAt: number
  undoneAt: number | null
  undoNote: string | null
}
export type TuneOutcome = { id: string; ok: boolean; message: string; entryId: string | null; dryRun: boolean }

/** From the memory-pressure sampler (Memory page). */
export type PressureSample = {
  t: number
  commitUsed: number
  commitLimit: number
  memAvailable: number
  memTotal: number
  pagesInPerSec: number
  compressed: number | null
  pagefileUsed: number | null
  level: "low" | "moderate" | "high"
}
export type PressureHistory = {
  samples: PressureSample[]
  since: number
  slowSpells: { start: number; end: number }[]
  peakCommitPct: number
  peakPagesIn: number
}

export const tuneApi = {
  deepScan: () => invoke<DeepScan>("deep_scan"),
  deepRun: (plan: DeepPlan) => invoke<DeepResult>("deep_run", { plan }),
  quarantineList: () => invoke<QuarantineBatch[]>("quarantine_list"),
  quarantineRestore: (id: string) => invoke<QuarantineResult>("quarantine_restore", { id }),
  list: () => invoke<TuneOverview>("tune_list"),
  apply: (ids: string[]) => invoke<TuneOutcome[]>("tune_apply", { ids }),
  undo: (entryId: string) => invoke<TuneOutcome>("tune_undo", { entryId }),
  history: () => invoke<JournalEntry[]>("tune_history"),
  /** Null until the sampler exists or has data. */
  pressure: async (hours: number): Promise<PressureHistory | null> => {
    try {
      const h = await invoke<PressureHistory | null>("pressure_history", { hours })
      return h && Array.isArray(h.samples) ? h : null
    } catch {
      return null
    }
  },
  wslShutdown: () => invoke<unknown>("wsl_shutdown"),
}

/** Session cache so switching tabs doesn't rescan. */
export const deepCache: { scan: DeepScan | null } = { scan: null }
