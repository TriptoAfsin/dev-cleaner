// Raw captures for the Microsoft Store listing, from the real app on this
// machine's real data. SHOTS_DIR is where they go.
//   npm run e2e:build && SHOTS_DIR=... node e2e/store-shots.mjs [projectsFolder]
import { mkdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { launch, ROOT } from "./lib/harness.mjs"
import { sleep } from "./lib/webdriver.mjs"

const OUT = process.env.SHOTS_DIR
const projectsRoot = process.argv[2] ?? path.resolve(ROOT, "..")
mkdirSync(OUT, { recursive: true })
const ctx = await launch({ realHome: true })
const { s } = ctx
const shot = async (name) => {
  await sleep(900)
  writeFileSync(path.join(OUT, `${name}.png`), await s.screenshot())
  console.log(`saved ${name}`)
}
const idle = (ms = 60000) => s.waitFor(async () => !(await s.exec(`return !!document.querySelector('.skeleton, .pulse-dot')`)), "idle", { timeout: ms }).catch(() => {})
const has = (sel) => s.exec(`return !!document.querySelector('${sel}')`)
try {
  await s.cmd("POST", "/window/rect", { width: 1240, height: 800, x: 60, y: 40 }).catch(() => {})
  await ctx.settings({ theme: "dark", accent: "#1d4ed8", projectRoots: [projectsRoot], projectDepth: 6 })
  await ctx.waitText(/reclaimable|can be rebuilt/, "first scan", 180000)
  await idle(120000)
  await shot("overview-dark")

  await ctx.nav("space")
  await ctx.click("space-scan")
  await s.waitFor(async () => (await s.invoke("space_last")) != null, "space scan", { timeout: 180000 })
  await idle()
  await shot("space-dark")

  await ctx.nav("memory")
  await idle()
  await sleep(2500)
  await shot("memory-dark")

  await ctx.nav("tune")
  await idle()
  await shot("tune-dark")

  await ctx.nav("junk")
  await ctx.click("junk-tab-deep")
  await s.waitFor(() => has("[data-testid=deep-clean]"), "deep", { timeout: 120000 })
  await idle(120000)
  await shot("deepclean-dark")
  await ctx.click("junk-tab-quick")

  await ctx.nav("disk")
  await idle()
  if (await has('[data-testid=cache-list] li[data-testid^="cache-"] > div')) await (await s.find('[data-testid=cache-list] li[data-testid^="cache-"] > div')).click()
  await shot("disk-dark")

  await ctx.nav("processes")
  await ctx.click("proc-tab-orphaned")
  await idle()
  await sleep(3000)
  await shot("orphans-dark")
  await ctx.click("proc-tab-apps")

  await ctx.nav("projects")
  await ctx.click("scan-projects")
  await ctx.waitText(/Found \d+ build folders/, "project scan", 240000)
  await shot("projects-dark")

  await ctx.nav("ports")
  await idle()
  await shot("ports-dark")

  await ctx.settings({ theme: "light" })
  await ctx.nav("space")
  await idle()
  await shot("space-light")
} finally {
  await ctx.stop()
}
