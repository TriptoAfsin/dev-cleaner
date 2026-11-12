import { useEffect, useMemo, useState, type ReactNode } from "react"
import { Check, ChevronDown, RefreshCw, RotateCcw, ShieldCheck } from "lucide-react"
import { api } from "@/lib/api"
import { ago, bytes, cn } from "@/lib/format"
import { useStore } from "@/lib/store"
import { deepCache, tuneApi, type DeepPlan, type DeepResult, type DeepScan } from "@/lib/tune"
import { Button, Checkbox, Dialog, Empty, PageHeader, Skeleton, Switch } from "@/components/ui"
import { Num, Panel } from "@/components/tune/parts"

type PageChoice = number | "keep"

/** System junk › Deep clean: old drivers, installer leftovers, the pagefile
    and the component store. One Windows prompt runs everything chosen. */
export function DeepClean({ tabs }: { tabs: ReactNode }) {
  const { toast, setActivity, platform } = useStore()
  const [scan, setScan] = useState<DeepScan | null>(deepCache.scan)
  const [loading, setLoading] = useState(false)
  const [offDrivers, setOffDrivers] = useState<Set<string>>(new Set())
  const [installers, setInstallers] = useState(true)
  const [showFiles, setShowFiles] = useState(false)
  const [page, setPage] = useState<PageChoice | null>(null)
  const [component, setComponent] = useState(true)
  const [resetBase, setResetBase] = useState(false)
  const [dropOld, setDropOld] = useState<Set<string>>(new Set())
  const [confirm, setConfirm] = useState(false)
  const [running, setRunning] = useState(false)
  const [result, setResult] = useState<DeepResult | null>(null)

  const load = async () => {
    if (!api.inTauri) return
    setLoading(true)
    setActivity({ kind: "busy", text: "Reading drivers, installer files and the pagefile" })
    try {
      const s = await tuneApi.deepScan()
      deepCache.scan = s
      setScan(s)
      setActivity({ kind: "ok", text: "Deep clean scan done" })
    } catch (e) {
      toast({ kind: "error", title: "Deep clean scan failed", detail: String(e) })
      setActivity({ kind: "error", text: "Deep clean scan failed" })
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    if (!scan) void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const pf = scan?.pagefile
  const recommended = pf?.options.findIndex((o) => o.recommended) ?? -1
  const pageChoice: PageChoice = page ?? (pf?.oversized && recommended >= 0 ? recommended : "keep")
  const pageOpt = typeof pageChoice === "number" ? pf?.options[pageChoice] : undefined

  const drivers = scan?.drivers ?? []
  const selDrivers = drivers.filter((g) => !offDrivers.has(g.key) && g.versions.some((v) => !v.keep))
  const driverBytes = selDrivers.reduce((n, g) => n + g.reclaim, 0)
  const inst = scan?.installer
  const instOn = installers && !!inst?.checked && inst.orphans.length > 0
  const oldBatches = (scan?.quarantine ?? []).filter((q) => q.old)
  const dropBytes = oldBatches.filter((q) => dropOld.has(q.id)).reduce((n, q) => n + q.size, 0)

  const plan: DeepPlan = useMemo(
    () => ({
      drivers: selDrivers.flatMap((g) => g.versions.filter((v) => !v.keep).map((v) => v.published)),
      installers: instOn ? inst!.orphans.map((f) => f.path) : [],
      pagefile: pageOpt ? { mode: pageOpt.mode, gb: pageOpt.gb } : null,
      component,
      resetBase: component && resetBase,
      deleteQuarantine: [...dropOld].filter((id) => oldBatches.some((q) => q.id === id)),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [scan, offDrivers, installers, pageChoice, component, resetBase, dropOld],
  )
  const total = driverBytes + (instOn ? inst!.orphanSize : 0) + (pageOpt?.saves ?? 0) + dropBytes
  const steps: string[] = [
    ...(plan.drivers.length ? [`pnputil /delete-driver for ${plan.drivers.length} old driver package${plan.drivers.length === 1 ? "" : "s"} (never /force)`] : []),
    ...(plan.installers.length ? [`Move ${plan.installers.length} unused installer file${plan.installers.length === 1 ? "" : "s"} to C:\\DevCleaner Quarantine (restorable)`] : []),
    ...(plan.deleteQuarantine.length ? [`Delete ${plan.deleteQuarantine.length} quarantine batch${plan.deleteQuarantine.length === 1 ? "" : "es"} older than 30 days`] : []),
    ...(pageOpt ? [pageOpt.mode === "system" ? "Set the pagefile to system-managed" : `Set the pagefile on ${pf?.system?.path ?? "C:"} to a fixed ${pageOpt.gb} GB`] : []),
    ...(component ? [`Dism /Online /Cleanup-Image /StartComponentCleanup${resetBase ? " /ResetBase" : ""}`] : []),
  ]

  const run = async () => {
    setConfirm(false)
    setRunning(true)
    setActivity({ kind: "busy", text: "Running deep clean (waiting for the Windows prompt)" })
    try {
      const r = await tuneApi.deepRun(plan)
      deepCache.scan = r.scan
      setScan(r.scan)
      setOffDrivers(new Set())
      setPage(null)
      setDropOld(new Set())
      setResult(r)
      if (r.dryRun) setActivity({ kind: "ok", text: "Deep clean: dry run" })
      else if (!r.ran) setActivity({ kind: "error", text: "Deep clean cancelled" })
      else setActivity({ kind: "ok", text: `Deep clean freed ${bytes(r.freed)}${r.restartNeeded ? " · restart to finish" : ""}` })
    } catch (e) {
      toast({ kind: "error", title: "Deep clean failed", detail: String(e) })
      setActivity({ kind: "error", text: "Deep clean failed" })
    } finally {
      setRunning(false)
    }
  }

  const restore = async (id: string) => {
    try {
      const r = await tuneApi.quarantineRestore(id)
      toast({ kind: "ok", title: r.dryRun ? "Dry run: nothing moved" : `Restored ${r.restored} installer file${r.restored === 1 ? "" : "s"}` })
      void load()
    } catch (e) {
      toast({ kind: "error", title: "Could not restore", detail: String(e) })
    }
  }

  const header = (
    <PageHeader
      title="System junk"
      tabs={tabs}
      lede="Deep clean: what Windows keeps forever. Everything here needs admin and runs after one Windows prompt."
      actions={
        <Button icon={<RefreshCw size={15} className={loading ? "spin" : undefined} />} disabled={loading || running} onClick={() => void load()}>
          {loading ? "Scanning" : "Rescan"}
        </Button>
      }
    />
  )

  if (platform !== "windows" || (scan && !scan.supported)) {
    return (
      <div className="flex flex-col gap-4">
        {header}
        <Empty icon={<ShieldCheck size={28} />} title="Deep clean is for Windows">
          Old driver packages, Windows Installer leftovers and the pagefile are Windows things. On this system, Quick clean covers the caches and logs that pile up.
        </Empty>
      </div>
    )
  }

  const nTasks = (plan.drivers.length ? 1 : 0) + (plan.installers.length ? 1 : 0) + (plan.deleteQuarantine.length ? 1 : 0) + (pageOpt ? 1 : 0) + (component ? 1 : 0)

  return (
    <div className="flex min-h-full flex-col gap-4" data-testid="deep-clean">
      {header}
      {scan?.dryRun && <p className="m-0 rounded-lg bg-accent-soft px-3 py-2 text-[12.5px] text-accent">Test sandbox: nothing on this machine is read or changed.</p>}

      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-start gap-4">
        <div className="flex flex-col gap-4">
          {/* Pagefile */}
          <Panel
            label="Pagefile"
            title={<>Pagefile on {pf?.system?.path.slice(0, 2) ?? "C:"}</>}
            aside={
              <>
                {pf?.oversized && <span className="rounded-full bg-warn-soft px-2 py-0.5 text-[11.5px] font-medium text-warn">oversized</span>}
                <Num className="text-[18px] font-semibold">{pf?.system ? bytes(pf.system.allocated) : scan ? "—" : ""}</Num>
              </>
            }
          >
            {!scan ? (
              <Skeleton className="h-24" />
            ) : !pf?.system ? (
              <p className="m-0 text-muted">{pf?.reason || "Could not read the pagefile settings."}</p>
            ) : (
              <>
                <div className="relative h-[26px] overflow-hidden rounded-lg bg-track">
                  <span className="absolute inset-y-0 left-0 bg-accent" style={{ width: `${Math.max(0.6, (pf.system.peak / pf.system.allocated) * 100)}%` }} />
                  {pageOpt && (
                    <span
                      className="absolute inset-y-0 left-0 border-r-2 border-dashed border-ok"
                      style={{ width: `${Math.min(100, ((pageOpt.gb * 1024 ** 3) / pf.system.allocated) * 100)}%`, background: "color-mix(in oklch, var(--ok) 10%, transparent)" }}
                    />
                  )}
                  <span className="absolute left-3 top-[5px] text-[11.5px] text-muted" style={{ left: pageOpt ? `calc(${Math.min(80, ((pageOpt.gb * 1024 ** 3) / pf.system.allocated) * 100)}% + 10px)` : 12 }}>
                    peak use {bytes(pf.system.peak)}
                    {pageOpt ? ` · new size ${pageOpt.mode === "system" ? "chosen by Windows" : `${pageOpt.gb} GB`}` : ""}
                  </span>
                </div>
                <p className="m-0 text-[12.5px] leading-[1.5] text-muted">{pf.reason}</p>
                <div role="radiogroup" aria-label="Pagefile size" className="flex flex-col gap-1.5">
                  {[...pf.options.map((o, i) => ({ key: i as PageChoice, label: o.label, detail: o.detail, gain: bytes(o.saves) })), { key: "keep" as PageChoice, label: `Keep ${bytes(pf.system.allocated)}`, detail: "no change", gain: "0 GB" }].map((o) => {
                    const on = pageChoice === o.key
                    return (
                      <button
                        key={String(o.key)}
                        type="button"
                        role="radio"
                        aria-checked={on}
                        onClick={() => setPage(o.key)}
                        className={cn("flex h-10 items-center gap-2.5 rounded-[10px] border px-3 text-left transition-colors", on ? "border-accent bg-accent-soft" : "border-border bg-surface hover:border-border-strong")}
                      >
                        <span className="box-border h-3.5 w-3.5 rounded-full bg-surface" style={{ border: on ? "4.5px solid var(--accent)" : "1.5px solid var(--border-strong)" }} />
                        <span className="flex-1">
                          <b className="font-medium">{o.label}</b>
                          <span className="text-subtle"> · {o.detail}</span>
                        </span>
                        <Num className="font-semibold text-ok">{o.gain}</Num>
                      </button>
                    )
                  })}
                </div>
                <span className="text-[12px] text-subtle">
                  Takes effect after a restart. Never removed: Windows needs a pagefile when memory runs short.
                  {pf.files.length > 1 ? ` Pagefiles on other drives stay as they are.` : ""}
                </span>
              </>
            )}
          </Panel>

          {/* Component store */}
          <Panel label="Component store" title="Windows component store" aside={<span className="text-[12px] text-subtle">WinSxS · size known after DISM runs</span>}>
            <div className="flex items-center gap-3">
              <span className="flex-1 text-muted">
                Remove superseded components <code className="text-[11.5px]">/StartComponentCleanup</code>
                <br />
                <span className="text-[12px] text-subtle">Takes a few minutes. Usually frees 1–3 GB.</span>
              </span>
              <Switch checked={component} onChange={setComponent} label="Clean the component store" />
            </div>
            <div className={cn("flex items-center gap-3 rounded-[10px] px-3 py-2.5 transition-colors", component && resetBase ? "bg-warn-soft" : "bg-surface-2", !component && "opacity-50")}>
              <span className="flex-1 leading-[1.45]">
                <b className="font-medium">Also reset the base</b> <code className="text-[11.5px]">/ResetBase</code>
                <br />
                <span className="text-[12px] text-muted">Frees more. Installed updates can no longer be uninstalled.</span>
              </span>
              <Switch checked={component && resetBase} onChange={(v) => component && setResetBase(v)} label="Reset base" />
            </div>
          </Panel>
        </div>

        <div className="flex flex-col gap-4">
          {/* Drivers */}
          <Panel
            label="Old drivers"
            title="Superseded drivers"
            aside={
              <>
                <span className="text-[12px] text-subtle">{scan ? `${drivers.length} with older versions · newest kept` : ""}</span>
                <Num className="text-[16px] font-semibold">{bytes(driverBytes)}</Num>
              </>
            }
            className="px-2.5 pb-2.5"
          >
            {!scan ? (
              <Skeleton className="mx-2 h-32" />
            ) : !scan.driversRead ? (
              <p className="m-0 px-2 text-muted">Could not read the driver store (pnputil).</p>
            ) : drivers.length === 0 ? (
              <p className="m-0 px-2 text-muted">Every driver has a single version. Nothing to remove.</p>
            ) : (
              <ul className="m-0 flex max-h-[340px] list-none flex-col gap-0.5 overflow-y-auto p-0">
                {drivers.map((g) => {
                  const on = !offDrivers.has(g.key)
                  return (
                    <li key={g.key} className="grid grid-cols-[18px_minmax(0,1fr)_68px] items-center gap-3 rounded-[10px] px-2 py-2 hover:bg-hover">
                      <Checkbox
                        label={`Remove old versions of ${g.original}`}
                        checked={on}
                        onChange={(v) => {
                          const s = new Set(offDrivers)
                          if (v) s.delete(g.key)
                          else s.add(g.key)
                          setOffDrivers(s)
                        }}
                      />
                      <div className="min-w-0">
                        <div className="truncate font-medium" title={`${g.original} · ${g.class}`}>
                          {g.provider} <span className="font-normal text-subtle">· {g.original.replace(/\.inf$/, "")}</span>
                        </div>
                        <div className="mt-1 flex flex-wrap gap-1.5">
                          {g.versions.map((v) => (
                            <span
                              key={v.published}
                              title={`${v.published} · ${v.date}${v.size ? ` · ${bytes(v.size)}` : ""}`}
                              className={cn(
                                "rounded-md px-[7px] py-[2px] text-[10.5px] tabular-nums",
                                v.keep ? "bg-ok-soft text-ok" : "bg-surface-2 text-muted",
                                !v.keep && on && "line-through decoration-subtle",
                              )}
                            >
                              {v.version}
                              {v.keep ? " · kept" : ""}
                            </span>
                          ))}
                        </div>
                      </div>
                      <Num className="text-right font-medium">{g.reclaim ? bytes(g.reclaim) : "<1 MB"}</Num>
                    </li>
                  )
                })}
              </ul>
            )}
          </Panel>

          {/* Installer leftovers */}
          <Panel label="Installer leftovers" title="Installer leftovers" aside={<Num className="text-[16px] font-semibold">{inst?.checked ? bytes(inst.orphanSize) : ""}</Num>} className="px-2.5 pb-2.5">
            {!scan ? (
              <Skeleton className="mx-2 h-14" />
            ) : !inst?.checked ? (
              <p className="m-0 px-2 text-muted">Could not read which installer files are in use, so none are offered.</p>
            ) : inst.orphans.length === 0 ? (
              <p className="m-0 px-2 text-muted">Every file in Windows\Installer belongs to an installed program.</p>
            ) : (
              <div className="flex flex-col">
                <div className="grid grid-cols-[18px_minmax(0,1fr)_68px] items-center gap-3 rounded-[10px] px-2 py-2 hover:bg-hover">
                  <Checkbox label="Quarantine unused installer files" checked={installers} onChange={setInstallers} />
                  <div className="min-w-0">
                    <div className="font-medium">Unused Windows Installer packages</div>
                    <div className="mt-0.5 text-[12px] leading-[1.45] text-muted">
                      {inst.orphans.length} .msi/.msp files of {inst.totalFiles} that no installed program lists. Moved to C:\DevCleaner Quarantine, restorable for 30 days.
                    </div>
                    <button type="button" className="mt-1 inline-flex items-center gap-1 text-[12px] text-accent hover:underline" onClick={() => setShowFiles(!showFiles)} aria-expanded={showFiles}>
                      {showFiles ? "Hide files" : "Show files"} <ChevronDown size={12} className={cn("transition-transform", showFiles && "rotate-180")} />
                    </button>
                  </div>
                  <Num className="text-right font-medium">{bytes(inst.orphanSize)}</Num>
                </div>
                {showFiles && (
                  <ul className="m-0 ml-[38px] mt-1 max-h-[160px] list-none overflow-y-auto p-0 text-[12px]">
                    {inst.orphans.map((f) => (
                      <li key={f.path} className="flex justify-between gap-3 py-0.5 text-muted">
                        <span className="truncate">{f.name}</span>
                        <Num>{bytes(f.size)}</Num>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
            {(scan?.quarantine.length ?? 0) > 0 && (
              <div className="mt-1 flex flex-col gap-1 border-t border-border px-2 pt-2.5">
                <span className="text-[11px] font-medium uppercase tracking-[0.06em] text-subtle">In quarantine</span>
                {scan!.quarantine.map((q) => (
                  <div key={q.id} className="flex items-center gap-2 text-[12.5px]">
                    {q.old && (
                      <Checkbox
                        label="Delete this batch for good"
                        checked={dropOld.has(q.id)}
                        onChange={(v) => {
                          const s = new Set(dropOld)
                          if (v) s.add(q.id)
                          else s.delete(q.id)
                          setDropOld(s)
                        }}
                      />
                    )}
                    <span className="flex-1 text-muted">
                      {q.files} file{q.files === 1 ? "" : "s"} · moved {ago(q.created)}
                      {q.old ? " · tick to delete for good" : ""}
                    </span>
                    <Num>{bytes(q.size)}</Num>
                    <Button size="sm" icon={<RotateCcw size={12} />} onClick={() => void restore(q.id)}>
                      Restore
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </Panel>
        </div>
      </div>

      <div className="sticky bottom-0 z-10 mt-auto flex items-center gap-3.5 rounded-[14px] border border-border bg-surface py-3 pl-[18px] pr-3 shadow-float">
        <ShieldCheck size={18} className="text-warn" />
        <span className="flex-1">
          <b className="font-semibold">
            {nTasks} admin task{nTasks === 1 ? "" : "s"}
          </b>
          <span className="text-muted">
            {" "}
            · one Windows prompt · {pageOpt ? "pagefile change needs a restart" : "no restart needed"}
            {component ? " · plus the component store" : ""}
          </span>
        </span>
        <Num className="text-[20px] font-semibold">~{bytes(total)}</Num>
        <Button variant="primary" className="h-[38px] px-[18px]" disabled={!nTasks || running || !scan} loading={running} onClick={() => setConfirm(true)} data-testid="deep-run">
          Run deep clean
        </Button>
      </div>

      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title="Run deep clean?"
        actions={
          <>
            <Button onClick={() => setConfirm(false)}>Cancel</Button>
            <Button variant="primary" onClick={() => void run()} data-testid="deep-confirm">
              Run with one Windows prompt
            </Button>
          </>
        }
      >
        <p className="m-0 mb-2">Windows asks for admin once, then this runs in order:</p>
        <ol className="m-0 flex list-decimal flex-col gap-1 pl-5 text-text">
          {steps.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
        {resetBase && component && <p className="m-0 mt-3 text-warn">After /ResetBase, updates installed so far can no longer be uninstalled.</p>}
      </Dialog>

      <Dialog
        open={!!result}
        onClose={() => setResult(null)}
        title={result?.dryRun ? "Dry run" : result?.ran ? `Freed ${bytes(result.freed)}` : "Nothing ran"}
        actions={
          <Button variant="primary" onClick={() => setResult(null)}>
            Done
          </Button>
        }
      >
        {result && (
          <div className="flex flex-col gap-1.5 text-text">
            {result.driversRemoved.length > 0 && (
              <div className="flex items-center gap-2">
                <Check size={14} className="text-ok" /> Removed {result.driversRemoved.length} old driver package{result.driversRemoved.length === 1 ? "" : "s"}
              </div>
            )}
            {result.quarantined > 0 && (
              <div className="flex items-center gap-2">
                <Check size={14} className="text-ok" /> Moved {result.quarantined} installer file{result.quarantined === 1 ? "" : "s"} to quarantine
              </div>
            )}
            {result.notes.map((n) => (
              <p key={n} className="m-0 text-muted">
                {n}
              </p>
            ))}
            {result.restartNeeded && <p className="m-0 font-medium text-warn">Restart Windows to finish the pagefile change.</p>}
          </div>
        )}
      </Dialog>
    </div>
  )
}
