// App-wide state: live machine stats, the cache scan, the activity shown in
// the status bar, and toasts. Pages read from here so switching pages never
// throws away a scan or a clean in progress.
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react"
import { api, on, type CacheItem, type CleanEvent, type LiveStats } from "./api"
import { bytes } from "./format"

export type Activity = { kind: "idle" | "busy" | "ok" | "error"; text: string }
export type Toast = { id: number; kind: "ok" | "error" | "info"; title: string; detail?: string; action?: { label: string; run: () => void } }
export type Report = {
  kind: "caches" | "projects" | "models"
  title: string
  total: number
  items: { name: string; icon: string; freed: number }[]
  at: number
}

type Ctx = {
  stats: LiveStats | null
  cpuHistory: number[]
  memHistory: number[]
  caches: CacheItem[]
  scanning: boolean
  scanTotal: number
  lastScan: number | null
  scan: () => Promise<void>
  cleaning: Record<string, CleanEvent>
  clean: (ids: string[]) => Promise<number>
  activity: Activity
  setActivity: (a: Activity) => void
  toasts: Toast[]
  toast: (t: Omit<Toast, "id">) => void
  dismiss: (id: number) => void
  admin: boolean
  platform: string
  portsInUse: number[]
  setPortsInUse: (p: number[]) => void
  report: Report | null
  openReport: (r: Report) => void
  closeReport: () => void
}

const StoreCtx = createContext<Ctx | null>(null)
const HISTORY = 60

export function StoreProvider({ children }: { children: ReactNode }) {
  const [stats, setStats] = useState<LiveStats | null>(null)
  const [cpuHistory, setCpu] = useState<number[]>([])
  const [memHistory, setMem] = useState<number[]>([])
  const [caches, setCaches] = useState<CacheItem[]>([])
  const [scanning, setScanning] = useState(false)
  const [scanTotal, setScanTotal] = useState(0)
  const [lastScan, setLastScan] = useState<number | null>(null)
  const [cleaning, setCleaning] = useState<Record<string, CleanEvent>>({})
  const [activity, setActivity] = useState<Activity>({ kind: "idle", text: "Ready" })
  const [toasts, setToasts] = useState<Toast[]>([])
  const [admin, setAdmin] = useState(false)
  const [platform, setPlatform] = useState("windows")
  const [portsInUse, setPortsInUse] = useState<number[]>([])
  const toastId = useRef(0)
  const [report, setReport] = useState<Report | null>(null)
  const openReport = useCallback((r: Report) => setReport(r), [])
  const closeReport = useCallback(() => setReport(null), [])

  // Live stats every 1.5 s, paused while the window is hidden.
  useEffect(() => {
    if (!api.inTauri) return
    let alive = true
    const tick = async () => {
      if (document.hidden) return
      try {
        const s = await api.liveStats()
        if (!alive) return
        setStats(s)
        setCpu((h) => [...h, s.cpu].slice(-HISTORY))
        setMem((h) => [...h, (s.memUsed / s.memTotal) * 100].slice(-HISTORY))
      } catch {
        /* next tick retries */
      }
    }
    void tick()
    const t = setInterval(tick, 1500)
    void api.isAdmin().then(setAdmin).catch(() => {})
    void api.platform().then(setPlatform).catch(() => {})
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])

  const toast = useCallback((t: Omit<Toast, "id">) => {
    const id = ++toastId.current
    setToasts((ts) => [...ts.slice(-3), { ...t, id }])
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), t.kind === "error" || t.action ? 9000 : 4500)
  }, [])
  const dismiss = useCallback((id: number) => setToasts((ts) => ts.filter((x) => x.id !== id)), [])

  const scan = useCallback(async () => {
    if (!api.inTauri) return
    setScanning(true)
    setActivity({ kind: "busy", text: "Scanning caches" })
    // Rows fill in as each target finishes measuring.
    setCaches([])
    const offStart = on<number>("cache-scan-start", setScanTotal)
    const offItem = on<CacheItem>("cache-scanned", (item) =>
      setCaches((cs) => [...cs.filter((c) => c.id !== item.id), item].sort((a, b) => b.size - a.size)),
    )
    try {
      const items = await api.scanCaches()
      setCaches(items)
      setLastScan(Date.now())
      const total = items.reduce((n, c) => n + c.size, 0)
      setActivity({ kind: "ok", text: `Scan done · ${bytes(total)} reclaimable` })
    } catch (e) {
      setActivity({ kind: "error", text: "Scan failed" })
      toast({ kind: "error", title: "Scan failed", detail: String(e) })
    } finally {
      offStart()
      offItem()
      setScanning(false)
    }
  }, [toast])

  const clean = useCallback(
    async (ids: string[]) => {
      setCleaning({})
      setActivity({ kind: "busy", text: `Cleaning ${ids.length} item${ids.length === 1 ? "" : "s"}` })
      const off = on<CleanEvent>("clean-progress", (e) => {
        setCleaning((c) => ({ ...c, [e.id]: e }))
        if (e.phase === "running") setActivity({ kind: "busy", text: e.note })
      })
      try {
        const results = await api.cleanCaches(ids)
        const freed = results.reduce((n, r) => n + r.freed, 0)
        const skipped = results.reduce((n, r) => n + r.skipped, 0)
        setActivity({ kind: "ok", text: `Freed ${bytes(freed)}${skipped ? ` · ${skipped} items in use skipped` : ""}` })
        const r: Report = {
          kind: "caches",
          title: "Caches cleaned",
          total: freed,
          items: results.map((x) => ({ name: caches.find((c) => c.id === x.id)?.name ?? x.id, icon: x.id, freed: x.freed })),
          at: Date.now(),
        }
        toast({
          kind: "ok",
          title: `Freed ${bytes(freed)}`,
          detail: skipped ? `${skipped} items were in use and left alone.` : undefined,
          action: freed > 0 ? { label: "Share", run: () => setReport(r) } : undefined,
        })
        // Re-measure what was cleaned, so sizes reflect reality.
        void scan()
        return freed
      } catch (e) {
        setActivity({ kind: "error", text: "Clean failed" })
        toast({ kind: "error", title: "Clean failed", detail: String(e) })
        return 0
      } finally {
        off()
      }
    },
    [scan, toast, caches],
  )

  // First scan on launch: read-only, so it is safe to start without asking.
  const started = useRef(false)
  useEffect(() => {
    if (started.current) return
    started.current = true
    void scan()
  }, [scan])

  return (
    <StoreCtx.Provider
      value={{
        stats,
        cpuHistory,
        memHistory,
        caches,
        scanning,
        scanTotal,
        lastScan,
        scan,
        cleaning,
        clean,
        activity,
        setActivity,
        toasts,
        toast,
        dismiss,
        admin,
        platform,
        portsInUse,
        setPortsInUse,
        report,
        openReport,
        closeReport,
      }}
    >
      {children}
    </StoreCtx.Provider>
  )
}

export function useStore() {
  const c = useContext(StoreCtx)
  if (!c) throw new Error("useStore outside provider")
  return c
}

/** Poll a loader while mounted and visible. */
export function usePoll<T>(load: () => Promise<T>, ms: number, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const refresh = useCallback(async () => {
    try {
      setData(await load())
      setError(null)
    } catch (e) {
      setError(String(e))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
  useEffect(() => {
    if (!api.inTauri) return
    void refresh()
    const t = setInterval(() => !document.hidden && void refresh(), ms)
    return () => clearInterval(t)
  }, [refresh, ms])
  return { data, error, refresh }
}
