import { SnapshotButton } from "@/components/Share"
import { useMemo, useState } from "react"
import { RefreshCw, Trash2 } from "lucide-react"
import type { CacheItem } from "@/lib/api"
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

/** Confirmation that lists exactly what will be cleaned and how. */
export function CleanDialog({ open, onClose, items }: { open: boolean; onClose: () => void; items: CacheItem[] }) {
  const { clean } = useStore()
  const [busy, setBusy] = useState(false)
  const total = items.reduce((n, c) => n + c.size, 0)
  return (
    <Dialog
      open={open}
      onClose={() => !busy && onClose()}
      title={`Clean ${items.length} cache${items.length === 1 ? "" : "s"} · ${bytes(total)}?`}
      actions={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon={<Trash2 size={14} />}
            loading={busy}
            data-testid="confirm-clean"
            onClick={async () => {
              setBusy(true)
              await clean(items.map((c) => c.id))
              setBusy(false)
              onClose()
            }}
          >
            {busy ? "Cleaning" : "Clean"}
          </Button>
        </>
      }
    >
      <ul className="m-0 mb-3 flex list-none flex-col gap-1.5 p-0">
        {items.map((c) => (
          <li key={c.id} className="flex items-center gap-2.5 text-text">
            <BrandIcon name={c.id} size={14} />
            <span className="flex-1">{c.name}</span>
            <span className="mono max-w-[200px] truncate text-[11.5px] text-subtle">{c.method}</span>
            <span className="mono w-16 text-right">{bytes(c.size)}</span>
          </li>
        ))}
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
      <p className="m-0">Each tool's own clean command runs first; if it is missing, the folder's contents are deleted. Files in use are skipped.</p>
    </Dialog>
  )
}
