import { useState } from "react"
import type { Page } from "@/components/Shell"
import { Tabs } from "@/components/tune/parts"
import { Disk } from "./Disk"
import { DeepClean } from "./DeepClean"

type Tab = "quick" | "deep"

// The tab survives leaving the page and coming back.
let lastTab: Tab = "quick"

/** System junk: Quick clean (temp files, logs, caches) and Deep clean
    (drivers, installer leftovers, pagefile, component store). Another page
    can open Deep clean by setting sessionStorage "dc-junk-tab" to "deep". */
export function Junk(_: { go: (p: Page) => void }) {
  const [tab, setTabState] = useState<Tab>(() => {
    try {
      const want = sessionStorage.getItem("dc-junk-tab")
      if (want === "deep" || want === "quick") {
        sessionStorage.removeItem("dc-junk-tab")
        lastTab = want
      }
    } catch {
      /* storage unavailable */
    }
    return lastTab
  })
  const setTab = (t: Tab) => {
    lastTab = t
    setTabState(t)
  }
  const tabs = (
    <Tabs
      value={tab}
      onChange={setTab}
      label="System junk views"
      testId="junk-tab"
      items={[
        ["quick", "Quick clean"],
        ["deep", "Deep clean"],
      ]}
    />
  )
  if (tab === "deep") return <DeepClean tabs={tabs} />
  return <Disk section="junk" key="junk" tabs={tabs} />
}
