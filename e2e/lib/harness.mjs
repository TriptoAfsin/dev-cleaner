// Starts a clean, isolated Dev Cleaner under WebDriver for one test file.
//
// Isolation:
// - The e2e build uses its own identifier (dev.tripto.dev-cleaner.e2e), so its
//   window state and WebView2 storage live apart from a real install. Both are
//   wiped before every launch.
// - DEV_CLEANER_FAKE_HOME points every cache path into a throwaway folder and
//   turns off the tools' own clean commands, so no test can empty a real cache.

import { spawn, execFileSync } from "node:child_process"
import { existsSync, mkdirSync, rmSync, writeFileSync, utimesSync } from "node:fs"
import net from "node:net"
import os from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { Session, sleep } from "./webdriver.mjs"

const here = path.dirname(fileURLToPath(import.meta.url))
export const ROOT = path.resolve(here, "..", "..")
export const IDENTIFIER = "dev.tripto.dev-cleaner.e2e"
export const APP = path.join(ROOT, "src-tauri", "target-e2e", "release", "dev-cleaner.exe")
const EDGE_DRIVER = path.join(ROOT, "e2e", ".bin", "msedgedriver.exe")
export const ARTIFACTS = path.join(ROOT, "e2e", ".artifacts")
export const CONFIG_DIR = path.join(process.env.APPDATA ?? "", IDENTIFIER)
const DATA_DIRS = [CONFIG_DIR, path.join(process.env.LOCALAPPDATA ?? "", IDENTIFIER)]
export const FAKE_HOME = path.join(os.tmpdir(), "dc-e2e-home")
export const PROJECTS = path.join(os.tmpdir(), "dc-e2e-projects")

export function freePort() {
  return new Promise((resolve) => {
    const srv = net.createServer()
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address()
      srv.close(() => resolve(port))
    })
  })
}

async function waitForPort(port, timeout = 20000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    const ok = await new Promise((r) => {
      const sock = net.connect(port, "127.0.0.1", () => {
        sock.destroy()
        r(true)
      })
      sock.on("error", () => r(false))
    })
    if (ok) return
    await sleep(150)
  }
  throw new Error(`port ${port} never opened`)
}

export function killTree(pid) {
  if (!pid) return
  try {
    execFileSync("taskkill", ["/F", "/T", "/PID", String(pid)], { stdio: "ignore" })
  } catch {
    /* already gone */
  }
}

function file(p, bytes = 1024, ageDays = 0) {
  mkdirSync(path.dirname(p), { recursive: true })
  writeFileSync(p, Buffer.alloc(bytes, 97))
  if (ageDays) {
    const t = Date.now() / 1000 - ageDays * 86400
    utimesSync(p, t, t)
  }
}

/** Caches with known sizes under the fake home. */
export function seedCaches() {
  rmSync(FAKE_HOME, { recursive: true, force: true })
  file(path.join(FAKE_HOME, "local", "npm-cache", "_cacache", "a.tgz"), 300_000)
  file(path.join(FAKE_HOME, "local", "npm-cache", "_cacache", "b.tgz"), 200_000)
  file(path.join(FAKE_HOME, ".cargo", "registry", "cache", "serde.crate"), 120_000)
  file(path.join(FAKE_HOME, "local", "pip", "Cache", "wheel.whl"), 80_000)
  file(path.join(FAKE_HOME, "temp", "old-installer.log"), 50_000, 3)
  file(path.join(FAKE_HOME, "temp", "fresh.lock"), 10_000)
  // AI harnesses: Claude Code debug logs (old and fresh) and agent-browser screenshots.
  file(path.join(FAKE_HOME, ".claude", "debug", "old-session.txt"), 60_000, 3)
  file(path.join(FAKE_HOME, ".claude", "debug", "live-session.txt"), 5_000)
  file(path.join(FAKE_HOME, ".claude", "projects", "p", "memory", "MEMORY.md"), 500, 90)
  file(path.join(FAKE_HOME, ".agent-browser", "tmp", "screenshots", "shot.png"), 70_000)
}

/** A folder of projects, each with a build folder, plus decoys. */
export function seedProjects() {
  rmSync(PROJECTS, { recursive: true, force: true })
  file(path.join(PROJECTS, "web", "package.json"), 20)
  file(path.join(PROJECTS, "web", "src", "index.ts"), 100)
  file(path.join(PROJECTS, "web", "node_modules", "react", "index.js"), 400_000)
  file(path.join(PROJECTS, "rs", "Cargo.toml"), 20)
  file(path.join(PROJECTS, "rs", "target", "debug", "app.exe"), 900_000)
  file(path.join(PROJECTS, "py", ".venv", "pyvenv.cfg"), 20)
  file(path.join(PROJECTS, "py", ".venv", "Lib", "site.py"), 150_000)
  // Decoys: no package.json beside it, and "target" outside a Cargo project.
  file(path.join(PROJECTS, "loose", "node_modules", "x.js"), 1000)
  file(path.join(PROJECTS, "docs", "target", "notes.md"), 1000)
}

function wipeData() {
  for (const dir of DATA_DIRS) if (dir.endsWith(IDENTIFIER)) rmSync(dir, { recursive: true, force: true })
}

/** Launch the app. `windowState` is written before launch to test restore. */
export async function launch({ keepData = false, windowState, realHome = false, env: extraEnv = {} } = {}) {
  if (!existsSync(APP)) throw new Error(`e2e build missing at ${APP}. Run: npm run e2e:build`)
  if (!keepData) wipeData()
  if (!realHome && !keepData) seedCaches()
  if (windowState) {
    mkdirSync(CONFIG_DIR, { recursive: true })
    writeFileSync(path.join(CONFIG_DIR, "window-state.json"), JSON.stringify(windowState))
  }

  const port = await freePort()
  const nativePort = await freePort()
  const env = { ...process.env, ...extraEnv }
  if (realHome) delete env.DEV_CLEANER_FAKE_HOME
  else env.DEV_CLEANER_FAKE_HOME = FAKE_HOME
  const driver = spawn(
    path.join(process.env.USERPROFILE ?? "", ".cargo", "bin", "tauri-driver.exe"),
    ["--port", String(port), "--native-port", String(nativePort), "--native-driver", EDGE_DRIVER],
    { stdio: "ignore", windowsHide: true, env },
  )
  await waitForPort(port)
  const s = await Session.create(`http://127.0.0.1:${port}`, { "tauri:options": { application: APP } })
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid=titlebar]')`), "the app to start", { timeout: 30000 })

  const ctx = makeContext(s)
  ctx.stop = async () => {
    await s.close()
    killTree(driver.pid)
  }
  ctx.restart = async (opts = {}) => {
    // Close through the window, as a person would, so state is saved on close
    // and WebView2 flushes localStorage.
    await sleep(800)
    await s.invoke("plugin:window|close", { label: "main" }).catch(() => undefined)
    await sleep(1500)
    await ctx.stop()
    return launch({ keepData: true, ...opts })
  }
  return ctx
}

function makeContext(s) {
  const ctx = {
    s,
    tid: (id) => s.find(`[data-testid="${id}"]`),
    async click(id) {
      const el = await s.waitFor(() => s.exec(`return document.querySelector('[data-testid="${id}"]')`), `[data-testid=${id}]`)
      await (await s.find(`[data-testid="${id}"]`)).click()
      return el
    },
    async nav(page) {
      await ctx.click(`nav-${page}`)
      await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="page-${page}"]')`), `page ${page}`)
      await sleep(250)
    },
    text: () => s.exec(`return document.body.innerText`),
    async waitText(re, what, timeout = 10000) {
      return s.waitFor(async () => re.test(await ctx.text()), what ?? String(re), { timeout })
    },
    /** Merge into the app's saved settings, then reload so they apply. */
    async settings(patch) {
      // The settings file is the source of truth; localStorage paints the
      // first frame. Write both, as the app does.
      const cur = (await s.invoke("load_settings")) ?? JSON.parse((await s.exec(`return localStorage.getItem('dc-settings')`)) || "{}")
      const next = { ...cur, ...patch }
      await s.invoke("save_settings", { settings: next })
      await s.exec(`localStorage.setItem('dc-settings', JSON.stringify(arguments[0]))`, next)
      await ctx.reload()
    },
    async reload() {
      await s.exec(`location.reload()`)
      await sleep(300)
      await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid=titlebar]')`), "reload", { timeout: 20000 })
      await sleep(300)
    },
    /** A visible button whose text includes `label`. */
    button: (label, scope = "") => s.waitFor(() => s.byText(`${scope} button`, label), `button "${label}"`),
    async shot(name) {
      mkdirSync(ARTIFACTS, { recursive: true })
      const png = await s.screenshot()
      writeFileSync(path.join(ARTIFACTS, `${name}.png`), png)
      return png
    },
  }
  return ctx
}
