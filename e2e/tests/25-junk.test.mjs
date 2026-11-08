// System junk: safe items are pre-selected and cleaned; your-data items
// (Trash) are never pre-selected and are emptied only when picked; admin
// items never appear under the fake home, so no test can touch the real OS.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"

let ctx
const f = (...p) => path.join(FAKE_HOME, ...p)
function write(p, bytes = 10_000) {
  mkdirSync(path.dirname(p), { recursive: true })
  writeFileSync(p, Buffer.alloc(bytes, 98))
}

before(async () => {
  ctx = await launch()
  write(f("Downloads", "movie.mkv.crdownload"), 70_000)
  write(f("Downloads", "report.pdf"), 5_000) // a real download: never touched
  write(f("trash", "old-notes.txt"), 30_000)
  write(f("local", "Microsoft", "Windows", "INetCache", "IE", "a.dat"), 40_000)
  write(f("local", "Microsoft", "Windows", "Explorer", "thumbcache_256.db"), 50_000)
  write(f("local", "Microsoft", "Windows", "Explorer", "ExplorerStartupLog.etl"), 1_000) // not a cache
})
after(async () => ctx?.stop())

const ids = async () => (await ctx.s.invoke("scan_caches", { drives: null })).filter((c) => c.junk)

test("junk targets are found, admin items are hidden under the fake home", async () => {
  const junk = await ids()
  const byId = Object.fromEntries(junk.map((c) => [c.id, c]))
  assert.equal(byId["broken-downloads"].size, 70_000)
  assert.equal(byId["recycle-bin"].size, 30_000)
  assert.equal(byId["recycle-bin"].risk, "data")
  assert.ok(junk.every((c) => !c.admin), "no admin items when testing")
  if (process.platform === "win32") {
    assert.equal(byId["inetcache"].size, 40_000)
    assert.equal(byId["thumbcache"].size, 50_000)
  }
})

test("the System junk page lists junk and pre-selects only safe items", async () => {
  const { s } = ctx
  await ctx.nav("junk")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="cache-broken-downloads"]')`), "junk rows")
  assert.equal(await s.exec(`return !!document.querySelector('[data-testid="cache-npm"]')`), false, "dev caches stay on Disk cleanup")
  assert.equal(await (await s.find('[aria-label="Select Unfinished downloads"]')).prop("checked"), true)
  const trash = process.platform === "win32" ? "Recycle Bin" : "Trash"
  assert.equal(await (await s.find(`[aria-label="Select ${trash}"]`)).prop("checked"), false)
  assert.match(await s.exec(`return document.querySelector('[data-testid="cache-recycle-bin"]').innerText`), /your data/)
})

test("cleaning removes the junk and leaves real downloads and the Trash", async () => {
  const { s } = ctx
  await ctx.click("clean-selected")
  await ctx.click("confirm-clean")
  await ctx.waitText(/Freed/, "freed toast", 20000)
  await ctx.click("clean-done")
  assert.equal(existsSync(f("Downloads", "movie.mkv.crdownload")), false)
  assert.equal(existsSync(f("Downloads", "report.pdf")), true)
  assert.equal(existsSync(f("trash", "old-notes.txt")), true, "Trash is never emptied unless picked")
  if (process.platform === "win32") {
    assert.equal(existsSync(f("local", "Microsoft", "Windows", "Explorer", "thumbcache_256.db")), false)
    assert.equal(existsSync(f("local", "Microsoft", "Windows", "Explorer", "ExplorerStartupLog.etl")), true)
  }
  await s.waitFor(() => s.exec(`return !document.querySelector('[role=dialog]')`), "dialog closed")
})

test("picking the Trash warns that it can't be undone, then empties it", async () => {
  const { s } = ctx
  const trash = process.platform === "win32" ? "Recycle Bin" : "Trash"
  const all = await s.find('[aria-label="Select all shown"]')
  if (!(await all.prop("checked"))) await all.click()
  await all.click()
  await s.waitFor(async () => !(await (await s.find(`[aria-label="Select ${trash}"]`)).prop("checked")), "cleared")
  await (await s.find(`[aria-label="Select ${trash}"]`)).click()
  await ctx.click("clean-selected")
  await s.waitFor(() => s.byText("[role=dialog]", "can't be undone"), "your-data warning")
  await ctx.click("confirm-clean")
  await s.waitFor(() => readdirSync(f("trash")).length === 0, "trash emptied", { timeout: 15000 })
  await ctx.click("clean-done")
})
