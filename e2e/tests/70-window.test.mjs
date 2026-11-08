// Window state: position and size come back after a restart, a maximized
// window comes back maximized, and a position on a monitor that no longer
// exists falls back to a visible one.
import { test, after } from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import path from "node:path"
import { CONFIG_DIR, launch } from "../lib/harness.mjs"

let ctx
after(async () => ctx?.stop())

const pos = () => ctx.s.invoke("plugin:window|outer_position", { label: "main" })
const monitors = () => ctx.s.invoke("plugin:window|available_monitors", { label: "main" })
const saved = () => JSON.parse(readFileSync(path.join(CONFIG_DIR, "window-state.json"), "utf8"))

test("a saved position and size are restored on launch", async () => {
  ctx = await launch({ windowState: { x: 180, y: 140, width: 1100, height: 700, maximized: false } })
  const p = await pos()
  assert.ok(Math.abs(p.x - 180) <= 2 && Math.abs(p.y - 140) <= 2, `position ${JSON.stringify(p)}`)
})

test("closing the window saves where it was", async () => {
  ctx = await ctx.restart()
  const s = saved()
  assert.ok(Math.abs(s.x - 180) <= 2 && Math.abs(s.y - 140) <= 2, `saved ${JSON.stringify(s)}`)
  assert.ok(s.width >= 1000 && s.height >= 600)
})

test("the size does not creep between restarts", async () => {
  const first = saved()
  ctx = await ctx.restart()
  ctx = await ctx.restart()
  const later = saved()
  assert.ok(Math.abs(later.width - first.width) <= 2, `width ${first.width} -> ${later.width}`)
  assert.ok(Math.abs(later.height - first.height) <= 2, `height ${first.height} -> ${later.height}`)
})

test("a maximized window comes back maximized", async () => {
  await ctx.s.invoke("plugin:window|maximize", { label: "main" }).catch(async () => (await ctx.s.byLabel("Maximize")).click())
  await ctx.s.waitFor(() => ctx.s.invoke("plugin:window|is_maximized", { label: "main" }), "maximized")
  ctx = await ctx.restart()
  // Maximized while still hidden, just before it is shown.
  await ctx.s.waitFor(() => ctx.s.invoke("plugin:window|is_maximized", { label: "main" }), "maximized after restart", { timeout: 5000 })
  assert.equal(saved().maximized, true)
  assert.ok(Math.abs(saved().x - 180) <= 2, "the normal position is kept for un-maximizing")
})

test("a position on an unplugged monitor falls back onto a real one", async () => {
  await ctx.stop()
  ctx = await launch({ windowState: { x: 40000, y: 20000, width: 1100, height: 700, maximized: false } })
  const p = await pos()
  const ms = await monitors()
  const visible = ms.some((m) => p.x >= m.position.x - 10 && p.x < m.position.x + m.size.width && p.y >= m.position.y - 10 && p.y < m.position.y + m.size.height)
  assert.ok(visible, `window at ${JSON.stringify(p)} is on a monitor`)
})
