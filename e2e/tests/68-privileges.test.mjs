// Privileges per OS. On Windows: the status bar says whether the app is
// elevated and offers "Restart as admin" behind a confirmation; Settings
// explains it. The test never confirms: that would show a real UAC prompt.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const onWindows = { skip: process.platform !== "win32" }

test("the backend reports this OS's privileges", async () => {
  const p = await ctx.s.invoke("privileges_info")
  assert.equal(p.os, process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux")
  assert.equal(p.canRelaunch, process.platform === "win32")
  if (process.platform !== "darwin") assert.equal(p.fullDiskAccess, null)
})

test("the status bar shows admin state and asks before restarting", onWindows, async () => {
  const { s } = ctx
  const status = await s.waitFor(() => s.exec(`return document.querySelector('[data-testid=admin-status]')`), "admin status")
  const p = await s.invoke("privileges_info")
  assert.match(await (await ctx.tid("admin-status")).text(), p.isAdmin ? /admin: yes/ : /admin: no/)
  if (p.isAdmin) return
  assert.ok(status)
  await ctx.click("admin-status")
  await s.waitFor(() => s.byText("[role=dialog]", "Restart as administrator?"), "confirmation dialog")
  await (await ctx.button("Cancel", "[role=dialog]")).click()
  await s.waitFor(() => s.exec(`return !document.querySelector('[role=dialog]')`), "dialog closed without restarting")
})

test("Settings explains permissions for this OS", async () => {
  await ctx.nav("settings")
  const text = await ctx.s.waitFor(() => ctx.s.exec(`return document.querySelector('[data-testid=permissions]')?.innerText ?? null`), "permissions card")
  if (process.platform === "win32") assert.match(text, /Restart as admin|Administrator/)
  if (process.platform === "darwin") assert.match(text, /Full Disk Access/)
  if (process.platform === "linux") assert.match(text, /pkexec/)
})
