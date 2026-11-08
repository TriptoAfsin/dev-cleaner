import { useState } from "react"
import { AnimatePresence, motion } from "motion/react"
import { ArrowRight, CircleAlert, CircleCheck, Copy, Lightbulb } from "lucide-react"
import type { Insight } from "@/lib/insights"
import { cn } from "@/lib/format"
import { useStore } from "@/lib/store"
import { Card, Skeleton } from "./ui"
import type { Page } from "./Shell"

export function InsightsCard({ items, go, limit, loading }: { items: Insight[]; go?: (p: Page) => void; limit?: number; loading?: boolean }) {
  const [all, setAll] = useState(false)
  const { toast } = useStore()
  const shown = all || !limit ? items : items.slice(0, limit)
  return (
    <Card className="overflow-hidden" data-testid="insights">
      <div className="flex items-center justify-between px-4 py-3">
        <h2 className="m-0 flex items-center gap-2 text-[13px] font-semibold">
          <Lightbulb size={14} className="text-accent" />
          Recommendations
        </h2>
        {limit && items.length > limit && (
          <button type="button" className="text-[12px] text-accent hover:underline" onClick={() => setAll((a) => !a)}>
            {all ? "Show fewer" : `Show all ${items.length}`}
          </button>
        )}
      </div>
      {loading && !items.length ? (
        <div className="flex flex-col gap-2 px-4 pb-4">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
      ) : (
        <ul className="m-0 list-none p-0">
          <AnimatePresence initial={false}>
            {shown.map((i) => (
              <motion.li
                key={i.id}
                layout="position"
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                className="grid grid-cols-[18px_minmax(0,1fr)_auto] items-start gap-3 border-t border-border px-4 py-3"
              >
                <span className={cn("mt-px", i.level === "warn" ? "text-warn" : i.level === "ok" ? "text-ok" : "text-accent")}>
                  {i.level === "warn" ? <CircleAlert size={16} /> : i.level === "ok" ? <CircleCheck size={16} /> : <Lightbulb size={16} />}
                </span>
                <div className="min-w-0">
                  <div className="text-[13px] font-medium">{i.title}</div>
                  <div className="mt-0.5 text-[12.5px] leading-[1.5] text-muted">{i.detail}</div>
                  {i.command && (
                    <button
                      type="button"
                      onClick={() => {
                        void navigator.clipboard.writeText(i.command!)
                        toast({ kind: "ok", title: "Command copied", detail: i.command })
                      }}
                      className="mono mt-2 inline-flex max-w-full items-center gap-2 rounded-md border border-border bg-surface-2 px-2 py-1 text-left text-[11.5px] text-text hover:border-border-strong"
                      title="Copy command"
                    >
                      <span className="truncate">{i.command}</span>
                      <Copy size={12} className="shrink-0 text-subtle" />
                    </button>
                  )}
                </div>
                {i.go && go && (
                  <button type="button" onClick={() => go(i.go!)} className="mt-0.5 flex items-center gap-1 whitespace-nowrap text-[12px] text-accent hover:underline">
                    Open <ArrowRight size={12} />
                  </button>
                )}
              </motion.li>
            ))}
          </AnimatePresence>
        </ul>
      )}
    </Card>
  )
}
