// Definitions downloads: a local server stands in for GitHub. Refresh fetches
// a newer set, uses it, and skips only the rule written for a newer app; a
// set that needs a newer app is refused and the current one stays.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import http from "node:http"
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import path from "node:path"
import { CONFIG_DIR, FAKE_HOME, ROOT, launch } from "../lib/harness.mjs"

const SRC = path.join(ROOT, "definitions")
const DIR = path.join(CONFIG_DIR, "definitions")
const builtin = JSON.parse(readFileSync(path.join(SRC, "index.json"), "utf8"))

/** The files the server hands out, by name. */
let served = {}
const serve = (index, caches) => {
  served = {}
  for (const f of builtin.files) served[f] = readFileSync(path.join(SRC, f), "utf8")
  served["index.json"] = JSON.stringify(index)
  if (caches) served["caches.json"] = JSON.stringify(caches)
}

let ctx
let server
before(async () => {
  rmSync(DIR, { recursive: true, force: true })
  server = http.createServer((req, res) => {
    const name = decodeURIComponent(req.url.split("/").pop())
    if (!(name in served)) {
      res.statusCode = 404
      return res.end()
    }
    res.setHeader("content-type", "application/json")
    res.end(served[name])
  })
  await new Promise((r) => server.listen(0, "127.0.0.1", r))
  ctx = await launch({ env: { DEV_CLEANER_DEFS_URL: `http://127.0.0.1:${server.address().port}/definitions/` } })
})
after(async () => {
  await ctx?.stop()
  server?.close()
  rmSync(DIR, { recursive: true, force: true })
})

const card = () => ctx.s.exec(`return document.querySelector('[data-testid=definitions]')?.innerText ?? null`)

test("the same version as the built-in set is up to date", async () => {
  serve(builtin)
  const r = await ctx.s.invoke("update_definitions")
  assert.equal(r.outcome, "up-to-date")
  assert.equal(r.info.source, "built-in")
})

test("Refresh downloads a newer set; a rule for a newer app is skipped, the rest used", async () => {
  const caches = JSON.parse(readFileSync(path.join(SRC, "caches.json"), "utf8"))
  caches.rules.push({ id: "dl-tool", name: "Downloaded tool cache", eco: "Testing", risk: "safe", about: "Added by a download.", paths: ["{home}/.dl-tool/cache"] })
  caches.rules.push({ id: "from-the-future", name: "Future", eco: "Testing", risk: "safe", about: "Uses a field this app doesn't know.", paths: [], teleport: true })
  serve({ ...builtin, version: "2099.02.01.1" }, caches)
  mkdirSync(path.join(FAKE_HOME, ".dl-tool", "cache"), { recursive: true })
  writeFileSync(path.join(FAKE_HOME, ".dl-tool", "cache", "blob.bin"), Buffer.alloc(30_000))

  await ctx.nav("settings")
  await ctx.click("reload-definitions")
  await ctx.s.waitFor(async () => /2099\.02\.01\.1[\s\S]*From folder/.test((await card()) ?? ""), "downloaded set in use")
  const info = await ctx.s.invoke("definitions_info")
  assert.deepEqual(info.skipped.map((s) => s.split(":")[0]), ["from-the-future"])
  assert.match(await card(), /1 rule needs a newer Dev Cleaner/)
  const items = await ctx.s.invoke("scan_caches")
  assert.equal(items.find((i) => i.id === "dl-tool")?.size, 30_000, "the downloaded rule is scanned")
  assert.ok(items.find((i) => i.id === "npm"), "the existing rules are still there")
  assert.equal(JSON.parse(readFileSync(path.join(DIR, "index.json"), "utf8")).version, "2099.02.01.1", "written to the folder")
})

test("a set that needs a newer app is not downloaded, and the current set stays", async () => {
  serve({ ...builtin, version: "2099.03.01.1", minApp: "99.0.0" })
  const r = await ctx.s.invoke("update_definitions")
  assert.equal(r.outcome, "needs-app")
  assert.equal(r.info.version, "2099.02.01.1")
})

test("a broken download is refused and the current set stays", async () => {
  serve({ ...builtin, version: "2099.04.01.1" })
  served["ports.json"] = "{ not json"
  await assert.rejects(ctx.s.invoke("update_definitions"), /didn't pass its checks/)
  const info = await ctx.s.invoke("definitions_info")
  assert.equal(info.version, "2099.02.01.1")
})
