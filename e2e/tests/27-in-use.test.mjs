// A file another program has open can't be deleted. The clean leaves it,
// and the result says how many items were in use and which program holds
// them (Windows Restart Manager).
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { existsSync, mkdirSync, utimesSync, writeFileSync } from "node:fs"
import path from "node:path"
import { FAKE_HOME, killTree, launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
let holder
const held = () => path.join(FAKE_HOME, "temp", "held-by-powershell.log")

before(async () => {
  ctx = await launch()
  mkdirSync(path.dirname(held()), { recursive: true })
  writeFileSync(held(), "locked")
  const old = Date.now() / 1000 - 3 * 86400
  utimesSync(held(), old, old)
  writeFileSync(path.join(FAKE_HOME, "temp", "free.log"), "x")
  utimesSync(path.join(FAKE_HOME, "temp", "free.log"), old, old)
  // Open with no sharing, as an installer holding its log would.
  holder = spawn("powershell", ["-NoProfile", "-Command", `$f = [IO.File]::Open('${held()}', 'Open', 'Read', 'None'); Start-Sleep 120`], { stdio: "ignore" })
  await sleep(2500)
})
after(async () => {
  killTree(holder?.pid)
  await ctx?.stop()
})

test("cleaning leaves the open file, says it's in use and names the program", { skip: process.platform !== "win32" }, async () => {
  const { s } = ctx
  await ctx.nav("junk")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="cache-temp"]')`), "temp row")
  const all = await s.find('[aria-label="Select all shown"]')
  if (!(await all.prop("checked"))) await all.click()
  await all.click()
  await (await s.find('[aria-label="Select Temp files"]')).click()
  await ctx.click("clean-selected")
  await ctx.click("confirm-clean")
  const text = await s.waitFor(async () => {
    const t = await s.exec(`return document.querySelector('[data-testid=in-use]')?.innerText ?? ''`)
    return t || null
  }, "in-use explanation", { timeout: 20000 })
  assert.match(text, /1 item is open in other programs/)
  assert.match(text, /powershell/i)
  assert.equal(existsSync(held()), true, "the open file is left alone")
  assert.equal(existsSync(path.join(FAKE_HOME, "temp", "free.log")), false, "the free file is cleaned")
  await ctx.click("clean-done")
})
