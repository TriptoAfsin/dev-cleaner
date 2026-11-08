// AI models: Hugging Face repos, LM Studio models and loose .gguf files are
// found under the fake home; nothing is pre-selected; deleting asks first
// and removes only what was picked. Ollama is never run under the fake home.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { closeSync, existsSync, mkdirSync, openSync, ftruncateSync, writeFileSync } from "node:fs"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"

const MB = 1024 * 1024
let ctx

/** A large file without writing its bytes. */
function big(p, size) {
  mkdirSync(path.dirname(p), { recursive: true })
  const fd = openSync(p, "w")
  ftruncateSync(fd, size)
  closeSync(fd)
}

const LOOSE = () => path.join(FAKE_HOME, "Downloads", "llama-3-8b-instruct.Q4_K_M.gguf")

before(async () => {
  ctx = await launch()
  big(LOOSE(), 60 * MB)
  big(path.join(FAKE_HOME, ".lmstudio", "models", "acme", "coder-7b", "coder-7b.Q8_0.gguf"), 70 * MB)
  const repo = path.join(FAKE_HOME, ".cache", "huggingface", "hub", "models--acme--tiny-bert")
  big(path.join(repo, "blobs", "abc123"), 55 * MB)
  mkdirSync(path.join(repo, "snapshots", "main"), { recursive: true })
  writeFileSync(path.join(repo, "snapshots", "main", "config.json"), "{}")
  // Decoys: too small to be a model, and a non-model extension.
  big(path.join(FAKE_HOME, "Downloads", "tiny.gguf"), 1 * MB)
  big(path.join(FAKE_HOME, "Downloads", "movie.mp4"), 80 * MB)
})
after(async () => ctx?.stop())

const rows = () => ctx.s.exec(`return [...document.querySelectorAll('[data-testid=model-list] li')].map(li => li.innerText)`)

test("the backend finds the HF repo, the LM Studio model and the loose GGUF, and no decoys", async () => {
  const models = await ctx.s.invoke("scan_models", { extraRoots: [] })
  const names = models.map((m) => m.name).join("\n")
  assert.match(names, /acme\/tiny-bert/)
  assert.match(names, /coder-7b/)
  assert.match(names, /llama-3-8b-instruct/)
  assert.doesNotMatch(names, /tiny\.gguf|movie/)
})

test("the AI models page lists them with nothing selected", async () => {
  await ctx.nav("models")
  await ctx.click("scan-models")
  await ctx.s.waitFor(async () => (await rows()).length >= 3, "three model rows", { timeout: 30000 })
  const checked = await ctx.s.exec(`return document.querySelectorAll('[data-testid=model-list] input[type=checkbox]:checked').length`)
  assert.equal(checked, 0)
})

test("deleting the loose GGUF asks first and removes only that file", async () => {
  const { s } = ctx
  const box = await s.waitFor(
    () => s.exec(`return [...document.querySelectorAll('[data-testid=model-list] input[type=checkbox]')].find(i => (i.getAttribute('aria-label')||'').includes('llama-3-8b')) ?? null`),
    "checkbox for the loose model",
  )
  await (await s.find(`[aria-label="${await s.exec(`return arguments[0].getAttribute('aria-label')`, box)}"]`)).click()
  await ctx.click("delete-models")
  await ctx.click("confirm-delete-models")
  await s.waitFor(() => !existsSync(LOOSE()), "file removed", { timeout: 15000 })
  assert.ok(existsSync(path.join(FAKE_HOME, ".lmstudio", "models", "acme", "coder-7b", "coder-7b.Q8_0.gguf")), "LM Studio model kept")
})

test("the backend refuses to delete something that is not a model", async () => {
  const res = await ctx.s.invoke("delete_models", { ids: [`file:${path.join(FAKE_HOME, "Downloads", "movie.mp4")}`] })
  assert.ok(res.every((r) => !r.ok))
  assert.ok(existsSync(path.join(FAKE_HOME, "Downloads", "movie.mp4")))
})
