import { useMemo, useRef, useState } from "react"
import { open as openDialog } from "@tauri-apps/plugin-dialog"
import { revealItemInDir } from "@tauri-apps/plugin-opener"
import { AnimatePresence, motion } from "motion/react"
import { BrainCircuit, FolderOpen, FolderPlus, Search, Square, Trash2, X } from "lucide-react"
import { siHuggingface, siLmstudio, siOllama, siPytorch } from "simple-icons"
import { on } from "@/lib/api"
import { ago, bytes, cn } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { useStore } from "@/lib/store"
import { useDrives, useSettings } from "@/lib/settings"
import { lastUsed, loadModelRoots, modelsApi, quantOf, saveModelRoots, type Model, type ModelDeleteEvent } from "@/lib/models"
import { AnimatedNumber, Button, Card, Checkbox, Dialog, Empty, IconButton, InfoTip, PageHeader, Skeleton } from "@/components/ui"

type Sort = "size" | "lru"
const DAY = 86400

type Report = {
  kind: "models"
  title: string
  total: number
  items: { name: string; icon: string; freed: number }[]
  at: number
}

const ICONS: Record<string, { path: string; hex: string }> = {
  ollama: siOllama,
  huggingface: siHuggingface,
  "hf-dataset": siHuggingface,
  lmstudio: siLmstudio,
  torch: siPytorch,
}

const LABELS: Record<string, string> = {
  ollama: "Ollama",
  huggingface: "Hugging Face",
  "hf-dataset": "HF datasets",
  lmstudio: "LM Studio",
  comfyui: "ComfyUI",
  a1111: "SD WebUI",
  jan: "Jan",
  gpt4all: "GPT4All",
  whisper: "Whisper",
  modelscope: "ModelScope",
  llamacpp: "llama.cpp",
  torch: "PyTorch hub",
  loose: "Loose files",
}

function dark(hex: string) {
  const n = parseInt(hex, 16)
  return 0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255) < 60
}

/** simple-icons marks for the model sources brandFor doesn't know; lettered badge otherwise. */
function SourceIcon({ kind, size = 15, mono }: { kind: string; size?: number; mono?: boolean }) {
  const icon = ICONS[kind]
  if (!icon) return <BrandIcon name={LABELS[kind] ?? kind} size={size} mono={mono} />
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden className="shrink-0" fill={mono || dark(icon.hex) ? "currentColor" : `#${icon.hex}`}>
      <path d={icon.path} />
    </svg>
  )
}

export function Models() {
  const { settings } = useSettings()
  const drives = useDrives()
  const store = useStore() as ReturnType<typeof useStore> & { openReport?: (r: Report) => void }
  const { toast, setActivity } = store
  const [roots, setRoots] = useState<string[]>(loadModelRoots)
  const [items, setItems] = useState<Model[]>([])
  const [scanning, setScanning] = useState(false)
  const [walking, setWalking] = useState("")
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [kind, setKind] = useState("all")
  const [sort, setSort] = useState<Sort>("size")
  const [confirm, setConfirm] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [gone, setGone] = useState<Set<string>>(new Set())
  const scanned = useRef(false)

  const updateRoots = (next: string[]) => {
    setRoots(next)
    saveModelRoots(next)
  }

  const addFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Add a folder to search for models" })
    if (typeof dir === "string" && !roots.includes(dir)) updateRoots([...roots, dir])
  }

  const scan = async () => {
    scanned.current = true
    setScanning(true)
    setItems([])
    setSelected(new Set())
    setGone(new Set())
    setActivity({ kind: "busy", text: "Looking for AI models" })
    const offWalk = on<string>("model-walk", setWalking)
    const offFound = on<Model>("model-found", (m) => setItems((xs) => (xs.some((x) => x.id === m.id) ? xs : [...xs, m])))
    try {
      const all = await modelsApi.scan(settings.modelDrives ? [...new Set([...roots, ...drives.roots])] : roots)
      setItems(all)
      const total = all.reduce((n, m) => n + m.size, 0)
      setActivity({ kind: "ok", text: `Found ${all.length} models · ${bytes(total)}` })
    } catch (e) {
      toast({ kind: "error", title: "Scan failed", detail: String(e) })
      setActivity({ kind: "error", text: "Model scan failed" })
    } finally {
      offWalk()
      offFound()
      setScanning(false)
      setWalking("")
    }
  }

  const live = useMemo(() => items.filter((m) => !gone.has(m.id)), [items, gone])

  const kinds = useMemo(() => {
    const m = new Map<string, { size: number; n: number }>()
    for (const a of live) {
      const k = m.get(a.kind) ?? { size: 0, n: 0 }
      k.size += a.size
      k.n++
      m.set(a.kind, k)
    }
    return [...m.entries()].sort((a, b) => b[1].size - a[1].size)
  }, [live])

  const shown = live
    .filter((m) => kind === "all" || m.kind === kind)
    .sort((a, b) => {
      if (sort === "size") return b.size - a.size
      // Least recently used first; unknown dates go last.
      return (lastUsed(a) || Infinity) - (lastUsed(b) || Infinity)
    })
  const selItems = live.filter((m) => selected.has(m.id))
  const selBytes = selItems.reduce((n, m) => n + m.size, 0)
  const total = live.reduce((n, m) => n + m.size, 0)
  const selectable = shown.filter((m) => m.deletable)
  const ollamaSel = selItems.filter((m) => m.id.startsWith("ollama:"))

  const toggle = (id: string, v: boolean) => {
    const s = new Set(selected)
    if (v) s.add(id)
    else s.delete(id)
    setSelected(s)
  }

  const remove = async () => {
    setDeleting(true)
    setActivity({ kind: "busy", text: `Removing ${selItems.length} model${selItems.length === 1 ? "" : "s"}` })
    const off = on<ModelDeleteEvent>("model-deleted", (e) => e.ok && setGone((g) => new Set(g).add(e.id)))
    const byId = new Map(selItems.map((m) => [m.id, m]))
    try {
      const res = await modelsApi.remove(selItems.map((m) => m.id))
      const freed = res.reduce((n, r) => n + r.freed, 0)
      const failed = res.filter((r) => !r.ok)
      setGone((g) => {
        const s = new Set(g)
        for (const r of res) if (r.ok) s.add(r.id)
        return s
      })
      setSelected(new Set())
      setActivity({ kind: failed.length ? "error" : "ok", text: `Freed ${bytes(freed)}${failed.length ? ` · ${failed.length} failed` : ""}` })
      toast({
        kind: failed.length ? "error" : "ok",
        title: `Freed ${bytes(freed)}`,
        detail: failed.length
          ? `${failed.length} could not be removed: ${failed[0].error}`
          : `${res.length} model${res.length === 1 ? "" : "s"} removed.`,
      })
      const done = res.filter((r) => r.freed > 0)
      if (done.length) {
        store.openReport?.({
          kind: "models",
          title: "AI models removed",
          total: freed,
          items: done.map((r) => {
            const m = byId.get(r.id)
            return { name: m?.name ?? r.id, icon: m?.source ?? "", freed: r.freed }
          }),
          at: Date.now(),
        })
      }
    } catch (e) {
      toast({ kind: "error", title: "Delete failed", detail: String(e) })
      setActivity({ kind: "error", text: "Model delete failed" })
    } finally {
      off()
      setDeleting(false)
      setConfirm(false)
    }
  }

  const cols = "grid-cols-[18px_20px_minmax(0,1fr)_150px_110px_84px_28px]"

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="AI models"
        lede="Models you downloaded for local AI: Ollama, LM Studio, Hugging Face, ComfyUI and loose .gguf / .safetensors files. Nothing is selected for you."
        actions={
          <>
            <Button icon={<FolderPlus size={15} />} onClick={() => void addFolder()} disabled={scanning}>
              Add folder
            </Button>
            {scanning ? (
              <Button icon={<Square size={13} />} onClick={() => void modelsApi.cancel()}>
                Stop
              </Button>
            ) : (
              <Button variant="primary" icon={<Search size={15} />} onClick={() => void scan()} data-testid="scan-models">
                Scan
              </Button>
            )}
          </>
        }
      />

      {roots.length > 0 && (
        <div className="flex flex-wrap items-center gap-2" data-testid="model-roots">
          <span className="flex items-center text-[12px] text-muted">
            Also searching
            <InfoTip text="Besides the usual app folders, Downloads, Desktop and Documents, these folders are searched up to 8 levels deep. A whole drive works too." />
          </span>
          <AnimatePresence initial={false}>
            {roots.map((r) => (
              <motion.span
                key={r}
                layout="position"
                initial={{ opacity: 0, scale: 0.96 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.12 } }}
                transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
                className="mono flex h-7 max-w-[320px] items-center gap-1.5 rounded-full border border-border-strong bg-surface pl-3 pr-1 text-[12px]"
              >
                <span className="truncate" title={r}>
                  {r}
                </span>
                <button
                  type="button"
                  aria-label={`Stop searching ${r}`}
                  disabled={scanning}
                  onClick={() => updateRoots(roots.filter((x) => x !== r))}
                  className="grid h-5 w-5 shrink-0 place-items-center rounded-full text-subtle hover:bg-hover hover:text-text disabled:opacity-50"
                >
                  <X size={12} />
                </button>
              </motion.span>
            ))}
          </AnimatePresence>
        </div>
      )}

      {(live.length > 0 || scanning) && (
        <div className="flex flex-wrap items-center gap-2">
          <Chip on={kind === "all"} onClick={() => setKind("all")}>
            All <span className={cn("mono", kind === "all" ? "opacity-70" : "text-subtle")}>{bytes(total)}</span>
          </Chip>
          {kinds.map(([k, v]) => (
            <Chip key={k} on={kind === k} onClick={() => setKind(k)}>
              <SourceIcon kind={k} size={12} mono={kind === k} />
              {LABELS[k] ?? k}{" "}
              <span className={cn("mono", kind === k ? "opacity-70" : "text-subtle")}>
                {v.n} · {bytes(v.size)}
              </span>
            </Chip>
          ))}
          <span className="flex-1" />
          <Button size="sm" variant="quiet" onClick={() => setSort(sort === "size" ? "lru" : "size")}>
            Sort: {sort === "size" ? "largest" : "least recently used"}
          </Button>
        </div>
      )}

      <Card className="overflow-clip">
        {!scanning && live.length === 0 ? (
          <Empty icon={<BrainCircuit size={28} />} title={scanned.current ? "No AI models found" : "Find downloaded AI models"}>
            {scanned.current
              ? "Nothing from Ollama, LM Studio, the Hugging Face cache or the usual app folders, and no large model files in Downloads, Desktop or Documents. Add a folder if you keep models elsewhere."
              : "Looks in Ollama, LM Studio, the Hugging Face cache, ComfyUI and similar apps, plus Downloads, Desktop and Documents. Only models are ever listed or deleted."}
          </Empty>
        ) : (
          <>
            <div className={cn("thead grid items-center gap-3 rounded-t-xl border-b border-border px-4 py-2.5 text-[11px] uppercase tracking-[0.06em] text-subtle", cols)}>
              <Checkbox
                label="Select all shown"
                checked={selectable.length > 0 && selectable.every((m) => selected.has(m.id))}
                onChange={(v) => setSelected(v ? new Set(selectable.map((m) => m.id)) : new Set())}
              />
              <span />
              <span>Model</span>
              <span>Source</span>
              <span>Last used</span>
              <span className="text-right">Size</span>
              <span />
            </div>
            <ul className="m-0 min-h-[120px] list-none p-0" data-testid="model-list">
              {scanning && shown.length === 0 &&
                [0, 1, 2].map((i) => (
                  <li key={`sk${i}`} className={cn("grid h-12 items-center gap-3 border-t border-border px-4 first:border-t-0", cols)}>
                    <Skeleton className="h-[15px] w-[15px] rounded" />
                    <Skeleton className="h-[15px] w-[15px] rounded" />
                    <div className="flex flex-col gap-1.5">
                      <Skeleton className="h-3 w-2/5 rounded" />
                      <Skeleton className="h-2.5 w-3/5 rounded" />
                    </div>
                    <Skeleton className="h-3 w-20 rounded" />
                    <Skeleton className="h-3 w-16 rounded" />
                    <Skeleton className="ml-auto h-3 w-12 rounded" />
                    <span />
                  </li>
                ))}
              <AnimatePresence initial={false}>
                {shown.map((m) => {
                  const used = lastUsed(m)
                  const stale = used && Date.now() / 1000 - used > 90 * DAY
                  const quant = quantOf(m.name)
                  return (
                    <motion.li
                      key={m.id}
                      layout="position"
                      initial={{ opacity: 0, y: 4 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, x: -16, transition: { duration: 0.18 } }}
                      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                      className={cn("grid h-12 items-center gap-3 border-t border-border px-4 first:border-t-0 hover:bg-surface-2", cols)}
                    >
                      {m.deletable ? (
                        <Checkbox label={`Select ${m.name}`} checked={selected.has(m.id)} onChange={(v) => toggle(m.id, v)} />
                      ) : (
                        <InfoTip text={m.note} />
                      )}
                      <SourceIcon kind={m.kind} />
                      <div className="min-w-0">
                        <div className="flex min-w-0 items-center gap-1.5">
                          <span className="min-w-[60px] truncate text-[13px] font-medium" title={m.note ? `${m.name} · ${m.note}` : m.name}>
                            {m.name}
                          </span>
                          {m.format && <Tag>{m.format}</Tag>}
                          {quant && <Tag accent>{quant}</Tag>}
                          {m.files > 1 && (
                            <span className="shrink-0 text-[11px] text-subtle" title={m.note || undefined}>
                              {m.files} files
                            </span>
                          )}
                        </div>
                        <div className="mono truncate text-[11px] text-subtle" title={m.path}>
                          {m.path}
                        </div>
                      </div>
                      <span className="truncate text-[12.5px] text-muted" title={m.source}>
                        {m.source}
                      </span>
                      <span className={cn("text-[12.5px]", stale ? "text-warn" : "text-muted")}>{ago(used)}</span>
                      <span className="mono text-right text-[12.5px]">{bytes(m.size)}</span>
                      <IconButton label="Show in folder" className="h-6 w-6 border-transparent bg-transparent" onClick={() => void revealItemInDir(m.path).catch(() => {})}>
                        <FolderOpen size={13} />
                      </IconButton>
                    </motion.li>
                  )
                })}
              </AnimatePresence>
            </ul>
            {scanning && (
              <div className="mono flex items-center gap-2 border-t border-border px-4 py-2 text-[11.5px] text-subtle">
                <span className="pulse-dot h-1.5 w-1.5 rounded-full bg-accent" />
                <span className="truncate">{walking || "Starting…"}</span>
                <span className="ml-auto shrink-0">{live.length} found</span>
              </div>
            )}
          </>
        )}
        {live.length > 0 && (
          <div className="tfoot flex items-center justify-between border-t border-border bg-surface-2 px-4 py-2.5">
            <span className="mono text-[12.5px]">
              {selItems.length} selected · <AnimatedNumber value={selBytes} format={(n) => bytes(n)} />
            </span>
            <Button
              variant="danger"
              size="sm"
              className="h-[30px] px-3"
              icon={<Trash2 size={14} />}
              disabled={!selItems.length || scanning}
              onClick={() => setConfirm(true)}
              data-testid="delete-models"
            >
              Delete selected
            </Button>
          </div>
        )}
      </Card>

      <Dialog
        open={confirm}
        onClose={() => !deleting && setConfirm(false)}
        title={`Delete ${selItems.length} model${selItems.length === 1 ? "" : "s"} · ${bytes(selBytes)}?`}
        actions={
          <>
            <Button onClick={() => setConfirm(false)} disabled={deleting}>
              Cancel
            </Button>
            <Button variant="danger" icon={<Trash2 size={14} />} loading={deleting} onClick={remove} data-testid="confirm-delete-models">
              Delete
            </Button>
          </>
        }
      >
        <p className="m-0 mb-3">
          These are removed permanently, not moved to the Recycle Bin. To use one again you'll need to download it again.
          {ollamaSel.length > 0 && (
            <>
              {" "}
              Ollama models are removed with <span className="mono text-text">ollama rm</span>.
            </>
          )}
        </p>
        <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
          {selItems.map((m) => (
            <li key={m.id} className="flex items-center gap-2 text-text">
              <SourceIcon kind={m.kind} size={13} />
              <span className="min-w-0 flex-1 truncate" title={m.path}>
                {m.name}
              </span>
              <span className="mono shrink-0 text-[11.5px] text-subtle">{bytes(m.size)}</span>
            </li>
          ))}
        </ul>
        <div className="mt-3 flex items-center justify-between border-t border-border pt-2.5 text-text">
          <span>Total</span>
          <span className="mono text-[12.5px]">{bytes(selBytes)}</span>
        </div>
      </Dialog>
    </div>
  )
}

function Tag({ children, accent }: { children: React.ReactNode; accent?: boolean }) {
  return (
    <span
      className={cn(
        "mono inline-flex h-[18px] shrink-0 items-center rounded-[5px] px-1.5 text-[10.5px] font-medium",
        accent ? "bg-accent-soft text-accent" : "bg-track text-muted",
      )}
    >
      {children}
    </span>
  )
}

function Chip({ on, onClick, children }: { on: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={cn(
        "flex h-7 items-center gap-1.5 rounded-full border px-3 text-[12px] font-medium transition-colors duration-150",
        on ? "border-text bg-text text-bg" : "border-border-strong bg-surface text-muted hover:text-text",
      )}
    >
      {children}
    </button>
  )
}
