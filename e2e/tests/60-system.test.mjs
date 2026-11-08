// System info: real facts from this machine, tooltips that explain them, and
// the Markdown report.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { launch } from "../lib/harness.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

test("the backend reports CPU, memory and the Node.js on PATH", async () => {
  const info = await ctx.s.invoke("system_info")
  assert.ok(info.cpuThreads >= info.cpuCores && info.cpuCores > 0)
  assert.ok(info.memTotal > 1e9)
  assert.equal(typeof info.compression, "boolean")
  const node = info.runtimes.find((t) => t.name === "Node.js")
  assert.equal(node.installed, true)
  assert.equal(node.version, execFileSync("node", ["--version"]).toString().trim().replace(/^v/, ""))
})

test("the System page renders sections, runtimes, editors and AI tools", async () => {
  await ctx.nav("system")
  for (const re of [/Processor/, /Memory compression/, /Page file|Swap/, /Drives/, /Runtimes and tools[\s\S]*Node\.js/, /Editors/, /AI coding CLIs/, /Recommendations/]) {
    await ctx.waitText(re, String(re), 30000)
  }
})

test("hovering an info icon shows a tooltip that explains the value", async () => {
  const { s } = ctx
  const icon = await s.waitFor(async () => {
    const r = await s.exec(`return [...document.querySelectorAll('span')].find(sp => sp.textContent.trim() === 'Memory compression')?.querySelector('button') ?? null`)
    return r ? s.find(`[aria-label="What does this mean?"]`) && r : null
  }, "info icon")
  await s.cmd("POST", "/actions", {
    actions: [{ type: "pointer", id: "m", parameters: { pointerType: "mouse" }, actions: [{ type: "pointerMove", duration: 0, origin: icon, x: 0, y: 0 }] }],
  })
  await s.waitFor(async () => /compresses memory/.test(await s.exec(`return document.querySelector('[role=tooltip]')?.innerText ?? ''`)), "tooltip text")
  await s.cmd("DELETE", "/actions")
})

test("keyboard focus shows the tooltip at once", async () => {
  const { s } = ctx
  await s.exec(`document.querySelector('[aria-label="What does this mean?"]').focus()`)
  await s.waitFor(() => s.exec(`return !!document.querySelector('[role=tooltip]')`), "tooltip on focus", { timeout: 1000 })
})

test("Copy as Markdown confirms with a toast", async () => {
  await (await ctx.button("Copy as Markdown")).click()
  await ctx.waitText(/Copied system report/, "copy toast")
})
