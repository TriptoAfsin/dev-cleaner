// Definitions: the built-in set is reported in Settings; a set placed in the
// data folder is picked up by Refresh and its rules show up in a scan; a
// broken set is ignored with the reason shown, and the built-in set is used.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import path from "node:path"
import { CONFIG_DIR, FAKE_HOME, ROOT, launch } from "../lib/harness.mjs"

const SRC = path.join(ROOT, "definitions")
const DIR = path.join(CONFIG_DIR, "definitions")
const builtin = JSON.parse(readFileSync(path.join(SRC, "index.json"), "utf8"))

let ctx
before(async () => (ctx = await launch()))
after(async () => {
  rmSync(DIR, { recursive: true, force: true })
  await ctx?.stop()
})

const card = () => ctx.s.exec(`return document.querySelector('[data-testid=definitions]')?.innerText ?? null`)

test("Settings shows the built-in set, its folder and what it holds", async () => {
  const info = await ctx.s.invoke("definitions_info")
  assert.equal(info.source, "built-in")
  assert.equal(info.version, builtin.version)
  assert.equal(path.normalize(info.folder).toLowerCase(), path.normalize(DIR).toLowerCase())
  assert.ok(info.counts.caches > 50 && info.counts.junk > 5)
  await ctx.nav("settings")
  const text = await ctx.s.waitFor(card, "definitions card")
  assert.match(text, new RegExp(builtin.version.replace(/\./g, "\.")))
  assert.match(text, /Built-in/)
  assert.ok(text.toLowerCase().includes(path.basename(CONFIG_DIR).toLowerCase()), "the folder path is shown")
})

test("a set in the folder is used after Refresh, and its new rule is scanned", async () => {
  cpSync(SRC, DIR, { recursive: true })
  const index = { ...builtin, version: "2099.01.01.1" }
  writeFileSync(path.join(DIR, "index.json"), JSON.stringify(index))
  const caches = JSON.parse(readFileSync(path.join(SRC, "caches.json"), "utf8"))
  caches.rules.push({ id: "e2e-tool", name: "E2E tool cache", eco: "Testing", risk: "safe", about: "Added by a definitions update.", paths: ["{home}/.e2e-tool/cache"] })
  writeFileSync(path.join(DIR, "caches.json"), JSON.stringify(caches))
  mkdirSync(path.join(FAKE_HOME, ".e2e-tool", "cache"), { recursive: true })
  writeFileSync(path.join(FAKE_HOME, ".e2e-tool", "cache", "blob.bin"), Buffer.alloc(40_000))

  await ctx.click("reload-definitions")
  await ctx.s.waitFor(async () => /2099\.01\.01\.1[\s\S]*From folder/.test((await card()) ?? ""), "folder set in use")
  const items = await ctx.s.invoke("scan_caches")
  const it = items.find((i) => i.id === "e2e-tool")
  assert.ok(it, "the new rule is scanned")
  assert.equal(it.size, 40_000)
})

test("a broken set is ignored with the reason, and the built-in set is used", async () => {
  writeFileSync(path.join(DIR, "ports.json"), "{ not json")
  await ctx.click("reload-definitions")
  await ctx.s.waitFor(async () => /wasn't used[\s\S]*ports\.json/.test((await card()) ?? ""), "problem shown")
  const info = await ctx.s.invoke("definitions_info")
  assert.equal(info.source, "built-in")
  assert.equal(info.version, builtin.version)
  const items = await ctx.s.invoke("scan_caches")
  assert.equal(items.find((i) => i.id === "e2e-tool"), undefined)
})
