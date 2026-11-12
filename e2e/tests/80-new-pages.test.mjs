// 0.7 pages: Space, memory pressure, orphaned processes, Deep clean and
// Responsiveness open, read real data, and stay inside the fake home.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const has = (id) => ctx.s.exec(`return !!document.querySelector('[data-testid="${id}"]')`)

test("Space scans and draws the map", async () => {
  await ctx.nav("space")
  await ctx.s.waitFor(() => has("space-page"), "space page")
  await ctx.click("space-scan")
  await ctx.s.waitFor(async () => (await ctx.s.invoke("space_last")) != null, "scan finishes", { timeout: 120000 })
  const last = await ctx.s.invoke("space_last")
  assert.ok(last.files >= 0 && last.total >= 0)
  assert.equal(last.cats.length, 5)
})

test("Memory shows a pressure level from the sampler", async () => {
  const now = await ctx.s.invoke("pressure_now")
  assert.ok(now.commitLimit > 0 && now.memTotal > 0)
  assert.ok(["low", "moderate", "high"].includes(now.level))
  await ctx.nav("memory")
  await ctx.s.waitFor(() => has("pressure-card"), "pressure card")
  await ctx.s.waitFor(() => has("pressure-level"), "pressure level")
})

test("Processes has an Orphaned tab", async () => {
  await ctx.nav("processes")
  await ctx.click("proc-tab-orphaned")
  await ctx.waitText(/Orphaned|orphan/i, "orphaned view", 20000)
  assert.ok(Array.isArray(await ctx.s.invoke("list_orphans")))
})

test("System junk opens Deep clean as a dry run", async () => {
  await ctx.nav("junk")
  await ctx.click("junk-tab-deep")
  await ctx.s.waitFor(() => has("deep-clean"), "deep clean", { timeout: 60000 })
  await ctx.click("junk-tab-quick")
})

test("Responsiveness lists tweaks and a history", async () => {
  await ctx.nav("tune")
  await ctx.s.waitFor(() => has("tune-list"), "tweak list", { timeout: 30000 })
  const o = await ctx.s.invoke("tune_list")
  assert.ok(o && typeof o === "object")
  await ctx.click("tune-tab-history")
  await ctx.waitText(/No changes yet|newest first/, "history", 15000)
})
