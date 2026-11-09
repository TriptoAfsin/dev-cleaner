// Programs that start when you sign in, each with a switch. Turning one off
// uses the OS's own switch (Task Manager's on Windows), so nothing is
// uninstalled and it can be turned back on here or there.
import { useEffect, useMemo, useState } from "react"
import { RefreshCw, Search, ShieldCheck } from "lucide-react"
import { api, type StartupItem } from "@/lib/api"
import { cn } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { Badge, Button, Card, PageHeader, Skeleton, Switch, Tooltip } from "@/components/ui"
import { ProcIcon } from "@/components/shared"
import { AdminBanner } from "@/components/Privileges"
import { SnapshotButton } from "@/components/Share"

type Filter = "all" | "on" | "off"

const VERDICT: Record<StartupItem["verdict"], { tone: "ok" | "muted" | "accent"; label: string }> = {
  optional: { tone: "ok", label: "Fine to turn off" },
  keep: { tone: "accent", label: "Keep" },
  unknown: { tone: "muted", label: "Check first" },
}

export function Startup() {
  const { toast } = useStore()
  const list = usePoll(api.listStartup, 30000)
  const [q, setQ] = useState("")
  const [filter, setFilter] = useState<Filter>("all")
  const [busy, setBusy] = useState<string | null>(null)
  const [local, setLocal] = useState<Record<string, StartupItem>>({})

  // A fresh list replaces what the switches changed locally.
  useEffect(() => setLocal({}), [list.data])
  const items = useMemo(() => (list.data ?? []).map((i) => local[i.id] ?? i), [list.data, local])
  const rows = useMemo(() => {
    const t = q.trim().toLowerCase()
    return items.filter(
      (i) =>
        (filter === "all" || (filter === "on") === i.enabled) &&
        (!t || [i.displayName, i.name, i.publisher ?? "", i.command].some((x) => x.toLowerCase().includes(t))),
    )
  }, [items, q, filter])
  const on = items.filter((i) => i.enabled)
  const optionalOn = on.filter((i) => i.verdict === "optional").length
  const anyAdmin = items.some((i) => i.needsAdmin)

  const toggle = async (i: StartupItem, enabled: boolean) => {
    setBusy(i.id)
    try {
      const next = await api.setStartup(i.id, enabled)
      setLocal((x) => ({ ...x, [i.id]: next }))
      toast({ kind: "ok", title: `${i.displayName} ${enabled ? "starts" : "won't start"} at sign-in`, detail: enabled ? undefined : "Nothing was uninstalled. Turn it back on here any time." })
    } catch (e) {
      toast({ kind: "error", title: `Couldn't change ${i.displayName}`, detail: String(e) })
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4" data-testid="startup-page">
      <PageHeader
        title="Startup"
        lede={
          list.data
            ? `${on.length} of ${items.length} programs start when you sign in${optionalOn ? `; ${optionalOn === 1 ? "1 is" : `${optionalOn} are`} fine to turn off` : ""}. Turning one off doesn't uninstall it.`
            : "Reading startup programs…"
        }
        actions={
          <>
            <SnapshotButton name="startup" />
            <Button icon={<RefreshCw size={14} />} onClick={() => void list.refresh()}>
              Refresh
            </Button>
          </>
        }
      />
      {anyAdmin && <AdminBanner what="All-users startup entries" />}
      <Card className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 px-4 py-3">
          <label className="flex h-8 min-w-[200px] flex-1 items-center gap-2 rounded-lg border border-border-strong bg-surface px-2.5 focus-within:border-accent">
            <Search size={14} className="text-subtle" />
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Filter by name, publisher or command"
              aria-label="Filter startup programs"
              data-testid="startup-filter"
              className="h-full min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-subtle"
            />
          </label>
          {(["all", "on", "off"] as const).map((f) => (
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
              {{ all: "All", on: "On", off: "Off" }[f]}
            </button>
          ))}
        </div>
        <div className="grid shrink-0 grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.5fr)_56px] gap-3 border-y border-border px-4 py-2 text-[11px] uppercase tracking-[0.06em] text-subtle">
          <span>Program</span>
          <span>Where</span>
          <span>Verdict</span>
          <span className="text-right">Starts</span>
        </div>
        <ul className="m-0 min-h-0 flex-1 list-none overflow-y-auto p-0" data-testid="startup-list">
          {!list.data &&
            Array.from({ length: 6 }).map((_, i) => (
              <li key={i} className="border-b border-border px-4 py-3.5">
                <Skeleton className="h-4" />
              </li>
            ))}
          {list.data && rows.length === 0 && (
            <li className="px-4 py-10 text-center text-[12.5px] text-muted">{items.length ? "Nothing matches." : "No programs start at sign-in."}</li>
          )}
          {rows.map((i) => (
            <li
              key={i.id}
              data-testid={`startup-${i.name}`}
              className={cn(
                "grid min-h-[52px] grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.5fr)_56px] items-center gap-3 border-b border-border px-4 py-2 text-[13px] last:border-b-0 hover:bg-surface-2",
                !i.enabled && "text-muted",
              )}
            >
              <span className="flex min-w-0 items-center gap-2.5" title={i.command}>
                <ProcIcon name={i.displayName} />
                <span className="min-w-0">
                  <span className={cn("block truncate", i.enabled ? "text-text" : "text-muted")}>{i.displayName}</span>
                  {(i.publisher ?? (i.name !== i.displayName ? i.name : null)) && (
                    <span className="block truncate text-[11.5px] text-subtle">{i.publisher ?? i.name}</span>
                  )}
                </span>
              </span>
              <span className="truncate text-[12px] text-muted">{i.location}</span>
              <span className="flex min-w-0 items-center gap-2">
                <Badge tone={VERDICT[i.verdict].tone}>{VERDICT[i.verdict].label}</Badge>
                <span className="truncate text-[11.5px] text-subtle" title={i.reason}>
                  {i.reason}
                </span>
              </span>
              <span className="flex items-center justify-end gap-1.5">
                {i.needsAdmin && (
                  <Tooltip text="Every account: needs administrator rights">
                    <ShieldCheck size={13} className="text-subtle" aria-label="Needs administrator rights" />
                  </Tooltip>
                )}
                <span className={cn(busy === i.id && "pointer-events-none opacity-50")}>
                  <Switch checked={i.enabled} onChange={(v) => void toggle(i, v)} label={`Start ${i.displayName} at sign-in`} />
                </span>
              </span>
            </li>
          ))}
        </ul>
      </Card>
    </div>
  )
}
