import { motion } from "motion/react"
import { Activity, ArchiveRestore, LayoutDashboard, Zap, BrainCircuit, ChevronDown, Trash2, Cable, FolderSearch, Gauge as GaugeIcon, HardDrive, MemoryStick, Monitor, Rocket, ServerCog, Settings } from "lucide-react"
import { bytes, cn } from "@/lib/format"
import { sectionOf } from "@/lib/sections"
import { useStore } from "@/lib/store"
import { PrivilegeStatus } from "./Privileges"
import { useEffect, useState, type ReactNode } from "react"

export type Page = "overview" | "space" | "tune" | "disk" | "junk" | "projects" | "models" | "configs" | "memory" | "processes" | "startup" | "services" | "ports" | "system" | "settings"

type NavItem = { id: Page; label: string; icon: ReactNode }
const item = (id: Page, label: string, icon: ReactNode): NavItem => ({ id, label, icon })

const TOP: NavItem[] = [item("overview", "Overview", <GaugeIcon size={15} />), item("memory", "Memory", <MemoryStick size={15} />)]
const GROUPS: { id: string; label: string; items: NavItem[] }[] = [
  {
    id: "clean",
    label: "Clean up",
    items: [
      item("space", "Space", <LayoutDashboard size={15} />),
      item("disk", "Disk cleanup", <HardDrive size={15} />),
      item("junk", "System junk", <Trash2 size={15} />),
      item("models", "AI / ML", <BrainCircuit size={15} />),
      item("projects", "Projects", <FolderSearch size={15} />),
    ],
  },
  {
    id: "manage",
    label: "Manage",
    items: [
      item("processes", "Processes", <Activity size={15} />),
      item("ports", "Ports", <Cable size={15} />),
      item("startup", "Startup", <Rocket size={15} />),
      item("services", "Services", <ServerCog size={15} />),
      item("configs", "Configs", <ArchiveRestore size={15} />),
    ],
  },
  { id: "machine", label: "Machine", items: [item("system", "System", <Monitor size={15} />), item("tune", "Responsiveness", <Zap size={15} />)] },
]

/** Sidebar order, top to bottom. Ctrl/Cmd + 1..9 follows it. */
export const ORDER: Page[] = [...TOP, ...GROUPS.flatMap((g) => g.items)].map((n) => n.id).concat("settings")

const FOLD_KEY = "dc-nav-folded"
function useFolded() {
  const [folded, setFolded] = useState<Set<string>>(() => {
    try {
      const v = JSON.parse(localStorage.getItem(FOLD_KEY) ?? "[]")
      return new Set(Array.isArray(v) ? v : [])
    } catch {
      return new Set()
    }
  })
  const set = (id: string, fold: boolean) =>
    setFolded((prev) => {
      if (prev.has(id) === fold) return prev
      const next = new Set(prev)
      if (fold) next.add(id)
      else next.delete(id)
      try {
        localStorage.setItem(FOLD_KEY, JSON.stringify([...next]))
      } catch {
        /* folding just won't be remembered */
      }
      return next
    })
  return [folded, set] as const
}

export function Sidebar({ page, go }: { page: Page; go: (p: Page) => void }) {
  const { caches, scanning, stats } = useStore()
  const [folded, setFold] = useFolded()
  // Opening a page inside a folded section (shortcut, link, insight) unfolds it.
  useEffect(() => {
    const g = GROUPS.find((g) => g.items.some((n) => n.id === page))
    if (g && folded.has(g.id)) setFold(g.id, false)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page])

  const reclaim = caches.filter((c) => sectionOf(c) === "disk").reduce((n, c) => n + c.size, 0)
  const aiBytes = caches.filter((c) => (sectionOf(c) === "harnesses" || sectionOf(c) === "mltools") && c.risk !== "data").reduce((n, c) => n + c.size, 0)
  const junkBytes = caches.filter((c) => c.junk && c.risk !== "data").reduce((n, c) => n + c.size, 0)
  const badge: Partial<Record<Page, ReactNode>> = {
    memory: stats ? <span title={`${bytes(stats.memAvailable)} of ${bytes(stats.memTotal, 0)} RAM free`}>{bytes(stats.memAvailable, 0)} free</span> : null,
    disk: scanning && !reclaim ? <span className="pulse-dot">…</span> : reclaim ? bytes(reclaim, 0) : null,
    junk: junkBytes ? bytes(junkBytes, 0) : null,
    models: aiBytes ? bytes(aiBytes, 0) : null,
  }
  const row = (n: NavItem) => {
    const on = n.id === page
    return (
      <button
        key={n.id}
        type="button"
        data-testid={`nav-${n.id}`}
        aria-current={on ? "page" : undefined}
        onClick={() => go(n.id)}
        className={cn(
          "relative flex h-8 w-full shrink-0 items-center gap-2.5 rounded-[7px] px-2.5 text-left text-[13px] transition-colors duration-150",
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
        <span className="relative min-w-0 flex-1 truncate">{n.label}</span>
        {badge[n.id] && <span className={cn("mono relative shrink-0 text-[11px]", on ? "text-accent" : "text-subtle")}>{badge[n.id]}</span>}
      </button>
    )
  }
  return (
    <nav aria-label="Sections" className="flex w-[208px] shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border px-2.5 py-3">
      {TOP.map(row)}
      {GROUPS.map((g) => {
        const open = !folded.has(g.id)
        return (
          <div key={g.id} className="mt-3 flex flex-col gap-0.5" data-testid={`nav-group-${g.id}`}>
            <button
              type="button"
              aria-expanded={open}
              onClick={() => setFold(g.id, open)}
              className="flex h-[22px] w-full items-center gap-1.5 rounded-[5px] px-2.5 text-left text-[10.5px] font-medium tracking-[0.09em] text-subtle uppercase transition-colors duration-150 hover:text-muted"
            >
              {g.label}
              <ChevronDown size={12} className={cn("ml-auto transition-transform duration-200", !open && "-rotate-90")} />
            </button>
            {open && g.items.map(row)}
          </div>
        )
      })}
      <div className="flex-1" />
      {row({ id: "settings", label: "Settings", icon: <Settings size={15} /> })}
    </nav>
  )
}

export function StatusBar() {
  const { activity, stats, portsInUse } = useStore()
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
      <PrivilegeStatus />
    </footer>
  )
}
