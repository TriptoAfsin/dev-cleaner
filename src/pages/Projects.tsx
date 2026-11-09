import { useMemo, useRef, useState } from "react"
import { open as openDialog } from "@tauri-apps/plugin-dialog"
import { revealItemInDir } from "@tauri-apps/plugin-opener"
import { AnimatePresence, motion } from "motion/react"
import { FolderOpen, FolderSearch, HardDrive, Search, Square, Trash2, X } from "lucide-react"
import { api, on, type Artifact, type DeleteEvent } from "@/lib/api"
import { ago, bytes, cn } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { useDrives, useSettings } from "@/lib/settings"
import { useStore } from "@/lib/store"
import { SnapshotButton } from "@/components/Share"
import { AnimatedNumber, Button, Card, Checkbox, DeleteProgress, Dialog, Empty, IconButton, InfoTip, PageHeader, ProgressBar, Tooltip } from "@/components/ui"

type Sort = "size" | "age"
const DAY = 86400

export function Projects() {
  const { settings, set } = useSettings()
  const { toast, setActivity, openReport } = useStore()
  const [items, setItems] = useState<Artifact[]>([])
  const [scanning, setScanning] = useState(false)
  const [walking, setWalking] = useState("")
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [kind, setKind] = useState("all")
  const [sort, setSort] = useState<Sort>("size")
  const [confirm, setConfirm] = useState(false)
  const [deleting, setDeleting] = useState(false)
  // The batch being deleted, frozen at the start so the counts don't shrink
  // as rows disappear.
  const [batch, setBatch] = useState<{ items: Artifact[]; n: number; size: number; done: number; freed: number } | null>(null)
  const [gone, setGone] = useState<Set<string>>(new Set())
  const scanned = useRef(false)

  const drives = useDrives()
  const roots = settings.projectRoots
  const addRoots = (more: string[]) => {
    const next = [...new Set([...roots, ...more])]
    set("projectRoots", next)
    return next
  }
  const pick = async () => {
    const dir = await openDialog({ directory: true, multiple: true, title: "Choose folders to scan", defaultPath: roots[0] || undefined })
    const picked = typeof dir === "string" ? [dir] : Array.isArray(dir) ? dir : []
    if (picked.length) void scan(addRoots(picked))
  }

  const scan = async (list = roots) => {
    if (!list.length) return void pick()
    const root = list.length === 1 ? list[0] : `${list.length} locations`
    scanned.current = true
    setScanning(true)
    setItems([])
    setSelected(new Set())
    setGone(new Set())
    setActivity({ kind: "busy", text: `Scanning ${root}` })
    const offWalk = on<string>("project-walk", setWalking)
    const offFound = on<Artifact>("project-artifact", (a) => setItems((xs) => [...xs, a]))
    try {
      const all = await api.scanProjects(list, settings.projectDepth)
      setItems(all)
      const total = all.reduce((n, a) => n + a.size, 0)
      setActivity({ kind: "ok", text: `Found ${all.length} build folders · ${bytes(total)}` })
    } catch (e) {
      toast({ kind: "error", title: "Scan failed", detail: String(e) })
      setActivity({ kind: "error", text: "Project scan failed" })
    } finally {
      offWalk()
      offFound()
      setScanning(false)
      setWalking("")
    }
  }

  const kinds = useMemo(() => {
    const m = new Map<string, { label: string; size: number; n: number }>()
    for (const a of items) {
      const k = m.get(a.kind) ?? { label: a.label, size: 0, n: 0 }
      k.size += a.size
      k.n++
      m.set(a.kind, k)
    }
    return [...m.entries()].sort((a, b) => b[1].size - a[1].size)
  }, [items])

  const shown = items
    .filter((a) => !gone.has(a.path) && (kind === "all" || a.kind === kind))
    .sort((a, b) => (sort === "size" ? b.size - a.size : a.modified - b.modified))
  const selItems = items.filter((a) => selected.has(a.path) && !gone.has(a.path))
  const selBytes = selItems.reduce((n, a) => n + a.size, 0)
  const total = items.filter((a) => !gone.has(a.path)).reduce((n, a) => n + a.size, 0)

  const toggle = (p: string, v: boolean) => {
    const s = new Set(selected)
    if (v) s.add(p)
    else s.delete(p)
    setSelected(s)
  }
  const selectOlder = (days: number) => {
    const cutoff = Date.now() / 1000 - days * DAY
    setSelected(new Set(shown.filter((a) => a.modified && a.modified < cutoff).map((a) => a.path)))
  }

  const remove = async () => {
    setDeleting(true)
    setBatch({ items: selItems, n: selItems.length, size: selBytes, done: 0, freed: 0 })
    setActivity({ kind: "busy", text: `Deleting ${selItems.length} folders` })
    const off = on<DeleteEvent>("project-deleted", (e) => {
      setBatch((b) => b && { ...b, done: b.done + 1, freed: b.freed + e.freed })
      if (e.ok) setGone((g) => new Set(g).add(e.path))
    })
    try {
      const res = await api.deleteArtifacts(selItems.map((a) => a.path))
      const freed = res.reduce((n, r) => n + r.freed, 0)
      const failed = res.filter((r) => !r.ok)
      setSelected(new Set())
      setActivity({ kind: failed.length ? "error" : "ok", text: `Freed ${bytes(freed)}${failed.length ? ` · ${failed.length} failed` : ""}` })
      toast({
        kind: failed.length ? "error" : "ok",
        title: `Freed ${bytes(freed)}`,
        detail: failed.length ? `${failed.length} could not be fully removed: ${failed[0].error}` : `${res.length} folders removed.`,
        action: freed > 0
          ? {
              label: "Share",
              run: () =>
                openReport({
                  kind: "projects",
                  title: "Build folders deleted",
                  total: freed,
                  items: res.map((r) => {
                    const a = items.find((x) => x.path === r.path)
                    return { name: a ? `${a.project} · ${a.label}` : r.path, icon: a?.kind ?? "", freed: r.freed }
                  }),
                  at: Date.now(),
                }),
            }
          : undefined,
      })
    } finally {
      off()
      setDeleting(false)
      setBatch(null)
      setConfirm(false)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Projects"
        lede={
          <>
            Finds node_modules, Python venvs, Rust target folders and framework build caches under a folder. Each comes back with the project's install or build command.
          </>
        }
        actions={
          <>
            <SnapshotButton name="projects" />
            <Button icon={<FolderOpen size={15} />} onClick={pick} disabled={scanning}>
              Add folder
            </Button>
            <Tooltip text={`Add the drives chosen in Settings (${drives.roots.join(" ") || "none"}) and scan them whole.`}>
              <Button icon={<HardDrive size={15} />} onClick={() => void scan(addRoots(drives.roots))} disabled={scanning || !drives.roots.length} data-testid="add-drives">
                Add drives
              </Button>
            </Tooltip>
            {scanning ? (
              <Button icon={<Square size={13} />} onClick={() => void api.cancelProjectScan()}>
                Stop
              </Button>
            ) : (
              <Button variant="primary" icon={<Search size={15} />} onClick={() => void scan()} data-testid="scan-projects">
                Scan
              </Button>
            )}
          </>
        }
      />

      <div className="flex items-center gap-3 rounded-xl border border-border bg-surface px-3.5 py-2.5">
        <FolderSearch size={15} className="shrink-0 text-subtle" />
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5" data-testid="project-root">
          {roots.length === 0 && <span className="text-[12.5px] text-subtle">No folder chosen</span>}
          {roots.map((r) => (
            <span key={r} className="mono inline-flex h-6 max-w-[320px] items-center gap-1 rounded-md border border-border bg-surface-2 pl-2 pr-1 text-[12px]">
              <span className="truncate" title={r}>
                {r}
              </span>
              <button
                type="button"
                aria-label={`Remove ${r}`}
                disabled={scanning}
                onClick={() => set("projectRoots", roots.filter((x) => x !== r))}
                className="grid h-4 w-4 place-items-center rounded text-subtle hover:text-text"
              >
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
        <label className="flex items-center gap-2 text-[12px] text-muted">
          Depth
          <InfoTip text="How many folders deep to look. Higher finds more nested projects but takes longer on big drives." />
          <input
            type="number"
            min={2}
            max={14}
            value={settings.projectDepth}
            onChange={(e) => set("projectDepth", Math.max(2, Math.min(14, Number(e.target.value) || 7)))}
            className="mono h-7 w-14 rounded-md border border-border-strong bg-surface px-2 text-[12px] text-text"
          />
        </label>
      </div>

      {(items.length > 0 || scanning) && (
        <div className="flex flex-wrap items-center gap-2">
          <Chip on={kind === "all"} onClick={() => setKind("all")}>
            All <span className="mono text-subtle">{bytes(total)}</span>
          </Chip>
          {kinds.map(([k, v]) => (
            <Chip key={k} on={kind === k} onClick={() => setKind(k)}>
              <BrandIcon name={k} size={12} mono={kind === k} />
              {v.label} <span className={cn("mono", kind === k ? "opacity-70" : "text-subtle")}>{v.n} · {bytes(v.size)}</span>
            </Chip>
          ))}
          <span className="flex-1" />
          <Button size="sm" variant="quiet" onClick={() => selectOlder(30)}>
            Select unused 30+ days
          </Button>
          <Button size="sm" variant="quiet" onClick={() => setSort(sort === "size" ? "age" : "size")}>
            Sort: {sort === "size" ? "largest" : "oldest"}
          </Button>
        </div>
      )}

      <Card className="flex min-h-[300px] flex-1 flex-col overflow-hidden">
        {!scanning && items.length === 0 ? (
          <Empty icon={<FolderSearch size={28} />} title={scanned.current ? "No build folders found" : "Scan a folder of projects"}>
            {scanned.current
              ? "Nothing that matches node_modules, a venv, a Rust target or a framework cache. Try a higher depth or another folder."
              : "Pick the folder that holds your repositories, such as ~/code or D:\\Work. Only recognised build folders are ever listed or deleted."}
          </Empty>
        ) : (
          <>
            <div className="shrink-0 grid grid-cols-[18px_20px_minmax(0,1fr)_130px_110px_84px_28px] items-center gap-3 border-b border-border px-4 py-2.5 text-[11px] uppercase tracking-[0.06em] text-subtle">
              <Checkbox
                label="Select all shown"
                checked={shown.length > 0 && shown.every((a) => selected.has(a.path))}
                onChange={(v) => setSelected(v ? new Set(shown.map((a) => a.path)) : new Set())}
              />
              <span />
              <span>Project</span>
              <span>Kind</span>
              <span>Last changed</span>
              <span className="text-right">Size</span>
              <span />
            </div>
            <ul className="m-0 min-h-0 flex-1 overflow-y-auto min-h-[120px] list-none p-0" data-testid="artifact-list">
              <AnimatePresence initial={false}>
                {shown.map((a) => {
                  const stale = a.modified && Date.now() / 1000 - a.modified > 30 * DAY
                  return (
                    <motion.li
                      key={a.path}
                      layout="position"
                      initial={{ opacity: 0, y: 4 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, x: -16, transition: { duration: 0.18 } }}
                      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                      className="grid h-11 grid-cols-[18px_20px_minmax(0,1fr)_130px_110px_84px_28px] items-center gap-3 border-t border-border px-4 first:border-t-0 hover:bg-surface-2"
                    >
                      <Checkbox label={`Select ${a.project} ${a.label}`} checked={selected.has(a.path)} onChange={(v) => toggle(a.path, v)} />
                      <BrandIcon name={a.kind} size={15} />
                      <div className="min-w-0">
                        <div className="truncate text-[13px] font-medium">{a.project}</div>
                        <div className="mono truncate text-[11px] text-subtle" title={a.path}>
                          {a.path}
                        </div>
                      </div>
                      <span className="flex min-w-0 items-center text-[12.5px] text-muted">
                        <span className="truncate">{a.label}</span>
                        <InfoTip text={<ArtifactHelp a={a} />} />
                      </span>
                      <span className={cn("text-[12.5px]", stale ? "text-warn" : "text-muted")}>{ago(a.modified)}</span>
                      <span className="mono text-right text-[12.5px]">{bytes(a.size)}</span>
                      <IconButton label="Show in folder" className="h-6 w-6 border-transparent bg-transparent" onClick={() => void revealItemInDir(a.path).catch(() => {})}>
                        <FolderOpen size={13} />
                      </IconButton>
                    </motion.li>
                  )
                })}
              </AnimatePresence>
            </ul>
            {scanning && (
              <div className="shrink-0 border-t border-border" data-testid="project-scan-progress">
                <ProgressBar value={null} className="h-0.5 rounded-none" />
                <div className="mono flex items-center gap-2 px-4 py-2 text-[11.5px] text-subtle">
                  <span className="pulse-dot h-1.5 w-1.5 rounded-full bg-accent" />
                  <span className="truncate">{walking || "Starting…"}</span>
                  <span className="ml-auto shrink-0">{items.length} found</span>
                </div>
              </div>
            )}
          </>
        )}
        {items.length > 0 && (
          <div className="shrink-0 flex items-center justify-between gap-4 border-t border-border bg-surface-2 px-4 py-2.5">
            {batch ? (
              <div className="min-w-0 flex-1">
                <DeleteProgress done={batch.done} total={batch.n} freed={batch.freed} label="folders" format={bytes} />
              </div>
            ) : (
              <span className="mono text-[12.5px]">
                {selItems.length} selected · <AnimatedNumber value={selBytes} format={(n) => bytes(n)} />
              </span>
            )}
            <Button variant="danger" size="sm" className="h-[30px] px-3" icon={<Trash2 size={14} />} disabled={!selItems.length || scanning} onClick={() => setConfirm(true)} data-testid="delete-artifacts">
              Delete selected
            </Button>
          </div>
        )}
      </Card>

      <Dialog
        open={confirm}
        onClose={() => !deleting && setConfirm(false)}
        title={batch ? `Deleting ${batch.n} build folder${batch.n === 1 ? "" : "s"} · ${bytes(batch.size)}` : `Delete ${selItems.length} build folder${selItems.length === 1 ? "" : "s"} · ${bytes(selBytes)}?`}
        actions={
          <>
            <Button onClick={() => setConfirm(false)} disabled={deleting}>
              Cancel
            </Button>
            <Button variant="danger" icon={<Trash2 size={14} />} loading={deleting} onClick={remove} data-testid="confirm-delete">
              Delete
            </Button>
          </>
        }
      >
        {batch && (
          <div className="mb-3">
            <DeleteProgress done={batch.done} total={batch.n} freed={batch.freed} label="folders" format={bytes} />
          </div>
        )}
        <p className="m-0 mb-3">These are removed permanently, not moved to the Recycle Bin. Source code is never touched. Restore each with:</p>
        <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
          {Object.entries(
            (batch?.items ?? selItems).reduce<Record<string, { n: number; restore: string; size: number }>>((m, a) => {
              const k = (m[a.label] ??= { n: 0, restore: a.restore, size: 0 })
              k.n++
              k.size += a.size
              return m
            }, {}),
          ).map(([label, v]) => (
            <li key={label} className="flex items-center gap-2 text-text">
              <span className="flex-1">
                {v.n} × {label}
              </span>
              <span className="mono text-[11.5px] text-subtle">{v.restore}</span>
            </li>
          ))}
        </ul>
      </Dialog>
    </div>
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

const KIND_HELP: Record<string, string> = {
  node_modules: "Packages npm, pnpm, Yarn or Bun installed for this project. Your package.json and lockfile stay, so one install brings it back exactly.",
  venv: "A Python virtual environment: an interpreter copy plus installed packages. Your code and requirements files stay.",
  target: "Rust's build output: compiled crates, incremental caches and binaries. cargo build recreates it.",
  next: "Next.js build output and its cache. Rebuilt by next dev or next build.",
  nuxt: "Nuxt build output. Rebuilt by nuxt dev or nuxt build.",
  svelte: "SvelteKit's generated files and build output.",
  turbo: "Turborepo's local task cache. Tasks run again instead of replaying.",
  parcel: "Parcel's build cache.",
  angular: "Angular CLI's build cache.",
  unsloth: "Kernels Unsloth compiled for this training script. Recompiled on the next run.",
  pycache: "Caches of mypy, ruff and pytest, or tox / nox test environments. Recreated on the next run.",
  maven: "Maven's build output: compiled classes, test reports and packaged jars. mvn package recreates it.",
  scala: "sbt's build output for this project. sbt compile recreates it.",
  clojure: "Leiningen's build output. lein compile recreates it.",
  gradle: "Gradle's build output or project-local cache (build, .gradle, .kotlin). The next ./gradlew build recreates it.",
  dotnet: "MSBuild output (bin) or intermediate files (obj) of a .NET project. dotnet build recreates both.",
  composer: "PHP packages Composer installed for this project. composer install brings them back from composer.lock.",
  bundler: "Gems Bundler installed into the project. bundle install brings them back from Gemfile.lock.",
  swift: "SwiftPM's build folder and checked-out dependencies. swift build recreates it.",
  dart: "Dart and Flutter's generated tool files or build output. dart pub get / flutter build recreate them.",
  elixir: "Mix's compiled output (_build) or fetched dependencies (deps). mix deps.get && mix compile recreate them.",
  haskell: "Stack or Cabal build output for this project. stack build / cabal build recreate it.",
  zig: "Zig's local cache or install output. zig build recreates it.",
  cmake: "A CMake build tree (it has a CMakeCache.txt). Re-run cmake and build to recreate it.",
  unity: "Unity's Library folder: imported assets and caches, often many GB. Unity re-imports everything the next time you open the project, which can take a while.",
  godot: "Godot's import cache. Re-created when you open the project.",
  unreal: "Unreal's Intermediate or DerivedDataCache folder. Rebuilt by the editor; the first open is slower.",
}

function ArtifactHelp({ a }: { a: Artifact }) {
  return (
    <span className="flex flex-col gap-1.5 text-left">
      <span className="font-semibold">{a.label}</span>
      <span>{KIND_HELP[a.kind] ?? "A build folder this project's tools regenerate."}</span>
      <span>
        <b>Deletes:</b> {a.path} ({a.files.toLocaleString()} files)
      </span>
      <span>
        <b>Bring it back:</b> <span className="mono">{a.restore}</span>
      </span>
      <span>Your source code is never touched.</span>
    </span>
  )
}
