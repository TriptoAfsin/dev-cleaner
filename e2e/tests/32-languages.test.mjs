// More languages: JVM, .NET, Ruby, Haskell and Node extras on the Disk
// page, and Maven, Gradle, .NET, Composer, Flutter, CMake and Unity build
// folders on the Projects page, each only beside its project file.
import { test, before, after } from "node:test"
import assert from "node:assert/strict"
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { FAKE_HOME, launch } from "../lib/harness.mjs"

let ctx
const ROOT = path.join(os.tmpdir(), "dc-e2e-langs")
function file(p, bytes = 10_000) {
  mkdirSync(path.dirname(p), { recursive: true })
  writeFileSync(p, Buffer.alloc(bytes, 99))
}
const home = (...p) => path.join(FAKE_HOME, ...p)
const win = process.platform === "win32"

before(async () => {
  ctx = await launch()
  // Global caches
  file(home(".gradle", "wrapper", "dists", "gradle-8.10-bin", "gradle.zip"), 300_000)
  file(home(".gradle", "caches", "modules-2", "a.jar"), 100_000)
  file(home(".nuget", "packages", "newtonsoft.json", "13.0.3", "lib.dll"), 50_000)
  file(win ? home("local", "NuGet", "v3-cache", "x.dat") : home(".local", "share", "NuGet", "http-cache", "x.dat"), 70_000)
  file(home(".gem", "ruby", "3.3.0", "cache", "rails-8.0.gem"), 40_000)
  file(win ? path.join(FAKE_HOME, "roaming", "cabal", "packages", "base.tar.gz") : home(".cabal", "packages", "base.tar.gz"), 30_000)
  file(win ? home("local", "node", "corepack", "pnpm", "pnpm.js") : home(".cache", "node", "corepack", "pnpm", "pnpm.js"), 20_000)
  // Projects
  rmSync(ROOT, { recursive: true, force: true })
  file(path.join(ROOT, "shop-api", "pom.xml"), 20)
  file(path.join(ROOT, "shop-api", "target", "app.jar"), 200_000)
  file(path.join(ROOT, "mobile", "build.gradle.kts"), 20)
  file(path.join(ROOT, "mobile", "build", "outputs", "app.apk"), 150_000)
  file(path.join(ROOT, "mobile", ".gradle", "8.10", "fileHashes.bin"), 10_000)
  file(path.join(ROOT, "Billing", "Billing.csproj"), 20)
  file(path.join(ROOT, "Billing", "bin", "Debug", "Billing.dll"), 80_000)
  file(path.join(ROOT, "Billing", "obj", "project.assets.json"), 30_000)
  file(path.join(ROOT, "site", "composer.json"), 20)
  file(path.join(ROOT, "site", "vendor", "autoload.php"), 60_000)
  file(path.join(ROOT, "flutter_app", "pubspec.yaml"), 20)
  file(path.join(ROOT, "flutter_app", ".dart_tool", "package_config.json"), 9_000)
  file(path.join(ROOT, "engine", "CMakeLists.txt"), 20)
  file(path.join(ROOT, "engine", "build", "CMakeCache.txt"), 5_000)
  file(path.join(ROOT, "Game", "ProjectSettings", "ProjectSettings.asset"), 20)
  file(path.join(ROOT, "Game", "Assets", "Player.cs"), 20)
  file(path.join(ROOT, "Game", "Library", "ArtifactDB"), 400_000)
  // Decoys: same folder names without the project file beside them.
  file(path.join(ROOT, "scripts", "bin", "deploy.sh"), 100)
  file(path.join(ROOT, "docs", "vendor", "theme.css"), 100)
  file(path.join(ROOT, "notes", "build", "draft.md"), 100)
  await ctx.settings({ projectRoots: [ROOT] })
})
after(async () => ctx?.stop())

test("Disk cleanup finds the JVM, .NET, Ruby, Haskell and Corepack caches", async () => {
  const items = await ctx.s.invoke("scan_caches", { drives: null })
  const size = (id) => items.find((i) => i.id === id)?.size ?? -1
  assert.equal(size("gradle-wrapper"), 300_000)
  assert.equal(size("gradle"), 100_000)
  assert.equal(size("nuget"), 120_000, "packages plus the HTTP cache")
  assert.equal(size("ruby-gems"), 40_000)
  assert.equal(size("cabal"), 30_000)
  assert.equal(size("corepack"), 20_000)
})

test("Projects lists each language's build folders and none of the decoys", async () => {
  const { s } = ctx
  await ctx.nav("projects")
  await ctx.click("scan-projects")
  await ctx.waitText(/Found \d+ build folders/, "scan done", 30000)
  const rows = (await s.exec(`return [...document.querySelectorAll('[data-testid=artifact-list] li')].map(li => li.innerText)`)).join("\n")
  for (const re of [/shop-api[\s\S]*Maven target/, /mobile[\s\S]*Gradle build/, /Gradle project cache/, /Billing[\s\S]*\.NET bin\/obj/, /site[\s\S]*Composer vendor/, /Dart tool cache/, /CMake build/, /Game[\s\S]*Unity Library/]) {
    assert.match(rows, re)
  }
  const count = await s.exec(`return document.querySelectorAll('[data-testid=artifact-list] li').length`)
  assert.equal(count, 9, "maven 1, gradle build + .gradle 2, .NET bin + obj 2, composer 1, dart 1, cmake 1, unity 1")
  assert.doesNotMatch(rows, /scripts|docs|notes/)
})

test("deleting the .NET bin/obj keeps the project file", async () => {
  const res = await ctx.s.invoke("delete_artifacts", { paths: [path.join(ROOT, "Billing", "bin"), path.join(ROOT, "Billing", "obj"), path.join(ROOT, "scripts", "bin")] })
  assert.deepEqual(res.map((r) => r.ok), [true, true, false], "the decoy bin is refused")
  assert.equal(existsSync(path.join(ROOT, "Billing", "Billing.csproj")), true)
  assert.equal(existsSync(path.join(ROOT, "scripts", "bin", "deploy.sh")), true)
})
