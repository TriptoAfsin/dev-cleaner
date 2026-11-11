import { useEffect, useState } from "react"
import { AnimatePresence, MotionConfig, motion } from "motion/react"
import { TitleBar } from "./components/TitleBar"
import { ORDER, Sidebar, StatusBar, type Page } from "./components/Shell"
import { Toaster } from "./components/ui"
import { ShareReportDialog } from "./components/Share"
import { useSettings } from "./lib/settings"
import { useStore } from "./lib/store"
import { checkForUpdates, LAST_CHECK_KEY } from "./lib/updates"
import { api } from "./lib/api"
import { Overview } from "./pages/Overview"
import { Disk } from "./pages/Disk"
import { Projects } from "./pages/Projects"
import { AI } from "./pages/AI"
import { Configs } from "./pages/Configs"
import { Memory } from "./pages/Memory"
import { Processes } from "./pages/Processes"
import { Ports } from "./pages/Ports"
import { Startup } from "./pages/Startup"
import { Services } from "./pages/Services"
import { SystemPage } from "./pages/System"
import { SettingsPage } from "./pages/Settings"

export function App() {
  const { settings } = useSettings()
  const [page, setPage] = useState<Page>(() => (sessionStorage.getItem("dc-page") as Page) || "overview")

  useEffect(() => {
    sessionStorage.setItem("dc-page", page)
  }, [page])

  // Deep components (a dialog) can ask to open a section.
  useEffect(() => {
    const go = (e: Event) => setPage((e as CustomEvent<Page>).detail)
    window.addEventListener("dc-go", go)
    return () => window.removeEventListener("dc-go", go)
  }, [])

  // Update check: a few seconds after launch (never before the first paint
  // or scan), then once a day. A failure is silent here; Settings shows it.
  const { toast, scan } = useStore()
  useEffect(() => {
    if (!settings.checkUpdates || !api.inTauri) return
    const DAY = 24 * 60 * 60 * 1000
    const run = async () => {
      // New definitions (cache rules and the rest) come without an app update.
      void api
        .updateDefinitions()
        .then((r) => {
          if (r.outcome !== "updated") return
          window.dispatchEvent(new Event("dc-definitions"))
          toast({ kind: "ok", title: `Definitions updated to ${r.version}`, detail: `${r.info.counts.caches} caches and ${r.info.counts.junk} junk locations. Rescanning.` })
          void scan()
        })
        .catch(() => {})
      const s = await checkForUpdates()
      if (s.info?.newer) toast({ kind: "info", title: `Dev Cleaner ${s.info.latest} is out`, detail: "See what's new and how to update.", action: { label: "Details", run: () => window.dispatchEvent(new Event("dc-about")) } })
    }
    let last = 0
    try {
      last = Number(localStorage.getItem(LAST_CHECK_KEY) || 0)
    } catch {
      /* storage unavailable */
    }
    const first = window.setTimeout(run, Date.now() - last > DAY ? 4000 : Math.max(4000, last + DAY - Date.now()))
    const every = window.setInterval(run, DAY)
    return () => {
      clearTimeout(first)
      clearInterval(every)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.checkUpdates])

  // Ctrl/Cmd + 1..9 jumps between sections.
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return
      const n = Number(e.key)
      if (n >= 1 && n <= ORDER.length) {
        e.preventDefault()
        setPage(ORDER[n - 1])
      }
    }
    window.addEventListener("keydown", k)
    return () => window.removeEventListener("keydown", k)
  }, [])

  const views: Record<Page, React.ReactNode> = {
    models: <AI go={setPage} />,
    configs: <Configs />,
    overview: <Overview go={setPage} />,
    disk: <Disk />,
    junk: <Disk section="junk" key="junk" />,
    projects: <Projects />,
    memory: <Memory />,
    processes: <Processes />,
    startup: <Startup />,
    services: <Services />,
    ports: <Ports />,
    system: <SystemPage go={setPage} />,
    settings: <SettingsPage />,
  }
  const view = views[page]

  return (
    <MotionConfig reducedMotion={settings.reduceMotion ? "always" : "user"}>
      <div className="flex h-full flex-col bg-bg text-text">
        <TitleBar />
        <div className="flex min-h-0 flex-1">
          <Sidebar page={page} go={setPage} />
          <main className="relative min-w-0 flex-1 overflow-hidden">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={page}
                data-testid={`page-${page}`}
                className="absolute inset-0 overflow-y-auto px-[26px] py-[22px]"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -4 }}
                transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
              >
                {view}
              </motion.div>
            </AnimatePresence>
          </main>
        </div>
        <StatusBar />
        <Toaster />
        <ShareReportDialog />
      </div>
    </MotionConfig>
  )
}
