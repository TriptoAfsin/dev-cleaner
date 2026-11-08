import { useEffect, useState } from "react"
import { AnimatePresence, MotionConfig, motion } from "motion/react"
import { TitleBar } from "./components/TitleBar"
import { Sidebar, StatusBar, type Page } from "./components/Shell"
import { Toaster } from "./components/ui"
import { ShareReportDialog } from "./components/Share"
import { useSettings } from "./lib/settings"
import { Overview } from "./pages/Overview"
import { Disk } from "./pages/Disk"
import { Projects } from "./pages/Projects"
import { Models } from "./pages/Models"
import { Configs } from "./pages/Configs"
import { Memory } from "./pages/Memory"
import { Processes } from "./pages/Processes"
import { Ports } from "./pages/Ports"
import { SystemPage } from "./pages/System"
import { SettingsPage } from "./pages/Settings"

const ORDER: Page[] = ["overview", "disk", "junk", "projects", "models", "configs", "memory", "processes", "ports", "system", "settings"]

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
    models: <Models go={setPage} />,
    configs: <Configs />,
    overview: <Overview go={setPage} />,
    disk: <Disk />,
    junk: <Disk junk key="junk" />,
    projects: <Projects />,
    memory: <Memory />,
    processes: <Processes />,
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
