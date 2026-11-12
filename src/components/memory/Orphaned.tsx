import { useState, type ReactNode } from "react"
import { ArrowRight, Box, Check, Copy, Power, ShieldCheck, X } from "lucide-react"
import { SnapshotButton } from "@/components/Share"
import { ago, bytes, cn } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { ORPHAN_GROUPS, pressureApi, wslconfigDiff, type Orphan, type OrphanGroup } from "@/lib/pressure"
import { Button, Card, Checkbox, Dialog, Empty, PageHeader, Skeleton } from "@/components/ui"
import { ProcIcon, useKill } from "@/components/shared"

const ORDER: OrphanGroup[] = ["mcp", "lsp", "dev", "other"]
const go = (page: string) => window.dispatchEvent(new CustomEvent("dc-go", { detail: page }))

/** Processes › Orphaned: dev runtimes whose parent has exited, plus the
    memory held by the WSL VM and Docker containers. */
export function Orphaned({ tabs, orphans }: { tabs: ReactNode; orphans: { data: Orphan[] | null; refresh: () => Promise<void> } }) {
  const { platform } = useStore()
  const kill = useKill(() => void orphans.refresh())
  const list = (orphans.data ?? []).filter((o) => !o.protected)
  // Picked by default: everything but the "other" runtimes, which may be a
  // script someone started on purpose.
  const [sel, setSel] = useState<Set<number> | null>(null)
  const selected = sel ?? new Set(list.filter((o) => o.group !== "other").map((o) => o.pid))
  const chosen = list.filter((o) => selected.has(o.pid))
  const chosenBytes = chosen.reduce((n, o) => n + o.memory, 0)
  const toggle = (pid: number, v: boolean) => {
    const s = new Set(selected)
    if (v) s.add(pid)
    else s.delete(pid)
    setSel(s)
  }
  const endChosen = () =>
    kill.request(
      `${chosen.length} orphaned process${chosen.length === 1 ? "" : "es"}`,
      chosen.map((o) => o.pid),
      undefined,
      <ul className="mb-3 mt-0 flex max-h-40 list-none flex-col gap-1 overflow-auto p-0">
        {chosen.map((o) => (
          <li key={o.pid} className="flex justify-between gap-3 text-[12.5px]">
            <span className="truncate">
              {o.name} <span className="text-subtle">PID {o.pid}</span>
            </span>
            <span className="tabular-nums">{bytes(o.memory)}</span>
          </li>
        ))}
      </ul>,
    )

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader title="Processes" tabs={tabs} lede="Leftovers from editors and AI agents, and memory held by virtual machines and containers." actions={<SnapshotButton name="orphans" />} />
      <div className={cn("grid min-h-[380px] flex-1 grid-rows-[minmax(0,1fr)] gap-3.5", "grid-cols-[minmax(0,1fr)_360px]")}>
        <Card className="flex min-h-0 min-w-0 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-y-auto px-2 py-1.5" data-testid="orphan-list">
            {!orphans.data ? (
              Array.from({ length: 5 }).map((_, i) => (
                <div key={i} className="px-2 py-3">
                  <Skeleton className="h-6" />
                </div>
              ))
            ) : list.length === 0 ? (
              <Empty icon={<ShieldCheck size={22} />} title="No orphaned dev processes">
                Every node, Python and other dev runtime running now still has the editor, terminal or agent that started it.
              </Empty>
            ) : (
              ORDER.map((g) => {
                const rows = list.filter((o) => o.group === g)
                if (!rows.length) return null
                const all = rows.every((o) => selected.has(o.pid))
                return (
                  <div key={g}>
                    <div className="flex items-center gap-2 px-2.5 pb-1.5 pt-3">
                      <Checkbox
                        checked={all}
                        indeterminate={!all && rows.some((o) => selected.has(o.pid))}
                        onChange={(v) => {
                          const s = new Set(selected)
                          rows.forEach((o) => (v ? s.add(o.pid) : s.delete(o.pid)))
                          setSel(s)
                        }}
                        label={`Select all ${ORPHAN_GROUPS[g]}`}
                      />
                      <span className="text-[11px] font-semibold uppercase tracking-[0.06em] text-subtle">{ORPHAN_GROUPS[g]}</span>
                      <span className="h-px flex-1 bg-border" />
                      <span className="text-[11.5px] tabular-nums text-subtle">{bytes(rows.reduce((n, o) => n + o.memory, 0))}</span>
                    </div>
                    {rows.map((o) => (
                      <label key={o.pid} className="grid cursor-pointer grid-cols-[18px_minmax(0,1fr)_150px_72px] items-center gap-3 rounded-[10px] px-2.5 py-[7px] hover:bg-hover">
                        <Checkbox checked={selected.has(o.pid)} onChange={(v) => toggle(o.pid, v)} label={`${o.name} ${o.pid}`} />
                        <div className="min-w-0">
                          <div className="flex items-baseline gap-2">
                            <span className="flex items-center gap-1.5 font-medium">
                              <ProcIcon name={o.name} />
                              {o.name}
                            </span>
                            <span className="mono text-[11px] text-subtle">PID {o.pid}</span>
                          </div>
                          <div className="mono truncate text-[11px] text-muted" title={o.cmd}>
                            {o.cmd}
                          </div>
                        </div>
                        <div className="text-[11.5px] leading-[1.35] text-subtle">
                          {o.why}
                          <br />
                          started {ago(o.startedAt)}
                        </div>
                        <span className="text-right font-medium tabular-nums">{bytes(o.memory)}</span>
                      </label>
                    ))}
                  </div>
                )
              })
            )}
          </div>
          <div className="flex items-center gap-3 border-t border-border bg-surface-2 px-4 py-3">
            <ShieldCheck size={15} className="shrink-0 text-ok" aria-hidden />
            <span className="flex-1 text-[12px] text-muted">Orphaned means the editor or agent that started it has exited. Nothing with a live parent is listed.</span>
            <Button variant="primary" icon={<X size={13} strokeWidth={2.4} />} disabled={!chosen.length} onClick={endChosen} data-testid="end-orphans">
              End {chosen.length} · {bytes(chosenBytes)}
            </Button>
          </div>
        </Card>

        <aside className="flex min-h-0 flex-col gap-3.5 overflow-y-auto">
          {platform === "windows" && <WslCard />}
          <ContainersCard />
        </aside>
      </div>
      {kill.dialog}
    </div>
  )
}

function WslCard() {
  const { toast } = useStore()
  const wsl = usePoll(pressureApi.wslStatus, 20_000)
  const [busy, setBusy] = useState<"reclaim" | "shutdown" | null>(null)
  const [confirmStop, setConfirmStop] = useState(false)
  const [copied, setCopied] = useState(false)
  const w = wsl.data
  if (!w) return <Skeleton className="h-40 rounded-xl" />
  if (!w.available) return null

  const lx = w.linux
  const scale = Math.max(1, lx?.total ?? 1)
  const running = w.distros.filter((d) => d.running).map((d) => d.name)

  const act = async (kind: "reclaim" | "shutdown") => {
    setBusy(kind)
    const before = w.vmMemory ?? 0
    try {
      const r = kind === "reclaim" ? await pressureApi.wslReclaim() : await pressureApi.wslShutdown()
      const gave = Math.max(0, before - (r.vmMemory ?? 0))
      toast({
        kind: "ok",
        title: kind === "reclaim" ? "WSL page cache dropped" : "WSL shut down",
        detail: kind === "reclaim" ? "Windows takes the freed pages back over the next minute." : gave ? `Gave back ${bytes(gave)}.` : undefined,
      })
      void wsl.refresh()
    } catch (e) {
      toast({ kind: "error", title: kind === "reclaim" ? "Could not reclaim WSL memory" : "Could not shut down WSL", detail: String(e) })
    } finally {
      setBusy(null)
      setConfirmStop(false)
    }
  }

  const diff = wslconfigDiff(w.config)
  const changes = diff.filter((d) => d.kind === "add").length
  const snippet = "[experimental]\nautoMemoryReclaim=dropCache\nsparseVhd=true\n"

  return (
    <>
      <Card className="flex flex-col gap-3 p-4" data-testid="wsl-card">
        <div className="flex items-baseline gap-2">
          <h2 className="m-0 text-[13px] font-semibold">WSL virtual machine</h2>
          <span className="min-w-0 flex-1 truncate text-[12px] text-subtle">{running.length ? running.join(", ") : "not running"}</span>
          <span className="text-[17px] font-semibold tabular-nums">{w.vmMemory ? bytes(w.vmMemory) : "—"}</span>
        </div>
        {w.running && lx ? (
          <>
            <div className="flex h-3 gap-[3px]">
              <span className="rounded-[4px] bg-accent transition-[width] duration-700" style={{ width: `${(lx.used / scale) * 100}%` }} />
              <span className="rounded-[4px] bg-warn transition-[width] duration-700" style={{ width: `${(lx.cache / scale) * 100}%` }} />
              <span className="flex-1 rounded-[4px] bg-track" />
            </div>
            <div className="flex flex-wrap gap-x-3.5 gap-y-1 text-[12px] text-muted">
              <span className="flex items-center gap-1.5">
                <span className="h-2 w-2 rounded-full bg-accent" />
                Linux uses <span className="tabular-nums text-text">{bytes(lx.used)}</span>
              </span>
              <span className="flex items-center gap-1.5">
                <span className="h-2 w-2 rounded-full bg-warn" />
                Page cache <span className="tabular-nums text-text">{bytes(lx.cache)}</span>
              </span>
              <span className="ml-auto tabular-nums">cap {w.config.memory ?? bytes(lx.total, 0)}</span>
            </div>
            <div className="flex gap-2">
              <Button className="flex-1" loading={busy === "reclaim"} disabled={!!busy} onClick={() => act("reclaim")} data-testid="wsl-reclaim">
                Reclaim {bytes(lx.cache)} of cache
              </Button>
              <Button variant="quiet" icon={<Power size={13} />} disabled={!!busy} onClick={() => setConfirmStop(true)}>
                Shut down
              </Button>
            </div>
          </>
        ) : (
          <p className="m-0 text-[12.5px] text-muted">WSL isn't running, so it holds no memory now.</p>
        )}
      </Card>

      <Card className="flex flex-col gap-2 p-4" data-testid="wslconfig-card">
        <div className="flex items-center">
          <h2 className="m-0 text-[13px] font-semibold">.wslconfig</h2>
          <span className="flex-1" />
          <span className={cn("text-[12px]", changes ? "text-warn" : "text-ok")}>{changes ? `${changes} change${changes === 1 ? "" : "s"} suggested` : "looks good"}</span>
        </div>
        <div className="mono overflow-hidden rounded-[9px] border border-border text-[11.5px] leading-[1.7]">
          {diff.map((d) => (
            <div
              key={d.kind + d.text}
              className={cn("px-2.5", d.kind === "same" && "text-muted", d.kind === "add" && "bg-ok-soft text-ok", d.kind === "del" && "bg-danger-soft text-danger")}
            >
              {d.kind === "add" ? "+ " : d.kind === "del" ? "- " : "  "}
              {d.text}
            </div>
          ))}
        </div>
        <span className="text-[12px] leading-[1.45] text-muted">
          dropCache hands idle page cache back to Windows sooner; sparseVhd lets the disk image shrink. Applies after <span className="mono">wsl --shutdown</span>.
        </span>
        <div className="flex gap-2">
          <Button
            size="sm"
            icon={copied ? <Check size={12} /> : <Copy size={12} />}
            onClick={() => {
              void navigator.clipboard.writeText(snippet).then(() => {
                setCopied(true)
                setTimeout(() => setCopied(false), 1500)
              })
            }}
          >
            {copied ? "Copied" : "Copy snippet"}
          </Button>
          <Button size="sm" variant="quiet" onClick={() => go("tune")}>
            Apply on Responsiveness <ArrowRight size={12} />
          </Button>
        </div>
      </Card>

      <Dialog
        open={confirmStop}
        onClose={() => !busy && setConfirmStop(false)}
        title="Shut down WSL?"
        actions={
          <>
            <Button onClick={() => setConfirmStop(false)} disabled={!!busy}>
              Cancel
            </Button>
            <Button variant="danger" icon={<Power size={14} />} loading={busy === "shutdown"} onClick={() => act("shutdown")}>
              Shut down WSL
            </Button>
          </>
        }
      >
        <p className="m-0">
          Runs <span className="mono">wsl --shutdown</span>: every distro stops, and so do Docker containers that run in WSL. Unsaved work in Linux terminals is lost. The VM gives all{" "}
          {w.vmMemory ? bytes(w.vmMemory) : "its memory"} back, and starts again the next time you open a distro or Docker.
        </p>
      </Dialog>
    </>
  )
}

function ContainersCard() {
  const { toast } = useStore()
  const c = usePoll(pressureApi.containers, 20_000)
  const [stopping, setStopping] = useState<string | null>(null)
  if (!c.data) return <Skeleton className="h-32 rounded-xl" />
  if (!c.data.available) return null

  const stop = async (id: string, name: string) => {
    setStopping(id)
    try {
      await pressureApi.stopContainers([id])
      toast({ kind: "ok", title: `Stopped ${name}` })
      void c.refresh()
    } catch (e) {
      toast({ kind: "error", title: `Could not stop ${name}`, detail: String(e) })
    } finally {
      setStopping(null)
    }
  }

  return (
    <Card className="flex flex-col gap-0.5 px-2.5 pb-2.5 pt-3.5" data-testid="containers-card">
      <div className="flex px-2 pb-1.5">
        <h2 className="m-0 text-[13px] font-semibold">Containers</h2>
        <span className="flex-1" />
        <span className="text-[12px] text-subtle">{c.data.running ? (c.data.items.length ? "by memory" : "") : ""}</span>
      </div>
      {!c.data.running ? (
        <p className="m-0 px-2 pb-1 text-[12.5px] text-muted">Docker isn't running.</p>
      ) : c.data.items.length === 0 ? (
        <p className="m-0 px-2 pb-1 text-[12.5px] text-muted">No containers are running.</p>
      ) : (
        c.data.items.map((x) => (
          <div key={x.id} className="grid grid-cols-[minmax(0,1fr)_64px_58px] items-center gap-2.5 rounded-[9px] px-2 py-[7px] hover:bg-hover">
            <div className="min-w-0">
              <div className="flex items-center gap-1.5 truncate font-medium">
                <Box size={13} className="shrink-0 text-subtle" aria-hidden />
                <span className="truncate">{x.name}</span>
              </div>
              <div className="truncate text-[11.5px] text-subtle" title={x.image}>
                {x.image} · {x.status}
              </div>
            </div>
            <span className="text-right font-medium tabular-nums">{x.memory != null ? bytes(x.memory) : "—"}</span>
            <Button size="sm" loading={stopping === x.id} disabled={!!stopping} onClick={() => stop(x.id, x.name)}>
              Stop
            </Button>
          </div>
        ))
      )}
    </Card>
  )
}
