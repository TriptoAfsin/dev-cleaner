export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !isFinite(n)) return "—"
  const units = ["B", "KB", "MB", "GB", "TB"]
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(i === 0 ? 0 : v >= 100 ? 0 : digits)} ${units[i]}`
}

export function pct(n: number, digits = 0): string {
  return `${n.toFixed(digits)}%`
}

export function duration(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (d > 0) return `${d} d ${h} h`
  if (h > 0) return `${h} h ${m} m`
  return `${m} m`
}

export function ago(epochSecs: number): string {
  if (!epochSecs) return "—"
  const s = Date.now() / 1000 - epochSecs
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  const d = Math.round(s / 86400)
  if (d < 60) return `${d} days ago`
  const mo = Math.round(d / 30)
  return mo < 24 ? `${mo} months ago` : `${Math.round(d / 365)} years ago`
}

/** 0.4s, 12.3s, 1m 23s */
export function took(ms: number): string {
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)}s`
  const m = Math.floor(ms / 60_000)
  const s = Math.round((ms % 60_000) / 1000)
  return `${m}m ${s.toString().padStart(2, "0")}s`
}

export function cn(...xs: (string | false | null | undefined)[]): string {
  return xs.filter(Boolean).join(" ")
}
