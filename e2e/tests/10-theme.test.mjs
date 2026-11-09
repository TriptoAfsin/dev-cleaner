// Appearance popover (theme + accent), and both surviving a restart.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { launch } from "../lib/harness.mjs"
import { sleep } from "../lib/webdriver.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const isDark = () => ctx.s.exec(`return document.documentElement.classList.contains('dark')`)
const accent = () => ctx.s.exec(`return getComputedStyle(document.documentElement).getPropertyValue('--accent-base').trim()`)

const POPOVER = `[role=dialog][aria-label="Appearance"]`

test("the appearance popover switches between light and dark", async () => {
  const { s } = ctx
  await ctx.click("appearance-button")
  await s.waitFor(() => s.exec(`return !!document.querySelector('${POPOVER}')`), "appearance popover")
  await ctx.click("appearance-theme-dark")
  await s.waitFor(isDark, "dark theme")
  await ctx.click("appearance-theme-light")
  await s.waitFor(async () => !(await isDark()), "light theme")
  await s.press("Escape")
  await s.waitFor(() => s.exec(`return !document.querySelector('${POPOVER}')`), "popover closed")
})

test("the appearance popover opens, applies a preset, and closes with Escape", async () => {
  const { s } = ctx
  await ctx.click("appearance-button")
  await s.waitFor(() => s.exec(`return !!document.querySelector('${POPOVER}')`), "accent popover")
  await ctx.click("accent-violet")
  await s.waitFor(async () => (await accent()) === "#7c3aed", "violet accent")
  await s.press("Escape")
  await s.waitFor(() => s.exec(`return !document.querySelector('${POPOVER}')`), "popover closed")
})

test("clicking outside the appearance popover closes it", async () => {
  const { s } = ctx
  await ctx.click("appearance-button")
  await s.waitFor(() => s.exec(`return !!document.querySelector('${POPOVER}')`), "popover open")
  await (await s.find("main")).click()
  await s.waitFor(() => s.exec(`return !document.querySelector('${POPOVER}')`), "popover closed")
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
