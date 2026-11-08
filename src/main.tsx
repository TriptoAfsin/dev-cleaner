import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import "./index.css"
import { App } from "./App"
import { SettingsProvider } from "./lib/settings"
import { StoreProvider } from "./lib/store"
import { api } from "./lib/api"

async function boot() {
  // The settings file is the source of truth. Read it before the first render
  // so the theme and accent are right from the first frame; the window is
  // still hidden at this point.
  if (api.inTauri) {
    try {
      const saved = await api.loadSettings()
      if (saved) {
        const cur = JSON.parse(localStorage.getItem("dc-settings") || "{}")
        const next = { ...cur, ...saved }
        localStorage.setItem("dc-settings", JSON.stringify(next))
        if (typeof next.accent === "string") document.documentElement.style.setProperty("--accent-base", next.accent)
        const dark = next.theme === "dark" || (next.theme !== "light" && matchMedia("(prefers-color-scheme: dark)").matches)
        document.documentElement.classList.toggle("dark", dark)
      }
    } catch {
      /* first launch or unreadable file: defaults */
    }
  }

  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <SettingsProvider>
        <StoreProvider>
          <App />
        </StoreProvider>
      </SettingsProvider>
    </StrictMode>,
  )

  // Placed where it was last time while hidden; show it once React has
  // painted, then correct the size the frame arithmetic got wrong.
  if (api.inTauri) {
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        void api
          .revealWindow()
          .then(() => setTimeout(() => void api.settleWindow(), 60))
          .catch(() => {})
      }),
    )
  }
}

void boot()
