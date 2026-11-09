// Extra rights, the way each OS does them:
// - Windows: restart elevated through UAC.
// - macOS: Full Disk Access for protected folders; admin cleanups ask for
//   your password when they run. No root relaunch.
// - Linux: admin cleanups ask through pkexec when they run.
import { useCallback, useEffect, useState } from "react"
import { createPortal } from "react-dom"
import { KeyRound, ShieldAlert, ShieldCheck } from "lucide-react"
import { api, type Privileges } from "@/lib/api"
import { cn } from "@/lib/format"
import { useStore } from "@/lib/store"
import { Badge, Button, Card, Dialog } from "./ui"

/** Privileges, refreshed when the window regains focus (Full Disk Access is
    granted in System Settings, outside the app). */
export function usePrivileges() {
  const [p, setP] = useState<Privileges | null>(null)
  const refresh = useCallback(() => void api.privilegesInfo().then(setP).catch(() => {}), [])
  useEffect(() => {
    if (!api.inTauri) return
    refresh()
    window.addEventListener("focus", refresh)
    return () => window.removeEventListener("focus", refresh)
  }, [refresh])
  return { p, refresh }
}

/** Windows: confirm, then restart elevated. Stays open if UAC is cancelled. */
export function RelaunchDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { toast } = useStore()
  const [busy, setBusy] = useState(false)
  return createPortal(
    <Dialog
      open={open}
      onClose={() => !busy && onClose()}
      title="Restart as administrator?"
      actions={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon={<ShieldCheck size={14} />}
            loading={busy}
            data-testid="confirm-relaunch"
            onClick={async () => {
              setBusy(true)
              const ok = await api.relaunchAsAdmin().catch(() => false)
              setBusy(false)
              if (!ok) toast({ kind: "info", title: "Still running without admin", detail: "The Windows prompt was cancelled." })
              onClose()
            }}
          >
            Restart as admin
          </Button>
        </>
      }
    >
      <p className="m-0 mb-2">Windows will ask for permission, then Dev Cleaner reopens where it is now.</p>
      <p className="m-0">As admin it can also clean Windows temp, update and servicing leftovers without a prompt each time, trim memory of services, and free ports held by services.</p>
    </Dialog>,
    document.body,
  )
}

/** The status-bar item: what matters on this OS, clickable where it helps. */
export function PrivilegeStatus() {
  const { p } = usePrivileges()
  const [ask, setAsk] = useState(false)
  if (!p) return null
  if (p.os === "windows") {
    return (
      <>
        <button
          type="button"
          data-testid="admin-status"
          disabled={p.isAdmin}
          onClick={() => setAsk(true)}
          title={p.isAdmin ? "Running as administrator" : "Restart as administrator"}
          className={cn("mono", p.isAdmin ? "text-ok" : "text-warn underline-offset-2 hover:underline")}
        >
          admin: {p.isAdmin ? "yes" : "no"}
        </button>
        <RelaunchDialog open={ask} onClose={() => setAsk(false)} />
      </>
    )
  }
  if (p.os === "macos") {
    if (p.fullDiskAccess === null) return null
    return (
      <button
        type="button"
        data-testid="fda-status"
        disabled={p.fullDiskAccess}
        onClick={() => void api.openFullDiskAccess()}
        title={p.fullDiskAccess ? "Full Disk Access granted" : "Open Full Disk Access settings"}
        className={cn("mono", p.fullDiskAccess ? "text-ok" : "text-warn underline-offset-2 hover:underline")}
      >
        full disk access: {p.fullDiskAccess ? "on" : "off"}
      </button>
    )
  }
  return p.pkexec ? null : <span className="mono text-warn">pkexec: missing</span>
}

/** Settings: what the app can and can't do on this OS, and how to change it. */
export function PermissionsCard() {
  const { p, refresh } = usePrivileges()
  const [ask, setAsk] = useState(false)
  if (!p) return null
  return (
    <Card data-testid="permissions">
      <div className="flex items-start justify-between gap-6 px-4 py-3.5">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-[13px] font-medium">
            <KeyRound size={14} className="text-subtle" />
            Permissions
          </div>
          {p.os === "windows" && (
            <div className="mt-1 text-[12px] leading-[1.5] text-muted">
              {p.isAdmin
                ? "Running as administrator: admin cleanups, service memory and ports held by services all work without extra prompts."
                : "Running as a normal user. Admin cleanups still work: Windows asks once per clean. Restart as administrator to skip that, and to free ports or trim memory of services."}
            </div>
          )}
          {p.os === "macos" && (
            <div className="mt-1 text-[12px] leading-[1.5] text-muted">
              macOS protects Safari, Mail and device backups behind Full Disk Access. Grant it in System Settings, then come back; the status updates by itself. Admin cleanups (system logs, Time Machine snapshots) ask for your password when they run. Dev Cleaner never runs as root.
            </div>
          )}
          {p.os === "linux" && (
            <div className="mt-1 text-[12px] leading-[1.5] text-muted">
              Admin cleanups (journal, package caches, Snap, core dumps) ask through pkexec when they run, once per clean. Dev Cleaner never runs as root.
              {p.inFlatpak && " Running inside Flatpak: tools, processes and ports are reached on the host through flatpak-spawn."}
            </div>
          )}
        </div>
        <div className="flex shrink-0 flex-col items-end gap-2">
          {p.os === "windows" &&
            (p.isAdmin ? (
              <Badge tone="ok">Administrator</Badge>
            ) : (
              <Button icon={<ShieldCheck size={14} />} onClick={() => setAsk(true)} data-testid="relaunch-admin">
                Restart as admin
              </Button>
            ))}
          {p.os === "macos" && (
            <>
              {p.fullDiskAccess === true && <Badge tone="ok">Full Disk Access on</Badge>}
              {p.fullDiskAccess === false && <Badge tone="warn">Full Disk Access off</Badge>}
              {p.fullDiskAccess !== true && (
                <Button icon={<ShieldAlert size={14} />} onClick={() => void api.openFullDiskAccess().then(() => setTimeout(refresh, 1500))}>
                  Open Full Disk Access
                </Button>
              )}
            </>
          )}
          {p.os === "linux" && (p.pkexec ? <Badge tone="ok">pkexec ready</Badge> : <Badge tone="warn">pkexec not found</Badge>)}
        </div>
      </div>
      {p.os === "windows" && <RelaunchDialog open={ask} onClose={() => setAsk(false)} />}
    </Card>
  )
}

/** A banner on pages whose actions need admin rights. Windows: restart as
    admin once instead of a UAC prompt per change. macOS and Linux: says a
    password prompt will appear (Dev Cleaner never runs as root there). */
export function AdminBanner({ what, testid = "admin-banner" }: { what: string; testid?: string }) {
  const { p } = usePrivileges()
  const [ask, setAsk] = useState(false)
  if (!p || p.isAdmin) return null
  if (p.os === "linux" && p.inFlatpak) return null
  const text =
    p.os === "windows"
      ? `${what} need administrator rights. Windows asks each time; restart as administrator to skip the prompts.`
      : p.os === "macos"
        ? `${what} need administrator rights. macOS asks for your password each time.`
        : `${what} need administrator rights. You'll be asked through pkexec each time.`
  return (
    <div data-testid={testid} role="note" className="flex items-center gap-3 rounded-xl border border-warn/30 bg-warn-soft px-3.5 py-2.5">
      <ShieldAlert size={16} className="shrink-0 text-warn" />
      <p className="m-0 min-w-0 flex-1 text-[12.5px] leading-[1.45] text-text">{text}</p>
      {p.os === "windows" && p.canRelaunch && (
        <>
          <Button size="sm" icon={<ShieldCheck size={13} />} onClick={() => setAsk(true)} data-testid="banner-relaunch" className="shrink-0">
            Restart as admin
          </Button>
          <RelaunchDialog open={ask} onClose={() => setAsk(false)} />
        </>
      )}
    </div>
  )
}
