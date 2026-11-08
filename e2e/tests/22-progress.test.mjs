// Clean progress: while a big cache is deleted the dialog shows an overall
// bar, what is happening now and a timer; the result says how long it took.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { mkdirSync, writeFileSync, readdirSync } from "node:fs"
import { ARTIFACTS } from "../lib/harness.mjs"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"

let ctx
before(async () => {
  ctx = await launch()
  // Enough small files that deleting them takes a visible moment.
  const dir = path.join(FAKE_HOME, "local", "npm-cache", "_cacache", "content-v2")
  for (let d = 0; d < 60; d++) {
    const sub = path.join(dir, `d${d}`)
    mkdirSync(sub, { recursive: true })
    for (let f = 0; f < 500; f++) writeFileSync(path.join(sub, `f${f}`), "x".repeat(200))
  }
})
after(async () => ctx?.stop())

test("cleaning shows live progress, then how long it took", async () => {
  const { s } = ctx
  await ctx.reload() // rescans with the seeded files
  await ctx.nav("disk")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="cache-npm"]')`), "npm row")
  const all = await s.find('[aria-label="Select all shown"]')
  if (await all.prop("checked")) await all.click()
  else {
    await all.click()
    await all.click()
  }
  await (await s.find('[aria-label="Select npm cache"]')).click()
  await ctx.click("clean-selected")
  await s.waitFor(() => s.byText("[role=dialog]", "npm cache"), "confirm dialog")
  await ctx.click("confirm-clean")
  const seen = await s.waitFor(
    () => s.exec(`const p = document.querySelector('[data-testid=clean-progress]'); return p ? document.querySelector('[role=dialog]').innerText : null`),
    "progress view",
    { timeout: 10000 },
  )
  mkdirSync(ARTIFACTS, { recursive: true })
  writeFileSync(path.join(ARTIFACTS, "clean-progress.png"), await s.screenshot())
  assert.match(seen, /Cleaning 1 of 1 · \d/, "title shows position and a clock")
  const done = await ctx.waitText(/Freed [\d.]+ [KM]B in [\d.]+s/, "result with total time", 60000)
  assert.ok(done)
  const text = await s.exec(`return document.querySelector('[role=dialog]').innerText`)
  assert.match(text, /\d+(\.\d)?s/, "per-item time")
  await ctx.click("clean-done")
  assert.deepEqual(readdirSync(path.join(FAKE_HOME, "local", "npm-cache")), [])
})
