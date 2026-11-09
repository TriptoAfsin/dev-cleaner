import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type ReactNode,
} from "react"
import { createPortal } from "react-dom"
import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion } from "motion/react"
import { Check, CircleAlert, Info, Loader2, Search, X } from "lucide-react"
import { cn } from "@/lib/format"
import type { Verdict } from "@/lib/api"

// ------------------------------------------------------------------ buttons

type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger" | "quiet"
  size?: "sm" | "md"
  icon?: ReactNode
  loading?: boolean
}

export function Button({ variant = "ghost", size = "md", icon, loading, children, className, disabled, ...rest }: BtnProps) {
  return (
    <button
      type="button"
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={cn(
        "inline-flex select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-lg font-medium",
        "transition-[background-color,border-color,color,transform,opacity] duration-150 ease-out active:scale-[0.97]",
        "disabled:pointer-events-none disabled:opacity-50",
        size === "md" ? "h-8 px-3 text-[13px]" : "h-[26px] px-2 text-[12px]",
        variant === "primary" && "bg-accent text-on-accent hover:brightness-110",
        variant === "ghost" && "border border-border-strong bg-surface text-text hover:border-subtle/60",
        variant === "danger" && "bg-danger text-white hover:brightness-110 dark:text-[#1a0505]",
        variant === "quiet" && "text-muted hover:bg-hover hover:text-text",
        className,
      )}
      {...rest}
    >
      {loading ? <Loader2 size={size === "md" ? 15 : 13} className="spin" /> : icon}
      {children}
    </button>
  )
}

export function IconButton({
  label,
  children,
  className,
  round,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; round?: boolean }) {
  return (
    <Tooltip text={label}>
      <button
        type="button"
        aria-label={label}
        className={cn(
          "grid h-7 w-7 shrink-0 place-items-center border border-border-strong bg-surface text-text",
          "transition-[border-color,transform] duration-150 ease-out hover:border-subtle/60 active:scale-95",
          round ? "rounded-full" : "rounded-lg",
          className,
        )}
        {...rest}
      >
        {children}
      </button>
    </Tooltip>
  )
}

// ------------------------------------------------------------------ tooltip

/** Hover shows after a short delay; keyboard focus shows at once. Rendered in
    a portal so cards with overflow hidden never clip it. */
export function Tooltip({ text, children, side = "top", wide }: { text: ReactNode; children: ReactNode; side?: "top" | "bottom"; wide?: boolean }) {
  const [open, setOpen] = useState(false)
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null)
  const ref = useRef<HTMLSpanElement>(null)
  const timer = useRef<number | undefined>(undefined)
  const id = useId()

  const place = () => {
    const r = ref.current?.getBoundingClientRect()
    if (r) setPos({ x: r.left + r.width / 2, y: side === "top" ? r.top : r.bottom })
  }
  const show = (delay: number) => {
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      place()
      setOpen(true)
    }, delay)
  }
  const hide = () => {
    window.clearTimeout(timer.current)
    setOpen(false)
  }
  useEffect(() => () => window.clearTimeout(timer.current), [])

  return (
    <span
      ref={ref}
      className="inline-flex"
      onMouseEnter={() => show(450)}
      onMouseLeave={hide}
      onFocus={() => show(0)}
      onBlur={hide}
      aria-describedby={open ? id : undefined}
    >
      {children}
      {createPortal(
        <AnimatePresence>
          {open && pos && (
            <TooltipBubble id={id} x={pos.x} y={pos.y} side={side} wide={wide}>
              {text}
            </TooltipBubble>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </span>
  )
}

function TooltipBubble({ id, x, y, side, wide, children }: { id: string; x: number; y: number; side: "top" | "bottom"; wide?: boolean; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)
  const [dx, setDx] = useState(0)
  // Keep the bubble inside the window.
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect()
    if (!r) return
    const pad = 8
    if (r.left < pad) setDx(pad - r.left)
    else if (r.right > window.innerWidth - pad) setDx(window.innerWidth - pad - r.right)
  }, [])
  return (
    <motion.div
      ref={ref}
      id={id}
      role="tooltip"
      initial={{ opacity: 0, y: side === "top" ? 4 : -4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      transition={{ duration: 0.14, ease: [0.16, 1, 0.3, 1] }}
      style={{
        position: "fixed",
        left: x + dx,
        top: y,
        translateX: "-50%",
        translateY: side === "top" ? "calc(-100% - 8px)" : "8px",
        zIndex: 60,
      }}
      className={cn(
        "pointer-events-none rounded-lg bg-[#18181b] px-2.5 py-1.5 text-[12px] leading-[1.5] text-[#f4f4f5] shadow-float dark:bg-[#e8eaee] dark:text-[#18181b]",
        wide ? "w-[300px]" : "max-w-[260px] whitespace-normal",
      )}
    >
      {children}
    </motion.div>
  )
}

export function InfoTip({ text }: { text: ReactNode }) {
  return (
    <Tooltip text={text} wide>
      <button type="button" aria-label="What does this mean?" className="ml-1 inline-grid h-4 w-4 place-items-center rounded-full text-subtle hover:text-text">
        <Info size={13} />
      </button>
    </Tooltip>
  )
}

// ------------------------------------------------------------------ surfaces

export function Card({ children, className, ...rest }: { children: ReactNode; className?: string } & React.HTMLAttributes<HTMLElement>) {
  return (
    <section className={cn("rounded-xl border border-border bg-surface", className)} {...rest}>
      {children}
    </section>
  )
}

export function PageHeader({ title, lede, actions }: { title: string; lede?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="m-0 text-[20px] font-semibold tracking-[-0.02em]">{title}</h1>
        {lede && <p className="mt-1 text-[13px] text-muted">{lede}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  )
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("skeleton", className)} />
}

export function Empty({ icon, title, children }: { icon?: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center">
      {icon && <div className="text-subtle">{icon}</div>}
      <div className="text-[14px] font-medium">{title}</div>
      {children && <div className="max-w-[380px] text-[12.5px] text-muted">{children}</div>}
    </div>
  )
}

// ------------------------------------------------------------------ data viz

/** Ring gauge; the arc eases to its value. */
export function Gauge({ value, size = 84, stroke = 7, warn, children }: { value: number; size?: number; stroke?: number; warn?: boolean; children?: ReactNode }) {
  const r = (size - stroke) / 2
  const c = 2 * Math.PI * r
  const v = Math.max(0, Math.min(1, value))
  return (
    <div className="relative shrink-0" style={{ width: size, height: size }}>
      <svg width={size} height={size} style={{ transform: "rotate(-90deg)" }} aria-hidden>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--track)" strokeWidth={stroke} />
        <motion.circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={warn ? "var(--warn)" : "var(--accent)"}
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={c}
          initial={{ strokeDashoffset: c }}
          animate={{ strokeDashoffset: c * (1 - v) }}
          transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
        />
      </svg>
      <div className="absolute inset-0 grid place-items-center">{children}</div>
    </div>
  )
}

export function Bar({ value, className, tone = "accent", thin }: { value: number; className?: string; tone?: "accent" | "warn" | "danger" | "muted"; thin?: boolean }) {
  const color = { accent: "var(--accent)", warn: "var(--warn)", danger: "var(--danger)", muted: "var(--subtle)" }[tone]
  return (
    <div className={cn("overflow-hidden rounded-full bg-track", thin ? "h-1" : "h-1.5", className)}>
      <motion.div
        className="h-full rounded-full"
        style={{ background: color }}
        initial={{ width: 0 }}
        animate={{ width: `${Math.max(0, Math.min(100, value * 100))}%` }}
        transition={{ duration: 0.6, ease: [0.16, 1, 0.3, 1] }}
      />
    </div>
  )
}

export function Sparkline({ data, height = 40, max = 100 }: { data: number[]; height?: number; max?: number }) {
  if (data.length < 2) return <div style={{ height }} />
  const w = 300
  const pts = data.map((d, i) => [(i / (data.length - 1)) * w, height - (Math.min(d, max) / max) * (height - 2) - 1])
  const line = pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`).join(" ")
  return (
    <svg width="100%" height={height} viewBox={`0 0 ${w} ${height}`} preserveAspectRatio="none" aria-hidden>
      <path d={`${line} L${w} ${height} L0 ${height} Z`} fill="var(--accent)" fillOpacity={0.1} />
      <path d={line} fill="none" stroke="var(--accent)" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

/** Counts up to the new value instead of jumping. */
export function AnimatedNumber({ value, format }: { value: number; format: (n: number) => string }) {
  const reduce = useReducedMotion()
  const mv = useMotionValue(value)
  const [shown, setShown] = useState(value)
  useEffect(() => {
    if (reduce) {
      setShown(value)
      return
    }
    const c = animate(mv, value, { duration: 0.7, ease: [0.16, 1, 0.3, 1], onUpdate: setShown })
    return () => c.stop()
  }, [value, reduce, mv])
  return <>{format(shown)}</>
}

// ------------------------------------------------------------------ badges

const VERDICT: Record<Verdict, { label: string; cls: string }> = {
  safe: { label: "Safe to end", cls: "bg-ok-soft text-ok" },
  saveFirst: { label: "Save first", cls: "bg-warn-soft text-warn" },
  system: { label: "System", cls: "bg-track text-muted" },
  unknown: { label: "Check first", cls: "bg-track text-muted" },
}

export function VerdictPill({ verdict }: { verdict: Verdict }) {
  const v = VERDICT[verdict]
  return (
    <span className={cn("inline-flex h-[21px] items-center gap-1.5 whitespace-nowrap rounded-full px-2 text-[11.5px] font-medium", v.cls)}>
      <span className="h-1.5 w-1.5 rounded-full bg-current" />
      {v.label}
    </span>
  )
}

export function Badge({ tone = "muted", children }: { tone?: "ok" | "warn" | "muted" | "accent" | "danger"; children: ReactNode }) {
  const cls = {
    ok: "bg-ok-soft text-ok",
    warn: "bg-warn-soft text-warn",
    muted: "bg-track text-muted",
    accent: "bg-accent-soft text-accent",
    danger: "bg-danger-soft text-danger",
  }[tone]
  return (
    <span className={cn("inline-flex h-5 items-center gap-1.5 whitespace-nowrap rounded-full px-2 text-[11.5px] font-medium", cls)}>
      <span className="h-1.5 w-1.5 rounded-full bg-current" />
      {children}
    </span>
  )
}

// ------------------------------------------------------------------ search

/** A filter box with a clear button once there is text. Escape clears too. */
export function SearchField({
  value,
  onChange,
  placeholder,
  label,
  testId,
  className,
}: {
  value: string
  onChange: (v: string) => void
  placeholder: string
  label: string
  testId?: string
  className?: string
}) {
  const ref = useRef<HTMLInputElement>(null)
  return (
    <label className={cn("flex h-8 min-w-[200px] flex-1 items-center gap-2 rounded-lg border border-border-strong bg-surface pl-2.5 pr-1 focus-within:border-accent", className)}>
      <Search size={14} className="shrink-0 text-subtle" />
      <input
        ref={ref}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.stopPropagation()
            onChange("")
          }
        }}
        placeholder={placeholder}
        aria-label={label}
        data-testid={testId}
        spellCheck={false}
        className="h-full min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-subtle"
      />
      {value && (
        <button
          type="button"
          aria-label="Clear search"
          data-testid={testId ? `${testId}-clear` : undefined}
          onClick={() => {
            onChange("")
            ref.current?.focus()
          }}
          className="grid h-6 w-6 shrink-0 place-items-center rounded-md text-subtle transition-colors duration-150 hover:bg-hover hover:text-text"
        >
          <X size={13} />
        </button>
      )}
    </label>
  )
}

// ------------------------------------------------------------------ progress

/** A thin bar: a fraction from 0 to 1, or indeterminate when null. */
export function ProgressBar({ value, className }: { value: number | null; className?: string }) {
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value == null ? undefined : Math.round(value * 100)}
      className={cn("relative h-1 overflow-hidden rounded-full bg-track", className)}
    >
      {value == null ? (
        <div className="indeterminate absolute inset-y-0 w-2/5 rounded-full bg-accent" />
      ) : (
        <div className="h-full rounded-full bg-accent transition-[width] duration-300 ease-out" style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }} />
      )}
    </div>
  )
}

/** Progress of a batch delete: a bar, how many are done and what's freed. */
export function DeleteProgress({ done, total, freed, label, format }: { done: number; total: number; freed: number; label: string; format: (n: number) => string }) {
  return (
    <div className="flex flex-col gap-1.5" data-testid="delete-progress">
      <ProgressBar value={total ? done / total : 0} className="h-2" />
      <div className="mono flex justify-between text-[11.5px] text-subtle">
        <span>
          {Math.min(done + 1, total)} of {total} {label} · {format(freed)} freed
        </span>
        <span>{total ? Math.round((done / total) * 100) : 0}%</span>
      </div>
    </div>
  )
}

// ------------------------------------------------------------------ checkbox

export function Checkbox({ checked, indeterminate, onChange, label }: { checked: boolean; indeterminate?: boolean; onChange: (v: boolean) => void; label: string }) {
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !!indeterminate
  }, [indeterminate])
  return (
    <input
      ref={ref}
      type="checkbox"
      aria-label={label}
      checked={checked}
      onChange={(e) => onChange(e.target.checked)}
      onClick={(e) => e.stopPropagation()}
      className="m-0 h-[15px] w-[15px] shrink-0 cursor-pointer"
      style={{ accentColor: "var(--accent)" }}
    />
  )
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={cn("relative h-[20px] w-[34px] shrink-0 rounded-full transition-colors duration-200", checked ? "bg-accent" : "bg-border-strong")}
    >
      <motion.span
        className="absolute top-[2px] h-4 w-4 rounded-full bg-white shadow"
        animate={{ left: checked ? 16 : 2 }}
        transition={{ type: "spring", stiffness: 600, damping: 38 }}
      />
    </button>
  )
}

// ------------------------------------------------------------------ dialog

export function Dialog({
  open,
  onClose,
  title,
  children,
  actions,
}: {
  open: boolean
  onClose: () => void
  title: string
  children: ReactNode
  actions: ReactNode
}) {
  useEffect(() => {
    if (!open) return
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", k)
    return () => window.removeEventListener("keydown", k)
  }, [open, onClose])
  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 grid place-items-center bg-black/30 p-6 dark:bg-black/55"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.16 }}
          onMouseDown={(e) => e.target === e.currentTarget && onClose()}
        >
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label={title}
            initial={{ opacity: 0, scale: 0.97, y: 6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.1 } }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            className="flex max-h-[80vh] w-[480px] max-w-full flex-col rounded-2xl border border-border bg-surface shadow-float"
          >
            <div className="px-5 pt-5 text-[15px] font-semibold">{title}</div>
            <div className="min-h-0 overflow-auto px-5 pb-2 pt-2 text-[13px] text-muted">{children}</div>
            <div className="flex justify-end gap-2 px-5 pb-5 pt-3">{actions}</div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  )
}

// ------------------------------------------------------------------ toasts

import { useStore } from "@/lib/store"

export function Toaster() {
  const { toasts, dismiss } = useStore()
  return (
    <div className="pointer-events-none fixed bottom-[40px] left-[212px] z-50 flex w-[340px] flex-col-reverse gap-2" aria-live="polite">
      <AnimatePresence initial={false}>
        {toasts.map((t) => (
          <motion.div
            key={t.id}
            layout
            initial={{ opacity: 0, y: 12, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, x: -24, transition: { duration: 0.16 } }}
            transition={{ duration: 0.26, ease: [0.16, 1, 0.3, 1] }}
            className="pointer-events-auto flex items-start gap-2.5 rounded-xl border border-border bg-surface p-3 shadow-float"
            role="status"
          >
            <span className={cn("mt-px", t.kind === "ok" ? "text-ok" : t.kind === "error" ? "text-danger" : "text-accent")}>
              {t.kind === "ok" ? <Check size={16} /> : t.kind === "error" ? <CircleAlert size={16} /> : <Info size={16} />}
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium">{t.title}</div>
              {t.detail && <div className="selectable mt-0.5 break-words text-[12px] text-muted">{t.detail}</div>}
              {t.action && (
                <button
                  type="button"
                  data-testid="toast-action"
                  onClick={() => {
                    t.action!.run()
                    dismiss(t.id)
                  }}
                  className="mt-1.5 text-[12px] font-medium text-accent hover:underline"
                >
                  {t.action.label}
                </button>
              )}
            </div>
            <button type="button" aria-label="Dismiss" onClick={() => dismiss(t.id)} className="text-subtle hover:text-text">
              <X size={14} />
            </button>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  )
}
