// The app shell: it starts, every section opens, keyboard shortcuts work,
// and the status bar shows live figures from the real backend.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

test("starts on Overview with the custom title bar and live stats", async () => {
  const { s } = ctx
  assert.ok(await s.exec(`return !!document.querySelector('[data-testid="page-overview"]')`))
  assert.match(await ctx.text(), /Dev Cleaner/)
  const stats = await s.invoke("live_stats")
  assert.ok(stats.memTotal > 0, "memory total from the backend")
  await s.waitFor(async () => /cpu \d+%/.test(await s.exec(`return document.querySelector('[data-testid=statusbar]').innerText`)), "status bar figures")
})

test("every section opens from the sidebar and marks itself current", async () => {
  for (const page of ["disk", "projects", "memory", "processes", "ports", "system", "settings", "overview"]) {
    await ctx.nav(page)
    const current = await ctx.s.exec(`return document.querySelector('[aria-current=page]')?.dataset.testid`)
    assert.equal(current, `nav-${page}`)
  }
})

test("Ctrl+number jumps between sections", async () => {
  const { s } = ctx
  await s.exec(`document.body.focus()`)
  await s.press("3", { ctrl: true })
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="page-projects"]')`), "projects via Ctrl+3")
  await s.press("1", { ctrl: true })
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="page-overview"]')`), "overview via Ctrl+1")
})

test("the window buttons are real buttons with labels", async () => {
  for (const label of ["Minimize", "Maximize", "Close"]) {
    assert.ok(await ctx.s.byLabel(label), `${label} button`)
  }
})

test("maximize and restore from the title bar keep the app usable", async () => {
  const { s } = ctx
  await (await s.byLabel("Maximize")).click()
  await s.waitFor(() => s.invoke("plugin:window|is_maximized", { label: "main" }), "window maximized")
  await sleep(300)
  await (await s.byLabel("Restore")).click()
  await s.waitFor(async () => !(await s.invoke("plugin:window|is_maximized", { label: "main" })), "window restored")
})
