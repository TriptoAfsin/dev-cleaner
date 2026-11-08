#!/usr/bin/env node
// Bump the version everywhere it lives, following semantic versioning.
//
//   node scripts/bump-version.mjs patch        0.1.0 -> 0.1.1  (fixes)
//   node scripts/bump-version.mjs minor        0.1.0 -> 0.2.0  (new features)
//   node scripts/bump-version.mjs major        0.1.0 -> 1.0.0  (breaking changes)
//   node scripts/bump-version.mjs 0.3.0-beta.1                  (explicit)
//   add --tag to commit and create the vX.Y.Z tag (push it to release)
//
// Updates package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json, and
// opens a dated section in CHANGELOG.md from the [Unreleased] notes.
import { readFileSync, writeFileSync } from "node:fs"
import { execFileSync } from "node:child_process"

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/
const [arg, ...flags] = process.argv.slice(2)
if (!arg) {
  console.error("usage: bump-version.mjs <patch|minor|major|X.Y.Z[-pre]> [--tag]")
  process.exit(1)
}

const pkg = JSON.parse(readFileSync("package.json", "utf8"))
const cur = pkg.version.match(SEMVER)
if (!cur) throw new Error(`current version ${pkg.version} is not semver`)
const [maj, min, pat] = cur.slice(1, 4).map(Number)
const next =
  arg === "major" ? `${maj + 1}.0.0` : arg === "minor" ? `${maj}.${min + 1}.0` : arg === "patch" ? `${maj}.${min}.${pat + 1}` : arg
if (!SEMVER.test(next)) throw new Error(`${next} is not a valid semantic version (MAJOR.MINOR.PATCH[-pre])`)

pkg.version = next
writeFileSync("package.json", JSON.stringify(pkg, null, 2) + "\n")

const cargo = readFileSync("src-tauri/Cargo.toml", "utf8").replace(/^version = ".*"$/m, `version = "${next}"`)
writeFileSync("src-tauri/Cargo.toml", cargo)

const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"))
conf.version = next
writeFileSync("src-tauri/tauri.conf.json", JSON.stringify(conf, null, 2) + "\n")

const date = new Date().toISOString().slice(0, 10)
let log = readFileSync("CHANGELOG.md", "utf8")
if (!log.includes(`## [${next}]`)) {
  log = log.replace("## [Unreleased]", `## [Unreleased]\n\n## [${next}] - ${date}`)
  writeFileSync("CHANGELOG.md", log)
}

// Keep Cargo.lock in step.
try {
  execFileSync("cargo", ["update", "-p", "dev-cleaner", "--offline"], { cwd: "src-tauri", stdio: "ignore" })
} catch {
  /* lockfile refreshes on the next build */
}

console.log(`${cur[0]} -> ${next}`)
if (flags.includes("--tag")) {
  execFileSync("git", ["add", "package.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/tauri.conf.json", "CHANGELOG.md"], { stdio: "inherit" })
  execFileSync("git", ["commit", "-m", `Release v${next}`], { stdio: "inherit" })
  execFileSync("git", ["tag", "-a", `v${next}`, "-m", `v${next}`], { stdio: "inherit" })
  console.log(`Tagged v${next}. Push with: git push && git push origin v${next}`)
}
