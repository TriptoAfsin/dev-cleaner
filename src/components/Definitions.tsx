// Settings: which definitions are in use (what the app knows about caches,
// project folders, processes, ports, model folders and config files), where
// they came from, the folder a newer set is read from, and a manual reload.
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
    if (api.inTauri) load()
  }, [load])
  if (!info) return null

  const c = info.counts
  const folder = info.source === "folder"
  const reload = async () => {
    setBusy(true)
    const before = info.version
    try {
      const next = await api.reloadDefinitions()
      setInfo(next)
      if (next.error) toast({ kind: "error", title: "Folder copy not used", detail: next.error })
      else
        toast({
          kind: "ok",
          title: next.version === before ? "Definitions are up to date" : `Definitions ${next.version} loaded`,
          detail: next.source === "folder" ? "From the definitions folder. Rescanning caches." : "Built-in set. Rescanning caches.",
        })
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
            What Dev Cleaner knows about caches, project folders, processes, ports, AI model folders and config files. Kept as data, so it can be updated without a new app release.
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
            <>Read from the definitions folder below{info.builtinVersion !== info.version && <span className="text-subtle"> · built-in set is {info.builtinVersion}</span>}</>
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
            {info.folderExists ? "A complete, valid set here (index.json and its files) is used instead of the built-in one." : "Doesn't exist yet. A set placed here is used instead of the built-in one."}
          </div>
        </Line>
        {info.error && (
          <Line label="Problem" testid="defs-error">
            <span className="text-warn">The folder copy wasn't used: {info.error}. Using the built-in set.</span>
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
