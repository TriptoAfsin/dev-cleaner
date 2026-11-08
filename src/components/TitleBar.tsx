import { useEffect, useRef, useState, type ReactNode } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { AnimatePresence, motion } from "motion/react"
import { Palette } from "lucide-react"
import { api } from "@/lib/api"
import { cn } from "@/lib/format"
import { ACCENTS, useSettings } from "@/lib/settings"
import { useStore } from "@/lib/store"
import { Tooltip } from "./ui"
import { AboutButton } from "./About"

const DRAG_THRESHOLD = 4

export function AppMark({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 168 168" aria-hidden>
      <rect x="4" y="4" width="160" height="160" rx="38" fill="var(--accent)" />
      <path d="M84 38 A46 46 0 1 1 41 101" fill="none" stroke="var(--on-accent)" strokeWidth="16" strokeLinecap="round" />
      <path d="M66 85l12 12 24-27" fill="none" stroke="var(--on-accent)" strokeWidth="13" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

export function TitleBar() {
  const { platform } = useStore()
  const mac = platform === "macos"
  const win = api.inTauri ? getCurrentWindow() : null
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    if (!win) return
    const check = () => void win.isMaximized().then(setMaximized).catch(() => {})
    check()
    const un = win.onResized(check)
    return () => void un.then((f) => f())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const isDragSurface = (ev: React.MouseEvent) => !(ev.target as HTMLElement).closest("button, input, a, [role='dialog'], [data-no-drag]")

  // The drag starts only once the pointer has moved a few pixels, like a
  // native title bar. Starting on mousedown hands the press to the OS, the
  // page never sees the mouseup, and the next click reads as a double-click.
  // Dragging through the OS keeps Aero Snap, and dragging a maximized window
  // restores it.
  const onMouseDown = (ev: React.MouseEvent) => {
    if (!win || ev.button !== 0 || !isDragSurface(ev)) return
    const x0 = ev.screenX
    const y0 = ev.screenY
    const stop = () => {
      window.removeEventListener("mousemove", move)
      window.removeEventListener("mouseup", stop)
      window.removeEventListener("blur", stop)
    }
    const move = (m: MouseEvent) => {
      if ((m.buttons & 1) === 0) return stop()
      if (Math.abs(m.screenX - x0) + Math.abs(m.screenY - y0) < DRAG_THRESHOLD) return
      stop()
      void win.startDragging()
    }
    window.addEventListener("mousemove", move)
    window.addEventListener("mouseup", stop)
    window.addEventListener("blur", stop)
  }
  const onDoubleClick = (ev: React.MouseEvent) => {
    if (win && isDragSurface(ev)) void win.toggleMaximize()
  }

  return (
    <header
      onMouseDown={onMouseDown}
      onDoubleClick={onDoubleClick}
      data-testid="titlebar"
      className={cn("relative z-30 flex h-[38px] shrink-0 select-none items-center border-b border-border bg-bg", mac ? "pl-[78px]" : "pl-3.5")}
    >
      <div className="pointer-events-none flex items-center gap-2">
        <AppMark />
        <span className="text-[12.5px] font-semibold">Dev Cleaner</span>
      </div>
      <div className="h-full flex-1" />
      <div className="flex items-center gap-1.5 pr-2">
        <AboutButton />
        <AccentPicker />
        <ThemeToggle />
      </div>
      {!mac && win && (
        <div className="flex h-full items-center">
          <WinButton label="Minimize" onClick={() => void win.minimize()}>
            <path d="M0 5.5h10" stroke="currentColor" />
          </WinButton>
          <WinButton label={maximized ? "Restore" : "Maximize"} onClick={() => void win.toggleMaximize()}>
            {maximized ? (
              <>
                <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" />
                <path d="M2.5 2.5V0.5h7v7h-2" fill="none" stroke="currentColor" />
              </>
            ) : (
              <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
            )}
          </WinButton>
          <WinButton label="Close" danger onClick={() => void win.close()}>
            <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" />
          </WinButton>
        </div>
      )}
    </header>
  )
}

function WinButton({ label, onClick, danger, children }: { label: string; onClick: () => void; danger?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "grid h-full w-[46px] place-items-center text-muted transition-colors duration-150",
        danger ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-hover hover:text-text",
      )}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
        {children}
      </svg>
    </button>
  )
}

function ThemeToggle() {
  const { dark, toggleTheme } = useSettings()
  return (
    <Tooltip text={dark ? "Light mode" : "Dark mode"} side="bottom">
      <button
        type="button"
        aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
        data-testid="theme-toggle"
        onClick={toggleTheme}
        className="grid h-7 w-7 place-items-center rounded-full border border-border-strong bg-surface transition-transform duration-150 hover:border-subtle/60 active:scale-95"
      >
        <motion.svg width="14" height="14" viewBox="0 0 16 16" aria-hidden animate={{ rotate: dark ? 180 : 0 }} transition={{ duration: 0.45, ease: [0.65, 0, 0.35, 1] }}>
          <circle cx="8" cy="8" r="6.5" fill="none" stroke="var(--text)" strokeWidth="1.3" />
          <path d="M8 1.5a6.5 6.5 0 0 1 0 13z" fill="var(--text)" />
        </motion.svg>
      </button>
    </Tooltip>
  )
}

function AccentPicker() {
  const { settings, set } = useSettings()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false)
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false)
    window.addEventListener("mousedown", close)
    window.addEventListener("keydown", esc)
    return () => {
      window.removeEventListener("mousedown", close)
      window.removeEventListener("keydown", esc)
    }
  }, [open])

  return (
    <div ref={ref} className="relative" data-no-drag>
      <Tooltip text="Accent colour" side="bottom">
        <button
          type="button"
          aria-label="Accent colour"
          aria-expanded={open}
          data-testid="accent-button"
          onClick={() => setOpen((o) => !o)}
          className="grid h-7 w-7 place-items-center rounded-full border bg-surface text-text transition-transform duration-150 active:scale-95"
          style={{ borderColor: open ? "var(--accent)" : "var(--border-strong)" }}
        >
          <Palette size={14} />
        </button>
      </Tooltip>
      <AnimatePresence>
        {open && (
          <motion.div
            role="dialog"
            aria-label="Choose accent colour"
            initial={{ opacity: 0, y: -4, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -4, transition: { duration: 0.1 } }}
            transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
            style={{ transformOrigin: "top right" }}
            className="absolute right-0 top-9 z-40 flex w-[232px] flex-col gap-3 rounded-xl border border-border bg-surface p-3.5 shadow-float"
          >
            <div className="text-[12px] font-semibold">Accent</div>
            <div className="flex gap-2">
              {ACCENTS.map((a) => {
                const on = a.hex.toLowerCase() === settings.accent.toLowerCase()
                return (
                  <button
                    key={a.hex}
                    type="button"
                    aria-label={a.name}
                    aria-pressed={on}
                    data-testid={`accent-${a.name.toLowerCase()}`}
                    onClick={() => set("accent", a.hex)}
                    className="h-[22px] w-[22px] rounded-full transition-transform duration-150 hover:scale-110"
                    style={{ background: a.hex, boxShadow: on ? `0 0 0 2px var(--surface), 0 0 0 4px ${a.hex}` : undefined }}
                  />
                )
              })}
            </div>
            <label className="flex h-[30px] items-center gap-2 rounded-lg border border-border px-2.5">
              <input
                type="color"
                aria-label="Custom accent colour"
                value={settings.accent}
                onChange={(e) => set("accent", e.target.value)}
                className="h-4 w-4 cursor-pointer appearance-none border-0 bg-transparent p-0"
              />
              <span className="mono text-[12px] uppercase">{settings.accent}</span>
              <span className="ml-auto text-[11.5px] text-subtle">Custom</span>
            </label>
            <p className="m-0 text-[11.5px] leading-[1.45] text-subtle">Dark mode lightens the accent so text on it stays readable.</p>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
