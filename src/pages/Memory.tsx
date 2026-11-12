import { SnapshotButton } from "@/components/Share"
import { useMemo, useState, type ReactNode } from "react"
import { ArrowRight, MemoryStick, X } from "lucide-react"
import { api, type FreeResult, type ProcGroup } from "@/lib/api"
import { bytes, cn } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { LEVEL_COLOR, LEVEL_TEXT, pressureApi, type Grower, type PressureSample } from "@/lib/pressure"
import { Button, Card, Dialog, InfoTip, PageHeader, Skeleton, Tooltip, VerdictPill } from "@/components/ui"
import { ProcIcon, useKill } from "@/components/shared"
import { MiniSpark, PressureChart, Segmented } from "@/components/memory/charts"
import { openOrphans } from "./Processes"

/** Exactly what "Free memory" does on this OS. */
export function freeMemoryHelp(platform: string): React.ReactNode {
  if (platform === "macos")
    return (
      <>
        <b>How it works on macOS:</b> runs <code>purge</code>, which writes pending changes to disk and drops the file cache and inactive memory. macOS asks for your admin password. No apps are closed; files are read from disk again the next time they're needed, so things may feel slower for a moment.
      </>
    )
  if (platform === "linux")
    return (
      <>
        <b>How it works on Linux:</b> runs <code>sync</code>, then writes 1 to <code>/proc/sys/vm/drop_caches</code> through pkexec (asks for your password). This frees the page cache only; no apps are closed and no app memory is touched. Files are read from disk again when next needed.
      </>
    )
  return (
    <>
      <b>How it works on Windows:</b> asks every process Dev Cleaner may open to give back its idle pages (<code>EmptyWorkingSet</code>). The pages move to the standby list, so “In use” drops right away, and an app gets them back from RAM the moment it touches them. Nothing is closed and nothing is written to the page file. Without admin, services and other users' apps are skipped. Memory a busy app needs comes back within seconds, so this helps most after closing big apps or builds.
    </>
  )
}

/** "Free memory": explain, run, and report before/after. */
export function useFreeMemory() {
  const { toast, setActivity, platform } = useStore()
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<FreeResult | null>(null)
  const run = async () => {
    setBusy(true)
    setActivity({ kind: "busy", text: "Freeing memory" })
    try {
      const r = await api.freeMemory()
      const freed = Math.max(0, r.before - r.after)
      setResult(r)
      setActivity({ kind: "ok", text: `Freed ${bytes(freed)} of memory` })
      toast({ kind: "ok", title: `Freed ${bytes(freed)} of memory`, detail: r.note })
    } catch (e) {
      toast({ kind: "error", title: "Could not free memory", detail: String(e) })
      setActivity({ kind: "error", text: "Free memory failed" })
    } finally {
      setBusy(false)
    }
  }
  const dialog = (
    <Dialog
      open={!!result}
      onClose={() => setResult(null)}
      title="Memory freed"
      actions={
        <Button variant="primary" onClick={() => setResult(null)}>
          Done
        </Button>
      }
    >
      {result && (
        <div className="flex flex-col gap-2 text-text">
          <div className="mono flex justify-between">
            <span className="text-muted">Before</span>
            {bytes(result.before)}
          </div>
          <div className="mono flex justify-between">
            <span className="text-muted">After</span>
            {bytes(result.after)}
          </div>
          <p className="m-0 mt-1 text-muted">
            {result.note} {platform === "windows" ? "No apps were closed; they reload pages as they need them." : ""}
          </p>
        </div>
      )}
    </Dialog>
  )
  return { run, busy, dialog }
}

type Range = "1" | "3" | "8"
const RANGES: { id: Range; label: string }[] = [
  { id: "1", label: "1 h" },
  { id: "3", label: "3 h" },
  { id: "8", label: "8 h" },
]
let lastRange: Range = "1"

const asGroup = (g: Grower): ProcGroup => ({ name: g.name, pids: g.pids, memory: g.memory, cpu: 0, verdict: g.verdict, reason: g.reason, protected: g.protected, exe: null })
const go = (page: string) => window.dispatchEvent(new CustomEvent("dc-go", { detail: page }))
const GB = 1024 ** 3

export function Memory() {
  const { stats, platform, toast } = useStore()
  const [range, setRangeState] = useState<Range>(lastRange)
  const setRange = (r: Range) => {
    lastRange = r
    setRangeState(r)
  }
  const hours = Number(range)
  const now = usePoll(pressureApi.now, 2000)
  const history = usePoll(() => pressureApi.history(hours), 10_000, [hours])
  const growth = usePoll(pressureApi.growth, 30_000)
  const orphans = usePoll(pressureApi.orphans, 20_000)
  const wsl = usePoll(() => (platform === "windows" ? pressureApi.wslStatus() : Promise.resolve(null)), 30_000, [platform])
  const procs = usePoll(api.listProcesses, 4000)
  const kill = useKill(() => {
    void procs.refresh()
    void growth.refresh()
    void orphans.refresh()
  })
  const free = useFreeMemory()
  const [reclaiming, setReclaiming] = useState(false)

  const total = stats?.memTotal ?? 0
  const used = stats?.memUsed ?? 0
  const compressed = stats?.compressed ?? 0
  const apps = Math.max(0, used - compressed)
  const freeB = Math.max(0, total - used)
  const parts = [
    { label: "Apps", value: apps, color: "var(--accent)", tip: "Memory apps are actively using." },
    { label: "Compressed", value: compressed, color: "color-mix(in oklch, var(--accent) 55%, var(--surface))", tip: "Pages the OS compressed instead of writing to disk. Saves RAM for a little CPU." },
    { label: "Free", value: freeB, color: "var(--track)", tip: "Not in use. The OS also uses part of this as a file cache that it hands back instantly." },
  ]
  const top = (procs.data ?? []).slice(0, 10)

  // The chart: history plus the live samples since the last history poll.
  const s = now.data
  const samples = useMemo(() => {
    const h = history.data?.samples ?? []
    const last = h[h.length - 1]?.t ?? 0
    return s && s.t > last ? [...h, s] : h
  }, [history.data, s])
  const tNow = s?.t ?? Math.floor(Date.now() / 1000)
  const spells = history.data?.slowSpells ?? []

  const growers = (growth.data ?? []).filter((g) => g.growth > 16 * 1024 * 1024).slice(0, 6)
  const orphanList = (orphans.data ?? []).filter((o) => !o.protected)
  const orphanBytes = orphanList.reduce((n, o) => n + o.memory, 0)

  const reclaim = async () => {
    setReclaiming(true)
    const before = wsl.data?.vmMemory ?? 0
    try {
      const r = await pressureApi.wslReclaim()
      const gave = Math.max(0, before - (r.vmMemory ?? before))
      toast({ kind: "ok", title: "WSL page cache dropped", detail: gave > 0 ? `The VM gave back ${bytes(gave)} so far; Windows takes the rest back over a minute.` : "Windows takes the freed pages back over the next minute." })
      void wsl.refresh()
    } catch (e) {
      toast({ kind: "error", title: "Could not reclaim WSL memory", detail: String(e) })
    } finally {
      setReclaiming(false)
    }
  }

  // What would actually lower pressure right now, biggest win first.
  const tips: { key: string; title: string; detail: string; gain?: number; run: () => void; busy?: boolean }[] = []
  if (orphanList.length > 0)
    tips.push({
      key: "orphans",
      title: `End ${orphanList.length} orphaned dev process${orphanList.length === 1 ? "" : "es"}`,
      detail: "Left behind by editors and AI agents whose window has closed.",
      gain: orphanBytes,
      run: openOrphans,
    })
  const lx = wsl.data?.linux
  if (wsl.data?.running && lx && lx.cache > GB)
    tips.push({ key: "wsl", title: "Drop WSL's page cache", detail: "Linux keeps file cache inside the VM; Windows can't use that memory until it's dropped.", gain: lx.cache, run: reclaim, busy: reclaiming })
  const big = growers.find((g) => g.growth > GB && !g.protected)
  if (big)
    tips.push({
      key: "grow",
      title: `Restart ${big.name}`,
      detail: `It grew by ${bytes(big.growth)} in the last 2 hours. A restart gives that back${big.verdict === "saveFirst" ? "; save your work first" : ""}.`,
      gain: big.growth,
      run: () => kill.request(big.name, big.pids, asGroup(big)),
    })
  if (wsl.data?.available && !/dropcache|gradual/i.test(wsl.data.config.autoMemoryReclaim ?? ""))
    tips.push({ key: "wslconfig", title: "Let WSL return memory by itself", detail: "Turn on autoMemoryReclaim in .wslconfig. The Responsiveness page can apply it.", run: () => go("tune") })
  tips.sort((a, b) => (b.gain ?? 0) - (a.gain ?? 0))

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Memory"
        lede="Is the machine short of memory, and who is making it so."
        actions={
          <>
            <Segmented value={range} options={RANGES} onChange={setRange} label="Time range" testId="mem-range" />
            <SnapshotButton name="memory" />
            <Tooltip text={freeMemoryHelp(platform)} wide side="bottom">
              <Button variant="primary" icon={<MemoryStick size={15} />} loading={free.busy} onClick={free.run} data-testid="free-memory">
                Free memory
              </Button>
            </Tooltip>
          </>
        }
      />

      <Card className="grid grid-cols-[300px_minmax(0,1fr)] overflow-hidden" data-testid="pressure-card">
        <div className="flex flex-col gap-2.5 border-r border-border p-5">
          <span className="text-[12px] text-muted">Memory pressure</span>
          {s ? <LevelHeadline s={s} platform={platform} /> : <Skeleton className="h-16" />}
          <div className="mt-1 grid grid-cols-2 gap-3">
            <Stat k="Committed" v={s ? bytes(s.commitUsed) : "—"} u={s ? `of ${bytes(s.commitLimit, 0)}` : ""} tip={platform === "macos" ? "RAM in use plus swap in use, against RAM plus swap." : "Memory programs have asked for. When it reaches the limit (RAM plus page file), apps fail to allocate."} />
            <Stat k="Compressed" v={s?.compressed != null ? bytes(s.compressed) : stats?.compressed ? bytes(stats.compressed) : "—"} tip="Pages compressed in RAM instead of written to disk." />
            <Stat k="Paging from disk" v={s ? Math.round(s.pagesInPerSec).toLocaleString() : "—"} u="/s" tip="Pages read back from disk per second (hard faults). Sustained high numbers are what makes a PC sluggish." />
            <Stat k={platform === "windows" ? "Page file in use" : "Swap in use"} v={s?.pagefileUsed != null ? bytes(s.pagefileUsed) : "—"} />
          </div>
          {stats ? (
            <div className="mt-2 flex flex-col gap-1.5">
              <span className="text-[11.5px] text-subtle">What's using memory</span>
              <div className="flex h-2 gap-0.5 overflow-hidden rounded-full">
                {parts.map((p) => (
                  <div key={p.label} className="h-full transition-[width] duration-700 ease-out" style={{ width: `${(p.value / Math.max(1, total)) * 100}%`, background: p.color }} />
                ))}
              </div>
              <div className="flex gap-3 text-[11.5px] text-muted">
                {parts.map((p) => (
                  <span key={p.label} className="flex items-center gap-1">
                    <span className="h-2 w-2 rounded-sm" style={{ background: p.color }} />
                    {p.label} <span className="tabular-nums text-text">{bytes(p.value, 0)}</span>
                    <InfoTip text={p.tip} />
                  </span>
                ))}
              </div>
            </div>
          ) : null}
        </div>
        <div className="flex min-w-0 flex-col gap-2 px-5 pb-3 pt-4">
          <div className="flex items-center gap-4 text-[12px] text-muted">
            <Legend color="var(--accent)" label="Committed" />
            <Legend color="var(--warn)" label="Paging from disk" />
            {spells.length > 0 && <Legend color="color-mix(in oklch, var(--danger) 30%, transparent)" label={`${spells.length} slow spell${spells.length === 1 ? "" : "s"}`} block />}
            <span className="flex-1" />
            <span className="tabular-nums">RAM {bytes(total || s?.memTotal || 0, 1)}</span>
          </div>
          <PressureChart samples={samples} hours={hours} now={tNow} memTotal={s?.memTotal ?? total} spells={spells} />
          <div className="flex justify-between text-[11px] tabular-nums text-subtle">
            <span>
              {hours} h ago
              {history.data && history.data.since > tNow - hours * 3600 + 120 && samples.length > 1 ? ` · measuring since ${new Date(history.data.since * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}` : ""}
            </span>
            <span>
              {history.data ? `peak ${Math.round(history.data.peakCommitPct)}% committed · ${Math.round(history.data.peakPagesIn).toLocaleString()} pages/s` : ""}
              <span className="ml-3">now</span>
            </span>
          </div>
        </div>
      </Card>

      <div className="grid grid-cols-[minmax(0,1fr)_320px] gap-3.5">
        <Card className="flex min-w-0 flex-col overflow-hidden" data-testid="growers">
          <div className="flex items-center px-4 pb-2 pt-3.5">
            <h2 className="m-0 text-[13px] font-semibold">Who is growing</h2>
            <span className="flex-1" />
            <span className="text-[12px] text-subtle">change over the last 2 h</span>
          </div>
          <ul className="m-0 flex list-none flex-col p-0 px-2 pb-2">
            {!growth.data
              ? Array.from({ length: 4 }).map((_, i) => (
                  <li key={i} className="px-2 py-3">
                    <Skeleton className="h-5" />
                  </li>
                ))
              : growers.map((g) => (
                  <li key={g.name} className="grid h-[50px] grid-cols-[minmax(0,1fr)_110px_76px_72px_92px] items-center gap-3 rounded-[10px] px-2 hover:bg-hover">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 font-medium">
                        <ProcIcon name={g.name} />
                        <span className="truncate">{g.name}</span>
                        {g.pids.length > 1 && <span className="text-[11.5px] font-normal text-subtle">×{g.pids.length}</span>}
                      </div>
                      <div className="truncate text-[11.5px] text-subtle">{g.reason}</div>
                    </div>
                    <MiniSpark data={g.series} color={g.growth > 512 * 1024 * 1024 ? "var(--warn)" : "var(--accent)"} />
                    <span className={cn("text-right tabular-nums", g.growth > 512 * 1024 * 1024 ? "font-medium text-warn" : "text-muted")}>+{bytes(g.growth)}</span>
                    <span className="text-right font-medium tabular-nums">{bytes(g.memory)}</span>
                    <span className="text-right">
                      {g.protected ? (
                        <span className="text-[11.5px] text-subtle">system</span>
                      ) : (
                        <Button size="sm" icon={<X size={11} strokeWidth={2.4} />} onClick={() => kill.request(g.name, g.pids, asGroup(g))} className="hover:border-danger hover:text-danger" aria-label={`End ${g.name}`}>
                          End
                        </Button>
                      )}
                    </span>
                  </li>
                ))}
            {growth.data && growers.length === 0 && (
              <li className="px-3 py-6 text-center text-[12.5px] text-muted">Nothing has grown much yet. Dev Cleaner notes each app's memory every 30 seconds while it is open.</li>
            )}
          </ul>
        </Card>

        <aside className="flex flex-col gap-2.5" aria-label="What would help now" data-testid="memory-tips">
          <h2 className="m-0 mx-0.5 mt-0.5 text-[13px] font-semibold">What would help now</h2>
          {tips.length === 0 ? (
            <Card className="px-3.5 py-3 text-[12.5px] text-muted">
              {s?.level === "low" || !s ? "Nothing to do: there is room to spare and almost nothing is read back from disk." : "No single app stands out. Close what you aren't using, or the biggest apps below."}
            </Card>
          ) : (
            tips.slice(0, 4).map((t) => (
              <button
                key={t.key}
                type="button"
                onClick={t.run}
                disabled={t.busy}
                className="group flex items-center gap-3 rounded-[13px] border border-border bg-surface px-3.5 py-3 text-left transition-colors hover:border-border-strong disabled:opacity-60"
              >
                <span className="min-w-0 flex-1">
                  <span className="block font-medium">{t.title}</span>
                  <span className="mt-0.5 block text-[12px] leading-[1.45] text-muted">{t.detail}</span>
                </span>
                {t.gain ? <span className="whitespace-nowrap font-semibold tabular-nums text-ok">{bytes(t.gain)}</span> : <ArrowRight size={14} className="text-subtle group-hover:text-text" />}
              </button>
            ))
          )}
          <p className="m-0.5 text-[11.5px] leading-normal text-subtle">
            Free memory trims idle pages from apps; under pressure they are read straight back, so it doesn't lower pressure. These do.
          </p>
        </aside>
      </div>

      <Card className="overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3">
          <h2 className="m-0 text-[13px] font-semibold">Biggest apps</h2>
          <span className="text-[12px] text-subtle">grouped by app</span>
        </div>
        <ul className="m-0 list-none p-0">
          {(procs.data ? top : Array.from<undefined>({ length: 6 })).map((p: ProcGroup | undefined, i) =>
            p ? (
              <li key={p.name} className="grid h-11 grid-cols-[minmax(0,1.2fr)_90px_minmax(0,1.5fr)_96px] items-center gap-3 border-t border-border px-4 text-[13px] hover:bg-surface-2">
                <span className="flex min-w-0 items-center gap-2 truncate">
                  <ProcIcon name={p.name} />
                  <span className="truncate">{p.name}</span>
                  {p.pids.length > 1 && <span className="text-[12px] text-subtle">×{p.pids.length}</span>}
                </span>
                <span className="text-right tabular-nums">{bytes(p.memory)}</span>
                <span className="flex min-w-0 items-center gap-2">
                  <VerdictPill verdict={p.verdict} />
                  <span className="truncate text-[11.5px] text-subtle">{p.reason}</span>
                </span>
                <span className="text-right">
                  {p.protected ? (
                    <span className="text-[11.5px] text-subtle">protected</span>
                  ) : (
                    <Button size="sm" icon={<X size={11} strokeWidth={2.4} />} onClick={() => kill.request(p.name, p.pids, p)} className="hover:border-danger hover:text-danger">
                      End task
                    </Button>
                  )}
                </span>
              </li>
            ) : (
              <li key={i} className="border-t border-border px-4 py-3">
                <Skeleton className="h-4" />
              </li>
            ),
          )}
        </ul>
      </Card>
      {kill.dialog}
      {free.dialog}
    </div>
  )
}

function LevelHeadline({ s, platform }: { s: PressureSample; platform: string }) {
  const who = platform === "windows" ? "Windows" : platform === "macos" ? "macOS" : "Linux"
  const pages = Math.round(s.pagesInPerSec)
  const strong = (x: ReactNode) => <b className="font-semibold text-text tabular-nums">{x}</b>
  const committed = s.commitLimit ? Math.round((s.commitUsed / s.commitLimit) * 100) : 0
  return (
    <>
      <div className="flex items-center gap-2.5" data-testid="pressure-level">
        <span aria-hidden className="h-3 w-3 rounded-full" style={{ background: LEVEL_COLOR[s.level], boxShadow: `0 0 0 5px color-mix(in oklch, ${LEVEL_COLOR[s.level]} 20%, transparent)` }} />
        <span className="text-[26px] font-semibold tracking-[-0.02em]">
          {LEVEL_TEXT[s.level]}
        </span>
      </div>
      <p className="m-0 leading-[1.55] text-muted">
        {s.level === "low" ? (
          <>
            Room to spare: {strong(`${committed}%`)} of the commit limit is used and {pages < 5 ? "almost nothing is" : <>{strong(pages)} pages/s are</>} read back from disk.
          </>
        ) : (
          <>
            {s.compressed ? <>{who} is compressing {strong(bytes(s.compressed))} and </> : <>{who} is </>}
            reading about {strong(`${pages.toLocaleString()} pages/s`)} back from disk. {s.level === "high" ? "Switching apps and typing will lag." : "Switching windows will start to lag."}
          </>
        )}
      </p>
    </>
  )
}

function Stat({ k, v, u, tip }: { k: string; v: string; u?: string; tip?: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="flex items-center gap-1 text-[11.5px] text-subtle">
        {k}
        {tip && <InfoTip text={tip} />}
      </span>
      <span className="truncate text-[15px] font-semibold tabular-nums tracking-[-0.01em]">
        {v}
        {u && <span className="text-[11.5px] font-normal text-subtle"> {u}</span>}
      </span>
    </div>
  )
}

function Legend({ color, label, block }: { color: string; label: string; block?: boolean }) {
  return (
    <span className="flex items-center gap-1.5">
      <span aria-hidden className={block ? "h-2.5 w-2.5 rounded-sm" : "h-[3px] w-3.5 rounded-full"} style={{ background: color }} />
      {label}
    </span>
  )
}

