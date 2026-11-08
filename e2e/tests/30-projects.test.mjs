// Project build folders: scan a folder, only real artifacts are listed,
// delete a selection, and the backend refuses anything else.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { existsSync } from "node:fs"
import path from "node:path"
import { PROJECTS, launch, seedProjects } from "../lib/harness.mjs"

let ctx
before(async () => {
  seedProjects()
  ctx = await launch()
  await ctx.settings({ projectRoot: PROJECTS })
})
after(async () => ctx?.stop())

const rows = () => ctx.s.exec(`return [...document.querySelectorAll('[data-testid=artifact-list] li')].map(li => li.innerText)`)

test("before a scan the page explains what to do", async () => {
  await ctx.nav("projects")
  assert.match(await ctx.text(), /Scan a folder of projects/)
  assert.match(await ctx.s.exec(`return document.querySelector('[data-testid=project-root]').innerText`), /dc-e2e-projects/)
})

test("scanning lists node_modules, the Rust target and the venv, and no decoys", async () => {
  await ctx.click("scan-projects")
  await ctx.s.waitFor(async () => (await rows()).length === 3 && /Found 3/.test(await ctx.text()), "three artifacts")
  const r = (await rows()).join("\n")
  assert.match(r, /web[\s\S]*node_modules/)
  assert.match(r, /rs[\s\S]*Rust target/)
  assert.match(r, /py[\s\S]*Python venv/)
  assert.doesNotMatch(r, /loose|docs/)
})

test("kind chips filter the list", async () => {
  await (await ctx.button("Rust target")).click()
  await ctx.s.waitFor(async () => (await rows()).length === 1, "one row for Rust")
  await (await ctx.button("All")).click()
  await ctx.s.waitFor(async () => (await rows()).length === 3, "all rows back")
})

test("deleting a selected node_modules removes it and leaves source and other projects", async () => {
  const { s } = ctx
  await (await s.find('[aria-label="Select web node_modules"]')).click()
  await ctx.click("delete-artifacts")
  await s.waitFor(() => s.byText("[role=dialog]", "npm / pnpm / yarn / bun install"), "dialog shows how to restore")
  await ctx.click("confirm-delete")
  await s.waitFor(async () => (await rows()).length === 2, "row removed")
  assert.equal(existsSync(path.join(PROJECTS, "web", "node_modules")), false)
  assert.equal(existsSync(path.join(PROJECTS, "web", "src", "index.ts")), true)
  assert.equal(existsSync(path.join(PROJECTS, "rs", "target")), true)
})

test("the backend refuses to delete a folder that is not a build artifact", async () => {
  const res = await ctx.s.invoke("delete_artifacts", { paths: [path.join(PROJECTS, "web", "src"), path.join(PROJECTS, "docs", "target")] })
  assert.ok(res.every((r) => !r.ok))
  assert.equal(existsSync(path.join(PROJECTS, "web", "src", "index.ts")), true)
  assert.equal(existsSync(path.join(PROJECTS, "docs", "target", "notes.md")), true)
})
