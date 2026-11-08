// Light/dark toggle, accent picker, and both surviving a restart.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const isDark = () => ctx.s.exec(`return document.documentElement.classList.contains('dark')`)
const accent = () => ctx.s.exec(`return getComputedStyle(document.documentElement).getPropertyValue('--accent-base').trim()`)

test("the round toggle flips between light and dark", async () => {
  const before = await isDark()
  await ctx.click("theme-toggle")
  await ctx.s.waitFor(async () => (await isDark()) !== before, "theme to flip")
  await ctx.click("theme-toggle")
  await ctx.s.waitFor(async () => (await isDark()) === before, "theme to flip back")
})

test("the accent picker opens, applies a preset, and closes with Escape", async () => {
  const { s } = ctx
  await ctx.click("accent-button")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[role=dialog][aria-label="Choose accent colour"]')`), "accent popover")
  await ctx.click("accent-violet")
  await s.waitFor(async () => (await accent()) === "#7c3aed", "violet accent")
  await s.press("Escape")
  await s.waitFor(() => s.exec(`return !document.querySelector('[aria-label="Choose accent colour"]')`), "popover closed")
})

test("clicking outside the accent popover closes it", async () => {
  const { s } = ctx
  await ctx.click("accent-button")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[aria-label="Choose accent colour"]')`), "popover open")
  await (await s.find("main")).click()
  await s.waitFor(() => s.exec(`return !document.querySelector('[aria-label="Choose accent colour"]')`), "popover closed")
})

test("Settings page theme radio sets dark explicitly", async () => {
  await ctx.nav("settings")
  await ctx.click("theme-dark")
  await ctx.s.waitFor(isDark, "dark theme")
  assert.equal(await ctx.s.exec(`return document.querySelector('[data-testid=theme-dark]').getAttribute('aria-checked')`), "true")
})

test("theme and accent survive a restart", async () => {
  await sleep(300)
  ctx = await ctx.restart()
  assert.equal(await isDark(), true)
  assert.equal(await accent(), "#7c3aed")
})
