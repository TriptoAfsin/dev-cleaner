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
  for (const page of ["disk", "junk", "projects", "models", "configs", "memory", "processes", "startup", "services", "ports", "system", "settings", "overview"]) {
    await ctx.nav(page)
    const current = await ctx.s.exec(`return document.querySelector('[aria-current=page]')?.dataset.testid`)
    assert.equal(current, `nav-${page}`)
  }
})

test("sidebar sections fold, remember it, and unfold when a page inside opens", async () => {
  const { s } = ctx
  const head = `document.querySelector('[data-testid=nav-group-manage] > button')`
  await s.exec(`${head}.click()`)
  await s.waitFor(() => s.exec(`return !document.querySelector('[data-testid=nav-processes]')`), "Manage folded")
  assert.equal(await s.exec(`return ${head}.getAttribute('aria-expanded')`), "false")
  assert.match(await s.exec(`return localStorage.getItem('dc-nav-folded')`), /manage/)
  await s.exec(`document.body.focus()`)
  await s.press("7", { ctrl: true }) // Processes
  await s.waitFor(() => s.exec(`return document.querySelector('[data-testid=nav-processes]')?.getAttribute('aria-current') === 'page'`), "Manage unfolds for Processes")
  await ctx.nav("overview")
})

test("Memory sits under Overview and shows free RAM", async () => {
  const order = await ctx.s.exec(`return [...document.querySelectorAll('nav[aria-label=Sections] [data-testid^=nav-]:not([data-testid^=nav-group])')].map(b => b.dataset.testid)`)
  assert.deepEqual(order.slice(0, 2), ["nav-overview", "nav-memory"])
  await ctx.s.waitFor(async () => /\d.*free/.test(await ctx.s.exec(`return document.querySelector('[data-testid=nav-memory]').innerText`)), "memory badge")
  const clipped = await ctx.s.exec(`return [...document.querySelectorAll('nav[aria-label=Sections] button span.truncate')].filter(e => e.scrollWidth > e.clientWidth).map(e => e.innerText)`)
  assert.deepEqual(clipped, [], "no sidebar label is cut off")
})

test("the Overview gauges open their pages", async () => {
  for (const [gauge, page] of [["cpu", "processes"], ["memory", "memory"], ["disk", "disk"]]) {
    await ctx.nav("overview")
    await ctx.s.exec(`document.querySelector('[data-testid=gauge-${gauge}]').click()`)
    await ctx.s.waitFor(() => ctx.s.exec(`return !!document.querySelector('[data-testid="page-${page}"]')`), `${gauge} → ${page}`)
  }
  await ctx.nav("overview")
})

test("Ctrl+number jumps between sections", async () => {
  const { s } = ctx
  await s.exec(`document.body.focus()`)
  await s.press("4", { ctrl: true })
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid="page-junk"]')`), "System junk via Ctrl+4")
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
