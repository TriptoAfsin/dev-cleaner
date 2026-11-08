// Download the msedgedriver that matches the installed WebView2 runtime.
// WebView2 updates itself with Edge; a driver even one major version off
// refuses to start a session, so the suite checks this before every run.
import { execFileSync } from "node:child_process"
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync, copyFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"

const BIN = path.join(path.dirname(fileURLToPath(import.meta.url)), ".bin")
const DRIVER = path.join(BIN, "msedgedriver.exe")

function webview2Version() {
  for (const key of [
    "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKCU\\Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
  ]) {
    try {
      const out = execFileSync("reg", ["query", key, "/v", "pv"], { encoding: "utf8" })
      const m = out.match(/pv\s+REG_SZ\s+([\d.]+)/)
      if (m) return m[1]
    } catch {
      /* try the next key */
    }
  }
  throw new Error("WebView2 runtime not found")
}

function driverVersion() {
  if (!existsSync(DRIVER)) return null
  try {
    return execFileSync(DRIVER, ["--version"], { encoding: "utf8" }).match(/([\d.]+)/)?.[1] ?? null
  } catch {
    return null
  }
}

const want = webview2Version()
const have = driverVersion()
if (have === want) {
  console.log(`msedgedriver ${have} matches WebView2`)
  process.exit(0)
}
console.log(`WebView2 ${want}, driver ${have ?? "missing"}: downloading msedgedriver ${want}`)
const res = await fetch(`https://msedgedriver.microsoft.com/${want}/edgedriver_win64.zip`)
if (!res.ok) throw new Error(`download failed: ${res.status}`)
// Unpack in a fresh, empty folder, then copy only the driver out.
const tmp = mkdtempSync(path.join(os.tmpdir(), "edgedriver-"))
const zip = path.join(tmp, "driver.zip")
writeFileSync(zip, Buffer.from(await res.arrayBuffer()))
execFileSync("powershell", ["-NoProfile", "-Command", `Expand-Archive -LiteralPath '${zip}' -DestinationPath '${path.join(tmp, "x")}' -Force`])
mkdirSync(BIN, { recursive: true })
copyFileSync(path.join(tmp, "x", "msedgedriver.exe"), DRIVER)
rmSync(tmp, { recursive: true, force: true })
console.log(`msedgedriver ${driverVersion()} installed`)
