// Background services: what runs, how each starts, and changing that. Dev
// servers that start with the machine (databases, Docker, web servers) are
// the usual reason to be here: Manual keeps them installed but idle until
// you start them.
import { useEffect, useMemo, useState } from "react"
import { Lock, Play, RefreshCw, RotateCw, Square } from "lucide-react"
import { api, type Service, type ServiceAction } from "@/lib/api"
import { bytes, cn } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { Badge, Button, Card, Dialog, PageHeader, Skeleton, SearchField } from "@/components/ui"
import { AdminBanner } from "@/components/Privileges"
import { SnapshotButton } from "@/components/Share"

type Filter = "all" | "running" | "auto" | "dev" | "optional"

const START: Record<Service["startType"], string> = { auto: "Automatic", delayed: "Automatic (delayed)", manual: "Manual", disabled: "Disabled", unknown: "—" }
const STATE: Record<Service["state"], string> = { running: "Running", stopped: "Stopped", starting: "Starting", stopping: "Stopping", paused: "Paused", unknown: "—" }
const VERDICT: Record<Service["verdict"], { tone: "ok" | "muted" | "accent" | "warn"; label: string } | null> = {
  dev: { tone: "accent", label: "Dev server" },
  optional: { tone: "ok", label: "Optional" },
  protected: { tone: "muted", label: "Protected" },
  unknown: null,
}
const autoStarts = (s: Service) => s.startType === "auto" || s.startType === "delayed"

function StateDot({ s }: { s: Service }) {
  return (
    <span className="flex items-center gap-1.5">
      <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", s.state === "running" ? "bg-ok" : s.state === "stopped" ? "bg-subtle/60" : "bg-warn")} />
      {STATE[s.state]}
    </span>
  )
}

export function Services() {
  const { toast } = useStore()
  const list = usePoll(api.listServices, 15000)
  const [q, setQ] = useState("")
  const [filter, setFilter] = useState<Filter>("all")
  const [selName, setSelName] = useState<string | null>(null)
  const [busy, setBusy] = useState<ServiceAction | null>(null)
  const [confirm, setConfirm] = useState<Service | null>(null)
  const [local, setLocal] = useState<Record<string, Service>>({})
  useEffect(() => setLocal({}), [list.data])

  const items = useMemo(() => (list.data ?? []).map((s) => local[s.name] ?? s), [list.data, local])
  const rows = useMemo(() => {
    const t = q.trim().toLowerCase()
    return items.filter(
      (s) =>
        (filter === "all" ||
          (filter === "running" && s.state === "running") ||
          (filter === "auto" && autoStarts(s)) ||
          (filter === "dev" && s.verdict === "dev") ||
          (filter === "optional" && s.verdict === "optional")) &&
        (!t || [s.displayName, s.name, s.description].some((x) => x.toLowerCase().includes(t))),
    )
  }, [items, q, filter])
  const sel = items.find((s) => s.name === selName) ?? null
  const running = items.filter((s) => s.state === "running").length
  const devAuto = items.filter((s) => s.verdict === "dev" && autoStarts(s))
  const devMem = devAuto.reduce((n, s) => n + (s.memory ?? 0), 0)

  const act = async (s: Service, action: ServiceAction) => {
    setBusy(action)
    try {
      const next = await api.serviceAction(s.name, action)
      setLocal((x) => ({ ...x, [s.name]: next }))
      const done = { start: "started", stop: "stopped", restart: "restarted", auto: "starts automatically", manual: "set to Manual", disabled: "disabled and stopped" }[action]
      toast({ kind: "ok", title: `${s.displayName} ${done}` })
    } catch (e) {
      toast({ kind: "error", title: `Couldn't change ${s.displayName}`, detail: String(e) })
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4" data-testid="services-page">
      <PageHeader
        title="Services"
        lede={
          list.data
            ? `${items.length} services, ${running} running.${devAuto.length ? ` ${devAuto.length} dev server${devAuto.length === 1 ? "" : "s"} start${devAuto.length === 1 ? "s" : ""} automatically${devMem ? `, using ${bytes(devMem)}` : ""}: Manual keeps ${devAuto.length === 1 ? "it" : "them"} idle until you need ${devAuto.length === 1 ? "it" : "them"}.` : ""}`
            : "Reading services…"
        }
        actions={
          <>
            <SnapshotButton name="services" />
            <Button icon={<RefreshCw size={14} />} onClick={() => void list.refresh()}>
              Refresh
            </Button>
          </>
        }
      />
      {items.some((s) => s.needsAdmin && s.canChange) && <AdminBanner what="Service changes" />}
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_340px] grid-rows-[minmax(0,1fr)] gap-3.5">
        <Card className="flex min-h-0 min-w-0 flex-col overflow-hidden">
          <div className="flex flex-wrap items-center gap-2 px-4 py-3">
            <SearchField value={q} onChange={setQ} placeholder="Filter by name or description" label="Filter services" testId="service-filter" className="min-w-[180px]" />
            {(["all", "running", "auto", "dev", "optional"] as const).map((f) => (
              <button
                key={f}
                type="button"
                aria-pressed={filter === f}
                onClick={() => setFilter(f)}
                className={cn(
                  "h-7 rounded-full border px-2.5 text-[12px] font-medium transition-colors",
                  filter === f ? "border-text bg-text text-bg" : "border-border-strong bg-surface text-muted hover:text-text",
                )}
              >
                {{ all: "All", running: "Running", auto: "Auto-start", dev: "Dev servers", optional: "Optional" }[f]}
              </button>
            ))}
          </div>
          <div className="grid shrink-0 grid-cols-[minmax(0,1.6fr)_88px_minmax(0,1fr)_72px_104px] gap-3 border-y border-border px-4 py-2 text-[11px] uppercase tracking-[0.06em] text-subtle">
            <span>Service</span>
            <span>Status</span>
            <span>Starts</span>
            <span className="text-right">Memory</span>
            <span />
          </div>
          <ul className="m-0 min-h-0 flex-1 list-none overflow-y-auto p-0" data-testid="service-list">
            {!list.data &&
              Array.from({ length: 8 }).map((_, i) => (
                <li key={i} className="border-b border-border px-4 py-3.5">
                  <Skeleton className="h-4" />
                </li>
              ))}
            {list.data && rows.length === 0 && <li className="px-4 py-10 text-center text-[12.5px] text-muted">No service matches.</li>}
            {rows.map((s) => (
              <li key={s.name}>
                <button
                  type="button"
                  data-testid={`service-${s.name}`}
                  aria-pressed={sel?.name === s.name}
                  onClick={() => setSelName(s.name)}
                  className={cn(
                    "grid min-h-[48px] w-full grid-cols-[minmax(0,1.6fr)_88px_minmax(0,1fr)_72px_104px] items-center gap-3 border-b border-border px-4 py-1.5 text-left text-[13px] hover:bg-surface-2",
                    sel?.name === s.name && "bg-accent-soft/50 hover:bg-accent-soft/60",
                  )}
                >
                  <span className="min-w-0">
                    <span className="block truncate text-text">{s.displayName}</span>
                    <span className="mono block truncate text-[11px] text-subtle">{s.name}</span>
                  </span>
                  <span className="text-[12px] text-muted">
                    <StateDot s={s} />
                  </span>
                  <span className={cn("truncate text-[12px]", s.verdict === "dev" && autoStarts(s) ? "text-warn" : "text-muted")}>{START[s.startType]}</span>
                  <span className="mono text-right text-[12px] text-muted">{s.memory ? bytes(s.memory) : ""}</span>
                  <span className="flex justify-end">
                    {s.verdict === "protected" ? (
                      <span className="flex items-center gap-1 text-[11.5px] text-subtle">
                        <Lock size={11} /> protected
                      </span>
                    ) : (
                      VERDICT[s.verdict] && <Badge tone={VERDICT[s.verdict]!.tone}>{VERDICT[s.verdict]!.label}</Badge>
                    )}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Card>
        <Card className="flex min-h-0 flex-col overflow-y-auto" data-testid="service-inspector">
          {sel ? (
            <Inspector s={sel} busy={busy} act={act} askDisable={() => setConfirm(sel)} />
          ) : (
            <div className="m-auto max-w-[240px] px-4 py-10 text-center text-[12.5px] text-muted">Select a service to see what it is, start or stop it, and choose how it starts.</div>
          )}
        </Card>
      </div>
      <Dialog
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title={confirm ? `Disable ${confirm.displayName}?` : ""}
        actions={
          <>
            <Button onClick={() => setConfirm(null)}>Cancel</Button>
            <Button
              variant="danger"
              data-testid="confirm-disable"
              onClick={() => {
                const s = confirm
                setConfirm(null)
                if (s) void act(s, "disabled")
              }}
            >
              Disable and stop
            </Button>
          </>
        }
      >
        <p className="m-0 mb-2">It stops now and won't start again, even when an app asks for it, until you set it back to Manual or Automatic here.</p>
        <p className="m-0">For a dev server you use now and then, Manual is usually better: it stays off until you start it.</p>
      </Dialog>
    </div>
  )
}

function Field({ label, children, mono }: { label: string; children: React.ReactNode; mono?: boolean }) {
  return (
    <div className="grid grid-cols-[76px_minmax(0,1fr)] gap-2 py-1">
      <dt className="text-[12px] text-subtle">{label}</dt>
      <dd className={cn("m-0 min-w-0 break-words text-[12.5px] text-text", mono && "mono text-[11.5px]")}>{children}</dd>
    </div>
  )
}

function Inspector({ s, busy, act, askDisable }: { s: Service; busy: ServiceAction | null; act: (s: Service, a: ServiceAction) => Promise<void>; askDisable: () => void }) {
  const v = VERDICT[s.verdict]
  const locked = !s.canChange
  const current = s.startType === "delayed" ? "auto" : s.startType
  return (
    <div className="flex flex-col gap-3.5 p-4">
      <div>
        <div className="text-[14px] font-semibold leading-snug text-text">{s.displayName}</div>
        <div className="mono mt-0.5 text-[11.5px] text-subtle">{s.name}</div>
        {(v || s.reason) && (
          <div className="mt-2 flex flex-wrap items-center gap-2">
            {v && <Badge tone={v.tone}>{v.label}</Badge>}
            {s.reason && <span className="text-[12px] text-muted">{s.reason}</span>}
          </div>
        )}
      </div>
      {s.description && <p className="m-0 text-[12.5px] leading-[1.5] text-muted">{s.description}</p>}
      <dl className="m-0 border-y border-border py-1.5">
        <Field label="Status">
          <StateDot s={s} />
        </Field>
        <Field label="Starts">{START[s.startType]}</Field>
        {s.pid ? <Field label="PID">{s.pid}</Field> : null}
        {s.memory ? <Field label="Memory">{bytes(s.memory)}</Field> : null}
        {s.account && <Field label="Account">{s.account}</Field>}
        {s.path && (
          <Field label="Path" mono>
            {s.path}
          </Field>
        )}
      </dl>
      {locked ? (
        <div className="flex items-start gap-2 rounded-lg bg-surface-2 px-3 py-2.5 text-[12px] text-muted" data-testid="service-locked">
          <Lock size={13} className="mt-0.5 shrink-0" />
          The system needs this service, so Dev Cleaner won't change it.
        </div>
      ) : (
        <>
          <div className="flex flex-wrap gap-1.5">
            {s.state === "running" ? (
              <>
                <Button size="sm" icon={<Square size={11} />} loading={busy === "stop"} disabled={!!busy} onClick={() => void act(s, "stop")} data-testid="service-stop">
                  Stop
                </Button>
                <Button size="sm" icon={<RotateCw size={12} />} loading={busy === "restart"} disabled={!!busy} onClick={() => void act(s, "restart")}>
                  Restart
                </Button>
              </>
            ) : (
              <Button size="sm" icon={<Play size={11} />} loading={busy === "start"} disabled={!!busy || s.startType === "disabled"} onClick={() => void act(s, "start")} data-testid="service-start">
                Start
              </Button>
            )}
          </div>
          <div>
            <div className="mb-1.5 text-[12px] text-subtle">How it starts</div>
            <div className="flex rounded-lg border border-border-strong p-0.5" role="radiogroup" aria-label="Startup type">
              {(["auto", "manual", "disabled"] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  role="radio"
                  aria-checked={current === t}
                  disabled={!!busy}
                  data-testid={`start-${t}`}
                  onClick={() => (current === t ? undefined : t === "disabled" ? askDisable() : void act(s, t))}
                  className={cn(
                    "flex h-7 flex-1 items-center justify-center rounded-md text-[12px] transition-colors disabled:opacity-60",
                    current === t ? (t === "disabled" ? "bg-danger-soft font-medium text-danger" : "bg-accent-soft font-medium text-accent") : "text-muted hover:text-text",
                  )}
                >
                  {busy === t ? "…" : { auto: "Automatic", manual: "Manual", disabled: "Disabled" }[t]}
                </button>
              ))}
            </div>
            <p className="m-0 mt-1.5 text-[11.5px] leading-[1.45] text-subtle">
              {current === "auto" && "Starts with the system."}
              {current === "manual" && "Starts only when you or an app asks for it."}
              {current === "disabled" && "Can't start until you change this."}
              {s.verdict === "dev" && current === "auto" && " A dev server: Manual keeps it idle until you need it."}
            </p>
          </div>
        </>
      )}
    </div>
  )
}
