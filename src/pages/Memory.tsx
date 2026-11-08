import { SnapshotButton } from "@/components/Share"
import { useState } from "react"
import { MemoryStick, X } from "lucide-react"
import { api, type FreeResult, type ProcGroup } from "@/lib/api"
import { bytes } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { AnimatedNumber, Button, Card, Dialog, InfoTip, PageHeader, Skeleton, Sparkline, Tooltip, VerdictPill } from "@/components/ui"
import { ProcIcon, useKill } from "@/components/shared"

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

export function Memory() {
  const { stats, memHistory, platform } = useStore()
  const procs = usePoll(api.listProcesses, 2500)
  const kill = useKill(procs.refresh)
  const free = useFreeMemory()

  const total = stats?.memTotal ?? 0
  const used = stats?.memUsed ?? 0
  const compressed = stats?.compressed ?? 0
  const apps = Math.max(0, used - compressed)
  const freeB = Math.max(0, total - used)
  const parts = [
    { label: "Apps", value: apps, color: "var(--accent)", tip: "Memory apps are actively using." },
    { label: "Compressed", value: compressed, color: "color-mix(in oklch, var(--accent) 55%, var(--surface))", tip: "Pages Windows compressed instead of writing to disk. Saves RAM for a little CPU." },
    { label: "Free", value: freeB, color: "var(--track)", tip: "Not in use. Windows also uses part of this as a file cache that it hands back instantly." },
  ]
  const top = (procs.data ?? []).slice(0, 12)

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Memory"
        lede={stats ? <>In use: <AnimatedNumber value={used} format={(n) => bytes(n)} /> of {bytes(total, 0)}.</> : "Reading memory…"}
        actions={
          <>
          <SnapshotButton name="memory" />
          <Tooltip text={freeMemoryHelp(platform)} wide side="bottom">
            <Button variant="primary" icon={<MemoryStick size={15} />} loading={free.busy} onClick={free.run} data-testid="free-memory">
              Free memory
            </Button>
          </Tooltip>
          </>
        }
      />
      <div className="grid grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)] gap-3.5">
        <Card className="flex flex-col gap-3.5 p-4">
          <div className="flex items-baseline justify-between">
            <h2 className="m-0 text-[13px] font-semibold">What's using memory</h2>
            <span className="mono text-[12px] text-subtle">{total ? `${Math.round((used / total) * 100)}%` : ""}</span>
          </div>
          {stats ? (
            <div className="flex h-3.5 gap-0.5 overflow-hidden rounded-full">
              {parts.map((p) => (
                <div key={p.label} className="h-full transition-[width] duration-700 ease-out" style={{ width: `${(p.value / Math.max(1, total)) * 100}%`, background: p.color }} />
              ))}
            </div>
          ) : (
            <Skeleton className="h-3.5" />
          )}
          <div className="grid grid-cols-3 gap-3">
            {parts.map((p) => (
              <div key={p.label} className="flex flex-col gap-0.5">
                <span className="flex items-center gap-1.5 text-[12px] text-muted">
                  <span className="h-2 w-2 rounded-sm" style={{ background: p.color }} />
                  {p.label}
                  <InfoTip text={p.tip} />
                </span>
                <span className="mono text-[15px] font-medium">{bytes(p.value)}</span>
              </div>
            ))}
          </div>
          <p className="m-0 text-[12px] text-muted">
            Free memory trims idle pages from running apps and gives them back to the system. Nothing is closed.
          </p>
        </Card>
        <Card className="flex flex-col p-4">
          <div className="flex items-baseline justify-between">
            <h2 className="m-0 text-[13px] font-semibold">Last 90 seconds</h2>
            <span className="mono text-[12px] text-subtle">{memHistory.length ? `${memHistory[memHistory.length - 1].toFixed(0)}%` : ""}</span>
          </div>
          <div className="mt-3 flex-1">
            <Sparkline data={memHistory} height={110} />
          </div>
          {stats && stats.swapTotal > 0 && (
            <div className="mono mt-2 text-[12px] text-muted">
              Page file / swap: {bytes(stats.swapUsed)} of {bytes(stats.swapTotal, 0)}
            </div>
          )}
        </Card>
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
                <span className="mono text-right">{bytes(p.memory)}</span>
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
