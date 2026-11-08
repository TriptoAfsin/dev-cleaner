// README screenshots, taken from the real app on this machine's real data.
//   npm run e2e:build && node e2e/screenshots.mjs [projectsFolder]
import { mkdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { launch, ROOT } from "./lib/harness.mjs"
import { sleep } from "./lib/webdriver.mjs"

const OUT = path.join(ROOT, "docs", "screenshots")
const projectsRoot = process.argv[2] ?? path.resolve(ROOT, "..")
mkdirSync(OUT, { recursive: true })

const ctx = await launch({ realHome: true })
const { s } = ctx
const shot = async (name) => {
  await sleep(700)
  writeFileSync(path.join(OUT, `${name}.png`), await s.screenshot())
  console.log(`saved ${name}.png`)
}
const idle = (ms = 60000) => s.waitFor(async () => !(await s.exec(`return !!document.querySelector('.skeleton, .pulse-dot')`)), "loading to finish", { timeout: ms }).catch(() => {})

try {
  // A consistent window size for every shot.
  await s.cmd("POST", "/window/rect", { width: 1440, height: 900, x: 60, y: 40 }).catch(() => {})
  await ctx.settings({ theme: "dark", accent: "#1d4ed8", projectRoots: [projectsRoot], projectDepth: 6 })

  await ctx.waitText(/reclaimable|can be rebuilt/, "first scan", 120000)
  await idle(120000)
  await shot("overview-dark")

  await ctx.nav("disk")
  await idle()
  const row = await s.exec(`return document.querySelector('[data-testid=cache-list] li[data-testid^="cache-"] > div')`)
  if (row) await (await s.find('[data-testid=cache-list] li[data-testid^="cache-"] > div')).click()
  await shot("disk-dark")

  await ctx.nav("junk")
  await idle()
  await shot("junk-dark")

  await ctx.nav("projects")
  await ctx.click("scan-projects")
  await ctx.waitText(/Found \d+ build folders/, "project scan", 180000)
  await shot("projects-dark")

  await ctx.nav("models")
  const scan = await s.byText("button", "Scan")
  if (scan) await scan.click()
  await sleep(4000)
  await idle(120000)
  await shot("models-dark")

  await ctx.nav("processes")
  await idle()
  await shot("processes-dark")

  await ctx.nav("ports")
  await idle()
  await shot("ports-dark")

  await ctx.nav("system")
  await ctx.waitText(/Runtimes and tools[\s\S]*found/, "system info", 60000)
  await idle()
  await shot("system-dark")

  if (await s.exec(`return !!document.querySelector('[data-testid=nav-configs]')`)) {
    await ctx.nav("configs")
    await idle()
    await shot("configs-dark")
  }

  // Light theme
  await ctx.settings({ theme: "light" })
  await ctx.nav("overview")
  await ctx.waitText(/reclaimable|can be rebuilt/, "scan", 120000)
  await idle(120000)
  await shot("overview-light")
  await ctx.nav("memory")
  await idle()
  await shot("memory-light")
  await ctx.nav("system")
  await ctx.waitText(/Runtimes and tools[\s\S]*found/, "system info", 60000)
  await idle()
  await shot("system-light")

  // The accent picker, open
  await ctx.nav("overview")
  await ctx.click("accent-button")
  await sleep(400)
  await shot("accent-light")
} finally {
  await ctx.stop()
}
