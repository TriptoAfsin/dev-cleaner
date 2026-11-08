import { useCallback, useState, type ReactNode } from "react"
import { AnimatePresence, motion } from "motion/react"
import { AppWindow, ChevronDown, X } from "lucide-react"
import { api, type CacheItem, type ProcGroup } from "@/lib/api"
import { bytes, cn } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { useSettings } from "@/lib/settings"
import { useStore } from "@/lib/store"
import { Badge, Bar, Button, Checkbox, Dialog, InfoTip, Skeleton, VerdictPill } from "./ui"

/** Ending processes: confirm (unless turned off for safe apps), end, report. */
export function useKill(after?: () => void) {
  const { toast, setActivity } = useStore()
  const { settings } = useSettings()
  const [pending, setPending] = useState<{ title: string; pids: number[]; group?: ProcGroup; detail?: ReactNode } | null>(null)
  const [busy, setBusy] = useState(false)

  const run = useCallback(
    async (title: string, pids: number[]) => {
      setBusy(true)
      setActivity({ kind: "busy", text: `Ending ${title}` })
      try {
        const r = await api.killProcesses(pids)
        if (r.ended > 0) {
          setActivity({ kind: "ok", text: `Ended ${title} · freed ${bytes(r.freed)}` })
          toast({ kind: "ok", title: `Ended ${title}`, detail: `Freed ${bytes(r.freed)}${r.failed ? ` · ${r.failed} could not be ended` : ""}.` })
        } else {
          setActivity({ kind: "error", text: `Could not end ${title}` })
          toast({ kind: "error", title: `Could not end ${title}`, detail: "It may be protected or need admin rights." })
        }
      } catch (e) {
        toast({ kind: "error", title: "End task failed", detail: String(e) })
      } finally {
        setBusy(false)
        setPending(null)
        after?.()
      }
    },
    [after, setActivity, toast],
  )

  const request = useCallback(
    (title: string, pids: number[], group?: ProcGroup, detail?: ReactNode) => {
      if (group?.protected) return
      if (!settings.confirmKills && group?.verdict === "safe") return void run(title, pids)
      setPending({ title, pids, group, detail })
    },
    [run, settings.confirmKills],
  )

  const dialog = (
    <Dialog
      open={!!pending}
      onClose={() => !busy && setPending(null)}
      title={`End ${pending?.title ?? ""}?`}
      actions={
        <>
          <Button onClick={() => setPending(null)} disabled={busy}>
            Cancel
          </Button>
          <Button variant="danger" icon={<X size={14} />} loading={busy} onClick={() => pending && run(pending.title, pending.pids)} data-testid="confirm-kill">
            End {pending && pending.pids.length > 1 ? `${pending.pids.length} processes` : "process"}
          </Button>
        </>
      }
    >
      {pending?.group && (
        <div className="mb-3 flex items-center gap-2">
          <VerdictPill verdict={pending.group.verdict} />
          <span>{pending.group.reason}</span>
        </div>
      )}
      {pending?.detail}
      <p className="m-0">Anything unsaved in it is lost. You can start it again afterwards.</p>
    </Dialog>
  )

  return { request, dialog }
}

/** Top CPU users as live bars, each with its own End button. */
export function TopCpu({ procs, limit = 5, onKill }: { procs: ProcGroup[] | null; limit?: number; onKill: (g: ProcGroup) => void }) {
  if (!procs) {
    return (
      <div className="flex flex-col gap-3 px-4 pb-4 pt-1">
        {Array.from({ length: limit }).map((_, i) => (
          <Skeleton key={i} className="h-7" />
        ))}
      </div>
    )
  }
  const top = [...procs].sort((a, b) => b.cpu - a.cpu).slice(0, limit)
  const max = Math.max(1, top[0]?.cpu ?? 1)
  return (
    <ul className="m-0 flex list-none flex-col gap-2.5 px-4 pb-4 pt-1" data-testid="top-cpu">
      {top.map((p) => (
        <motion.li key={p.name} layout transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }} className="grid grid-cols-[minmax(0,1fr)_58px] items-center gap-2.5">
          <div className="min-w-0">
            <div className="mb-1 flex items-center justify-between gap-2 text-[12.5px]">
              <span className="flex min-w-0 items-center gap-1.5 truncate">
                <ProcIcon name={p.name} />
                <span className="truncate">{p.name}</span>
                {p.pids.length > 1 && <span className="text-[11px] text-subtle">×{p.pids.length}</span>}
              </span>
              <span className="mono">{p.cpu.toFixed(1)}%</span>
            </div>
            <Bar value={p.cpu / max} tone={p.cpu > 25 ? "warn" : "accent"} />
          </div>
          {p.protected ? (
            <span className="text-center text-[10.5px] text-subtle">system</span>
          ) : (
            <Button size="sm" icon={<X size={11} strokeWidth={2.4} />} onClick={() => onKill(p)} aria-label={`End ${p.name}`} className="hover:border-danger hover:text-danger">
              End
            </Button>
          )}
        </motion.li>
      ))}
    </ul>
  )
}

/** Processes get one generic mark; a few dev tools get their brand. */
export function ProcIcon({ name }: { name: string }) {
  const n = name.toLowerCase()
  const brand = ["node", "bun", "deno", "python", "python3", "docker", "postgres", "redis-server", "mongod", "mysqld", "ollama", "cargo", "rustc", "go", "java"].find((b) => n === b || n.startsWith(b + " "))
  if (brand) return <BrandIcon name={brand === "redis-server" ? "redis" : brand === "mongod" ? "mongodb" : brand === "mysqld" ? "mysql" : brand === "rustc" ? "rust" : brand} size={13} />
  return <AppWindow size={13} className="shrink-0 text-subtle" aria-hidden />
}

/** One cache row: select, brand, name, bar, size, and an expandable
    explanation of what it is and what will run. */
export function CacheRow({
  c,
  max,
  selected,
  onToggle,
  expanded,
  onExpand,
  status,
}: {
  c: CacheItem
  max: number
  selected: boolean
  onToggle: (v: boolean) => void
  expanded?: boolean
  onExpand?: () => void
  status?: { phase: string; freed: number; note: string }
}) {
  const empty = !c.found || c.size === 0
  return (
    <li className={cn("border-t border-border first:border-t-0", empty && "opacity-60")} data-testid={`cache-${c.id}`}>
      <div
        className={cn("grid h-11 grid-cols-[18px_20px_minmax(0,1fr)_minmax(60px,120px)_78px_22px] items-center gap-3 px-4 transition-colors duration-150 hover:bg-surface-2", onExpand && "cursor-pointer")}
        onClick={onExpand}
      >
        <Checkbox label={`Select ${c.name}`} checked={selected} onChange={onToggle} />
        <BrandIcon name={c.id} size={16} />
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
            <span className="min-w-0 truncate">
              {c.name} <span className="font-normal text-subtle">· {c.eco}</span>
            </span>
            <span className="shrink-0" onClick={(e) => e.stopPropagation()}>
              <InfoTip text={<CacheHelp c={c} />} />
            </span>
            {c.risk === "slow" && <Badge tone="warn">slow rebuild</Badge>}
            {c.risk === "data" && <Badge tone="danger">your data</Badge>}
            {c.admin && <Badge tone="accent">admin</Badge>}
            {status?.phase === "running" && <Badge tone="accent">cleaning</Badge>}
            {status?.phase === "done" && <Badge tone="ok">freed {bytes(status.freed)}</Badge>}
          </div>
        </div>
        <Bar value={max ? c.size / max : 0} thin />
        <span className="mono text-right text-[12.5px]">{!c.found ? "not found" : c.measured ? bytes(c.size) : "—"}</span>
        {onExpand ? (
          <ChevronDown size={14} className={cn("text-subtle transition-transform duration-200", expanded && "rotate-180")} aria-hidden />
        ) : (
          <span />
        )}
      </div>
      <AnimatePresence initial={false}>
        {expanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            className="overflow-hidden"
          >
            <dl className="selectable mx-4 mb-3 ml-[54px] grid grid-cols-[64px_minmax(0,1fr)] gap-x-3 gap-y-1.5 rounded-lg border-l-2 border-accent bg-surface-2 px-3.5 py-3 text-[12.5px]">
              <dt className="text-subtle">What</dt>
              <dd className="m-0">{c.about}</dd>
              <dt className="text-subtle">Risk</dt>
              <dd className="m-0">
                {c.risk === "safe" ? "Safe. Rebuilt on demand." : c.risk === "slow" ? "Safe, but the next build, launch or install is slower or needs a download." : "Your files. Never selected for you; deleting can't be undone."}
                {c.admin ? " Needs admin approval." : ""}
                {!c.measured ? " Size is known only after it runs." : ""}
              </dd>
              <dt className="text-subtle">Runs</dt>
              <dd className="mono m-0 text-[12px]">{c.method}</dd>
              {c.paths.length > 0 && (
                <>
                  <dt className="text-subtle">Where</dt>
                  <dd className="mono m-0 break-all text-[11.5px] text-muted">{c.paths.join("  ·  ")}</dd>
                </>
              )}
              {c.files > 0 && (
                <>
                  <dt className="text-subtle">Files</dt>
                  <dd className="mono m-0">{c.files.toLocaleString()}</dd>
                </>
              )}
              {status?.note && status.phase === "done" && (
                <>
                  <dt className="text-subtle">Result</dt>
                  <dd className="m-0">{status.note}</dd>
                </>
              )}
            </dl>
          </motion.div>
        )}
      </AnimatePresence>
    </li>
  )
}

/** Tooltip body for a cleanup row: what it is, what is deleted, what runs. */
export function CacheHelp({ c }: { c: CacheItem }) {
  const risk =
    c.risk === "safe"
      ? "Safe: rebuilt on demand."
      : c.risk === "slow"
        ? "Safe, but the next build, launch or install is slower or downloads again."
        : "Your own files: never selected for you, and can't be undone."
  return (
    <span className="flex flex-col gap-1.5 text-left">
      <span className="font-semibold">{c.name}</span>
      <span>{c.about}</span>
      <span>
        <b>Deletes:</b> {c.paths.length ? c.paths.length === 1 ? c.paths[0] : `${c.paths.length} locations (expand the row to see them)` : "nothing directly; the command below does the work"}
      </span>
      <span>
        <b>Runs:</b> <span className="mono">{c.method}</span>
      </span>
      <span>
        <b>Risk:</b> {risk}
        {c.admin ? " Asks for admin approval." : ""}
      </span>
    </span>
  )
}
