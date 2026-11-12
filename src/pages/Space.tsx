import { useCallback, useEffect, useMemo, useState } from "react"
import { ArrowRight, ChevronLeft, ChevronRight, FolderOpen, FolderSearch, LayoutDashboard, RefreshCw, Sparkles, X } from "lucide-react"
import { open as openDialog } from "@tauri-apps/plugin-dialog"
import { revealItemInDir } from "@tauri-apps/plugin-opener"
import type { Page } from "@/components/Shell"
import { api, on, type Drive } from "@/lib/api"
import { bytes, cn, took } from "@/lib/format"
import { useStore } from "@/lib/store"
import { Button, Card, Dialog, IconButton, PageHeader, Skeleton } from "@/components/ui"
import { Ring, SPACE_TOKENS, Treemap } from "@/components/space/Treemap"
import {
  CAT_ABOUT,
  CAT_LABEL,
  CATS,
  crumbs,
  parentOf,
  space,
  type SpaceCat,
  type SpaceFix,
  type SpaceNode,
  type SpaceProgress,
  type SpaceSummary,
} from "@/lib/space"

// The folder on screen survives leaving the page and coming back.
let lastPath: string | null = null
let lastSel: string | null = null

const count = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(2)} M` : n >= 1e4 ? `${Math.round(n / 1e3)} k` : n.toLocaleString())
const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase()

/** Find a node by path in a tree slice. A folder shown merged with its only
    big subfolder ("Users › you") matches the outer folder's path too. */
function findIn(n: SpaceNode | null, path: string | null): SpaceNode | null {
  if (!n || !path) return null
  const exact = (x: SpaceNode): SpaceNode | null => {
    if (x.path === path) return x
    for (const k of x.children ?? []) {
      const hit = exact(k)
      if (hit) return hit
    }
    return null
  }
  const inside = (x: SpaceNode): SpaceNode | null => {
    for (const k of x.children ?? []) {
      if (k.name.includes(" › ") && (k.path.startsWith(path + "\\") || k.path.startsWith(path + "/"))) return k
      const hit = inside(k)
      if (hit) return hit
    }
    return null
  }
  return exact(n) ?? inside(n)
}

/** Space: where a drive's space went. Read-only; every finding links to the page that cleans it. */
export function Space({ go }: { go: (p: Page) => void }) {
  const { toast, setActivity, platform, stats } = useStore()
  const [summary, setSummary] = useState<SpaceSummary | null>(null)
  const [loaded, setLoaded] = useState(!api.inTauri)
  const [scanning, setScanning] = useState<string | null>(null)
  const [progress, setProgress] = useState<SpaceProgress | null>(null)
  const [drives, setDrives] = useState<Drive[]>([])
  const [path, setPathState] = useState<string | null>(lastPath)
  const [level, setLevel] = useState<SpaceNode | null>(null)
  const [sel, setSelState] = useState<string | null>(lastSel)
  const [detail, setDetail] = useState<SpaceNode | null>(null)
  const [filter, setFilter] = useState<SpaceCat | null>(null)
  const [findingsOpen, setFindingsOpen] = useState(false)

  const setPath = (p: string | null) => {
    lastPath = p
    setPathState(p)
  }
  const setSel = (p: string | null) => {
    lastSel = p
    setSelState(p)
  }

  useEffect(() => {
    if (!api.inTauri) return
    void api
      .listDrives()
      .then((ds) => setDrives(ds.filter((d) => !d.removable && d.total > 0)))
      .catch(() => {})
    void space
      .last()
      .then((s) => {
        if (s) {
          setSummary(s)
          if (!lastPath) setPath(s.root)
        }
      })
      .catch(() => {})
      .finally(() => setLoaded(true))
  }, [])

  // The folder on screen, two levels deep.
  useEffect(() => {
    if (!summary || !path) return
    let alive = true
    space
      .tree(path, 2)
      .then((n) => alive && setLevel(n))
      .catch(() => {
        if (!alive) return
        setPath(summary.root)
        setSel(null)
      })
    return () => {
      alive = false
    }
  }, [summary, path])

  // The selected item: from the slice on screen, or the biggest one.
  const selNode = useMemo(() => findIn(level, sel) ?? level?.children?.[0] ?? null, [level, sel])
  useEffect(() => {
    setDetail(null)
    if (!selNode || selNode.kind !== "dir" || !selNode.hasChildren) return
    if (selNode.children?.length) return setDetail(selNode)
    let alive = true
    void space
      .tree(selNode.path, 1)
      .then((n) => alive && setDetail(n))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [selNode])

  const scan = useCallback(
    async (root: string) => {
      setScanning(root)
      setProgress(null)
      setActivity({ kind: "busy", text: `Scanning ${root}` })
      const off = on<SpaceProgress>("space-progress", setProgress)
      try {
        const s = await space.scan(root)
        setSummary(s)
        setLevel(null)
        setPath(s.root)
        setSel(null)
        setFilter(null)
        setActivity({ kind: "ok", text: `${s.root} scanned · ${count(s.files)} files in ${took(s.elapsedMs)}` })
        if (s.cancelled) toast({ kind: "info", title: "Scan stopped", detail: "The map shows what was measured before you stopped it." })
      } catch (e) {
        setActivity({ kind: "error", text: "Space scan failed" })
        toast({ kind: "error", title: "Scan failed", detail: String(e) })
      } finally {
        off()
        setScanning(null)
      }
    },
    [setActivity, toast],
  )

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Choose a folder to map" }).catch(() => null)
    if (typeof dir === "string") void scan(dir)
  }

  const runFix = (fix: SpaceFix) => {
    if (fix.uri) {
      void space.openSettings(fix.uri).catch((e) => toast({ kind: "error", title: "Could not open Settings", detail: String(e) }))
      return
    }
    if (!fix.page) return
    if (fix.tab) {
      try {
        sessionStorage.setItem("dc-junk-tab", fix.tab)
      } catch {
        /* the page opens on its default tab */
      }
    }
    go(fix.page as Page)
  }

  const open = (n: SpaceNode) => {
    if (n.kind !== "dir" || !n.hasChildren) return
    setPath(n.path)
    setSel(null)
  }
  const up = () => {
    if (!summary || !path || norm(path) === norm(summary.root)) return
    const p = parentOf(path)
    setSel(path)
    setPath(p)
  }
  const reveal = (n: SpaceNode) => {
    const target = n.kind === "files" ? parentOf(n.path.split("\u0000")[0]) : n.path
    void revealItemInDir(target).catch(() => {})
  }

  // Backspace goes up a level; Enter opens the selected folder.
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return
      if (e.key === "Backspace") {
        e.preventDefault()
        up()
      } else if (e.key === "Enter" && selNode?.kind === "dir" && t?.closest("[aria-label='Folder map']")) {
        e.preventDefault()
        open(selNode)
      }
    }
    window.addEventListener("keydown", k)
    return () => window.removeEventListener("keydown", k)
  })

  const systemDrive = stats?.disk?.mount ?? drives[0]?.mount ?? (platform === "windows" ? "C:" : "/")
  const scannedMount = summary?.drive && summary.root.replace(/[\\/]+$/, "").toLowerCase() === summary.drive.mount.toLowerCase() ? summary.drive.mount : null
  const isDriveRoot = !!scannedMount
  const reveal_label = platform === "macos" ? "Show in Finder" : platform === "windows" ? "Show in Explorer" : "Show in folder"

  const driveButtons = (
    <div className="flex flex-wrap items-center gap-2">
      {drives.slice(0, 4).map((d) => {
        const used = 1 - d.free / Math.max(1, d.total)
        const pressed = !!summary && norm(summary.root) === norm(d.mount)
        return (
          <button
            key={d.mount}
            type="button"
            aria-pressed={pressed}
            disabled={!!scanning}
            onClick={() => void scan(d.mount)}
            title={`Map ${d.mount}`}
            className={cn(
              "flex h-11 items-center gap-2.5 rounded-xl border bg-surface pl-2 pr-3.5 transition-colors disabled:opacity-60",
              pressed ? "border-text" : "border-border hover:border-border-strong",
            )}
          >
            <Ring value={used} color={used > 0.85 ? "var(--c-fix)" : "var(--accent)"} />
            <span className="flex flex-col items-start leading-tight">
              <b className="text-[13px] font-semibold">{d.mount}</b>
              <span className="text-[11.5px] tabular-nums text-subtle">{bytes(d.free, 0)} free</span>
            </span>
          </button>
        )
      })}
      <IconButton label="Map a folder" className="h-11 w-10 rounded-xl" onClick={() => void pickFolder()} disabled={!!scanning}>
        <FolderSearch size={15} />
      </IconButton>
      {summary && (
        <IconButton label="Scan again" className="h-11 w-10 rounded-xl" onClick={() => void scan(summary.root)} disabled={!!scanning}>
          <RefreshCw size={15} />
        </IconButton>
      )}
    </div>
  )

  const denom = summary ? (isDriveRoot && summary.drive ? Math.max(summary.drive.total, summary.total) : Math.max(1, summary.total)) : 1
  const fixTotal = summary?.findings.reduce((n, f) => n + f.size, 0) ?? 0
  const big = summary ? bytes(isDriveRoot && summary.drive ? summary.drive.used : summary.total).split(" ") : []

  return (
    <div className="space-root flex h-full min-h-0 flex-col gap-4" data-testid="space-page">
      <style>{SPACE_TOKENS}</style>
      <PageHeader title="Space" lede="Where your disk went. Nothing here deletes; every item links to the page that can clean it." actions={driveButtons} />

      {scanning ? (
        <ScanProgress root={scanning} p={progress} />
      ) : !summary ? (
        loaded ? (
          <Card className="flex flex-1 flex-col items-center justify-center gap-3 px-6 py-14 text-center">
            <span className="grid h-12 w-12 place-items-center rounded-2xl bg-accent-soft text-accent">
              <LayoutDashboard size={22} />
            </span>
            <h2 className="m-0 text-[16px] font-semibold">See where your space went</h2>
            <p className="m-0 max-w-[420px] text-[13px] text-muted">
              Maps every folder on a drive by size, marks what is rebuildable cache, what has a known fix, and what is yours. Read-only; a full drive takes a few seconds to a minute.
            </p>
            <div className="mt-1 flex gap-2">
              <Button variant="primary" icon={<LayoutDashboard size={15} />} onClick={() => void scan(systemDrive)} data-testid="space-scan">
                Map {systemDrive}
              </Button>
              <Button icon={<FolderOpen size={15} />} onClick={() => void pickFolder()}>
                Choose a folder
              </Button>
            </div>
          </Card>
        ) : (
          <Skeleton className="h-40 rounded-xl" />
        )
      ) : (
        <>
          <Card className="flex flex-wrap items-center gap-x-7 gap-y-3 rounded-2xl px-5 py-4">
            <div className="flex min-w-[150px] flex-col gap-0.5">
              <span className="text-[34px] font-semibold leading-none tabular-nums tracking-[-0.02em]">
                {big[0]}
                <span className="ml-1 text-[18px] font-medium text-muted">{big[1]}</span>
              </span>
              <span className="text-[12px] text-muted tabular-nums">
                {isDriveRoot && summary.drive ? `used of ${bytes(summary.drive.total, 0)} · ${bytes(summary.drive.free, 0)} free` : `in ${summary.root}`}
              </span>
            </div>
            <div className="flex min-w-[260px] flex-1 flex-col gap-3">
              <div className="flex h-3 gap-[3px]">
                {CATS.map((c, i) => (
                  <span
                    key={c}
                    className="rounded-[4px] transition-[opacity,width] duration-300"
                    style={{ width: `${(summary.cats[i] / denom) * 100}%`, background: `var(--c-${c})`, opacity: filter && filter !== c ? 0.2 : 1 }}
                  />
                ))}
                <span className="flex-1 rounded-[4px] bg-track" />
              </div>
              <div className="flex flex-wrap gap-1.5">
                {CATS.map((c, i) =>
                  summary.cats[i] > 0 ? (
                    <button
                      key={c}
                      type="button"
                      aria-pressed={filter === c}
                      onClick={() => setFilter(filter === c ? null : c)}
                      className={cn(
                        "flex h-7 items-center gap-[7px] rounded-full border px-[11px] text-[12px] transition-colors",
                        filter === c ? "border-text bg-surface-2" : "border-border bg-surface hover:border-border-strong",
                      )}
                    >
                      <span className="h-2 w-2 rounded-full" style={{ background: `var(--c-${c})` }} />
                      {CAT_LABEL[c]}
                      <span className="tabular-nums text-subtle">{bytes(summary.cats[i])}</span>
                    </button>
                  ) : null,
                )}
              </div>
            </div>
            {(summary.findings.length > 0 || summary.cacheTotal > 0) && (
              <button
                type="button"
                onClick={() => setFindingsOpen(true)}
                data-testid="space-findings"
                className="flex min-w-[176px] flex-col gap-0.5 rounded-xl bg-warn-soft px-4 py-3 text-left text-warn transition-[filter] hover:brightness-[1.03]"
              >
                <span className="flex items-center gap-1.5 text-[12px] font-medium">
                  <Sparkles size={13} />
                  {summary.findings.length} finding{summary.findings.length === 1 ? "" : "s"}
                  {summary.cacheTotal > 0 && " + caches"}
                </span>
                <span className="text-[20px] font-semibold tabular-nums text-text">
                  {bytes(fixTotal + summary.cacheTotal)} <span className="text-[12px] font-medium text-warn">to review →</span>
                </span>
              </button>
            )}
          </Card>

          <div className="grid min-h-[340px] flex-1 grid-cols-[minmax(0,1fr)_300px] gap-4">
            <div className="flex min-h-0 min-w-0 flex-col gap-2">
              <div className="flex h-[30px] items-center gap-0.5">
                <button
                  type="button"
                  aria-label="Up one level"
                  onClick={up}
                  disabled={!path || norm(path) === norm(summary.root)}
                  className="grid h-7 w-7 place-items-center rounded-[7px] text-muted transition-colors hover:bg-hover disabled:opacity-40"
                >
                  <ChevronLeft size={15} />
                </button>
                <nav aria-label="Folder path" className="flex min-w-0 items-center overflow-hidden">
                  {crumbs(summary.root, path ?? summary.root).map((c, i, all) => (
                    <span key={c.path} className="flex min-w-0 items-center">
                      {i > 0 && <ChevronRight size={12} className="shrink-0 text-subtle" />}
                      <button
                        type="button"
                        onClick={() => {
                          setPath(c.path)
                          setSel(null)
                        }}
                        className={cn(
                          "truncate rounded-md px-[7px] py-1 transition-colors hover:bg-hover",
                          i === all.length - 1 ? "font-semibold" : "text-muted",
                          i < all.length - 3 && i > 0 && "max-w-[90px]",
                        )}
                      >
                        {c.name}
                      </button>
                    </span>
                  ))}
                </nav>
                <span className="flex-1" />
                <span className="hidden whitespace-nowrap text-[11.5px] text-subtle lg:inline">Click to inspect · double-click to open</span>
              </div>
              <Card className="relative min-h-0 flex-1 overflow-hidden rounded-[14px]">
                {level ? (
                  level.children?.length ? (
                    <Treemap level={level} sel={selNode?.path ?? null} filter={filter} onSelect={(n) => setSel(n.path)} onOpen={open} />
                  ) : (
                    <div className="grid h-full place-items-center text-[13px] text-muted">This folder is empty.</div>
                  )
                ) : (
                  <Skeleton className="absolute inset-1.5 rounded-xl" />
                )}
              </Card>
              <div className="flex items-center gap-3 text-[11.5px] tabular-nums text-subtle">
                <span>
                  {count(summary.files)} files · {count(summary.dirs)} folders in {took(summary.elapsedMs)}
                </span>
                {summary.unreadable > 0 && <span>{count(summary.unreadable)} folders unreadable</span>}
                {summary.cancelled && <span className="text-warn">stopped early: sizes are partial</span>}
                <span className="ml-auto">read-only</span>
              </div>
            </div>

            <Inspector
              n={selNode}
              detail={detail}
              denom={denom}
              where={isDriveRoot && scannedMount ? scannedMount : "this folder"}
              revealLabel={reveal_label}
              onFix={runFix}
              onOpen={open}
              onReveal={reveal}
              onPick={(child) => {
                if (!selNode) return
                setPath(selNode.path)
                setSel(child.path)
              }}
            />
          </div>
        </>
      )}

      {summary && (
        <FindingsDialog
          open={findingsOpen}
          onClose={() => setFindingsOpen(false)}
          summary={summary}
          onShow={(p) => {
            setFindingsOpen(false)
            setFilter(null)
            setPath(parentOf(p))
            setSel(p)
          }}
          onFix={(f) => {
            setFindingsOpen(false)
            runFix(f)
          }}
          onCaches={() => {
            setFindingsOpen(false)
            go("disk")
          }}
        />
      )}
    </div>
  )
}

function ScanProgress({ root, p }: { root: string; p: SpaceProgress | null }) {
  return (
    <Card className="flex flex-1 flex-col items-center justify-center gap-4 px-6 py-14 text-center" data-testid="space-progress">
      <div className="relative grid h-14 w-14 place-items-center">
        <span className="absolute inset-0 animate-ping rounded-2xl bg-accent-soft" style={{ animationDuration: "1.6s" }} />
        <span className="relative grid h-12 w-12 place-items-center rounded-2xl bg-accent-soft text-accent">
          <LayoutDashboard size={22} />
        </span>
      </div>
      <div>
        <h2 className="m-0 text-[16px] font-semibold">Mapping {root}</h2>
        <p className="m-0 mt-1 text-[13px] tabular-nums text-muted">
          {p ? `${count(p.files)} files · ${bytes(p.bytes)} · ${took(p.elapsedMs)}` : "Starting…"}
        </p>
      </div>
      <p className="m-0 h-4 max-w-[520px] truncate font-mono text-[11px] text-subtle">{p?.current}</p>
      <Button icon={<X size={14} />} onClick={() => void space.cancel()}>
        Stop
      </Button>
    </Card>
  )
}

function Inspector({
  n,
  detail,
  denom,
  where,
  revealLabel,
  onFix,
  onOpen,
  onReveal,
  onPick,
}: {
  n: SpaceNode | null
  detail: SpaceNode | null
  denom: number
  where: string
  revealLabel: string
  onFix: (f: SpaceFix) => void
  onOpen: (n: SpaceNode) => void
  onReveal: (n: SpaceNode) => void
  onPick: (child: SpaceNode) => void
}) {
  if (!n)
    return (
      <Card className="rounded-2xl p-[18px]">
        <Skeleton className="h-6 w-28" />
      </Card>
    )
  const [num, unit] = bytes(n.size).split(" ")
  const pct = (n.size / Math.max(1, denom)) * 100
  const kids = (detail?.children ?? []).filter((k) => k.kind !== "more")
  const top = kids.filter((k) => k.size >= (kids[0]?.size ?? 0) * 0.002 && k.size > 0).slice(0, 5)
  const isDir = n.kind === "dir" && n.hasChildren
  const fixTitle = n.cat === "cache" ? "Can be cleaned" : "Fix available"
  const about = n.kind === "more" ? "Smaller items, folded together. Open the folder they're in to see each." : n.kind === "files" ? "Files directly in this folder, each too small to show on its own." : n.about ?? CAT_ABOUT[n.cat]
  return (
    <Card className="flex min-h-0 flex-col gap-3.5 overflow-y-auto rounded-2xl p-[18px]" aria-label="Selected item">
      <span
        className="flex h-6 items-center gap-1.5 self-start rounded-full px-2.5 text-[11.5px] font-medium"
        style={{ background: `color-mix(in oklch, var(--c-${n.cat}) 16%, var(--surface))` }}
      >
        <span className="h-[7px] w-[7px] rounded-full" style={{ background: `var(--c-${n.cat})` }} />
        {CAT_LABEL[n.cat]}
      </span>
      <div className="min-w-0">
        <div className="break-words text-[18px] font-semibold tracking-[-0.015em]">{n.name}</div>
        {n.kind !== "more" && n.kind !== "other" && <div className="mt-1 break-all font-mono text-[11px] text-subtle">{n.kind === "files" ? n.path.split("\u0000")[0] : n.path}</div>}
      </div>
      <div className="flex items-center gap-3.5">
        <div className="flex-1">
          <div className="text-[32px] font-semibold leading-none tabular-nums tracking-[-0.02em]">
            {num}
            <span className="ml-1 text-[15px] font-medium text-muted">{unit}</span>
          </div>
          <div className="mt-1 text-[12px] tabular-nums text-muted">
            {pct < 0.1 ? "<0.1" : pct.toFixed(1)}% of {where}
            {n.files > 1 && ` · ${count(n.files)} files`}
          </div>
        </div>
        <Ring value={pct / 100} size={52} stroke={5} color={`var(--c-${n.cat})`} />
      </div>
      <p className="m-0 text-[13px] leading-[1.55] text-muted">{about}</p>
      {n.fix && (
        <div
          className="flex flex-col gap-2.5 rounded-xl p-3.5"
          style={{
            background: n.cat === "cache" ? "var(--accent-soft)" : "var(--warn-soft)",
            border: `1px solid color-mix(in oklch, var(--c-${n.cat}) 30%, transparent)`,
          }}
        >
          <div className={cn("flex items-center gap-1.5 text-[12px] font-semibold", n.cat === "cache" ? "text-accent" : "text-warn")}>
            <Sparkles size={13} />
            {fixTitle}
          </div>
          <div className="text-[13px] leading-normal">{n.fix.text}</div>
          {n.fix.label && (
            <button
              type="button"
              onClick={() => onFix(n.fix!)}
              className="flex h-9 items-center justify-center gap-1.5 rounded-[9px] bg-text font-medium text-bg transition-[filter] hover:brightness-110"
              data-testid="space-fix"
            >
              {n.fix.label}
              <ArrowRight size={13} />
            </button>
          )}
        </div>
      )}
      {top.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-[11px] font-semibold tracking-[0.06em] text-subtle">LARGEST INSIDE</div>
          {top.map((k) => (
            <button key={k.path} type="button" onClick={() => onPick(k)} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-2.5 gap-y-1 rounded-md text-left">
              <span className="truncate text-[13px] hover:underline">{k.name}</span>
              <span className="text-right text-[12px] tabular-nums text-muted">{bytes(k.size)}</span>
              <span className="col-span-2 h-1 rounded-full bg-track">
                <span className="block h-full rounded-full" style={{ width: `${(k.size / Math.max(1, top[0].size)) * 100}%`, background: `var(--c-${k.cat})` }} />
              </span>
            </button>
          ))}
        </div>
      )}
      <span className="flex-1" />
      <div className="flex gap-2">
        {n.kind !== "more" && n.kind !== "other" && (
          <Button className="h-9 flex-1" onClick={() => onReveal(n)}>
            {revealLabel}
          </Button>
        )}
        {isDir && (
          <Button variant="primary" className="h-9 flex-1" onClick={() => onOpen(n)}>
            Open folder
          </Button>
        )}
      </div>
    </Card>
  )
}

function FindingsDialog({
  open,
  onClose,
  summary,
  onShow,
  onFix,
  onCaches,
}: {
  open: boolean
  onClose: () => void
  summary: SpaceSummary
  onShow: (path: string) => void
  onFix: (f: SpaceFix) => void
  onCaches: () => void
}) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="What you could get back"
      actions={
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      }
    >
      <div className="flex flex-col gap-2">
        {summary.findings.map((f) => (
          <div key={f.path} className="flex flex-col gap-1.5 rounded-xl border border-border p-3 text-text">
            <div className="flex items-baseline gap-2">
              <b className="min-w-0 flex-1 truncate font-semibold">{f.name}</b>
              <span className="tabular-nums font-semibold">{bytes(f.size)}</span>
            </div>
            <div className="truncate font-mono text-[11px] text-subtle">{f.path}</div>
            {f.fix && <p className="m-0 text-[12.5px] leading-normal text-muted">{f.fix.text}</p>}
            <div className="flex gap-2 pt-0.5">
              <Button size="sm" onClick={() => onShow(f.path)}>
                Show on map
              </Button>
              {f.fix?.label && (
                <Button size="sm" variant="primary" onClick={() => onFix(f.fix!)}>
                  {f.fix.label}
                </Button>
              )}
            </div>
          </div>
        ))}
        {summary.cacheTotal > 0 && (
          <div className="flex flex-col gap-1.5 rounded-xl border border-border p-3 text-text">
            <div className="flex items-baseline gap-2">
              <b className="flex-1 font-semibold">Rebuildable caches and build output</b>
              <span className="tabular-nums font-semibold">{bytes(summary.cacheTotal)}</span>
            </div>
            <p className="m-0 text-[12.5px] leading-normal text-muted">
              Package caches, temp folders, node_modules and build folders. Disk cleanup, Projects and AI / ML clean them with each tool's own command.
            </p>
            <div className="flex gap-2 pt-0.5">
              <Button size="sm" variant="primary" onClick={onCaches}>
                Open Disk cleanup
              </Button>
            </div>
          </div>
        )}
        <p className="m-0 pt-1 text-[12px] text-subtle">Sizes are what each item takes now; how much a fix frees depends on the fix.</p>
      </div>
    </Dialog>
  )
}
