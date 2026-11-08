// About Dev Cleaner: version, how it was installed, links, and update checks
// against GitHub releases. Shown as a Settings card and as a dialog from the
// title bar; the title-bar button gets a dot when an update is out.
import { useEffect, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Check, Copy, ExternalLink, Info, RefreshCw } from "lucide-react"
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
    <div className="flex flex-col gap-3" data-testid="update-status">
      <div className="flex items-center justify-between gap-4">
        <div className="text-[12.5px] text-muted" aria-live="polite">
          {checking && "Checking GitHub releases…"}
          {u.status === "idle" && "Not checked yet this session."}
          {u.status === "error" && <span className="text-danger">{u.error}</span>}
          {u.status === "done" && u.info && !u.info.newer && (
            <span className="flex items-center gap-1.5 text-ok">
              <Check size={14} /> You have the latest version ({u.info.current}).
            </span>
          )}
          {u.status === "done" && u.info?.newer && <span className="font-medium text-accent">Version {u.info.latest} is available.</span>}
        </div>
        <Button icon={<RefreshCw size={14} className={cn(checking && "spin")} />} disabled={checking} onClick={() => void checkForUpdates()} data-testid="check-updates">
          Check for updates
        </Button>
      </div>
      {u.status === "done" && u.info?.newer && (
        <div className="flex flex-col gap-2.5 rounded-xl border border-accent/30 bg-accent-soft/40 p-3.5" data-testid="update-available">
          <div className="flex items-center gap-2 text-[13px] font-medium text-text">
            Dev Cleaner {u.info.latest}
            {u.info.publishedAt && <span className="text-[12px] font-normal text-subtle">· released {when(u.info.publishedAt)}</span>}
          </div>
          {u.info.notes && <div className="max-h-40 overflow-y-auto whitespace-pre-wrap text-[12px] leading-[1.55] text-muted">{u.info.notes}</div>}
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
  ]
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3.5">
        <AppMark size={44} />
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-[15px] font-semibold text-text">
            Dev Cleaner
            {app && (
              <Badge tone="accent">
                <span data-testid="app-version">v{app.version}</span>
              </Badge>
            )}
          </div>
          <div className="mt-0.5 text-[12.5px] text-muted">Reclaim disk and memory from developer caches, processes and ports.</div>
          {app && (
            <div className="mono mt-1 text-[11.5px] text-subtle">
              {app.os} · {app.arch} · {VIA[app.installedVia] ?? app.installedVia} · MIT licence
            </div>
          )}
        </div>
      </div>
      <UpdateStatus />
      {withToggle && (
        <div className="flex items-center justify-between gap-6">
          <div>
            <div className="text-[13px] font-medium text-text">Check for updates automatically</div>
            <div className="mt-0.5 text-[12px] text-muted">A few seconds after launch, then once a day. One plain request to GitHub, nothing about you is sent.</div>
          </div>
          <Switch checked={settings.checkUpdates} onChange={(v) => set("checkUpdates", v)} label="Check for updates automatically" />
        </div>
      )}
      <div className="flex flex-wrap gap-1.5">
        {links.map(([label, url]) => (
          <Button key={label} size="sm" icon={<ExternalLink size={13} />} onClick={() => open(url)}>
            {label}
          </Button>
        ))}
      </div>
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
