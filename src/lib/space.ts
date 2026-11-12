// Space analyzer: types and calls for the backend in src-tauri/src/space.rs.
import { invoke } from "@tauri-apps/api/core"

export type SpaceCat = "cache" | "fix" | "user" | "apps" | "system"
/** Order of `mix` and `cats` arrays. */
export const CATS: SpaceCat[] = ["cache", "fix", "user", "apps", "system"]

export type SpaceFix = {
  text: string
  /** Button text, when there is something to open. */
  label: string | null
  /** A page of this app, and the tab to open on it. */
  page: string | null
  tab: string | null
  /** A Windows settings page (ms-settings:…). */
  uri: string | null
}

export type SpaceNode = {
  path: string
  name: string
  size: number
  files: number
  /** dir, file, files (a folder's small files summed), more (folded smaller items), other (not a real folder). */
  kind: "dir" | "file" | "files" | "more" | "other"
  cat: SpaceCat
  /** Bytes by category inside this item, in CATS order. */
  mix: number[]
  about: string | null
  fix?: SpaceFix
  hasChildren: boolean
  children?: SpaceNode[]
}

export type SpaceFinding = {
  path: string
  name: string
  size: number
  cat: SpaceCat
  about: string | null
  fix: SpaceFix | null
  isFile: boolean
}

export type SpaceSummary = {
  root: string
  total: number
  files: number
  dirs: number
  unreadable: number
  elapsedMs: number
  cancelled: boolean
  cats: number[]
  findings: SpaceFinding[]
  cacheTotal: number
  drive: { mount: string; total: number; used: number; free: number } | null
  at: number
}

export type SpaceProgress = { files: number; bytes: number; dirs: number; current: string; elapsedMs: number }

export const space = {
  scan: (root: string) => invoke<SpaceSummary>("space_scan", { root }),
  tree: (path: string | null, depth = 2) => invoke<SpaceNode>("space_tree", { path, depth }),
  last: () => invoke<SpaceSummary | null>("space_last"),
  cancel: () => invoke<void>("space_cancel"),
  openSettings: (uri: string) => invoke<void>("space_open", { uri }),
}

export const CAT_LABEL: Record<SpaceCat, string> = {
  cache: "Rebuildable cache",
  fix: "Has a fix",
  user: "Your files",
  apps: "Apps & games",
  system: "System",
}

export const CAT_ABOUT: Record<SpaceCat, string> = {
  cache: "Rebuildable. The tool that made it downloads or regenerates it when needed.",
  fix: "Has a known fix; see below.",
  user: "Your files. Dev Cleaner never deletes these: move or archive them yourself.",
  apps: "Installed software. Uninstall it in Settings › Apps, or move a game library to another drive.",
  system: "Part of the operating system. Leave it alone.",
}

/** Split an absolute path into the parts breadcrumbs show, each with its full path. */
export function crumbs(root: string, path: string): { name: string; path: string }[] {
  const sep = root.includes("\\") || /^[A-Za-z]:/.test(root) ? "\\" : "/"
  const trim = (s: string) => (s.length > 1 ? s.replace(/[\\/]+$/, "") : s)
  const r = trim(root)
  const out = [{ name: r === "" ? sep : r.split(/[\\/]/).pop() || r, path: root }]
  if (trim(path).length <= r.length) return out
  const rest = trim(path).slice(r.length).split(/[\\/]/).filter(Boolean)
  let acc = r
  for (const part of rest) {
    acc = acc.endsWith(sep) ? acc + part : acc + sep + part
    out.push({ name: part, path: acc })
  }
  return out
}

export function parentOf(path: string): string {
  const t = path.replace(/[\\/]+$/, "")
  const i = Math.max(t.lastIndexOf("\\"), t.lastIndexOf("/"))
  if (i <= 0) return path
  const p = t.slice(0, i)
  return /^[A-Za-z]:$/.test(p) ? p + "\\" : p
}

export type Rect<T> = { item: T; x: number; y: number; w: number; h: number }

/** Squarified treemap (Bruls, Huizing, van Wijk). Items must be sorted biggest first. */
export function squarify<T extends { size: number }>(items: T[], x: number, y: number, w: number, h: number): Rect<T>[] {
  const out: Rect<T>[] = []
  let rest = items.filter((i) => i.size > 0)
  let X = x,
    Y = y,
    W = w,
    H = h
  while (rest.length && W > 0.5 && H > 0.5) {
    const short = Math.min(W, H)
    const scale = (W * H) / rest.reduce((a, b) => a + b.size, 0)
    const worst = (row: T[]) => {
      let s = 0,
        mx = 0,
        mn = Infinity
      for (const o of row) {
        const a = o.size * scale
        s += a
        mx = Math.max(mx, a)
        mn = Math.min(mn, a)
      }
      return Math.max((short * short * mx) / (s * s), (s * s) / (short * short * mn))
    }
    const row = [rest[0]]
    let i = 1
    while (i < rest.length && worst([...row, rest[i]]) <= worst(row)) row.push(rest[i++])
    const s = row.reduce((a, b) => a + b.size * scale, 0)
    const thick = s / short
    let off = 0
    for (const o of row) {
      const len = (o.size * scale) / thick
      out.push(W >= H ? { item: o, x: X, y: Y + off, w: thick, h: len } : { item: o, x: X + off, y: Y, w: len, h: thick })
      off += len
    }
    if (W >= H) {
      X += thick
      W -= thick
    } else {
      Y += thick
      H -= thick
    }
    rest = rest.slice(i)
  }
  return out
}
