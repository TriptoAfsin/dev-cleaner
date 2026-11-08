// Disk cleanup against seeded fake caches: scan, explain, select, confirm,
// clean, and check the files on disk afterwards.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { existsSync, readdirSync } from "node:fs"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const row = (id) => `[data-testid="cache-${id}"]`

test("the scan finds the seeded caches with their sizes", async () => {
  const items = await ctx.s.invoke("scan_caches")
  const npm = items.find((i) => i.id === "npm")
  assert.equal(npm.found, true)
  assert.equal(npm.size, 500_000)
  assert.equal(items.find((i) => i.id === "cargo").size, 120_000)
  assert.equal(items.find((i) => i.id === "docker").found, false, "docker is never probed under the fake home")
})

test("the Disk page lists caches and a row expands to explain itself", async () => {
  const { s } = ctx
  await ctx.nav("disk")
  await s.waitFor(() => s.exec(`return !!document.querySelector('${row("npm")}')`), "npm row")
  assert.match(await s.exec(`return document.querySelector('${row("npm")}').innerText`), /npm cache[\s\S]*488 KB/)
  await (await s.find(`${row("npm")} > div`)).click()
  await s.waitFor(async () => /What[\s\S]*Runs[\s\S]*Delete folder/.test(await s.exec(`return document.querySelector('${row("npm")}').innerText`)), "explanation")
})

test("confirming a clean empties only the selected cache", async () => {
  const { s } = ctx
  // Select only npm: clear everything, then tick npm.
  const all = await s.find('[aria-label="Select all shown"]')
  if (await all.prop("checked")) await all.click()
  else {
    await all.click()
    await all.click()
  }
  await (await s.find('[aria-label="Select npm cache"]')).click()
  await ctx.click("clean-selected")
  await s.waitFor(() => s.byText("[role=dialog]", "npm cache"), "confirm dialog lists npm")
  const dialogText = await s.exec(`return document.querySelector('[role=dialog]').innerText`)
  assert.doesNotMatch(dialogText, /Cargo registry/, "only the selection is in the dialog")
  await ctx.click("confirm-clean")
  await ctx.waitText(/Freed 488 KB/, "freed toast", 15000)
  const npmDir = path.join(FAKE_HOME, "local", "npm-cache")
  assert.ok(existsSync(npmDir), "the cache folder itself is kept")
  assert.deepEqual(readdirSync(npmDir), [])
  assert.ok(existsSync(path.join(FAKE_HOME, ".cargo", "registry", "cache", "serde.crate")), "cargo untouched")
})

test("the temp clean removes only items older than a day", async () => {
  await ctx.s.invoke("clean_caches", { ids: ["temp"] })
  assert.equal(existsSync(path.join(FAKE_HOME, "temp", "old-installer.log")), false)
  assert.equal(existsSync(path.join(FAKE_HOME, "temp", "fresh.lock")), true)
})

test("after cleaning, the list re-measures", async () => {
  await sleep(500)
  await ctx.s.waitFor(async () => {
    const t = await ctx.s.exec(`return document.querySelector('${row("npm")}')?.innerText ?? ''`)
    return /0 B|not found/.test(t)
  }, "npm row to show 0 B")
})
