// Page snapshots and the share card after a clean, and recommendations.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

test("the camera button renders the page to a PNG preview", async () => {
  await ctx.nav("disk")
  await ctx.click("snapshot")
  const src = await ctx.s.waitFor(
    () => ctx.s.exec(`return document.querySelector('[role=dialog] img')?.getAttribute('src') ?? null`),
    "snapshot preview",
    { timeout: 20000 },
  )
  assert.match(src, /^data:image\/png;base64,/)
  assert.ok(src.length > 10_000, "a real image, not an empty one")
  await ctx.s.press("Escape")
})

test("after a clean, the toast offers Share and the card shows what was freed", async () => {
  const { s } = ctx
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="cache-cargo"]')`), "cargo row")
  // Clear selection, pick the seeded Cargo cache, clean it.
  const all = await s.find('[aria-label="Select all shown"]')
  if (!(await all.prop("checked"))) await all.click()
  await all.click()
  await (await s.find('[aria-label="Select Cargo registry"]')).click()
  await ctx.click("clean-selected")
  await ctx.click("confirm-clean")
  await ctx.click("clean-done")
  await ctx.click("toast-action")
  await s.waitFor(async () => /Caches cleaned[\s\S]*117 KB[\s\S]*Cargo registry/.test(await s.exec(`return document.querySelector('[data-testid=report-card]')?.innerText ?? ''`)), "report card")
})

test("Overview shows recommendations", async () => {
  await ctx.s.press("Escape")
  await ctx.nav("overview")
  await ctx.s.waitFor(() => ctx.s.exec(`return document.querySelectorAll('[data-testid=insights] li').length > 0`), "at least one recommendation", { timeout: 30000 })
})
