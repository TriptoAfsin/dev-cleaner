import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { api, type Drive } from "./api"

export type ThemeMode = "light" | "dark" | "system"
export type Settings = {
  theme: ThemeMode
  accent: string
  reduceMotion: boolean
  extraPorts: number[]
  /** Folders (or whole drives) the Projects page scans. */
  projectRoots: string[]
  projectDepth: number
  /** Drives to include in cleanups; null means every fixed drive. */
  drives: string[] | null
  /** Also search the chosen drives for AI model files. */
  modelDrives: boolean
  confirmKills: boolean
}

export const ACCENTS: { name: string; hex: string }[] = [
  { name: "Cobalt", hex: "#1d4ed8" },
  { name: "Violet", hex: "#7c3aed" },
  { name: "Teal", hex: "#0f766e" },
  { name: "Lime", hex: "#4d7c0f" },
  { name: "Amber", hex: "#b45309" },
  { name: "Rose", hex: "#be123c" },
  { name: "Graphite", hex: "#3f3f46" },
]

const DEFAULTS: Settings = {
  theme: "system",
  accent: ACCENTS[0].hex,
  reduceMotion: false,
  extraPorts: [],
  projectRoots: [],
  projectDepth: 7,
  drives: null,
  modelDrives: false,
  confirmKills: true,
}

const KEY = "dc-settings"

function load(): Settings {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) || "{}")
    // Older versions kept a single project folder.
    if (typeof raw.projectRoot === "string" && raw.projectRoot && !raw.projectRoots) raw.projectRoots = [raw.projectRoot]
    delete raw.projectRoot
    return { ...DEFAULTS, ...raw }
  } catch {
    return DEFAULTS
  }
}

type Ctx = {
  settings: Settings
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void
  dark: boolean
  toggleTheme: () => void
}
const SettingsCtx = createContext<Ctx | null>(null)

const prefersDark = () => matchMedia("(prefers-color-scheme: dark)").matches

/** Swap the theme under a view transition, so it cross-fades instead of
    snapping. Falls back to an instant swap where the API is missing. */
function withTransition(fn: () => void, reduce: boolean) {
  const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown }
  if (!reduce && doc.startViewTransition && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
    doc.startViewTransition(fn)
  } else fn()
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(load)
  const [systemDark, setSystemDark] = useState(prefersDark)
  const dark = settings.theme === "dark" || (settings.theme === "system" && systemDark)

  useEffect(() => {
    const mq = matchMedia("(prefers-color-scheme: dark)")
    const on = () => setSystemDark(mq.matches)
    mq.addEventListener("change", on)
    return () => mq.removeEventListener("change", on)
  }, [])

  // The settings file is the source of truth; localStorage only paints the
  // first frame (index.html reads it before React loads).
  const loaded = useRef(!api.inTauri)
  useEffect(() => {
    if (!api.inTauri) return
    void api
      .loadSettings()
      .then((saved) => {
        if (saved) setSettings((s) => ({ ...s, ...(saved as Partial<Settings>) }))
      })
      .catch(() => {})
      .finally(() => (loaded.current = true))
  }, [])

  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(settings))
    } catch {
      /* storage unavailable */
    }
    // Don't overwrite the file with defaults before it has been read.
    if (loaded.current && api.inTauri) void api.saveSettings(settings).catch(() => {})
  }, [settings])

  useEffect(() => {
    document.documentElement.style.setProperty("--accent-base", settings.accent)
  }, [settings.accent])

  useEffect(() => {
    document.documentElement.classList.toggle("reduce-motion", settings.reduceMotion)
  }, [settings.reduceMotion])

  useEffect(() => {
    withTransition(() => document.documentElement.classList.toggle("dark", dark), settings.reduceMotion)
    if (api.inTauri) void getCurrentWindow().setTheme(dark ? "dark" : "light").catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dark])

  const set = useCallback(<K extends keyof Settings>(k: K, v: Settings[K]) => {
    setSettings((s) => ({ ...s, [k]: v }))
  }, [])

  const toggleTheme = useCallback(() => {
    setSettings((s) => ({ ...s, theme: dark ? "light" : "dark" }))
  }, [dark])

  return <SettingsCtx.Provider value={{ settings, set, dark, toggleTheme }}>{children}</SettingsCtx.Provider>
}

export function useSettings() {
  const c = useContext(SettingsCtx)
  if (!c) throw new Error("useSettings outside provider")
  return c
}

/** Fixed drives on this machine and which ones are chosen in Settings. */
export function useDrives() {
  const { settings, set } = useSettings()
  const [all, setAll] = useState<Drive[]>([])
  useEffect(() => {
    if (!api.inTauri) return
    void api
      .listDrives()
      .then((ds) => setAll(ds.filter((d) => !d.removable && d.total > 0)))
      .catch(() => {})
  }, [])
  const selected = settings.drives ?? all.map((d) => d.mount)
  const toggle = (mount: string, on: boolean) => {
    const next = on ? [...new Set([...selected, mount])] : selected.filter((m) => m !== mount)
    set("drives", next.length === all.length ? null : next)
  }
  /** Drive roots as paths to scan ("T:" becomes "T:\"). */
  const roots = selected.map((m) => (/^[A-Za-z]:$/.test(m) ? m + "\\" : m))
  return { all, selected, toggle, roots }
}
