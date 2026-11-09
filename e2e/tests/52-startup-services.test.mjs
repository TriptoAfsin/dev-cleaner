// Startup and Services. Startup reads a seeded Startup folder in the fake
// home and keeps its on/off state in a file there, so the registry is never
// touched. Services lists the real machine read-only: every change is
// refused under the fake home, and protected services are refused always.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"
import { ARTIFACTS, FAKE_HOME, launch } from "../lib/harness.mjs"

const STARTUP = path.join(FAKE_HOME, "roaming", "Microsoft", "Windows", "Start Menu", "Programs", "Startup")

let ctx
before(async () => {
  ctx = await launch()
  mkdirSync(STARTUP, { recursive: true })
  writeFileSync(path.join(STARTUP, "Spotify.lnk"), "")
  writeFileSync(path.join(STARTUP, "SecurityHealth.lnk"), "")
})
after(async () => ctx?.stop())

test("Startup lists the seeded programs with verdicts", async () => {
  const items = await ctx.s.invoke("list_startup")
  const spotify = items.find((i) => i.name === "Spotify")
  assert.ok(spotify, "Spotify is listed")
  assert.equal(spotify.enabled, true)
  assert.equal(spotify.verdict, "optional")
  assert.equal(items.find((i) => i.name === "SecurityHealth").verdict, "keep")
})

test("turning a program off and on uses its switch and sticks", async () => {
  const { s } = ctx
  await ctx.nav("startup")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="startup-Spotify"]')`), "Spotify row")
  assert.match(await s.exec(`return document.querySelector('[data-testid="startup-Spotify"]').innerText`), /Fine to turn off/)
  mkdirSync(ARTIFACTS, { recursive: true })
  writeFileSync(path.join(ARTIFACTS, "startup.png"), await s.screenshot())
  await (await s.find('[aria-label="Start Spotify at sign-in"]')).click()
  await ctx.waitText(/Spotify won't start at sign-in/, "toast")
  const state = JSON.parse(readFileSync(path.join(FAKE_HOME, "startup-approved.json"), "utf8"))
  assert.equal(state["Spotify.lnk"], false)
  assert.equal((await s.invoke("list_startup")).find((i) => i.name === "Spotify").enabled, false)
  await (await s.find('[aria-label="Start Spotify at sign-in"]')).click()
  await s.waitFor(async () => (await s.invoke("list_startup")).find((i) => i.name === "Spotify").enabled === true, "back on")
})

test("the backend refuses to change protected services, and anything under the fake home", async () => {
  const list = await ctx.s.invoke("list_services")
  assert.ok(list.length > 0)
  const prot = list.find((x) => x.verdict === "protected")
  if (prot) {
    assert.equal(prot.canChange, false)
    await assert.rejects(ctx.s.invoke("service_action", { name: prot.name, action: "stop" }), /protected/)
  }
  const other = list.find((x) => x.canChange)
  if (other) await assert.rejects(ctx.s.invoke("service_action", { name: other.name, action: "manual" }), /off in tests/)
  await assert.rejects(ctx.s.invoke("service_action", { name: "x", action: "delete" }), /Unknown action/)
})

test("the Services page shows a protected service locked, and the admin banner when not elevated", async () => {
  const { s } = ctx
  const list = await s.invoke("list_services")
  const prot = list.find((x) => x.verdict === "protected")
  await ctx.nav("services")
  await s.waitFor(() => s.exec(`return document.querySelectorAll('[data-testid=service-list] li button').length > 0`), "services listed", { timeout: 30000 })
  const p = await s.invoke("privileges_info")
  if (process.platform === "win32" && !p.isAdmin) {
    await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid=admin-banner]')`), "admin banner")
    assert.match(await s.exec(`return document.querySelector('[data-testid=admin-banner]').innerText`), /Restart as admin/)
  }
  if (!prot) return
  const input = await ctx.tid("service-filter")
  await input.click()
  await s.type(prot.name)
  await ctx.click(`service-${prot.name}`)
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid=service-locked]')`), "locked note")
  writeFileSync(path.join(ARTIFACTS, "services.png"), await s.screenshot())
  assert.equal(await s.exec(`return !!document.querySelector('[data-testid=start-disabled]')`), false, "no startup-type switch")
})
