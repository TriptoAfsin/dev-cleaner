// About and update checks. A local server stands in for GitHub's releases
// API, so the test never reaches the network.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import http from "node:http"
import { mkdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { ARTIFACTS, launch } from "../lib/harness.mjs"

let ctx
let server
let release = { tag_name: "v9.9.0", html_url: "https://github.com/TriptoAfsin/dev-cleaner/releases/tag/v9.9.0", published_at: "2026-11-01T10:00:00Z", body: "- Faster scans\n- New caches" }
let hits = 0

before(async () => {
  server = http.createServer((req, res) => {
    hits++
    res.setHeader("content-type", "application/json")
    res.end(JSON.stringify(release))
  })
  await new Promise((r) => server.listen(0, "127.0.0.1", r))
  const url = `http://127.0.0.1:${server.address().port}/releases/latest`
  ctx = await launch({ env: { DEV_CLEANER_UPDATE_URL: url } })
})
after(async () => {
  await ctx?.stop()
  server?.close()
})

test("the backend compares the latest release with this version", async () => {
  const info = await ctx.s.invoke("check_update")
  const app = await ctx.s.invoke("app_info")
  assert.equal(info.latest, "9.9.0")
  assert.equal(info.current, app.version)
  assert.equal(info.newer, true)
  assert.match(app.repo, /github\.com\/TriptoAfsin\/dev-cleaner/)
})

test("Settings shows the version and offers a newer release", async () => {
  const { s } = ctx
  await ctx.nav("settings")
  const app = await s.invoke("app_info")
  await s.waitFor(() => s.exec(`return document.querySelector('[data-testid=app-version]')?.innerText ?? null`), "version")
  assert.equal(await s.exec(`return document.querySelector('[data-testid=about] [data-testid=app-version]').innerText`), `v${app.version}`)
  await (await s.find('[data-testid=about] [data-testid=check-updates]')).click()
  const text = await s.waitFor(() => s.exec(`return document.querySelector('[data-testid=about] [data-testid=update-available]')?.innerText ?? null`), "update offer")
  assert.match(text, /Dev Cleaner 9\.9\.0/)
  assert.match(text, /Faster scans/)
  await s.waitFor(() => s.exec(`return !!document.querySelector('[data-testid=update-dot]')`), "dot on the title-bar button")
  await s.exec(`document.querySelector('[data-testid=about]').scrollIntoView()`)
  mkdirSync(ARTIFACTS, { recursive: true })
  writeFileSync(path.join(ARTIFACTS, "about.png"), await s.screenshot())
})

test("the title-bar button opens About; an up-to-date check says so", async () => {
  const { s } = ctx
  const app = await s.invoke("app_info")
  release = { ...release, tag_name: `v${app.version}` }
  await ctx.click("about-button")
  await s.waitFor(() => s.exec(`return !!document.querySelector('[role=dialog] [data-testid=check-updates]')`), "about dialog")
  await (await s.find('[role=dialog] [data-testid=check-updates]')).click()
  await s.waitFor(async () => /latest version/.test(await s.exec(`return document.querySelector('[role=dialog]').innerText`)), "up to date")
  assert.equal(await s.exec(`return !!document.querySelector('[data-testid=update-dot]')`), false)
  await (await ctx.button("Close", "[role=dialog]")).click()
  assert.ok(hits >= 2)
})
