// AI / ML page: the Harnesses tab lists what coding agents leave behind,
// Disk cleanup doesn't, and a clean keeps fresh session files and memory.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { existsSync } from "node:fs"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"

let ctx
before(async () => (ctx = await launch()))
after(async () => ctx?.stop())

const row = (id) => `[data-testid="cache-${id}"]`
const has = (sel) => ctx.s.exec(`return !!document.querySelector('${sel}')`)

test("the scan finds the seeded harness leftovers", async () => {
  const items = await ctx.s.invoke("scan_caches")
  const junk = items.find((i) => i.id === "claude-code-session-junk")
  assert.equal(junk.eco, "AI harnesses")
  assert.equal(junk.size, 65_000)
  assert.equal(items.find((i) => i.id === "agent-browser").size, 70_000)
})

test("the Harnesses tab lists them and Disk cleanup doesn't", async () => {
  const { s } = ctx
  await ctx.nav("disk")
  await s.waitFor(() => has(row("npm")), "disk list")
  assert.equal(await has(row("agent-browser")), false, "harness items stay off Disk cleanup")

  await ctx.nav("models")
  await ctx.click("ai-tab-harnesses")
  await s.waitFor(() => has(row("agent-browser")), "agent-browser row on the Harnesses tab")
  assert.equal(await has(row("claude-code-session-junk")), true)
  assert.equal(await has(row("npm")), false, "Disk cleanup items stay off the tab")

  await ctx.click("ai-tab-models")
  await s.waitFor(() => has('[data-testid="scan-models"]'), "Models tab")
})

test("cleaning Claude Code's debug logs keeps fresh files and memory", async () => {
  await ctx.s.invoke("clean_caches", { ids: ["claude-code-session-junk", "agent-browser"] })
  const claude = path.join(FAKE_HOME, ".claude")
  assert.equal(existsSync(path.join(claude, "debug", "old-session.txt")), false)
  assert.equal(existsSync(path.join(claude, "debug", "live-session.txt")), true, "a running session's file is younger than a day")
  assert.equal(existsSync(path.join(claude, "projects", "p", "memory", "MEMORY.md")), true, "memory is never touched")
  assert.equal(existsSync(path.join(FAKE_HOME, ".agent-browser", "tmp", "screenshots", "shot.png")), false)
})
