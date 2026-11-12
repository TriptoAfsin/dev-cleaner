import type { ReactNode } from "react"
import { cn } from "@/lib/format"

/** Three bars, filled by impact (1 low, 2 medium, 3 high). */
export function ImpactMeter({ level }: { level: 1 | 2 | 3 }) {
  const label = ["", "Low", "Med", "High"][level]
  return (
    <span className="flex items-end gap-[2px]" title={`Impact on this PC: ${label}`}>
      {[6, 9, 12].map((h, i) => (
        <span
          key={h}
          aria-hidden
          className="w-1 rounded-[2px]"
          style={{ height: h, background: i < level ? (level === 3 ? "var(--ok)" : "var(--accent)") : "var(--track)" }}
        />
      ))}
      <span className="ml-1.5 w-[30px] text-[11.5px] text-subtle">{label}</span>
    </span>
  )
}

/** A card with a title row: the app's softer, larger-radius panel. */
export function Panel({ title, aside, children, className, label }: { title: ReactNode; aside?: ReactNode; children: ReactNode; className?: string; label?: string }) {
  return (
    <section aria-label={label} className={cn("flex flex-col gap-3 rounded-2xl border border-border bg-surface p-[18px] shadow-[0_1px_2px_rgb(16_24_40/0.05)]", className)}>
      <div className="flex items-baseline gap-2">
        <h2 className="m-0 text-[14px] font-semibold">{title}</h2>
        <span className="flex-1" />
        {aside}
      </div>
      {children}
    </section>
  )
}

/** Tabular, sans numbers. */
export function Num({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cn("tabular-nums tracking-[-0.02em]", className)}>{children}</span>
}

/** The segmented tab control used in page headers. */
export function Tabs<T extends string>({ value, onChange, items, label, testId }: { value: T; onChange: (v: T) => void; items: [T, string][]; label: string; testId: string }) {
  return (
    <div className="flex rounded-lg border border-border-strong p-0.5" role="tablist" aria-label={label}>
      {items.map(([t, text]) => (
        <button
          key={t}
          type="button"
          role="tab"
          aria-selected={value === t}
          data-testid={`${testId}-${t}`}
          onClick={() => onChange(t)}
          className={cn("flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] transition-colors", value === t ? "bg-accent-soft font-medium text-accent" : "text-muted hover:text-text")}
        >
          {text}
        </button>
      ))}
    </div>
  )
}
