import { SnapshotButton } from "@/components/Share"
import { useMemo, useState } from "react"
import { X } from "lucide-react"
import { api, type ProcGroup, type Verdict } from "@/lib/api"
import { bytes, cn } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { Button, Card, PageHeader, Skeleton, Sparkline, VerdictPill, SearchField } from "@/components/ui"
import { ProcIcon, TopCpu, useKill } from "@/components/shared"
import { Segmented } from "@/components/memory/charts"
import { Orphaned } from "@/components/memory/Orphaned"
import { pressureApi } from "@/lib/pressure"

type SortKey = "memory" | "cpu" | "name"
type Tab = "apps" | "orphaned"

// The tab survives leaving the page and coming back.
let lastTab: Tab = "apps"

/** Open Processes on the Orphaned tab (from Memory's tips). */
export function openOrphans() {
  lastTab = "orphaned"
  window.dispatchEvent(new CustomEvent("dc-go", { detail: "processes" }))
}

export function Processes() {
  const [tab, setTabState] = useState<Tab>(lastTab)
  const setTab = (t: Tab) => {
    lastTab = t
    setTabState(t)
  }
  const orphans = usePoll(pressureApi.orphans, 5000)
  const count = (orphans.data ?? []).filter((o) => !o.protected).length
  const tabs = (
    <Segmented
      value={tab}
      onChange={setTab}
      label="Process views"
      testId="proc-tab"
      options={[
        { id: "apps", label: "Apps" },
        {
          id: "orphaned",
          label: (
            <>
              Orphaned
              {count > 0 && <span className="rounded-full bg-warn-soft px-1.5 text-[11px] font-medium tabular-nums text-warn">{count}</span>}
            </>
          ),
        },
      ]}
    />
  )
  if (tab === "orphaned") return <Orphaned tabs={tabs} orphans={orphans} />
  return <AllApps tabs={tabs} />
}

function AllApps({ tabs }: { tabs: React.ReactNode }) {
  const { stats, cpuHistory } = useStore()
  const procs = usePoll(api.listProcesses, 2000)
  const kill = useKill(procs.refresh)
  const [q, setQ] = useState("")
  const [sort, setSort] = useState<SortKey>("memory")
  const [verdict, setVerdict] = useState<Verdict | "all">("all")

  const rows = useMemo(() => {
    const term = q.trim().toLowerCase()
    const list = (procs.data ?? []).filter(
      (p) => (verdict === "all" || p.verdict === verdict) && (!term || p.name.toLowerCase().includes(term) || p.pids.some((id) => String(id) === term)),
    )
    return list.sort((a, b) => (sort === "memory" ? b.memory - a.memory : sort === "cpu" ? b.cpu - a.cpu : a.name.localeCompare(b.name)))
  }, [procs.data, q, sort, verdict])

  const head = (k: SortKey, label: string, right?: boolean) => (
    <button type="button" onClick={() => setSort(k)} className={cn("uppercase tracking-[0.06em] hover:text-text", right && "text-right", sort === k && "text-text")} aria-pressed={sort === k}>
      {label}
      {sort === k ? " ↓" : ""}
    </button>
  )

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader title="Processes" tabs={tabs} lede={stats ? `${stats.processCount} processes running. Each app gets a verdict on whether it is safe to end.` : "Reading processes…"} actions={<SnapshotButton name="processes" />} />
      <div className="grid min-h-[360px] flex-1 grid-cols-[360px_minmax(0,1fr)] grid-rows-[minmax(0,1fr)] gap-3.5">
        <Card className="flex flex-col self-start">
          <div className="flex items-baseline justify-between px-4 pt-3.5">
            <h2 className="m-0 text-[13px] font-semibold">Top CPU</h2>
            <span className="mono text-[12px] text-subtle">{stats ? `${stats.cpu.toFixed(0)}% total` : ""}</span>
          </div>
          <div className="px-4 pb-2 pt-3">
            <Sparkline data={cpuHistory} height={56} />
          </div>
          <TopCpu procs={procs.data} limit={6} onKill={(g) => kill.request(g.name, g.pids, g)} />
        </Card>
        <Card className="flex min-h-0 min-w-0 flex-col overflow-hidden">
          <div className="flex flex-wrap items-center gap-2 px-4 py-3">
            <SearchField value={q} onChange={setQ} placeholder="Filter by name or PID" label="Filter processes" testId="process-filter" />
            {(["all", "safe", "saveFirst", "unknown", "system"] as const).map((v) => (
              <button
                key={v}
                type="button"
                aria-pressed={verdict === v}
                onClick={() => setVerdict(v)}
                className={cn(
                  "h-7 rounded-full border px-2.5 text-[12px] font-medium transition-colors",
                  verdict === v ? "border-text bg-text text-bg" : "border-border-strong bg-surface text-muted hover:text-text",
                )}
              >
                {{ all: "All", safe: "Safe", saveFirst: "Save first", unknown: "Unknown", system: "System" }[v]}
              </button>
            ))}
          </div>
          <div className="shrink-0 grid grid-cols-[minmax(0,1.3fr)_80px_64px_minmax(0,1.5fr)_92px] gap-3 border-y border-border px-4 py-2 text-left text-[11px] text-subtle">
            {head("name", "App")}
            {head("memory", "Memory", true)}
            {head("cpu", "CPU", true)}
            <span className="uppercase tracking-[0.06em]">Verdict</span>
            <span />
          </div>
          <ul className="m-0 min-h-0 flex-1 overflow-y-auto list-none p-0" data-testid="process-list">
            {procs.data
              ? rows.map((p) => <Row key={p.name} p={p} onKill={() => kill.request(p.name, p.pids, p)} />)
              : Array.from({ length: 8 }).map((_, i) => (
                  <li key={i} className="border-b border-border px-4 py-3">
                    <Skeleton className="h-4" />
                  </li>
                ))}
            {procs.data && rows.length === 0 && <li className="px-4 py-8 text-center text-[12.5px] text-muted">No process matches “{q}”.</li>}
          </ul>
        </Card>
      </div>
      {kill.dialog}
    </div>
  )
}

function Row({ p, onKill }: { p: ProcGroup; onKill: () => void }) {
  return (
    <li className="grid h-11 grid-cols-[minmax(0,1.3fr)_80px_64px_minmax(0,1.5fr)_92px] items-center gap-3 border-b border-border px-4 text-[13px] last:border-b-0 hover:bg-surface-2">
      <span className="flex min-w-0 items-center gap-2" title={p.exe ?? undefined}>
        <ProcIcon name={p.name} />
        <span className="truncate">{p.name}</span>
        {p.pids.length > 1 ? <span className="shrink-0 text-[12px] text-subtle">×{p.pids.length}</span> : <span className="mono shrink-0 text-[11px] text-subtle">{p.pids[0]}</span>}
      </span>
      <span className="mono text-right">{bytes(p.memory)}</span>
      <span className="mono text-right text-muted">{p.cpu.toFixed(1)}%</span>
      <span className="flex min-w-0 items-center gap-2">
        <VerdictPill verdict={p.verdict} />
        <span className="truncate text-[11.5px] text-subtle">{p.reason}</span>
      </span>
      <span className="text-right">
        {p.protected ? (
          <span className="text-[11.5px] text-subtle">protected</span>
        ) : (
          <Button size="sm" icon={<X size={11} strokeWidth={2.4} />} onClick={onKill} className="hover:border-danger hover:text-danger" aria-label={`End ${p.name}`}>
            End task
          </Button>
        )}
      </span>
    </li>
  )
}
