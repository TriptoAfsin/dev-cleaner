// Settings: which definitions are in use (what the app knows about caches,
// project folders, processes, ports, model folders and config files), where
// they came from, and Refresh: download the newest set from GitHub (or, if
// that fails, read the folder again).
import { useCallback, useEffect, useState } from "react"
import { BookOpen, FolderOpen, RefreshCw } from "lucide-react"
import { api, type DefinitionsInfo } from "@/lib/api"
import { cn } from "@/lib/format"
import { useStore } from "@/lib/store"
import { Badge, Button, Card } from "./ui"

function at(secs: number): string {
  if (!secs) return "—"
  return new Date(secs * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })
}

function Line({ label, children, testid }: { label: string; children: React.ReactNode; testid?: string }) {
  return (
    <div className="grid grid-cols-[96px_minmax(0,1fr)] items-baseline gap-3 py-1.5">
      <dt className="text-[12px] text-subtle">{label}</dt>
      <dd className="m-0 min-w-0 text-[12.5px] text-text" data-testid={testid}>
        {children}
      </dd>
    </div>
  )
}

export function DefinitionsCard() {
  const { toast, scan } = useStore()
  const [info, setInfo] = useState<DefinitionsInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const load = useCallback(() => void api.definitionsInfo().then(setInfo).catch(() => {}), [])
  useEffect(() => {
    if (!api.inTauri) return
    load()
    // The daily background check downloaded a newer set.
    window.addEventListener("dc-definitions", load)
    return () => window.removeEventListener("dc-definitions", load)
  }, [load])
  if (!info) return null

  const c = info.counts
  const folder = info.source === "folder"
  // Download the newest set; whatever happens, read the folder again too, so
  // a set placed there by hand is picked up.
  const reload = async () => {
    setBusy(true)
    const before = info.version
    try {
      let fetched: Awaited<ReturnType<typeof api.updateDefinitions>> | null = null
      let failed = ""
      try {
        fetched = await api.updateDefinitions()
      } catch (e) {
        failed = String(e)
      }
      const next = fetched?.outcome === "updated" ? fetched.info : await api.reloadDefinitions()
      setInfo(next)
      if (next.error) toast({ kind: "error", title: "Folder copy not used", detail: next.error })
      else if (fetched?.outcome === "updated") toast({ kind: "ok", title: `Definitions updated to ${next.version}`, detail: "Downloaded from GitHub. Rescanning caches." })
      else if (fetched?.outcome === "needs-app")
        toast({ kind: "info", title: `Definitions ${fetched.version} need Dev Cleaner ${fetched.minApp}`, detail: "Update the app to get them. The current set stays in use." })
      else if (next.version !== before) toast({ kind: "ok", title: `Definitions ${next.version} loaded`, detail: "From the definitions folder. Rescanning caches." })
      else if (failed) toast({ kind: "error", title: "Couldn't check for new definitions", detail: `${failed} The current set stays in use.` })
      else toast({ kind: "ok", title: "Definitions are up to date", detail: `${next.version} is the newest set.` })
      void scan()
    } catch (e) {
      toast({ kind: "error", title: "Couldn't reload definitions", detail: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card data-testid="definitions" className="px-4 py-3.5">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-[13px] font-medium">
            <BookOpen size={14} className="text-subtle" />
            Definitions
          </div>
          <div className="mt-1 text-[12px] leading-[1.5] text-muted">
            What Dev Cleaner knows about caches, project folders, processes, ports, AI model folders and config files. Kept as data: Refresh downloads the newest set from GitHub, no app update needed (it also checks daily when update checks are on).
          </div>
        </div>
        <Button icon={<RefreshCw size={14} className={cn(busy && "spin")} />} disabled={busy} onClick={() => void reload()} data-testid="reload-definitions">
          Refresh
        </Button>
      </div>
      <dl className="m-0 mt-2.5 divide-y divide-border border-t border-border">
        <Line label="Version" testid="defs-version">
          <span className="mono">{info.version}</span>
          <span className="ml-2 align-middle">
            <Badge tone={folder ? "accent" : "muted"}>{folder ? "From folder" : "Built-in"}</Badge>
          </span>
        </Line>
        <Line label="Source" testid="defs-source">
          {folder ? (
            <>Downloaded, kept in the folder below{info.builtinVersion !== info.version && <span className="text-subtle"> · built-in set is {info.builtinVersion}</span>}</>
          ) : (
            <>Built into this version of Dev Cleaner</>
          )}
        </Line>
        <Line label="Folder" testid="defs-folder">
          <div className="flex items-center gap-2">
            <span className="mono min-w-0 flex-1 truncate text-[11.5px]" title={info.folder ?? ""}>
              {info.folder ?? "—"}
            </span>
            <Button size="sm" icon={<FolderOpen size={13} />} onClick={() => void api.openDefinitionsFolder().then(load).catch(() => {})}>
              Open
            </Button>
          </div>
          <div className="mt-0.5 text-[11.5px] text-subtle">
            {info.folderExists
              ? "Downloaded sets live here. A complete, valid set here is used when it's newer than the built-in one."
              : "Created on the first download. A set placed here by hand is also used when it's newer than the built-in one."}
          </div>
        </Line>
        {info.error && (
          <Line label="Problem" testid="defs-error">
            <span className="text-warn">The folder copy wasn't used: {info.error}. Using the built-in set.</span>
          </Line>
        )}
        {info.skipped.length > 0 && (
          <Line label="Skipped" testid="defs-skipped">
            <span className="text-muted" title={info.skipped.join(String.fromCharCode(10))}>
              {info.skipped.length} rule{info.skipped.length === 1 ? "" : "s"} need{info.skipped.length === 1 ? "s" : ""} a newer Dev Cleaner and {info.skipped.length === 1 ? "is" : "are"} left out. Everything else is in use.
            </span>
          </Line>
        )}
        <Line label="Contains">
          {c.caches} caches · {c.junk} junk locations · {c.projects} project rules · {c.processes} processes · {c.ports} port hints · {c.models} model folders · {c.configs} config files
        </Line>
        <Line label="Loaded">{at(info.loadedAt)}</Line>
      </dl>
    </Card>
  )
}
