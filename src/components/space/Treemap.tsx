import { useLayoutEffect, useRef, useState } from "react"
import { motion } from "motion/react"
import { ArrowRight } from "lucide-react"
import { bytes, cn } from "@/lib/format"
import { CATS, squarify, type SpaceCat, type SpaceNode } from "@/lib/space"

/** Category colours, light and dark. Leaves mix them toward the surface by rank. */
export const SPACE_TOKENS = `
.space-root{--c-cache:#3b5bdb;--c-cache-t:#ffffff;--c-fix:#ff922b;--c-fix-t:#2b1500;--c-user:#9775fa;--c-user-t:#160b33;--c-apps:#c3cddb;--c-apps-t:#1c222b;--c-system:#4a5260;--c-system-t:#ffffff}
.dark .space-root{--c-cache:#5c7cfa;--c-cache-t:#070b1c;--c-fix:#ffa94d;--c-fix-t:#1f1000;--c-user:#6d4fd6;--c-user-t:#f3f0ff;--c-apps:#2b333f;--c-apps-t:#d5dae2;--c-system:#59626f;--c-system-t:#f3f4f6}
.space-leaf{transition:filter .15s ease,opacity .25s ease,box-shadow .15s ease}
.space-leaf:hover{filter:brightness(1.07) saturate(1.05)}
.space-leaf:focus-visible,.space-frame-h:focus-visible{outline:2px solid var(--accent);outline-offset:1px}
`

const PAD = 6
const GAP = 2

/** True when nothing inside `n` is of the filtered category. */
const dimmed = (n: SpaceNode, filter: SpaceCat | null) => !!filter && (n.mix[CATS.indexOf(filter)] ?? 0) === 0

/** Nested squarified treemap of a folder: folders big enough show their
    contents inside a frame; everything else is a leaf. Click selects,
    double-click opens a folder. */
export function Treemap({
  level,
  sel,
  filter,
  onSelect,
  onOpen,
}: {
  level: SpaceNode
  sel: string | null
  filter: SpaceCat | null
  onSelect: (n: SpaceNode) => void
  onOpen: (n: SpaceNode) => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState({ w: 0, h: 0 })
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setSize({ w: Math.floor(e.contentRect.width), h: Math.floor(e.contentRect.height) }))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const frames: React.ReactNode[] = []
  const leaves: React.ReactNode[] = []
  const leaf = (n: SpaceNode, x: number, y: number, w: number, h: number, rank: number) => {
    const on = n.path === sel
    const tone = 100 - Math.min(rank, 4) * 6
    const folded = n.kind === "more" || n.kind === "files" || n.kind === "other"
    const lw = Math.max(w - GAP * 2, 0)
    const lh = Math.max(h - GAP * 2, 0)
    if (lw < 2 || lh < 2) return
    const base = `color-mix(in oklch, var(--c-${n.cat}) ${tone}%, var(--surface))`
    leaves.push(
      <button
        key={n.path}
        type="button"
        className="space-leaf absolute flex flex-col items-start gap-px overflow-hidden rounded-lg border-0 text-left"
        title={`${n.name} · ${bytes(n.size)}`}
        aria-label={`${n.name}, ${bytes(n.size)}`}
        aria-pressed={on}
        onClick={() => onSelect(n)}
        onDoubleClick={() => (n.kind === "dir" && n.hasChildren ? onOpen(n) : onSelect(n))}
        style={{
          left: x + GAP,
          top: y + GAP,
          width: lw,
          height: lh,
          padding: lw > 40 && lh > 24 ? "8px 9px" : 0,
          background: folded
            ? `repeating-linear-gradient(135deg, ${base} 0 7px, color-mix(in oklch, var(--c-${n.cat}) ${tone - 12}%, var(--surface)) 7px 14px)`
            : base,
          color: `var(--c-${n.cat}-t)`,
          opacity: dimmed(n, filter) ? 0.18 : 1,
          zIndex: on ? 3 : 1,
          borderRadius: Math.min(8, lw / 3, lh / 3),
          boxShadow: on
            ? "0 0 0 2px var(--surface), 0 0 0 4px var(--text), 0 8px 20px rgb(16 24 40 / .18)"
            : "inset 0 1px 0 rgb(255 255 255 / .16), inset 0 0 0 1px rgb(0 0 0 / .04)",
        }}
      >
        {lw > 58 && lh > 28 && <span className="max-w-full truncate text-[12px] font-semibold">{n.name}</span>}
        {lw > 58 && lh > 46 && <span className="text-[11.5px] tabular-nums opacity-80">{bytes(n.size)}</span>}
      </button>,
    )
  }

  if (size.w > 0 && size.h > 0) {
    const kids = level.children ?? []
    squarify(kids, PAD, PAD, size.w - PAD * 2, size.h - PAD * 2).forEach(({ item: n, x, y, w, h }, i) => {
      const inner = n.children ?? []
      if (n.kind === "dir" && inner.length > 0 && w > 96 && h > 84) {
        const on = n.path === sel
        frames.push(
          <div
            key={n.path}
            className="absolute rounded-[11px] bg-surface-2 transition-opacity duration-200"
            style={{
              left: x + GAP,
              top: y + GAP,
              width: w - GAP * 2,
              height: h - GAP * 2,
              boxShadow: on ? "inset 0 0 0 2px var(--text)" : "inset 0 0 0 1px var(--border)",
              opacity: dimmed(n, filter) ? 0.35 : 1,
            }}
          >
            <div className="flex h-7 items-center">
              <button
                type="button"
                className="space-frame-h flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-[9px] border-0 bg-transparent px-2.5 text-left text-[12px] text-muted hover:text-text"
                aria-label={`Folder ${n.name}, ${bytes(n.size)}`}
                aria-pressed={on}
                onClick={() => onSelect(n)}
                onDoubleClick={() => onOpen(n)}
              >
                <span className="truncate font-semibold">{n.name}</span>
                {w > 150 && <span className="shrink-0 tabular-nums text-subtle">{bytes(n.size)}</span>}
              </button>
              <button
                type="button"
                aria-label={`Open ${n.name}`}
                title="Open"
                onClick={() => onOpen(n)}
                className="mr-1 grid h-6 w-6 shrink-0 place-items-center rounded-md border-0 bg-transparent text-subtle transition-colors hover:bg-hover hover:text-text"
              >
                <ArrowRight size={12} />
              </button>
            </div>
          </div>,
        )
        squarify(inner, x + 5, y + 30, w - 10, h - 35).forEach((r, j) => leaf(r.item, r.x, r.y, r.w, r.h, j))
      } else leaf(n, x, y, w, h, i)
    })
  }

  return (
    <div ref={ref} role="group" aria-label="Folder map" className={cn("relative h-full w-full overflow-hidden")}>
      <motion.div
        key={level.path}
        className="absolute inset-0"
        initial={{ opacity: 0, scale: 0.985 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
      >
        {frames}
        {leaves}
      </motion.div>
    </div>
  )
}

/** A small usage ring. */
export function Ring({ value, size = 30, stroke = 5, color }: { value: number; size?: number; stroke?: number; color: string }) {
  const r = (size - stroke) / 2
  const c = 2 * Math.PI * r
  const v = Math.min(Math.max(value, 0.02), 1)
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden className="shrink-0">
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--track)" strokeWidth={stroke} />
      <motion.circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={color}
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={c}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
        initial={{ strokeDashoffset: c }}
        animate={{ strokeDashoffset: c * (1 - v) }}
        transition={{ duration: 0.7, ease: [0.16, 1, 0.3, 1] }}
      />
    </svg>
  )
}
