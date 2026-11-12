import { useEffect, useMemo, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Activity, Ban, Check, ChevronDown, Gauge, Power, RefreshCw, RotateCcw, Rocket } from "lucide-react"
import type { Page } from "@/components/Shell"
import { api } from "@/lib/api"
import { ago, cn, duration } from "@/lib/format"
import { useStore } from "@/lib/store"
import { tuneApi, type JournalEntry, type PressureHistory, type TuneItem, type TuneOverview } from "@/lib/tune"
import { Button, Empty, PageHeader, Skeleton, Switch } from "@/components/ui"
import { ImpactMeter, Num, Panel, Tabs } from "@/components/tune/parts"

type Tab = "tune" | "history"
let lastTab: Tab = "tune"

const SKIPPED: { t: string; why: string }[] = [
  { t: "Turning off memory compression", why: "compressing is far cheaper than paging to disk" },
  { t: "Removing the pagefile", why: "apps crash when committed memory runs out" },
  { t: "Disabling SysMain", why: "app launches get slower; it rarely causes the slowdown" },
  { t: "RAM “boosters” on a timer", why: "emptied memory just pages back in" },
  { t: "Defender exclusions for code folders", why: "a Dev Drive gets the speed-up without the risk" },
  { t: "Registry speed tweaks", why: "no measurable gain for builds or editors" },
]

/** Responsiveness: measured, reversible settings for a dev machine under load. */
export function Responsiveness({ go }: { go: (p: Page) => void }) {
  const { stats, toast, setActivity } = useStore()
  const [tab, setTabState] = useState<Tab>(lastTab)
  const setTab = (t: Tab) => {
    lastTab = t
    setTabState(t)
  }
  const [data, setData] = useState<TuneOverview | null>(null)
  const [history, setHistory] = useState<JournalEntry[] | null>(null)
  const [pressure, setPressure] = useState<PressureHistory | null>(null)
  const [pick, setPick] = useState<Record<string, boolean>>({})
  const [open, setOpen] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(false)

  const load = async () => {
    if (!api.inTauri) return
    setLoading(true)
    try {
      const [o, h, p] = await Promise.all([tuneApi.list(), tuneApi.history(), tuneApi.pressure(168)])
      setData(o)
      setHistory(h)
      setPressure(p)
    } catch (e) {
      toast({ kind: "error", title: "Could not read the settings", detail: String(e) })
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const items = data?.items ?? []
  const available = items.filter((i) => i.state === "available")
  const isOn = (i: TuneItem) => pick[i.id] ?? i.impact >= 2
  const chosen = available.filter(isOn)
  const notes = [...new Set(chosen.map((i) => i.note).filter((n) => n && n !== "No restart"))]
  const lastEntry = (id: string) => history?.find((h) => h.tweak === id && !h.undoneAt)

  const apply = async () => {
    setBusy(true)
    setActivity({ kind: "busy", text: `Applying ${chosen.length} change${chosen.length === 1 ? "" : "s"}` })
    try {
      const out = await tuneApi.apply(chosen.map((i) => i.id))
      const ok = out.filter((o) => o.ok)
      const bad = out.filter((o) => !o.ok)
      if (ok.length) toast({ kind: "ok", title: ok.some((o) => o.dryRun) ? "Dry run: nothing changed" : `Applied ${ok.length} change${ok.length === 1 ? "" : "s"}`, detail: ok.map((o) => o.message).join(" ") })
      if (bad.length) toast({ kind: "error", title: `${bad.length} change${bad.length === 1 ? "" : "s"} failed`, detail: bad.map((o) => o.message).join(" ") })
      setActivity({ kind: bad.length ? "error" : "ok", text: `Applied ${ok.length} of ${out.length}` })
      setPick({})
      await load()
    } catch (e) {
      toast({ kind: "error", title: "Could not apply", detail: String(e) })
      setActivity({ kind: "error", text: "Apply failed" })
    } finally {
      setBusy(false)
    }
  }

  const undo = async (entryId: string) => {
    try {
      const r = await tuneApi.undo(entryId)
      toast({ kind: "ok", title: "Undone", detail: r.message })
      await load()
    } catch (e) {
      toast({ kind: "error", title: "Could not undo", detail: String(e) })
    }
  }

  const shutdownWsl = async () => {
    try {
      await tuneApi.wslShutdown()
      toast({ kind: "ok", title: "WSL shut down", detail: "It starts again, with the new settings, the next time you use it." })
    } catch (e) {
      toast({ kind: "error", title: "Could not shut down WSL", detail: `${String(e)} Run "wsl --shutdown" in a terminal instead.` })
    }
  }

  const openLink = (url: string) =>
    void openUrl(url).catch(() => toast({ kind: "info", title: "Open Settings › System › Power & battery", detail: "Set Power mode to Best performance." }))

  const tabs = (
    <Tabs
      value={tab}
      onChange={setTab}
      label="Responsiveness views"
      testId="tune-tab"
      items={[
        ["tune", "Tune"],
        ["history", `History${history?.length ? ` ${history.length}` : ""}`],
      ]}
    />
  )

  return (
    <div className="flex min-h-full flex-col gap-4" data-testid="tune">
      <PageHeader
        title="Responsiveness"
        tabs={tabs}
        lede="Settings that keep this PC quick under a heavy dev load. Each one is checked against this machine, and each can be undone."
        actions={
          <Button icon={<RefreshCw size={15} className={loading ? "spin" : undefined} />} disabled={loading} onClick={() => void load()}>
            Recheck
          </Button>
        }
      />
      {data?.dryRun && <p className="m-0 rounded-lg bg-accent-soft px-3 py-2 text-[12.5px] text-accent">Test sandbox: system settings aren't read or changed.</p>}

      {tab === "history" ? (
        <HistoryView history={history} pressure={pressure} onUndo={undo} />
      ) : (
        <>
          <div className="grid grid-cols-[minmax(0,1.32fr)_minmax(0,1fr)] items-start gap-4">
            <Panel
              label="Recommended changes"
              title="Recommended for this PC"
              aside={<span className="text-[12px] text-subtle">{data ? `${items.filter((i) => i.state === "applied").length} of ${items.filter((i) => i.state !== "na").length} in place` : ""}</span>}
              className="px-2.5 pb-2"
            >
              <ul className="m-0 flex list-none flex-col gap-0.5 p-0" data-testid="tune-list">
                {!data &&
                  Array.from({ length: 5 }).map((_, i) => (
                    <li key={i} className="px-2 py-3">
                      <Skeleton className="h-8" />
                    </li>
                  ))}
                {[...items]
                  .sort((a, b) => rank(a) - rank(b) || b.impact - a.impact)
                  .map((it) => {
                    const expanded = open === it.id
                    const entry = lastEntry(it.id)
                    return (
                      <li key={it.id} className={cn("rounded-[10px]", expanded && "bg-surface-2", it.state === "na" && "opacity-70")} data-testid={`tune-${it.id}`}>
                        <div className="grid grid-cols-[40px_minmax(0,1fr)_74px_20px] items-center gap-3 rounded-[10px] px-2 py-[9px] hover:bg-hover">
                          <span className="flex justify-center">
                            {it.state === "available" ? (
                              <Switch checked={isOn(it)} onChange={(v) => setPick({ ...pick, [it.id]: v })} label={it.title} />
                            ) : it.state === "applied" ? (
                              <span className="grid h-5 w-5 place-items-center rounded-full bg-ok-soft text-ok" title="In place">
                                <Check size={12} strokeWidth={2.6} />
                              </span>
                            ) : (
                              <span className="grid h-5 w-5 place-items-center rounded-full bg-track text-subtle" title="Doesn't apply here">
                                <Ban size={11} />
                              </span>
                            )}
                          </span>
                          <button type="button" aria-expanded={expanded} onClick={() => setOpen(expanded ? null : it.id)} className="min-w-0 text-left">
                            <div className="flex items-center gap-1.5">
                              <span className="font-medium">{it.title}</span>
                              {it.state === "applied" && <span className="text-[11px] font-medium text-ok">{entry ? "applied by Dev Cleaner" : "already set"}</span>}
                            </div>
                            <div className="mt-0.5 truncate text-[12px] text-muted" title={it.evidence}>
                              {it.evidence}
                            </div>
                          </button>
                          <span className="flex justify-end">{it.state !== "na" && <ImpactMeter level={it.impact} />}</span>
                          <button type="button" aria-label="Details" onClick={() => setOpen(expanded ? null : it.id)} className="grid h-5 w-5 place-items-center rounded-[5px] text-subtle hover:bg-hover">
                            <ChevronDown size={12} className={cn("transition-transform duration-200", expanded && "rotate-180")} />
                          </button>
                        </div>
                        {expanded && (
                          <div className="flex flex-col gap-2 pb-3 pl-[60px] pr-2.5">
                            <p className="m-0 leading-[1.5] text-muted">{it.naReason ?? it.detail}</p>
                            <pre className="selectable m-0 whitespace-pre-wrap rounded-lg border border-border bg-surface px-3 py-2.5 font-mono text-[11.5px] leading-[1.55] text-text">{it.code}</pre>
                            <div className="flex flex-wrap items-center gap-1.5">
                              {[it.admin ? "Admin" : "No admin", it.note, "Undo from History"].map((f) => (
                                <span key={f} className="rounded-full border border-border bg-surface px-2 py-0.5 text-[11px] text-muted">
                                  {f}
                                </span>
                              ))}
                              <span className="flex-1" />
                              {it.link && (
                                <Button size="sm" onClick={() => openLink(it.link!)}>
                                  Open Settings
                                </Button>
                              )}
                              {it.id === "wsl" && it.state === "applied" && (
                                <Button size="sm" icon={<Power size={12} />} onClick={() => void shutdownWsl()}>
                                  Shut down WSL now
                                </Button>
                              )}
                              {entry && (
                                <Button size="sm" icon={<RotateCcw size={12} />} onClick={() => void undo(entry.id)}>
                                  Undo
                                </Button>
                              )}
                            </div>
                          </div>
                        )}
                      </li>
                    )
                  })}
                {data?.startupEnabled != null && (
                  <li className="grid grid-cols-[40px_minmax(0,1fr)_auto] items-center gap-3 rounded-[10px] px-2 py-[9px] hover:bg-hover">
                    <span className="flex justify-center text-subtle">
                      <Rocket size={15} />
                    </span>
                    <div className="min-w-0">
                      <div className="font-medium">Startup apps</div>
                      <div className="mt-0.5 truncate text-[12px] text-muted">
                        {data.startupEnabled} app{data.startupEnabled === 1 ? "" : "s"} start with Windows and hold memory all day. Turn off the ones you don't need.
                      </div>
                    </div>
                    <Button size="sm" onClick={() => go("startup")}>
                      Open Startup
                    </Button>
                  </li>
                )}
              </ul>
            </Panel>

            <div className="flex flex-col gap-4">
              <Baseline pressure={pressure} uptime={stats?.uptime ?? null} go={go} loaded={!!data} />
              <Panel label="Leave these alone" title="Already right">
                {!data ? (
                  <Skeleton className="h-6" />
                ) : data.good.length ? (
                  <div className="flex flex-wrap gap-1.5">
                    {data.good.map((g) => (
                      <span key={g} className="inline-flex items-center gap-1.5 rounded-full bg-ok-soft px-2.5 py-[3px] text-[12px] text-ok">
                        <Check size={11} strokeWidth={2.6} />
                        {g}
                      </span>
                    ))}
                  </div>
                ) : (
                  <span className="text-[12.5px] text-muted">Nothing to report on this system.</span>
                )}
                <h2 className="m-0 mt-1.5 text-[14px] font-semibold">Popular tweaks we skip</h2>
                <ul className="m-0 flex list-none flex-col gap-[7px] p-0">
                  {SKIPPED.map((m) => (
                    <li key={m.t} className="grid grid-cols-[14px_minmax(0,1fr)] gap-2 text-[12.5px] leading-[1.45]">
                      <Ban size={13} className="mt-[3px] text-subtle" />
                      <span>
                        <b className="font-medium">{m.t}</b>
                        <span className="text-muted"> · {m.why}</span>
                      </span>
                    </li>
                  ))}
                </ul>
              </Panel>
            </div>
          </div>

          <div className="sticky bottom-0 z-10 mt-auto flex items-center gap-3.5 rounded-[14px] border border-border bg-surface py-3 pl-[18px] pr-3 shadow-float">
            <Gauge size={18} className="text-accent" />
            <span className="flex-1">
              <b className="font-semibold">
                {chosen.length} change{chosen.length === 1 ? "" : "s"}
              </b>
              <span className="text-muted">
                {" "}
                · {chosen.some((i) => i.admin) ? "one Windows prompt" : "no admin needed"}
                {notes.length ? ` · ${notes.join(", ").toLowerCase()}` : " · no restart"} · undo any time from History
              </span>
            </span>
            <Button variant="primary" className="h-[38px] px-[18px]" disabled={!chosen.length || busy} loading={busy} onClick={() => void apply()} data-testid="tune-apply">
              {chosen.length ? `Apply ${chosen.length} change${chosen.length === 1 ? "" : "s"}` : available.length ? "Nothing selected" : "All set"}
            </Button>
          </div>
        </>
      )}
    </div>
  )
}

const rank = (i: TuneItem) => (i.state === "available" ? 0 : i.state === "applied" ? 1 : 2)

const pct = (s: { commitUsed: number; commitLimit: number }) => (s.commitLimit ? (s.commitUsed / s.commitLimit) * 100 : 0)

function Baseline({ pressure, uptime, go, loaded }: { pressure: PressureHistory | null; uptime: number | null; go: (p: Page) => void; loaded: boolean }) {
  const has = !!pressure && pressure.samples.length > 1
  const spellMins = has ? pressure!.slowSpells.reduce((n, s) => n + Math.max(0, s.end - s.start), 0) / 60 : 0
  const peakPaging = has ? Math.max(pressure!.peakPagesIn, ...pressure!.samples.map((s) => s.pagesInPerSec)) : 0
  const peakCommit = has ? Math.max(pressure!.peakCommitPct, ...pressure!.samples.map(pct)) : 0
  const highShare = has ? (pressure!.samples.filter((s) => s.level === "high").length / pressure!.samples.length) * 100 : 0
  const signals: [string, string, number, boolean][] = has
    ? [
        ["Committed memory at peak", `${peakCommit.toFixed(0)}%`, peakCommit, peakCommit > 85],
        ["Paging at peak", `${Math.round(peakPaging)}/s`, Math.min(100, (peakPaging / 300) * 100), peakPaging > 100],
        ["Time under high pressure", `${highShare.toFixed(highShare < 10 ? 1 : 0)}%`, Math.min(100, highShare * 4), highShare > 2],
      ]
    : []
  return (
    <Panel label="How this PC feels" title="Under load" aside={<span className="text-[12px] text-subtle">{has ? `measured since ${ago(pressure!.since)}` : ""}</span>}>
      {!loaded ? (
        <Skeleton className="h-24" />
      ) : has ? (
        <>
          <div className="flex items-center gap-3">
            <Num className="text-[34px] font-semibold leading-none">{pressure!.slowSpells.length}</Num>
            <span className="leading-[1.4] text-muted">
              <b className="font-semibold text-text">slow spell{pressure!.slowSpells.length === 1 ? "" : "s"}</b>, {Math.round(spellMins)} min in total
              <br />
              <span className="text-[12px]">times when memory ran short and Windows paged to disk</span>
            </span>
          </div>
          <div className="flex flex-col gap-[9px]">
            {signals.map(([label, v, w, bad]) => (
              <div key={label} className="grid grid-cols-[minmax(0,1fr)_92px_56px] items-center gap-2.5">
                <span className="truncate text-muted">{label}</span>
                <span className="h-1.5 overflow-hidden rounded-full bg-track">
                  <span className="block h-full rounded-full" style={{ width: `${Math.max(2, w)}%`, background: bad ? "var(--warn)" : "var(--ok)" }} />
                </span>
                <Num className="text-right font-medium">{v}</Num>
              </div>
            ))}
          </div>
        </>
      ) : (
        <div className="flex items-start gap-3 text-muted">
          <Activity size={18} className="mt-0.5 shrink-0 text-accent" />
          <span className="leading-[1.5]">
            <b className="font-medium text-text">Measuring starts now.</b> Dev Cleaner records memory pressure while it's open; after a day of normal work this shows how often the PC got slow, and History compares before and after each change.
          </span>
        </div>
      )}
      <div className="flex items-center gap-2.5 rounded-[10px] bg-accent-soft px-3 py-2.5 text-[12.5px] leading-[1.45]">
        <span className="flex-1">Measured only while Dev Cleaner runs. The Memory page shows it live.</span>
        <Button size="sm" onClick={() => go("memory")}>
          Memory
        </Button>
      </div>
      {uptime != null && uptime > 7 * 86400 && (
        <div className="flex items-center gap-2.5 text-[12.5px] text-muted" data-testid="restart-nudge">
          <span className="flex-1">
            <b className="font-semibold text-text">Up {duration(uptime)}.</b> Driver and kernel memory only shrinks with a restart; one this week is worth it.
          </span>
        </div>
      )}
    </Panel>
  )
}

function HistoryView({ history, pressure, onUndo }: { history: JournalEntry[] | null; pressure: PressureHistory | null; onUndo: (id: string) => void }) {
  const around = useMemo(() => {
    const samples = pressure?.samples ?? []
    const win = 3600
    return (t: number) => {
      const before = samples.filter((s) => s.t < t && s.t >= t - win * 24)
      const after = samples.filter((s) => s.t > t && s.t <= t + win * 24)
      if (before.length < 5 || after.length < 5) return null
      const avg = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length
      return {
        commit: [avg(before.map(pct)), avg(after.map(pct))],
        paging: [avg(before.map((s) => s.pagesInPerSec)), avg(after.map((s) => s.pagesInPerSec))],
      }
    }
  }, [pressure])
  if (!history) return <Skeleton className="h-40" />
  if (!history.length)
    return (
      <Empty icon={<RotateCcw size={26} />} title="No changes yet">
        Every change you apply on the Tune tab is listed here, with what it replaced and an Undo button.
      </Empty>
    )
  return (
    <Panel label="History" title="Changes" aside={<span className="text-[12px] text-subtle">newest first</span>} className="px-2.5 pb-2">
      <ul className="m-0 flex list-none flex-col p-0" data-testid="tune-history">
        {history.map((h) => {
          const cmp = around(h.appliedAt)
          return (
            <li key={h.id} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-t border-border px-2 py-3 first:border-t-0">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{h.title}</span>
                  <span className={cn("rounded-full px-2 py-[1px] text-[11px] font-medium", h.undoneAt ? "bg-track text-muted" : "bg-ok-soft text-ok")}>{h.undoneAt ? `undone ${ago(h.undoneAt)}` : "active"}</span>
                </div>
                <div className="mt-0.5 text-[12px] text-muted">
                  Applied {ago(h.appliedAt)} · {h.undoNote ?? h.summary}
                </div>
                {cmp && (
                  <div className="mt-1 text-[12px] text-subtle">
                    Day before → day after: committed memory <Num>{cmp.commit[0].toFixed(0)}% → {cmp.commit[1].toFixed(0)}%</Num>, paging{" "}
                    <Num>
                      {cmp.paging[0].toFixed(0)}/s → {cmp.paging[1].toFixed(0)}/s
                    </Num>
                  </div>
                )}
              </div>
              {!h.undoneAt && (
                <Button size="sm" icon={<RotateCcw size={12} />} onClick={() => onUndo(h.id)} data-testid={`undo-${h.tweak}`}>
                  Undo
                </Button>
              )}
            </li>
          )
        })}
      </ul>
    </Panel>
  )
}
