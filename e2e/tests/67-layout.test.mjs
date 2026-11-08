// Layout behaviour people notice: table headers and footers stay on screen
// while a long list scrolls, Free memory explains itself, the System page
// keeps recommendations in their own tab, and the Drives setting lists drives.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

/** Scroll the table's own list and report how the panel looks. */
const panel = () =>
  ctx.s.exec(`
    const list = document.querySelector('[data-testid=cache-list]')
    const card = list.parentElement
    list.scrollTop = list.scrollHeight / 2
    const page = document.querySelector('main [data-testid^="page-"]')
    const view = page.getBoundingClientRect()
    const c = card.getBoundingClientRect()
    const l = list.getBoundingClientRect()
    const head = list.previousElementSibling.getBoundingClientRect()
    const foot = card.lastElementChild.getBoundingClientRect()
    return {
      listScrolls: list.scrollHeight > list.clientHeight + 40 && list.scrollTop > 0,
      pageStill: page.scrollTop === 0,
      cardInView: c.top >= view.top - 1 && c.bottom <= view.bottom + 1,
      headAboveRows: head.bottom <= l.top + 1,
      footBelowRows: foot.top >= l.bottom - 1,
    }`)

test("long tables scroll inside their card; header and footer frame the rows", async () => {
  await ctx.s.cmd("POST", "/window/rect", { width: 1100, height: 640 }).catch(() => {})
  await ctx.nav("disk")
  await ctx.s.waitFor(() => ctx.s.exec(`return document.querySelectorAll('[data-testid=cache-list] li').length > 10`), "cache rows")
  await sleep(300)
  const r = await panel()
  assert.deepEqual(r, { listScrolls: true, pageStill: true, cardInView: true, headAboveRows: true, footBelowRows: true })
})

test("every cleanup row has a tooltip saying what it is, what it deletes and what runs", async () => {
  const { s } = ctx
  await s.exec(`document.querySelector('[data-testid=cache-list]').scrollTop = 0`)
  await s.exec(`document.querySelector('[data-testid="cache-npm"] [aria-label="What does this mean?"]').focus()`)
  await s.waitFor(async () => /npm cache[\s\S]*Deletes:[\s\S]*Runs:[\s\S]*Risk:/.test(await s.exec(`return document.querySelector('[role=tooltip]')?.innerText ?? ''`)), "row tooltip")
  const missing = await s.exec(`return [...document.querySelectorAll('[data-testid=cache-list] li[data-testid^="cache-"]')].filter(li => !li.querySelector('[aria-label="What does this mean?"]')).length`)
  assert.equal(missing, 0)
})

test("hovering Free memory explains exactly what it does", async () => {
  const { s } = ctx
  await ctx.nav("overview")
  const btn = await ctx.tid("free-memory")
  await s.cmd("POST", "/actions", {
    actions: [{ type: "pointer", id: "m", parameters: { pointerType: "mouse" }, actions: [{ type: "pointerMove", duration: 0, origin: { "element-6066-11e4-a52e-4f735466cecf": btn.id }, x: 0, y: 0 }] }],
  })
  await s.waitFor(async () => /How it works on Windows[\s\S]*EmptyWorkingSet[\s\S]*Nothing is closed/.test(await s.exec(`return document.querySelector('[role=tooltip]')?.innerText ?? ''`)), "free memory tooltip")
  await s.cmd("DELETE", "/actions")
})

test("System keeps recommendations in their own tab", async () => {
  await ctx.nav("system")
  assert.equal(await ctx.s.exec(`return !!document.querySelector('[data-testid=insights]')`), false, "not shown on Details")
  await ctx.click("system-tab-tips")
  await ctx.s.waitFor(() => ctx.s.exec(`return !!document.querySelector('[data-testid=insights]')`), "recommendations tab")
  await ctx.click("system-tab-details")
  await ctx.waitText(/Processor/, "details back")
})

test("Settings lists the drives and remembers the choice", async () => {
  const { s } = ctx
  await ctx.nav("settings")
  await s.waitFor(() => s.exec(`return document.querySelectorAll('[data-testid=drives-setting] input[type=checkbox]').length > 0`), "drive rows")
  const first = await s.find('[data-testid=drives-setting] input[type=checkbox]')
  await first.click()
  await s.waitFor(async () => Array.isArray((await s.invoke("load_settings"))?.drives), "drive choice saved")
  await (await s.byText("[data-testid=drives-setting] button", "Select all")).click()
  await s.waitFor(async () => (await s.invoke("load_settings"))?.drives === null, "back to all drives")
})

test("every button and control shows the pointer cursor", async () => {
  await ctx.nav("settings")
  const wrong = await ctx.s.exec(`
    const els = [...document.querySelectorAll('button:not(:disabled), [role=tab], [role=switch], input[type=checkbox]:not(:disabled), a[href]')]
    return els.filter(el => getComputedStyle(el).cursor !== 'pointer').map(el => el.outerHTML.slice(0, 80))`)
  assert.deepEqual(wrong, [])
})
