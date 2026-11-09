// About Dev Cleaner: version, how it was installed, links, and update checks
// against GitHub releases. Shown as a Settings card and as a dialog from the
// title bar; the title-bar button gets a dot when an update is out.
import { Markdown } from "./Markdown"
import { useEffect, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { ArrowUpRight, Check, Copy, ExternalLink, Info, RefreshCw, ShieldAlert } from "lucide-react"
import { api, type AppInfo } from "@/lib/api"
import { cn } from "@/lib/format"
import { useSettings } from "@/lib/settings"
import { checkForUpdates, useUpdates } from "@/lib/updates"
import { AppMark } from "./TitleBar"
import { Badge, Button, Card, Dialog, Switch, Tooltip } from "./ui"

const VIA: Record<string, string> = {
  scoop: "Scoop",
  "winget-or-installer": "Installer or WinGet",
  portable: "Portable zip",
  homebrew: "Homebrew",
  flatpak: "Flatpak",
  appimage: "AppImage",
  "deb-or-rpm": ".deb or .rpm package",
  "app-bundle": "App bundle",
  dev: "Development build",
}

const open = (url: string) => void openUrl(url).catch(() => {})

function useAppInfo() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  useEffect(() => {
    if (api.inTauri) void api.appInfo().then(setInfo).catch(() => {})
  }, [])
  return info
}

function when(iso: string): string {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? "" : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" })
}

function CopyCommand({ cmd }: { cmd: string }) {
  const [done, setDone] = useState(false)
  return (
    <div className="flex items-center gap-2 rounded-lg border border-border bg-surface-2 py-1 pl-3 pr-1">
      <code className="mono min-w-0 flex-1 truncate text-[12px] text-text">{cmd}</code>
      <Button
        size="sm"
        icon={done ? <Check size={13} /> : <Copy size={13} />}
        onClick={() => {
          void navigator.clipboard.writeText(cmd).then(() => {
            setDone(true)
            setTimeout(() => setDone(false), 1500)
          })
        }}
      >
        {done ? "Copied" : "Copy"}
      </Button>
    </div>
  )
}

function UpdateStatus() {
  const u = useUpdates()
  const app = useAppInfo()
  const checking = u.status === "checking"
  return (
    <div className="flex flex-col gap-2.5" data-testid="update-status">
      <div className="flex min-h-[52px] items-center justify-between gap-4 rounded-xl border border-border bg-surface-2 py-2 pl-3.5 pr-2">
        <div className="min-w-0 text-[12.5px] leading-[1.4] text-muted" aria-live="polite">
          {checking && "Checking GitHub releases…"}
          {u.status === "idle" && "Not checked yet this session."}
          {u.status === "error" && <span className="text-danger">{u.error}</span>}
          {u.status === "done" && u.info && !u.info.newer && (
            <span className="flex items-center gap-1.5 text-ok">
              <Check size={14} className="shrink-0" /> You have the latest version.
            </span>
          )}
          {u.status === "done" && u.info?.newer && <span className="font-medium text-accent">Version {u.info.latest} is available.</span>}
        </div>
        <Button size="sm" icon={<RefreshCw size={13} className={cn(checking && "spin")} />} disabled={checking} onClick={() => void checkForUpdates()} data-testid="check-updates" className="shrink-0">
          {checking ? "Checking" : "Check for updates"}
        </Button>
      </div>
      {u.status === "done" && u.info?.newer && (
        <div className="flex flex-col gap-2.5 rounded-xl border border-accent/30 bg-accent-soft/40 p-3.5" data-testid="update-available">
          <div className="flex items-center gap-2 text-[13px] font-medium text-text">
            Dev Cleaner {u.info.latest}
            {u.info.publishedAt && <span className="text-[12px] font-normal text-subtle">· released {when(u.info.publishedAt)}</span>}
          </div>
          {u.info.notes && (
            <div className="max-h-56 min-w-0 overflow-y-auto pr-1" data-testid="update-notes">
              <Markdown source={u.info.notes} />
            </div>
          )}
          {app?.upgradeCommand && (
            <div className="flex flex-col gap-1">
              <span className="text-[12px] text-subtle">You installed with {VIA[app.installedVia] ?? app.installedVia}. Update with:</span>
              <CopyCommand cmd={app.upgradeCommand} />
            </div>
          )}
          <div>
            <Button variant="primary" icon={<ExternalLink size={14} />} onClick={() => open(u.info!.url)}>
              Open release page
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

function AboutBody({ withToggle }: { withToggle?: boolean }) {
  const app = useAppInfo()
  const { settings, set } = useSettings()
  const repo = app?.repo ?? "https://github.com/TriptoAfsin/dev-cleaner"
  const links: [string, string][] = [
    ["GitHub", repo],
    ["Releases", `${repo}/releases`],
    ["Changelog", `${repo}/blob/main/CHANGELOG.md`],
    ["Report an issue", `${repo}/issues/new`],
    ["License", `${repo}/blob/main/LICENSE`],
  ]
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <span className="shrink-0">
          <AppMark size={56} />
        </span>
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex items-center gap-2 text-[16px] font-semibold leading-none text-text">
            Dev Cleaner
            {app && (
              <Badge tone="accent">
                <span data-testid="app-version">v{app.version}</span>
              </Badge>
            )}
          </div>
          <div className="truncate text-[12.5px] text-muted">Developer caches, memory, processes and ports, cleaned.</div>
          {app && (
            <div className="mono truncate text-[11px] text-subtle">
              {app.os} · {app.arch} · {VIA[app.installedVia] ?? app.installedVia} · MIT
            </div>
          )}
        </div>
      </div>
      <UpdateStatus />
      <div className="flex gap-2.5 rounded-lg border border-border bg-surface-2 px-3 py-2.5" data-testid="terms">
        <ShieldAlert size={15} className="mt-px shrink-0 text-warn" />
        <p className="m-0 text-[12px] leading-[1.5] text-muted">
          <span className="font-medium text-text">Use at your own risk.</span> Dev Cleaner is provided as is, without warranty. Deletes are permanent, and the authors aren't liable for lost data or any other damage. By using it you accept the{" "}
          <button type="button" onClick={() => open(`${repo}/blob/main/LICENSE`)} className="text-accent hover:underline">
            MIT License
          </button>{" "}
          and these{" "}
          <button type="button" onClick={() => open(`${repo}#terms-of-use-and-disclaimer`)} className="text-accent hover:underline">
            terms
          </button>
          .
        </p>
      </div>
      {withToggle && (
        <div className="flex items-center justify-between gap-6">
          <div>
            <div className="text-[13px] font-medium text-text">Check for updates automatically</div>
            <div className="mt-0.5 text-[12px] text-muted">A few seconds after launch, then once a day: new versions, plus new definitions (cache rules), which are applied without an app update. Plain requests to GitHub; nothing about you is sent.</div>
          </div>
          <Switch checked={settings.checkUpdates} onChange={(v) => set("checkUpdates", v)} label="Check for updates automatically" />
        </div>
      )}
      <nav aria-label="Project links" className="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-border pt-3">
        {links.map(([label, url]) => (
          <button
            key={label}
            type="button"
            onClick={() => open(url)}
            className="group flex items-center gap-1 text-[12.5px] text-muted transition-colors hover:text-text"
          >
            {label}
            <ArrowUpRight size={12} className="text-subtle transition-transform duration-150 group-hover:-translate-y-px group-hover:translate-x-px group-hover:text-text" />
          </button>
        ))}
      </nav>
    </div>
  )
}

export function AboutCard() {
  return (
    <Card data-testid="about" className="px-4 py-4">
      <AboutBody withToggle />
    </Card>
  )
}

/** Title-bar button: opens About; a dot marks an available update. */
export function AboutButton() {
  const [show, setShow] = useState(false)
  const u = useUpdates()
  const newer = !!u.info?.newer
  useEffect(() => {
    const f = () => setShow(true)
    window.addEventListener("dc-about", f)
    return () => window.removeEventListener("dc-about", f)
  }, [])
  return (
    <>
      <Tooltip text={newer ? `Update available: ${u.info!.latest}` : "About & updates"} side="bottom">
        <button
          type="button"
          aria-label={newer ? "About, update available" : "About and updates"}
          data-testid="about-button"
          onClick={() => setShow(true)}
          className="relative grid h-7 w-7 place-items-center rounded-full border border-border-strong bg-surface text-muted transition-transform duration-150 hover:border-subtle/60 hover:text-text active:scale-95"
        >
          <Info size={14} />
          {newer && <span className="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-accent ring-2 ring-bg" data-testid="update-dot" />}
        </button>
      </Tooltip>
      <Dialog
        open={show}
        onClose={() => setShow(false)}
        title="About"
        actions={
          <Button variant="primary" onClick={() => setShow(false)}>
            Close
          </Button>
        }
      >
        <AboutBody />
      </Dialog>
    </>
  )
}
