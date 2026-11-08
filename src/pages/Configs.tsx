import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react"
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog"
import { revealItemInDir } from "@tauri-apps/plugin-opener"
import { AnimatePresence, motion } from "motion/react"
import {
  Archive,
  ArchiveRestore,
  ArrowLeftRight,
  ArrowRight,
  ChevronRight,
  Copy,
  Download,
  Eye,
  FileArchive,
  FolderOpen,
  History,
  RefreshCw,
  TriangleAlert,
  Undo2,
} from "lucide-react"
import { api, on } from "@/lib/api"
import { ago, bytes, cn } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { useStore } from "@/lib/store"
import {
  backupFileName,
  brandOf,
  configsApi,
  type BackupInfo,
  type BundleInfo,
  type ConfigItem,
  type ConfigProgress,
  type ConfigTool,
  type MigrateOptions,
  type Migration,
  type PlanAction,
  type PlanStep,
} from "@/lib/configs"
import { Badge, Bar, Button, Card, Checkbox, Dialog, Empty, IconButton, InfoTip, PageHeader, Skeleton, Switch, Tooltip } from "@/components/ui"

type Tab = "backup" | "restore" | "copy"

const TABS: { id: Tab; label: string; icon: ReactNode }[] = [
  { id: "backup", label: "Back up", icon: <Archive size={14} /> },
  { id: "restore", label: "Restore", icon: <ArchiveRestore size={14} /> },
  { id: "copy", label: "Copy between tools", icon: <ArrowLeftRight size={14} /> },
]

const ROW = { initial: { opacity: 0, y: 4 }, animate: { opacity: 1, y: 0 }, transition: { duration: 0.2, ease: [0.16, 1, 0.3, 1] as const } }

export function Configs() {
  const [tab, setTab] = useState<Tab>(() => (sessionStorage.getItem("dc-configs-tab") as Tab) || "backup")
  const [reload, setReload] = useState(0)
  useEffect(() => sessionStorage.setItem("dc-configs-tab", tab), [tab])

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Configs"
        lede="Back up editor and AI tool settings to one zip, restore them on any machine, or copy them from one tool to another. Every restore and copy makes a backup first."
        actions={
          <IconButton label="Refresh" onClick={() => setReload((n) => n + 1)}>
            <RefreshCw size={14} />
          </IconButton>
        }
      />
      <div role="tablist" aria-label="Configs" className="flex w-fit gap-0.5 rounded-xl border border-border bg-surface p-0.5">
        {TABS.map((t) => {
          const on = t.id === tab
          return (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={on}
              data-testid={`configs-tab-${t.id}`}
              onClick={() => setTab(t.id)}
              className={cn(
                "relative flex h-7 items-center gap-1.5 rounded-[9px] px-3 text-[12.5px] font-medium transition-colors duration-150",
                on ? "text-bg" : "text-muted hover:text-text",
              )}
            >
              {on && <motion.span layoutId="configs-tab" className="absolute inset-0 rounded-[9px] bg-text" transition={{ type: "spring", stiffness: 500, damping: 40 }} />}
              <span className="relative flex items-center gap-1.5">
                {t.icon}
                {t.label}
              </span>
            </button>
          )
        })}
      </div>
      {tab === "backup" && <BackupTab key={reload} />}
      {tab === "restore" && <RestoreTab key={reload} />}
      {tab === "copy" && <CopyTab key={reload} />}
    </div>
  )
}

// ------------------------------------------------------------------ back up

function BackupTab() {
  const { toast, setActivity } = useStore()
  const [tools, setTools] = useState<ConfigTool[] | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [open, setOpen] = useState<Set<string>>(new Set())
  const [confirm, setConfirm] = useState(false)
  const [exporting, setExporting] = useState(false)
  const [progress, setProgress] = useState<ConfigProgress | null>(null)

  const load = useCallback(async () => {
    if (!api.inTauri) return setTools([])
    setTools(null)
    try {
      const ts = await configsApi.listSources()
      setTools(ts)
      setSelected(new Set(ts.flatMap((t) => t.items.filter((i) => i.defaultSelected).map((i) => i.id))))
    } catch (e) {
      setTools([])
      toast({ kind: "error", title: "Could not look for configs", detail: String(e) })
    }
  }, [toast])
  useEffect(() => void load(), [load])

  const all = useMemo(() => (tools ?? []).flatMap((t) => t.items), [tools])
  const sel = all.filter((i) => selected.has(i.id))
  const selBytes = sel.reduce((n, i) => n + i.size, 0)
  const secrets = sel.filter((i) => i.sensitive)

  const setMany = (ids: string[], v: boolean) => {
    const s = new Set(selected)
    for (const id of ids) v ? s.add(id) : s.delete(id)
    setSelected(s)
  }
  const toggleOpen = (id: string) => {
    const s = new Set(open)
    s.has(id) ? s.delete(id) : s.add(id)
    setOpen(s)
  }

  const run = async () => {
    const dest = await saveDialog({ defaultPath: backupFileName(), filters: [{ name: "Zip", extensions: ["zip"] }], title: "Save configs backup" })
    if (!dest) return
    setExporting(true)
    setActivity({ kind: "busy", text: `Backing up ${sel.length} config items` })
    const off = on<ConfigProgress>("config-export-progress", setProgress)
    try {
      const r = await configsApi.exportConfigs(
        sel.map((i) => i.id),
        dest,
      )
      setConfirm(false)
      setActivity({ kind: "ok", text: `Backed up ${r.items} items · ${bytes(r.bytes)}` })
      toast({
        kind: r.skipped.length ? "info" : "ok",
        title: `Saved ${r.items} items · ${bytes(r.bytes)}`,
        detail: r.skipped.length ? `${r.skipped.length} skipped: ${r.skipped.join("; ")}` : r.path,
      })
      void revealItemInDir(r.path).catch(() => {})
    } catch (e) {
      setActivity({ kind: "error", text: "Backup failed" })
      toast({ kind: "error", title: "Backup failed", detail: String(e) })
    } finally {
      off()
      setExporting(false)
      setProgress(null)
    }
  }

  if (tools === null) return <ListSkeleton />

  return (
    <>
      <Card className="overflow-hidden">
        {tools.length === 0 ? (
          <Empty icon={<Archive size={28} />} title="No configs found">
            None of the supported editors, terminals or AI tools have settings on this machine.
          </Empty>
        ) : (
          <>
            <div className="grid grid-cols-[18px_16px_minmax(0,1fr)_84px] items-center gap-3 border-b border-border px-4 py-2.5 text-[11px] uppercase tracking-[0.06em] text-subtle">
              <Checkbox
                label="Select everything"
                checked={sel.length === all.length && all.length > 0}
                indeterminate={sel.length > 0 && sel.length < all.length}
                onChange={(v) => setMany(all.map((i) => i.id), v)}
              />
              <span />
              <span>Tool</span>
              <span className="text-right">Size</span>
            </div>
            <ul className="m-0 max-h-[calc(100vh-330px)] min-h-[120px] list-none overflow-y-auto p-0" data-testid="config-sources">
              {tools.map((t) => {
                const ids = t.items.map((i) => i.id)
                const n = ids.filter((id) => selected.has(id)).length
                const expanded = open.has(t.id)
                return (
                  <motion.li key={t.id} {...ROW} className="border-t border-border first:border-t-0">
                    <div
                      role="button"
                      tabIndex={0}
                      aria-expanded={expanded}
                      onClick={() => toggleOpen(t.id)}
                      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), toggleOpen(t.id))}
                      className="grid h-11 cursor-pointer grid-cols-[18px_16px_minmax(0,1fr)_84px] items-center gap-3 px-4 hover:bg-surface-2"
                    >
                      <Checkbox label={`Select all of ${t.name}`} checked={n === ids.length} indeterminate={n > 0 && n < ids.length} onChange={(v) => setMany(ids, v)} />
                      <motion.span animate={{ rotate: expanded ? 90 : 0 }} transition={{ duration: 0.15 }} className="grid place-items-center text-subtle">
                        <ChevronRight size={14} />
                      </motion.span>
                      <div className="flex min-w-0 items-center gap-2.5">
                        <BrandIcon name={brandOf(t.id)} size={15} />
                        <span className="truncate text-[13px] font-medium">{t.name}</span>
                        <span className="mono text-[11.5px] text-subtle">
                          {n}/{ids.length}
                        </span>
                        {t.items.some((i) => i.sensitive && selected.has(i.id)) && <Badge tone="warn">secrets</Badge>}
                      </div>
                      <span className="mono text-right text-[12.5px]">{bytes(t.size)}</span>
                    </div>
                    <AnimatePresence initial={false}>
                      {expanded && (
                        <motion.ul
                          initial={{ height: 0, opacity: 0 }}
                          animate={{ height: "auto", opacity: 1 }}
                          exit={{ height: 0, opacity: 0 }}
                          transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                          className="m-0 list-none overflow-hidden p-0"
                        >
                          {t.items.map((i) => (
                            <SourceRow key={i.id} item={i} checked={selected.has(i.id)} onChange={(v) => setMany([i.id], v)} />
                          ))}
                        </motion.ul>
                      )}
                    </AnimatePresence>
                  </motion.li>
                )
              })}
            </ul>
          </>
        )}
        {all.length > 0 && (
          <div className="flex items-center gap-3 border-t border-border bg-surface-2 px-4 py-2.5">
            <span className="mono text-[12.5px]">
              {sel.length} items · {bytes(selBytes)}
            </span>
            {secrets.length > 0 && <Badge tone="warn">{secrets.length} with secrets</Badge>}
            <span className="flex-1" />
            <Button variant="primary" size="sm" className="h-[30px] px-3" icon={<Download size={14} />} disabled={!sel.length} onClick={() => setConfirm(true)} data-testid="export-configs">
              Export .zip
            </Button>
          </div>
        )}
      </Card>

      <Dialog
        open={confirm}
        onClose={() => !exporting && setConfirm(false)}
        title={`Back up ${sel.length} item${sel.length === 1 ? "" : "s"} · ${bytes(selBytes)}`}
        actions={
          <>
            <Button onClick={() => setConfirm(false)} disabled={exporting}>
              Cancel
            </Button>
            <Button variant="primary" icon={<Download size={14} />} loading={exporting} onClick={run} data-testid="confirm-export">
              Choose location and export
            </Button>
          </>
        }
      >
        {secrets.length > 0 && (
          <div className="mb-3 rounded-lg border border-warn/40 bg-warn-soft p-3 text-text" data-testid="secrets-warning">
            <div className="flex items-center gap-2 text-[13px] font-medium text-warn">
              <TriangleAlert size={14} />
              This file will contain login tokens/API keys in plain text. Keep it private.
            </div>
            <ul className="mono m-0 mt-2 flex list-none flex-col gap-0.5 p-0 text-[11.5px] text-muted">
              {secrets.map((i) => (
                <li key={i.id}>
                  {toolName(tools, i.tool)} · {i.label}
                </li>
              ))}
            </ul>
          </div>
        )}
        <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
          {tools
            .map((t) => ({ t, items: t.items.filter((i) => selected.has(i.id)) }))
            .filter((x) => x.items.length)
            .map(({ t, items }) => (
              <li key={t.id} className="flex items-center gap-2 text-text">
                <BrandIcon name={brandOf(t.id)} size={13} />
                <span className="flex-1 truncate">{t.name}</span>
                <span className="mono text-[11.5px] text-subtle">
                  {items.length} · {bytes(items.reduce((n, i) => n + i.size, 0))}
                </span>
              </li>
            ))}
        </ul>
        <p className="m-0 mt-3 text-[12px]">The zip also holds AGENT-RESTORE.md, so Claude Code, Codex or any coding agent can restore it without this app.</p>
        {exporting && <ProgressLine p={progress} />}
      </Dialog>
    </>
  )
}

function SourceRow({ item, checked, onChange }: { item: ConfigItem; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <li className="grid h-10 grid-cols-[18px_16px_minmax(0,1fr)_84px] items-center gap-3 border-t border-border/60 bg-surface-2/40 px-4 pl-9 hover:bg-surface-2">
      <Checkbox label={`Select ${item.label}`} checked={checked} onChange={onChange} />
      <span />
      <div className="flex min-w-0 items-center gap-2">
        <span className="shrink-0 text-[12.5px] font-medium">{item.label}</span>
        <span className="mono min-w-0 truncate text-[11px] text-subtle" title={item.path}>
          {item.path}
        </span>
        {item.sensitive && (
          <span className="flex shrink-0 items-center">
            <Badge tone="warn">sensitive</Badge>
            <InfoTip text={item.secret || "May contain secrets."} />
          </span>
        )}
        {item.note && <Badge tone="muted">{item.note}</Badge>}
      </div>
      <span className="mono text-right text-[12px] text-muted">{item.kind === "extensions" ? "—" : bytes(item.size)}</span>
    </li>
  )
}

// ------------------------------------------------------------------ restore

function RestoreTab() {
  const { toast, setActivity } = useStore()
  const [bundle, setBundle] = useState<BundleInfo | null>(null)
  const [opening, setOpening] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [confirm, setConfirm] = useState(false)
  const [restoring, setRestoring] = useState(false)
  const [progress, setProgress] = useState<ConfigProgress | null>(null)
  const [backups, setBackups] = useState<BackupInfo[] | null>(null)
  const [undoing, setUndoing] = useState<string | null>(null)

  const loadBackups = useCallback(async () => {
    if (!api.inTauri) return setBackups([])
    try {
      setBackups(await configsApi.listBackups())
    } catch {
      setBackups([])
    }
  }, [])
  useEffect(() => void loadBackups(), [loadBackups])

  const pick = async () => {
    const path = await openDialog({ multiple: false, directory: false, title: "Open a configs backup", filters: [{ name: "Zip", extensions: ["zip"] }] })
    if (typeof path !== "string") return
    setOpening(true)
    try {
      const b = await configsApi.readBundle(path)
      setBundle(b)
      setSelected(new Set(b.items.filter((i) => i.supported).map((i) => i.id)))
    } catch (e) {
      toast({ kind: "error", title: "Could not read that backup", detail: String(e) })
    } finally {
      setOpening(false)
    }
  }

  const usable = bundle?.items.filter((i) => i.supported) ?? []
  const sel = usable.filter((i) => selected.has(i.id))
  const replacing = sel.filter((i) => i.replaces)

  const restore = async () => {
    if (!bundle) return
    setRestoring(true)
    setActivity({ kind: "busy", text: `Restoring ${sel.length} config items` })
    const off = on<ConfigProgress>("config-import-progress", setProgress)
    try {
      const r = await configsApi.importConfigs(
        bundle.path,
        sel.map((i) => i.id),
      )
      const problems = [...r.failed.map((f) => `${f.label}: ${f.error}`), ...(r.extensionsFailed.length ? [`Extensions not installed: ${r.extensionsFailed.join(", ")}`] : [])]
      setActivity({ kind: problems.length ? "error" : "ok", text: `Restored ${r.restored} items` })
      toast({
        kind: problems.length ? "error" : "ok",
        title: `Restored ${r.restored} item${r.restored === 1 ? "" : "s"}${r.extensionsInstalled ? ` · ${r.extensionsInstalled} extensions` : ""}`,
        detail: problems.length ? problems.join("\n") : "Replaced files were backed up first. Undo from Previous restores.",
      })
      setConfirm(false)
      const b = await configsApi.readBundle(bundle.path).catch(() => null)
      if (b) setBundle(b)
      void loadBackups()
    } catch (e) {
      setActivity({ kind: "error", text: "Restore failed" })
      toast({ kind: "error", title: "Restore failed", detail: String(e) })
    } finally {
      off()
      setRestoring(false)
      setProgress(null)
    }
  }

  const undo = async (b: BackupInfo) => {
    setUndoing(b.id)
    try {
      const r = await configsApi.undoImport(b.id)
      toast({
        kind: r.failed.length ? "error" : "ok",
        title: `Put back ${r.restored} item${r.restored === 1 ? "" : "s"}`,
        detail: r.failed.length ? r.failed.join("\n") : `Undid “${b.reason}”.`,
      })
      void loadBackups()
      if (bundle) void configsApi.readBundle(bundle.path).then(setBundle).catch(() => {})
    } catch (e) {
      toast({ kind: "error", title: "Undo failed", detail: String(e) })
    } finally {
      setUndoing(null)
    }
  }

  return (
    <>
      <Card className="overflow-hidden">
        <div className="flex items-center gap-3 border-b border-border px-4 py-2.5">
          <FileArchive size={15} className="text-subtle" />
          <div className="min-w-0 flex-1">
            <div className={cn("mono truncate text-[12.5px]", !bundle && "text-subtle")} title={bundle?.path}>
              {bundle?.path ?? "No backup open"}
            </div>
            {bundle && (
              <div className="text-[11.5px] text-subtle">
                From {bundle.hostname || "unknown machine"} ({bundle.sourceOs}) · {new Date(bundle.createdAt).toLocaleString()}
              </div>
            )}
          </div>
          <Button icon={<FolderOpen size={14} />} loading={opening} onClick={pick} data-testid="open-bundle">
            Open backup .zip
          </Button>
        </div>
        {!bundle ? (
          <Empty icon={<ArchiveRestore size={28} />} title="Open a backup to restore">
            Pick a zip made with Back up. Paths are mapped to this machine, and anything replaced is copied aside first.
          </Empty>
        ) : (
          <>
            <div className="grid grid-cols-[18px_16px_minmax(0,1fr)_auto] items-center gap-3 border-b border-border px-4 py-2.5 text-[11px] uppercase tracking-[0.06em] text-subtle">
              <Checkbox
                label="Select everything"
                checked={sel.length === usable.length && usable.length > 0}
                indeterminate={sel.length > 0 && sel.length < usable.length}
                onChange={(v) => setSelected(v ? new Set(usable.map((i) => i.id)) : new Set())}
              />
              <span />
              <span>Item and where it goes</span>
              <span />
            </div>
            <ul className="m-0 max-h-[calc(100vh-470px)] min-h-[120px] list-none overflow-y-auto p-0" data-testid="bundle-items">
              {bundle.items.map((i) => (
                <motion.li
                  key={i.id}
                  {...ROW}
                  className={cn(
                    "grid min-h-11 grid-cols-[18px_16px_minmax(0,1fr)_auto] items-center gap-3 border-t border-border px-4 py-1.5 first:border-t-0 hover:bg-surface-2",
                    !i.supported && "opacity-55",
                  )}
                >
                  {i.supported ? (
                    <Checkbox
                      label={`Select ${i.toolName} ${i.label}`}
                      checked={selected.has(i.id)}
                      onChange={(v) => {
                        const s = new Set(selected)
                        v ? s.add(i.id) : s.delete(i.id)
                        setSelected(s)
                      }}
                    />
                  ) : (
                    <span />
                  )}
                  <BrandIcon name={brandOf(i.tool)} size={15} />
                  <div className="min-w-0">
                    <div className="truncate text-[13px] font-medium">
                      {i.toolName} <span className="text-muted">· {i.label}</span>
                    </div>
                    <div className="mono truncate text-[11px] text-subtle" title={i.target}>
                      {i.supported ? i.target : i.note}
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5">
                    {i.sensitive && <Badge tone="warn">sensitive</Badge>}
                    {i.supported && i.kind !== "extensions" && (i.replaces ? <Badge tone="warn">replaces existing</Badge> : <Badge tone="muted">new</Badge>)}
                    {!i.supported && <Badge tone="muted">skipped</Badge>}
                  </div>
                </motion.li>
              ))}
            </ul>
            <div className="flex items-center gap-3 border-t border-border bg-surface-2 px-4 py-2.5">
              <span className="mono text-[12.5px]">
                {sel.length} selected{replacing.length ? ` · ${replacing.length} replace existing` : ""}
              </span>
              <span className="flex-1" />
              <Button variant="primary" size="sm" className="h-[30px] px-3" icon={<ArchiveRestore size={14} />} disabled={!sel.length} onClick={() => setConfirm(true)} data-testid="restore-configs">
                Restore selected
              </Button>
            </div>
          </>
        )}
      </Card>

      <Card className="overflow-hidden">
        <div className="flex items-center gap-2 border-b border-border px-4 py-2.5">
          <History size={14} className="text-subtle" />
          <span className="text-[13px] font-medium">Previous restores</span>
          <InfoTip text="Before every restore or copy, the files it replaces are saved here. Undo puts them back and removes files the restore created." />
        </div>
        {backups === null ? (
          <div className="flex flex-col gap-2 p-4">
            <Skeleton className="h-5 w-2/3" />
            <Skeleton className="h-5 w-1/2" />
          </div>
        ) : backups.length === 0 ? (
          <div className="px-4 py-5 text-[12.5px] text-muted">Nothing restored or copied yet.</div>
        ) : (
          <ul className="m-0 max-h-[260px] list-none overflow-y-auto p-0" data-testid="config-backups">
            <AnimatePresence initial={false}>
              {backups.map((b) => (
                <motion.li key={b.id} layout="position" {...ROW} className="flex h-11 items-center gap-3 border-t border-border px-4 first:border-t-0 hover:bg-surface-2">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[13px]">{b.reason}</div>
                    <Tooltip text={<span className="mono text-[11px]">{b.targets.slice(0, 8).join("\n")}{b.targets.length > 8 ? `\n+${b.targets.length - 8} more` : ""}</span>}>
                      <span className="text-[11.5px] text-subtle">
                        {ago(b.createdAt)} · {b.count} paths · {b.replaced} replaced
                      </span>
                    </Tooltip>
                  </div>
                  {b.undone ? (
                    <Badge tone="muted">undone</Badge>
                  ) : (
                    <Button size="sm" icon={<Undo2 size={13} />} loading={undoing === b.id} disabled={!!undoing && undoing !== b.id} onClick={() => void undo(b)}>
                      Undo
                    </Button>
                  )}
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        )}
      </Card>

      <Dialog
        open={confirm}
        onClose={() => !restoring && setConfirm(false)}
        title={`Restore ${sel.length} item${sel.length === 1 ? "" : "s"}?`}
        actions={
          <>
            <Button onClick={() => setConfirm(false)} disabled={restoring}>
              Cancel
            </Button>
            <Button variant="primary" icon={<ArchiveRestore size={14} />} loading={restoring} onClick={restore} data-testid="confirm-restore">
              Back up, then restore
            </Button>
          </>
        }
      >
        <p className="m-0 mb-3">
          {replacing.length
            ? `${replacing.length} existing file${replacing.length === 1 ? "" : "s"} or folder${replacing.length === 1 ? "" : "s"} will be copied to a backup, then replaced. Folders are merged: same-named files are replaced, others kept.`
            : "Nothing on this machine is replaced; every item is new."}{" "}
          You can undo from Previous restores.
        </p>
        {sel.some((i) => i.kind === "extensions") && <p className="m-0 mb-3">Extensions are installed through each editor's CLI and are not removed by Undo.</p>}
        <ul className="m-0 flex list-none flex-col gap-1 p-0">
          {replacing.map((i) => (
            <li key={i.id} className="mono truncate text-[11.5px] text-text" title={i.target}>
              {i.target}
            </li>
          ))}
        </ul>
        {restoring && <ProgressLine p={progress} />}
      </Dialog>
    </>
  )
}

// ------------------------------------------------------------------ copy between tools

type CardState = { from: string; to: string[]; overwrite: boolean; extensions: boolean }

const ACTION_TONE: Record<PlanAction, "ok" | "warn" | "muted" | "accent"> = {
  copy: "ok",
  add: "ok",
  convert: "accent",
  install: "accent",
  overwrite: "warn",
  skip: "muted",
}

function CopyTab() {
  const { toast, setActivity } = useStore()
  const [list, setList] = useState<Migration[] | null>(null)
  const [state, setState] = useState<Record<string, CardState>>({})
  const [preview, setPreview] = useState<{ m: Migration; steps: PlanStep[] } | null>(null)
  const [busy, setBusy] = useState<{ id: string; what: "plan" | "run" } | null>(null)
  const [progress, setProgress] = useState<ConfigProgress | null>(null)

  const load = useCallback(async () => {
    if (!api.inTauri) return setList([])
    try {
      const ms = await configsApi.listMigrations()
      setList(ms)
      setState((old) => {
        const next: Record<string, CardState> = {}
        for (const m of ms) {
          const from = old[m.id]?.from ?? m.sources.find((s) => s.found)?.id ?? m.sources[0]?.id ?? ""
          next[m.id] = old[m.id] ?? { from, to: [], overwrite: false, extensions: m.extensions }
        }
        return next
      })
    } catch (e) {
      setList([])
      toast({ kind: "error", title: "Could not load copy options", detail: String(e) })
    }
  }, [toast])
  useEffect(() => void load(), [load])

  const opts = (m: Migration): MigrateOptions => {
    const s = state[m.id]
    return { from: s.from, to: s.to.filter((t) => t !== s.from), overwrite: s.overwrite, extensions: s.extensions }
  }
  const patch = (id: string, p: Partial<CardState>) => setState((s) => ({ ...s, [id]: { ...s[id], ...p } }))

  const plan = async (m: Migration) => {
    setBusy({ id: m.id, what: "plan" })
    try {
      setPreview({ m, steps: await configsApi.planMigration(m.id, opts(m)) })
    } catch (e) {
      toast({ kind: "error", title: "Could not plan the copy", detail: String(e) })
    } finally {
      setBusy(null)
    }
  }

  const run = async (m: Migration) => {
    setBusy({ id: m.id, what: "run" })
    setActivity({ kind: "busy", text: `Copying ${m.title.toLowerCase()}` })
    const off = on<ConfigProgress>("config-migrate-progress", setProgress)
    try {
      const r = await configsApi.runMigration(m.id, opts(m))
      const problems = [...r.failed, ...(r.extensionsFailed.length ? [`Not available: ${r.extensionsFailed.join(", ")}`] : [])]
      setActivity({ kind: problems.length ? "error" : "ok", text: `Copied ${m.title.toLowerCase()}` })
      toast({
        kind: problems.length ? "error" : r.done ? "ok" : "info",
        title: r.done ? `${m.title}: ${r.done} done${r.extensionsInstalled ? ` · ${r.extensionsInstalled} extensions` : ""}` : `${m.title}: nothing to copy`,
        detail: problems.length
          ? problems.join("\n")
          : `${r.skipped ? `${r.skipped} skipped (already there). ` : ""}${r.backupId ? "A backup was made first; undo it from the Restore tab." : ""}`,
      })
      setPreview(null)
      void load()
    } catch (e) {
      setActivity({ kind: "error", text: "Copy failed" })
      toast({ kind: "error", title: "Copy failed", detail: String(e) })
    } finally {
      off()
      setBusy(null)
      setProgress(null)
    }
  }

  if (list === null) return <ListSkeleton cards />

  return (
    <>
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2" data-testid="migrations">
        {list.map((m, idx) => {
          const s = state[m.id]
          if (!s) return null
          const src = m.sources.find((x) => x.id === s.from)
          const targets = m.targets.filter((t) => t.id !== s.from)
          const chosen = s.to.filter((t) => t !== s.from)
          const disabled = !src?.found || !chosen.length || !!busy
          return (
            <motion.div key={m.id} {...ROW} transition={{ ...ROW.transition, delay: idx * 0.03 }}>
              <Card className="flex h-full flex-col gap-3 p-4">
                <div>
                  <div className="text-[14px] font-semibold">{m.title}</div>
                  <p className="m-0 mt-0.5 text-[12.5px] text-muted">{m.about}</p>
                </div>
                <div className="flex flex-col gap-2">
                  <div className="text-[11px] uppercase tracking-[0.06em] text-subtle">From</div>
                  <div className="flex flex-wrap gap-1.5">
                    {m.sources.map((e) => (
                      <EndpointChip key={e.id} id={e.id} name={e.name} detail={e.found ? e.detail : "Not found on this machine"} on={s.from === e.id} disabled={!e.found} onClick={() => patch(m.id, { from: e.id })} />
                    ))}
                  </div>
                  <div className="flex items-center gap-1.5 text-[11px] uppercase tracking-[0.06em] text-subtle">
                    <ArrowRight size={12} /> To
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {targets.map((e) => (
                      <EndpointChip
                        key={e.id}
                        id={e.id}
                        name={e.name}
                        detail={e.found ? e.path : `Not detected; ${e.path} will be created`}
                        dim={!e.found}
                        on={s.to.includes(e.id)}
                        onClick={() => patch(m.id, { to: s.to.includes(e.id) ? s.to.filter((x) => x !== e.id) : [...s.to, e.id] })}
                      />
                    ))}
                  </div>
                </div>
                <div className="mt-auto flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-border pt-3">
                  <label className="flex items-center gap-2 text-[12px] text-muted">
                    <Switch checked={s.overwrite} onChange={(v) => patch(m.id, { overwrite: v })} label="Overwrite existing" />
                    Overwrite existing
                  </label>
                  {m.extensions && (
                    <label className="flex items-center gap-2 text-[12px] text-muted">
                      <Switch checked={s.extensions} onChange={(v) => patch(m.id, { extensions: v })} label="Include extensions" />
                      Extensions
                    </label>
                  )}
                  <span className="flex-1" />
                  <Button size="sm" icon={<Eye size={13} />} disabled={disabled} loading={busy?.id === m.id && busy.what === "plan"} onClick={() => void plan(m)} data-testid={`preview-${m.id}`}>
                    Preview
                  </Button>
                  <Button variant="primary" size="sm" icon={<Copy size={13} />} disabled={disabled} loading={busy?.id === m.id && busy.what === "run"} onClick={() => void run(m)} data-testid={`copy-${m.id}`}>
                    Copy
                  </Button>
                </div>
                {busy?.id === m.id && busy.what === "run" && <ProgressLine p={progress} />}
              </Card>
            </motion.div>
          )
        })}
      </div>

      <Dialog
        open={!!preview}
        onClose={() => !busy && setPreview(null)}
        title={preview ? `${preview.m.title}: ${preview.steps.filter((x) => x.action !== "skip").length} changes` : ""}
        actions={
          <>
            <Button onClick={() => setPreview(null)} disabled={!!busy}>
              Close
            </Button>
            <Button
              variant="primary"
              icon={<Copy size={14} />}
              loading={busy?.what === "run"}
              disabled={!preview || !preview.steps.some((x) => x.action !== "skip")}
              onClick={() => preview && void run(preview.m)}
              data-testid="confirm-copy"
            >
              Copy
            </Button>
          </>
        }
      >
        {preview && preview.steps.length === 0 ? (
          <p className="m-0">Nothing to copy.</p>
        ) : (
          <ul className="m-0 flex list-none flex-col p-0">
            {preview?.steps.map((st, i) => (
              <li key={i} className="flex flex-col gap-0.5 border-t border-border py-2 first:border-t-0">
                <div className="flex items-center gap-2">
                  <Badge tone={ACTION_TONE[st.action] ?? "muted"}>{st.action}</Badge>
                  <span className="mono min-w-0 truncate text-[11.5px] text-text" title={st.from}>
                    {st.from}
                  </span>
                </div>
                <div className="mono flex min-w-0 items-center gap-1.5 pl-1 text-[11.5px] text-subtle">
                  <ArrowRight size={11} className="shrink-0" />
                  <span className="truncate" title={st.to}>
                    {st.to}
                  </span>
                </div>
                {st.detail && <div className="pl-1 text-[11.5px] text-muted">{st.detail}</div>}
              </li>
            ))}
          </ul>
        )}
        {busy?.what === "run" && <ProgressLine p={progress} />}
      </Dialog>
    </>
  )
}

function EndpointChip({ id, name, detail, on, disabled, dim, onClick }: { id: string; name: string; detail?: string; on: boolean; disabled?: boolean; dim?: boolean; onClick: () => void }) {
  const chip = (
    <button
      type="button"
      aria-pressed={on}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex h-7 items-center gap-1.5 rounded-full border px-3 text-[12px] font-medium transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-45",
        on ? "border-text bg-text text-bg" : "border-border-strong bg-surface text-muted hover:text-text",
        dim && !on && "border-dashed",
      )}
    >
      <BrandIcon name={brandOf(id)} size={12} mono={on} />
      {name}
    </button>
  )
  return detail ? <Tooltip text={<span className="break-all">{detail}</span>}>{chip}</Tooltip> : chip
}

// ------------------------------------------------------------------ shared

function ProgressLine({ p }: { p: ConfigProgress | null }) {
  return (
    <div className="mt-3 flex flex-col gap-1.5">
      <Bar value={p && p.total ? p.done / p.total : 0} thin />
      <div className="mono truncate text-[11px] text-subtle">{p?.current || "Starting…"}</div>
    </div>
  )
}

function ListSkeleton({ cards }: { cards?: boolean }) {
  if (cards)
    return (
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        {[0, 1, 2, 3].map((i) => (
          <Card key={i} className="flex flex-col gap-3 p-4">
            <Skeleton className="h-4 w-1/3" />
            <Skeleton className="h-3 w-3/4" />
            <Skeleton className="h-7 w-full" />
            <Skeleton className="h-7 w-2/3" />
          </Card>
        ))}
      </div>
    )
  return (
    <Card className="overflow-hidden">
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <div key={i} className="flex h-11 items-center gap-3 border-t border-border px-4 first:border-t-0">
          <Skeleton className="h-4 w-4" />
          <Skeleton className="h-4 w-4 rounded-full" />
          <Skeleton className="h-3.5 w-40" />
          <span className="flex-1" />
          <Skeleton className="h-3.5 w-14" />
        </div>
      ))}
    </Card>
  )
}

function toolName(tools: ConfigTool[], id: string) {
  return tools.find((t) => t.id === id)?.name ?? id
}
