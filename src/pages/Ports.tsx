import { useEffect, useMemo, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { AnimatePresence, motion } from "motion/react"
import { Cable, ExternalLink, RefreshCw, Search, X } from "lucide-react"
import { api, type PortEntry, type PortState } from "@/lib/api"
import { bytes, cn } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { useSettings } from "@/lib/settings"
import { usePoll, useStore } from "@/lib/store"
import { Button, Card, Empty, IconButton, PageHeader, Skeleton, Switch } from "@/components/ui"
import { ProcIcon, useKill } from "@/components/shared"
import { SnapshotButton } from "@/components/Share"

const STATE: Record<PortState, { label: string; cls: string }> = {
  inUse: { label: "In use", cls: "text-danger" },
  free: { label: "Free", cls: "text-ok" },
  system: { label: "System", cls: "text-subtle" },
}

export function Ports() {
  const { settings } = useSettings()
  const { setPortsInUse, admin } = useStore()
  const [all, setAll] = useState(false)
  const ports = usePoll(() => api.listPorts(all, settings.extraPorts), 2000, [all, settings.extraPorts.join(",")])
  const kill = useKill(ports.refresh)
  const [q, setQ] = useState("")
  const [filter, setFilter] = useState<PortState | "all">("all")
  const [picked, setPicked] = useState<number | null>(null)

  const list = ports.data ?? []
  useEffect(() => {
    if (ports.data) setPortsInUse(ports.data.filter((p) => p.state === "inUse" && p.common).map((p) => p.port))
  }, [ports.data, setPortsInUse])

  const counts = useMemo(() => {
    const c = { inUse: 0, free: 0, system: 0 }
    list.forEach((p) => c[p.state]++)
    return c
  }, [list])
  const term = q.trim().toLowerCase()
  const shown = list.filter(
    (p) =>
      (filter === "all" || p.state === filter) &&
      (!term || String(p.port).includes(term) || p.process?.toLowerCase().includes(term) || String(p.pid ?? "").includes(term) || p.hint?.toLowerCase().includes(term)),
  )
  const sel = list.find((p) => p.port === picked) ?? shown.find((p) => p.state === "inUse") ?? null

  const free = (p: PortEntry) => {
    if (!p.pid || p.state !== "inUse") return
    kill.request(`${p.process} (:${p.port})`, [p.pid], undefined, (
      <p className="m-0 mb-2">
        Frees port <span className="mono text-text">{p.port}</span> by ending <span className="text-text">{p.process}</span> (PID {p.pid}).
      </p>
    ))
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Ports"
        lede={`${counts.inUse} port${counts.inUse === 1 ? " is" : "s are"} held by processes. Free one by ending what holds it.`}
        actions={
          <>
            <label className="flex items-center gap-2 text-[12.5px] text-muted">
              <Switch checked={all} onChange={setAll} label="Show all listening ports" />
              Show all listening ports
            </label>
            <SnapshotButton name="ports" />
            <IconButton label="Refresh ports" onClick={() => void ports.refresh()}>
              <RefreshCw size={14} />
            </IconButton>
          </>
        }
      />
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex h-[34px] min-w-[240px] flex-1 items-center gap-2 rounded-lg border border-border-strong bg-surface px-3 focus-within:border-accent">
          <Search size={14} className="text-subtle" />
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search port, process or PID"
            aria-label="Search ports"
            data-testid="port-search"
            className="h-full min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-subtle"
          />
        </label>
        {(
          [
            ["all", `All ${list.length}`, ""],
            ["inUse", `In use ${counts.inUse}`, "bg-danger"],
            ["free", `Free ${counts.free}`, "bg-ok"],
            ["system", `System ${counts.system}`, "bg-subtle"],
          ] as const
        ).map(([k, label, dot]) => (
          <button
            key={k}
            type="button"
            aria-pressed={filter === k}
            onClick={() => setFilter(k)}
            className={cn(
              "flex h-7 items-center gap-1.5 rounded-full border px-3 text-[12px] font-medium transition-colors",
              filter === k ? "border-text bg-text text-bg" : "border-border-strong bg-surface text-muted hover:text-text",
            )}
          >
            {dot && <span className={cn("h-1.5 w-1.5 rounded-full", dot)} />}
            {label}
          </button>
        ))}
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_268px] gap-3.5">
        <Card className="flex min-h-0 flex-col overflow-hidden">
          <div className="grid grid-cols-[70px_86px_minmax(0,1fr)_64px_70px] gap-3 border-b border-border px-4 py-2 text-[11px] uppercase tracking-[0.06em] text-subtle">
            <span>Port</span>
            <span>State</span>
            <span>Process</span>
            <span>PID</span>
            <span />
          </div>
          <ul className="m-0 min-h-0 flex-1 list-none overflow-y-auto p-0" data-testid="port-list">
            {!ports.data &&
              Array.from({ length: 8 }).map((_, i) => (
                <li key={i} className="border-b border-border px-4 py-3">
                  <Skeleton className="h-4" />
                </li>
              ))}
            {ports.data && shown.length === 0 && (
              <Empty icon={<Cable size={26} />} title="No ports match">
                {term ? `Nothing matches “${q}”.` : "Turn on “Show all listening ports” to see everything that listens."}
              </Empty>
            )}
            <AnimatePresence initial={false}>
              {shown.map((p) => (
                <motion.li
                  key={p.port}
                  layout="position"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.18 }}
                  onClick={() => setPicked(p.port)}
                  data-testid={`port-${p.port}`}
                  className={cn(
                    "grid h-[42px] cursor-pointer grid-cols-[70px_86px_minmax(0,1fr)_64px_70px] items-center gap-3 border-b border-border px-4 text-[13px] hover:bg-surface-2",
                    sel?.port === p.port && "bg-accent-soft hover:bg-accent-soft",
                  )}
                >
                  <span className="mono font-medium">{p.port}</span>
                  <span className={cn("flex items-center gap-1.5 text-[12px] font-medium", STATE[p.state].cls)}>
                    <span className="h-1.5 w-1.5 rounded-full bg-current" />
                    {STATE[p.state].label}
                  </span>
                  <span className="flex min-w-0 items-center gap-1.5 truncate">
                    {p.process ? (
                      <>
                        <ProcIcon name={p.process} />
                        <span className="truncate">{p.process}</span>
                        {p.hint && <span className="truncate text-[12px] text-subtle">{p.hint}</span>}
                      </>
                    ) : (
                      <span className="text-subtle">—</span>
                    )}
                  </span>
                  <span className="mono text-muted">{p.pid ?? "—"}</span>
                  <span className="flex justify-end">
                    {p.state === "inUse" && (
                      <Button
                        size="sm"
                        icon={<X size={11} strokeWidth={2.4} />}
                        onClick={(e) => {
                          e.stopPropagation()
                          free(p)
                        }}
                        aria-label={`Free port ${p.port}`}
                        className="hover:border-danger hover:text-danger"
                      >
                        Free
                      </Button>
                    )}
                  </span>
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        </Card>

        <Card className="flex flex-col gap-3.5 p-4" data-testid="port-inspector">
          {sel && sel.state !== "free" ? (
            <>
              <div className="mono text-[11px] uppercase tracking-[0.06em] text-subtle">Port {sel.port}</div>
              <div>
                <div className="flex items-center gap-2 text-[16px] font-semibold">
                  {sel.hint && <BrandIcon name={sel.hint} size={16} />}
                  {sel.process}
                </div>
                {sel.hint && <div className="mt-0.5 text-[12.5px] text-muted">{sel.hint}</div>}
              </div>
              <dl className="m-0 grid grid-cols-[72px_minmax(0,1fr)] gap-x-2.5 gap-y-2 text-[12.5px]">
                <dt className="text-subtle">PID</dt>
                <dd className="mono m-0">{sel.pid}</dd>
                <dt className="text-subtle">Address</dt>
                <dd className="mono m-0 break-all">{sel.addresses.join(", ")}</dd>
                {sel.memory != null && (
                  <>
                    <dt className="text-subtle">Memory</dt>
                    <dd className="mono m-0">{bytes(sel.memory)}</dd>
                  </>
                )}
                {sel.cmd && (
                  <>
                    <dt className="text-subtle">Command</dt>
                    <dd className="mono selectable m-0 line-clamp-3 break-all text-[11.5px]">{sel.cmd}</dd>
                  </>
                )}
              </dl>
              {sel.state === "system" ? (
                <div className="rounded-lg bg-track px-3 py-2.5 text-[12px] text-muted">Held by the operating system. Dev Cleaner will not end it.</div>
              ) : (
                <div className="rounded-lg bg-warn-soft px-3 py-2.5 text-[12px] leading-[1.5] text-warn">
                  Ending it drops anything connected to this port. Dev servers start again with their usual command.
                </div>
              )}
              <div className="mt-auto flex flex-col gap-2">
                {sel.state === "inUse" && (
                  <Button variant="danger" icon={<X size={14} />} onClick={() => free(sel)} data-testid="free-port">
                    End process · free :{sel.port}
                  </Button>
                )}
                <Button icon={<ExternalLink size={14} />} onClick={() => void openUrl(`http://localhost:${sel.port}`).catch(() => {})}>
                  Open localhost:{sel.port}
                </Button>
              </div>
            </>
          ) : (
            <Empty icon={<Cable size={26} />} title="Pick a port">
              Select a port in use to see what holds it.
            </Empty>
          )}
          {!admin && <p className="m-0 text-[11.5px] text-subtle">Not running as admin: ports held by services can't be freed.</p>}
        </Card>
      </div>
      {kill.dialog}
    </div>
  )
}
