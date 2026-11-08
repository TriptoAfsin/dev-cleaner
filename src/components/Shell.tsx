import { motion } from "motion/react"
import { Activity, ArchiveRestore, BrainCircuit, Trash2, Cable, FolderSearch, Gauge as GaugeIcon, HardDrive, MemoryStick, Monitor, Settings } from "lucide-react"
import { bytes, cn } from "@/lib/format"
import { useStore } from "@/lib/store"
import type { ReactNode } from "react"

export type Page = "overview" | "disk" | "junk" | "projects" | "models" | "configs" | "memory" | "processes" | "ports" | "system" | "settings"

const NAV: { id: Page; label: string; icon: ReactNode }[] = [
  { id: "overview", label: "Overview", icon: <GaugeIcon size={15} /> },
  { id: "disk", label: "Disk cleanup", icon: <HardDrive size={15} /> },
  { id: "junk", label: "System junk", icon: <Trash2 size={15} /> },
  { id: "projects", label: "Projects", icon: <FolderSearch size={15} /> },
  { id: "models", label: "AI models", icon: <BrainCircuit size={15} /> },
  { id: "configs", label: "Configs", icon: <ArchiveRestore size={15} /> },
  { id: "memory", label: "Memory", icon: <MemoryStick size={15} /> },
  { id: "processes", label: "Processes", icon: <Activity size={15} /> },
  { id: "ports", label: "Ports", icon: <Cable size={15} /> },
  { id: "system", label: "System", icon: <Monitor size={15} /> },
]

export function Sidebar({ page, go }: { page: Page; go: (p: Page) => void }) {
  const { caches, scanning, portsInUse } = useStore()
  const reclaim = caches.filter((c) => !c.junk).reduce((n, c) => n + c.size, 0)
  const junkBytes = caches.filter((c) => c.junk && c.risk !== "data").reduce((n, c) => n + c.size, 0)
  const badge: Partial<Record<Page, ReactNode>> = {
    disk: scanning && !reclaim ? <span className="pulse-dot">…</span> : reclaim ? bytes(reclaim, 0) : null,
    ports: portsInUse.length ? `${portsInUse.length} open` : null,
    junk: junkBytes ? bytes(junkBytes, 0) : null,
  }
  const item = (n: (typeof NAV)[number]) => {
    const on = n.id === page
    return (
      <button
        key={n.id}
        type="button"
        data-testid={`nav-${n.id}`}
        aria-current={on ? "page" : undefined}
        onClick={() => go(n.id)}
        className={cn(
          "relative flex h-8 w-full items-center gap-2.5 rounded-[7px] px-2.5 text-left text-[13px] transition-colors duration-150",
          on ? "font-medium text-accent" : "text-muted hover:bg-hover hover:text-text",
        )}
      >
        {on && (
          <motion.span
            layoutId="nav-active"
            className="absolute inset-0 rounded-[7px] bg-accent-soft"
            transition={{ type: "spring", stiffness: 500, damping: 40 }}
          />
        )}
        <span className="relative">{n.icon}</span>
        <span className="relative flex-1">{n.label}</span>
        {badge[n.id] && <span className={cn("mono relative text-[11px]", on ? "text-accent" : "text-subtle")}>{badge[n.id]}</span>}
      </button>
    )
  }
  return (
    <nav aria-label="Sections" className="flex w-[196px] shrink-0 flex-col gap-0.5 border-r border-border px-2.5 py-3">
      {NAV.map(item)}
      <div className="flex-1" />
      {item({ id: "settings", label: "Settings", icon: <Settings size={15} /> })}
    </nav>
  )
}

export function StatusBar() {
  const { activity, stats, admin, portsInUse } = useStore()
  const color = { idle: "text-accent", busy: "text-accent", ok: "text-ok", error: "text-danger" }[activity.kind]
  return (
    <footer data-testid="statusbar" className="mono flex h-7 shrink-0 items-center gap-4 overflow-hidden whitespace-nowrap border-t border-border bg-surface-2 px-3.5 text-[11.5px] text-muted">
      <span className={cn("flex min-w-0 items-center gap-1.5 truncate", color)} data-testid="activity">
        <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full bg-current", activity.kind === "busy" && "pulse-dot")} />
        <span className="truncate">{activity.text}</span>
      </span>
      <span className="ml-auto" />
      {stats && (
        <>
          <span>cpu {stats.cpu.toFixed(0)}%</span>
          <span>
            mem {bytes(stats.memUsed)} / {bytes(stats.memTotal, 0)}
          </span>
          {stats.disk && (
            <span>
              {stats.disk.mount} {bytes(stats.disk.free, 0)} free
            </span>
          )}
        </>
      )}
      {portsInUse.length > 0 && <span className="hidden xl:inline">{portsInUse.map((p) => `:${p}`).join(" ")}</span>}
      <span className={admin ? "text-ok" : "text-warn"}>admin: {admin ? "yes" : "no"}</span>
    </footer>
  )
}
