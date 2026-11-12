import type { PressureSample } from "@/lib/pressure"

const W = 640

/** A path through points, broken where the data has a gap (the app was closed). */
function paths(pts: [number, number, number][], gap: number, x: (t: number) => number, y: (v: number) => number, base: number) {
  let line = ""
  let area = ""
  let seg: [number, number][] = []
  const flush = () => {
    if (seg.length === 0) return
    const l = seg.map(([px, py], i) => `${i ? "L" : "M"}${px.toFixed(1)} ${py.toFixed(1)}`).join(" ")
    line += l + " "
    area += `${l} L${seg[seg.length - 1][0].toFixed(1)} ${base} L${seg[0][0].toFixed(1)} ${base} Z `
    seg = []
  }
  let last = -Infinity
  for (const [t, v] of pts) {
    if (t - last > gap) flush()
    seg.push([x(t), y(v)])
    last = t
  }
  flush()
  return { line, area }
}

/** Committed memory (line and wash) over paging from disk (amber, along the
    bottom), with slow spells shaded. Labels are HTML so they never stretch. */
export function PressureChart({
  samples,
  hours,
  now,
  memTotal,
  spells,
  height = 168,
}: {
  samples: PressureSample[]
  hours: number
  now: number
  memTotal: number
  spells: { start: number; end: number }[]
  height?: number
}) {
  const span = hours * 3600
  const from = now - span
  const pts = samples.filter((s) => s.t >= from)
  const x = (t: number) => Math.max(0, Math.min(W, ((t - from) / span) * W))
  const top = 6
  const H = height
  const yMax = Math.max(memTotal * 1.25, ...pts.map((s) => s.commitUsed * 1.06), 1)
  const y = (v: number) => H - (v / yMax) * (H - top)
  const step = pts.length > 1 ? (pts[pts.length - 1].t - pts[0].t) / (pts.length - 1) : 2
  const gap = Math.max(300, step * 4)
  const commit = paths(pts.map((s) => [s.t, s.commitUsed, 0]), gap, x, y, H)
  const pMax = Math.max(120, ...pts.map((s) => s.pagesInPerSec)) * 1.1
  const band = H * 0.42
  const page = paths(pts.map((s) => [s.t, s.pagesInPerSec, 0]), gap, x, (v) => H - (v / pMax) * band, H)
  const ramY = y(memTotal)

  return (
    <div className="relative" style={{ height }}>
      <svg width="100%" height={height} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="Committed memory and paging over time" className="block">
        {spells.map((s) => (
          <rect key={s.start} x={x(s.start)} y={0} width={Math.max(2, x(s.end) - x(s.start))} height={H} fill="color-mix(in oklch, var(--danger) 9%, transparent)" />
        ))}
        <line x1={0} x2={W} y1={ramY} y2={ramY} stroke="var(--border-strong)" strokeDasharray="3 4" vectorEffect="non-scaling-stroke" />
        <path d={commit.area} fill="color-mix(in oklch, var(--accent) 13%, transparent)" />
        <path d={commit.line} fill="none" stroke="var(--accent)" strokeWidth={2} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
        <path d={page.area} fill="color-mix(in oklch, var(--warn) 42%, transparent)" />
      </svg>
      <span className="pointer-events-none absolute left-1 text-[10.5px] text-subtle" style={{ top: Math.max(0, (ramY / H) * height - 15) }}>
        RAM size
      </span>
      {pts.length < 2 && (
        <span className="absolute inset-0 grid place-items-center text-[12px] text-subtle">Collecting samples… the chart fills in as Dev Cleaner runs.</span>
      )}
    </div>
  )
}

/** A small trend line for one app's memory. */
export function MiniSpark({ data, width = 110, height = 28, color = "var(--accent)" }: { data: number[]; width?: number; height?: number; color?: string }) {
  if (data.length < 2) return <svg width={width} height={height} aria-hidden />
  const min = Math.min(...data)
  const max = Math.max(...data)
  const r = max - min || 1
  const d = data.map((v, i) => `${i ? "L" : "M"}${((i / (data.length - 1)) * (width - 4) + 2).toFixed(1)} ${(height - 3 - ((v - min) / r) * (height - 6)).toFixed(1)}`).join(" ")
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden className="shrink-0">
      <path d={d} fill="none" stroke={color} strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

/** Segmented tabs in the app's style. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  testId,
}: {
  value: T
  options: { id: T; label: React.ReactNode }[]
  onChange: (v: T) => void
  label: string
  testId?: string
}) {
  return (
    <div className="flex rounded-lg border border-border-strong p-0.5" role="tablist" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="tab"
          aria-selected={value === o.id}
          data-testid={testId ? `${testId}-${o.id}` : undefined}
          onClick={() => onChange(o.id)}
          className={
            "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] transition-colors " +
            (value === o.id ? "bg-accent-soft font-medium text-accent" : "text-muted hover:text-text")
          }
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}
