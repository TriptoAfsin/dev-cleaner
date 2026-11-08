// Configs: discovery, export to a zip with agent instructions, restore with
// a backup first, undo, and copying commands between harnesses. Runs under
// the fake home, so real configs are never read or written. The native save
// and open dialogs can't be driven, so files are passed to the commands.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"

// Windows' own bsdtar reads zips; Git Bash's tar would treat "T:" as a host.
const TAR = path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe")

let ctx
const ZIP = path.join(os.tmpdir(), "dc-e2e-configs.zip")
const claude = (...p) => path.join(FAKE_HOME, ".claude", ...p)
const vscode = () => path.join(FAKE_HOME, "roaming", "Code", "User", "settings.json")

function write(p, text) {
  mkdirSync(path.dirname(p), { recursive: true })
  writeFileSync(p, text)
}

before(async () => {
  ctx = await launch()
  write(claude("settings.json"), '{ "theme": "dark" }')
  write(claude("CLAUDE.md"), "# Global rules\nBe brief.\n")
  write(claude("skills", "release", "SKILL.md"), "---\nname: release\ndescription: Cut a release\n---\nSteps.\n")
  write(claude("commands", "review.md"), "---\ndescription: Review a diff\n---\nReview $ARGUMENTS carefully. Context: !`git diff`\n")
  write(vscode(), '{\n  // editor\n  "editor.fontSize": 14,\n}\n')
  rmSync(ZIP, { force: true })
})
after(async () => ctx?.stop())

let items = []

test("discovery finds Claude Code and VS Code configs under the fake home", async () => {
  const tools = await ctx.s.invoke("list_config_sources")
  const names = tools.map((t) => t.name).join(", ")
  assert.match(names, /Claude Code/)
  assert.match(names, /VS Code/)
  items = tools.flatMap((t) => t.items).filter((i) => i.exists)
  assert.ok(items.some((i) => i.path.endsWith("CLAUDE.md")))
})

test("the Configs page shows the found tools", async () => {
  await ctx.nav("configs")
  await ctx.waitText(/Claude Code[\s\S]*VS Code|VS Code[\s\S]*Claude Code/, "tool groups", 20000)
})

test("export writes a zip with a manifest and AGENT-RESTORE.md", async () => {
  const pick = items.filter((i) => /CLAUDE\.md|settings\.json|skills/.test(i.path) && !i.sensitive).map((i) => i.id)
  assert.ok(pick.length >= 3, `picked ${pick.length}`)
  const res = await ctx.s.invoke("export_configs", { itemIds: pick, destPath: ZIP })
  assert.ok(existsSync(ZIP))
  assert.equal(res.items, pick.length)
  const listing = execFileSync(TAR, ["-tf", ZIP], { encoding: "utf8" })
  assert.match(listing, /manifest\.json/)
  assert.match(listing, /AGENT-RESTORE\.md/)
  const restore = execFileSync(TAR, ["-xOf", ZIP, "AGENT-RESTORE.md"], { encoding: "utf8" })
  assert.match(restore, /CLAUDE\.md/)
  assert.match(restore, /back up/i)
})

test("restore backs up the current file, replaces it, and undo brings it back", async () => {
  write(claude("CLAUDE.md"), "# Changed on this machine\n")
  const bundle = await ctx.s.invoke("read_bundle", { zipPath: ZIP })
  const id = bundle.items.find((i) => /CLAUDE\.md/.test(i.label + i.originalPath))?.id ?? bundle.items[0].id
  const res = await ctx.s.invoke("import_configs", { zipPath: ZIP, itemIds: [id] })
  assert.equal(res.failed.length, 0)
  assert.match(readFileSync(claude("CLAUDE.md"), "utf8"), /Be brief/)
  const backups = await ctx.s.invoke("list_config_backups")
  assert.ok(backups.some((b) => b.id === res.backupId))
  await ctx.s.invoke("undo_config_import", { backupId: res.backupId })
  assert.match(readFileSync(claude("CLAUDE.md"), "utf8"), /Changed on this machine/)
})

test("a hostile bundle path is refused", async () => {
  const r = await ctx.s.invokeResult("import_configs", { zipPath: ZIP, itemIds: ["../../etc/passwd"] })
  const wrote = existsSync(path.join(FAKE_HOME, "..", "etc", "passwd"))
  assert.equal(wrote, false)
  assert.ok(r.err || r.ok.failed.length > 0 || r.ok.restored === 0)
})

test("Claude commands copy to Gemini as TOML", async () => {
  const plan = await ctx.s.invoke("plan_migration", { id: "commands", options: { from: null, to: ["gemini"], overwrite: false, extensions: false } })
  assert.ok(plan.length >= 1)
  await ctx.s.invoke("run_migration", { id: "commands", options: { from: null, to: ["gemini"], overwrite: false, extensions: false } })
  const toml = readFileSync(path.join(FAKE_HOME, ".gemini", "commands", "review.toml"), "utf8")
  assert.match(toml, /description = "Review a diff"/)
  assert.match(toml, /\{\{args\}\}/)
  assert.match(toml, /!\{git diff\}/)
})
