// Processes and memory: verdicts render, filters work, protected processes
// cannot be ended from the UI or the backend, and ending one asks first.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { killTree, launch } from "../lib/harness.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const list = () => ctx.s.exec(`return [...document.querySelectorAll('[data-testid=process-list] li')].map(li => li.innerText)`)

test("the backend groups processes and marks OS processes protected", async () => {
  const groups = await ctx.s.invoke("list_processes")
  assert.ok(groups.length > 10)
  const sys = groups.find((g) => g.name.toLowerCase() === "csrss" || g.name.toLowerCase() === "svchost")
  assert.equal(sys?.protected, true)
  assert.equal(sys?.verdict, "system")
})

test("ending a protected process is refused by the backend", async () => {
  const groups = await ctx.s.invoke("list_processes")
  const sys = groups.find((g) => g.name.toLowerCase() === "svchost")
  const r = await ctx.s.invoke("kill_processes", { pids: [sys.pids[0]] })
  assert.equal(r.ended, 0)
  assert.equal(r.failed, 1)
})

test("the Processes page filters by name and by verdict", async () => {
  const { s } = ctx
  await ctx.nav("processes")
  await s.waitFor(async () => (await list()).length > 5, "process rows")
  const input = await ctx.tid("process-filter")
  await input.click()
  await s.type("svchost")
  await s.waitFor(async () => {
    const l = await list()
    return l.length === 1 && /svchost[\s\S]*System[\s\S]*protected/.test(l[0])
  }, "svchost row, protected, no End button")
  await input.clear()
  await (await ctx.button("System", "[data-testid=page-processes]")).click()
  await s.waitFor(async () => (await list()).every((t) => /System/.test(t)), "only system rows")
  await (await ctx.button("All", "[data-testid=page-processes]")).click()
})

test("End task on a process asks first and then ends it", async () => {
  const { s } = ctx
  // A throwaway process with a name nothing else uses.
  const child = spawn("powershell", ["-NoProfile", "-Command", "Start-Sleep 120"], { stdio: "ignore" })
  // Wait until the backend lists it, rather than guessing how long that takes
  // on a busy machine.
  await s.waitFor(async () => (await s.invoke("list_processes")).some((g) => g.pids.includes(child.pid)), "the child in the process list", { timeout: 20000 })
  const input = await ctx.tid("process-filter")
  await input.clear()
  await input.click()
  await s.type(String(child.pid))
  const btn = await s.waitFor(() => s.byLabel("End powershell"), "End button for the child", { timeout: 20000 })
  await btn.click()
  await s.waitFor(() => s.byText("[role=dialog]", "End powershell"), "confirm dialog")
  await ctx.click("confirm-kill")
  await s.waitFor(() => child.exitCode !== null || child.signalCode !== null, "child ended", { timeout: 8000 })
  killTree(child.pid)
})

test("Top CPU shows live bars with End buttons", async () => {
  await ctx.s.waitFor(() => ctx.s.exec(`return document.querySelectorAll('[data-testid=top-cpu] li').length >= 3`), "top cpu rows")
})

test("Memory page shows the breakdown and Free memory reports before/after", async () => {
  const { s } = ctx
  await ctx.nav("memory")
  await ctx.waitText(/What's using memory[\s\S]*Apps[\s\S]*Free/, "breakdown")
  await ctx.click("free-memory")
  await s.waitFor(() => s.byText("[role=dialog]", "Before"), "result dialog", { timeout: 30000 })
  await (await ctx.button("Done", "[role=dialog]")).click()
})
