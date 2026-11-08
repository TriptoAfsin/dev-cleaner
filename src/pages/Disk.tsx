import { SnapshotButton } from "@/components/Share"
import { useMemo, useState } from "react"
import { AppWindow, Check, Loader2, RefreshCw, Trash2 } from "lucide-react"
import type { CacheItem, CleanEvent } from "@/lib/api"
import { bytes, cn } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { useStore } from "@/lib/store"
import { useDrives } from "@/lib/settings"
import { AnimatedNumber, Button, Card, Checkbox, Dialog, PageHeader, Skeleton } from "@/components/ui"
import { CacheRow } from "@/components/shared"

/** Selected by default: safe, measured, no admin prompt, never your data. */
export const preselect = (c: CacheItem) => c.found && c.size > 0 && c.risk === "safe" && !c.admin

export function Disk({ junk = false }: { junk?: boolean }) {
  const store = useStore()
  const drives = useDrives()
  const { scanning, scan, scanTotal, cleaning } = store
  const caches = store.caches.filter((c) => !!c.junk === junk)
  const [filter, setFilter] = useState("All")
  const [sel, setSel] = useState<Set<string> | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const [confirm, setConfirm] = useState(false)

  const found = caches.filter((c) => c.found && (c.size > 0 || !c.measured))
  const ecos = useMemo(() => ["All", ...Array.from(new Set(found.map((c) => c.eco)))], [found])
  const shown = caches.filter((c) => filter === "All" || c.eco === filter)
  const selected = sel ?? new Set(found.filter(preselect).map((c) => c.id))
  const selItems = caches.filter((c) => selected.has(c.id) && c.found)
  const selBytes = selItems.reduce((n, c) => n + c.size, 0)
  const total = found.reduce((n, c) => n + c.size, 0)
  const max = Math.max(1, ...caches.map((c) => c.size))
  const allShownSelected = shown.filter((c) => c.found).every((c) => selected.has(c.id))

  const toggle = (id: string, v: boolean) => {
    const s = new Set(selected)
    if (v) s.add(id)
    else s.delete(id)
    setSel(s)
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title={junk ? "System junk" : "Disk cleanup"}
        lede={
          <>
            {junk ? `Temp files, update leftovers, error reports, logs, browser caches and the Recycle Bin, plus unfinished downloads and Steam shader caches on ${drives.selected.length === drives.all.length ? "every drive" : `${drives.selected.length} drives`}:` : "Package and tool caches on this machine:"} <AnimatedNumber value={total} format={(n) => bytes(n)} /> in {found.length} places. Click a row to see what it is and what will run.
          </>
        }
        actions={
          <>
            <SnapshotButton name={junk ? "junk" : "disk"} />
            <Button variant="primary" icon={<RefreshCw size={15} className={scanning ? "spin" : undefined} />} disabled={scanning} onClick={() => void scan()}>
              {scanning ? `Scanning ${store.caches.length}/${scanTotal || "…"}` : "Scan"}
            </Button>
          </>
        }
      />

      <div className="flex flex-wrap items-center gap-2">
        {ecos.map((e) => (
          <button
            key={e}
            type="button"
            onClick={() => setFilter(e)}
            aria-pressed={filter === e}
            className={cn(
              "flex h-7 items-center gap-1.5 rounded-full border px-3 text-[12px] font-medium transition-colors duration-150",
              filter === e ? "border-text bg-text text-bg" : "border-border-strong bg-surface text-muted hover:text-text",
            )}
          >
            {e !== "All" && <BrandIcon name={e} size={12} mono={filter === e} />}
            {e}
          </button>
        ))}
      </div>

      <Card className="flex min-h-[300px] flex-1 flex-col overflow-hidden">
        <div className="shrink-0 flex items-center gap-3 border-b border-border px-4 py-2.5 text-[11px] uppercase tracking-[0.06em] text-subtle">
          <Checkbox
            label="Select all shown"
            checked={allShownSelected && shown.length > 0}
            onChange={(v) => {
              const s = new Set(selected)
              shown.filter((c) => c.found).forEach((c) => (v ? s.add(c.id) : s.delete(c.id)))
              setSel(s)
            }}
          />
          <span>Cache</span>
          <span className="ml-auto">Size</span>
        </div>
        <ul className="m-0 min-h-0 flex-1 overflow-y-auto list-none p-0" data-testid="cache-list">
          {shown.map((c) => (
            <CacheRow
              key={c.id}
              c={c}
              max={max}
              selected={selected.has(c.id)}
              onToggle={(v) => toggle(c.id, v)}
              expanded={open === c.id}
              onExpand={() => setOpen(open === c.id ? null : c.id)}
              status={cleaning[c.id]}
            />
          ))}
          {scanning &&
            Array.from({ length: Math.max(0, Math.min(8, (scanTotal || 8) - store.caches.length)) }).map((_, i) => (
              <li key={`s${i}`} className="border-t border-border px-4 py-3.5">
                <Skeleton className="h-4" />
              </li>
            ))}
        </ul>
        <div className="shrink-0 flex items-center justify-between border-t border-border bg-surface-2 px-4 py-2.5">
          <span className="mono text-[12.5px]">
            {selItems.length} selected · <AnimatedNumber value={selBytes} format={(n) => bytes(n)} />
          </span>
          <Button variant="primary" size="sm" className="h-[30px] px-3" icon={<Trash2 size={14} />} disabled={!selItems.length || scanning} onClick={() => setConfirm(true)} data-testid="clean-selected">
            Clean selected
          </Button>
        </div>
      </Card>
      <CleanDialog open={confirm} onClose={() => setConfirm(false)} items={selItems} />
    </div>
  )
}

/** Confirmation that lists exactly what will be cleaned and how; shows
    live progress while it runs, then what was freed and what was left
    behind, and which programs hold the files that couldn't be removed. */
export function CleanDialog({ open, onClose, items }: { open: boolean; onClose: () => void; items: CacheItem[] }) {
  const { clean, cleaning } = useStore()
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<CleanEvent[] | null>(null)
  const total = items.reduce((n, c) => n + c.size, 0)
  const close = () => {
    if (busy) return
    setResult(null)
    onClose()
  }
  const run = async () => {
    setBusy(true)
    setResult(null)
    const r = await clean(items.map((c) => c.id))
    setBusy(false)
    setResult(r)
  }

  if (result) {
    const freed = result.reduce((n, r) => n + r.freed, 0)
    const inUse = result.reduce((n, r) => n + (r.inUse ?? 0), 0)
    const kept = result.reduce((n, r) => n + (r.kept ?? 0), 0)
    const failed = result.filter((r) => r.phase === "failed")
    const holders = new Map<string, { name: string; pids: Set<number> }>()
    for (const r of result)
      for (const h of r.heldBy ?? []) {
        const k = h.name.toLowerCase()
        const e = holders.get(k) ?? { name: h.name, pids: new Set<number>() }
        e.pids.add(h.pid)
        holders.set(k, e)
      }
    const samples = result.flatMap((r) => r.inUseSample ?? []).slice(0, 6)
    return (
      <Dialog
        open={open}
        onClose={close}
        title={`Freed ${bytes(freed)}`}
        actions={
          <>
            {inUse > 0 && (
              <Button icon={<RefreshCw size={14} />} onClick={run} data-testid="clean-again">
                Clean again
              </Button>
            )}
            <Button variant="primary" onClick={close} data-testid="clean-done">
              Done
            </Button>
          </>
        }
      >
        <ul className="m-0 mb-3 flex list-none flex-col gap-1.5 p-0" data-testid="clean-result">
          {result.map((r) => {
            const c = items.find((x) => x.id === r.id)
            return (
              <li key={r.id} className="flex items-center gap-2.5 text-text">
                <BrandIcon name={r.id} size={14} />
                <span className="flex-1">{c?.name ?? r.id}</span>
                {r.phase === "failed" ? (
                  <span className="text-[11.5px] text-danger">{r.note}</span>
                ) : (
                  <>
                    {(r.inUse ?? 0) > 0 && <span className="text-[11.5px] text-warn">{r.inUse} in use</span>}
                    <span className="mono w-16 text-right">{bytes(r.freed)}</span>
                  </>
                )}
              </li>
            )
          })}
        </ul>
        {inUse > 0 && (
          <div className="mb-2 flex flex-col gap-2 rounded-lg bg-warn-soft px-3 py-2.5 text-[12px] leading-[1.5] text-warn" data-testid="in-use">
            <div>
              {inUse.toLocaleString()} item{inUse === 1 ? " is" : "s are"} open in other programs and {inUse === 1 ? "was" : "were"} left alone.
              {holders.size > 0 ? " Held by:" : " Windows didn't say which program holds them; closing apps you aren't using and cleaning again usually frees them."}
            </div>
            {holders.size > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {[...holders.values()].slice(0, 12).map((h) => (
                  <span key={h.name} className="inline-flex h-6 items-center gap-1.5 rounded-md bg-surface px-2 text-text">
                    <AppWindow size={12} className="text-subtle" />
                    {h.name}
                    {h.pids.size > 1 && <span className="text-subtle">×{h.pids.size}</span>}
                  </span>
                ))}
              </div>
            )}
            {samples.length > 0 && (
              <details className="text-muted">
                <summary className="cursor-pointer text-warn">Show files</summary>
                <ul className="mono m-0 mt-1 list-none break-all p-0 text-[11px]">
                  {samples.map((p) => (
                    <li key={p}>{p}</li>
                  ))}
                </ul>
              </details>
            )}
            <div className="flex items-center gap-3">
              <span>Close those programs and clean again, or end them from Processes.</span>
              <button
                type="button"
                className="shrink-0 font-medium underline"
                onClick={() => {
                  close()
                  window.dispatchEvent(new CustomEvent("dc-go", { detail: "processes" }))
                }}
              >
                Open Processes
              </button>
            </div>
          </div>
        )}
        {kept > 0 && <p className="m-0 mb-2">{kept.toLocaleString()} newer item{kept === 1 ? " was" : "s were"} kept on purpose; installers and running apps may still need them.</p>}
        {failed.length > 0 && <p className="m-0 text-danger">{failed.map((f) => items.find((x) => x.id === f.id)?.name ?? f.id).join(", ")}: {failed[0].note}</p>}
      </Dialog>
    )
  }

  return (
    <Dialog
      open={open}
      onClose={close}
      title={busy ? `Cleaning ${items.length} item${items.length === 1 ? "" : "s"}…` : `Clean ${items.length} cache${items.length === 1 ? "" : "s"} · ${bytes(total)}?`}
      actions={
        <>
          <Button onClick={close} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" icon={<Trash2 size={14} />} loading={busy} data-testid="confirm-clean" onClick={run}>
            {busy ? "Cleaning" : "Clean"}
          </Button>
        </>
      }
    >
      <ul className="m-0 mb-3 flex list-none flex-col gap-1.5 p-0">
        {items.map((c) => {
          const st = busy ? cleaning[c.id] : undefined
          return (
            <li key={c.id} className="flex items-center gap-2.5 text-text">
              <BrandIcon name={c.id} size={14} />
              <span className="flex-1">{c.name}</span>
              {busy ? (
                <span className="flex items-center gap-1.5 text-[11.5px] text-subtle">
                  {st?.phase === "done" ? <Check size={13} className="text-ok" /> : st?.phase === "running" ? <Loader2 size={13} className="spin text-accent" /> : <span className="h-1.5 w-1.5 rounded-full bg-border-strong" />}
                  {st?.phase === "done" ? bytes(st.freed) : st?.phase === "running" ? "cleaning" : "waiting"}
                </span>
              ) : (
                <>
                  <span className="mono max-w-[200px] truncate text-[11.5px] text-subtle">{c.method}</span>
                  <span className="mono w-16 text-right">{bytes(c.size)}</span>
                </>
              )}
            </li>
          )
        })}
      </ul>
      {items.some((c) => c.risk === "data") && (
        <div className="mb-2 rounded-lg bg-danger-soft px-3 py-2.5 text-[12px] leading-[1.5] text-danger">
          Includes your own files ({items.filter((c) => c.risk === "data").map((c) => c.name).join(", ")}). Deleting them can't be undone.
        </div>
      )}
      {items.some((c) => c.admin) && (
        <div className="mb-2 rounded-lg bg-warn-soft px-3 py-2.5 text-[12px] leading-[1.5] text-warn">
          {items.filter((c) => c.admin).length} item{items.filter((c) => c.admin).length === 1 ? " needs" : "s need"} admin rights. You'll be asked to approve once, for all of them.
        </div>
      )}
      <p className="m-0">Each tool's own clean command runs first; if it is missing, the folder's contents are deleted. Files open in other programs are left alone, and you'll see which programs hold them.</p>
    </Dialog>
  )
}
