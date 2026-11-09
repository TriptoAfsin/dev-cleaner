import { useState } from "react"
import { cn } from "@/lib/format"
import { useStore } from "@/lib/store"
import type { Page } from "@/components/Shell"
import { sectionOf } from "@/lib/sections"
import { Disk } from "./Disk"
import { Models } from "./Models"

type Tab = "models" | "harnesses" | "mltools"
const TABS: [Tab, string][] = [
  ["models", "Models"],
  ["harnesses", "Harnesses"],
  ["mltools", "CLI tools"],
]

// The tab survives leaving the page and coming back.
let lastTab: Tab = "models"

/** AI / ML: downloaded models, what AI coding agents leave behind, and the
    caches of ML tools, each on its own tab. */
export function AI({ go }: { go: (p: Page) => void }) {
  const [tab, setTabState] = useState<Tab>(lastTab)
  const setTab = (t: Tab) => {
    lastTab = t
    setTabState(t)
  }
  const { caches } = useStore()
  const count = (t: Tab) => caches.filter((c) => sectionOf(c) === t && c.found && c.size > 0).length

  const tabs = (
    <div className="flex rounded-lg border border-border-strong p-0.5" role="tablist" aria-label="AI / ML sections">
      {TABS.map(([t, label]) => {
        const n = t === "models" ? 0 : count(t)
        return (
          <button
            key={t}
            type="button"
            role="tab"
            aria-selected={tab === t}
            data-testid={`ai-tab-${t}`}
            onClick={() => setTab(t)}
            className={cn(
              "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] transition-colors",
              tab === t ? "bg-accent-soft font-medium text-accent" : "text-muted hover:text-text",
            )}
          >
            {label}
            {n > 0 && <span className="mono text-[11px] opacity-70">{n}</span>}
          </button>
        )
      })}
    </div>
  )

  if (tab === "models") return <Models go={go} tabs={tabs} />
  return <Disk key={tab} section={tab} tabs={tabs} />
}
