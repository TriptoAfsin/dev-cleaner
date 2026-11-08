// Brand marks from simple-icons (CC0). Names with no mark (Microsoft and
// OpenAI products are not in simple-icons) get a neutral lettered badge.
import {
  siAndroidstudio, siAngular, siAnaconda, siApachemaven, siApple, siBun, siClaude, siDeno, siDocker, siDotnet,
  siGit, siGithub, siGithubcopilot, siGo, siGooglegemini, siGradle, siHomebrew, siJetbrains, siNeovim,
  siNextdotjs, siNodedotjs, siNotepadplusplus, siNpm, siNuget, siNuxt, siOllama, siOpenjdk, siPhp, siPnpm,
  siPoetry, siPython, siQwen, siRuby, siRust, siSublimetext, siSvelte, siTurborepo, siUbuntu, siUv, siWindsurf,
  siXcode, siYarn, siZedindustries, siCursor, siOpencode, siPostgresql, siRedis, siMongodb, siMysql, siVite,
  siLinux, siAmd, siNvidia, siIntel, siDjango, siFlask, siElectron,
} from "simple-icons"
import { cn } from "./format"

type Icon = { path: string; hex: string; title: string }

const MAP: Record<string, Icon> = {
  // ecosystems and caches
  node: siNodedotjs, "node.js": siNodedotjs, npm: siNpm, pnpm: siPnpm, yarn: siYarn, bun: siBun, deno: siDeno,
  "node-gyp": siNodedotjs, rust: siRust, cargo: siRust, "cargo-git": siRust, python: siPython, pip: siPython,
  uv: siUv, poetry: siPoetry, conda: siAnaconda, go: siGo, "go-build": siGo, "go-mod": siGo, java: siOpenjdk,
  gradle: siGradle, maven: siApachemaven, ".net": siDotnet, nuget: siNuget, docker: siDocker, apple: siApple,
  xcode: siXcode, simulator: siApple, homebrew: siHomebrew, ruby: siRuby, php: siPhp, git: siGit,
  "github cli": siGithub, linux: siLinux, ubuntu: siUbuntu,
  // project artifacts
  node_modules: siNodedotjs, venv: siPython, target: siRust, next: siNextdotjs, nuxt: siNuxt, svelte: siSvelte,
  turbo: siTurborepo, angular: siAngular, vite: siVite, electron: siElectron,
  // editors
  cursor: siCursor, windsurf: siWindsurf, zed: siZedindustries, "sublime text": siSublimetext, neovim: siNeovim,
  "jetbrains ides": siJetbrains, "android studio": siAndroidstudio, "notepad++": siNotepadplusplus,
  // AI tools
  "claude code": siClaude, "gemini cli": siGooglegemini, "github copilot cli": siGithubcopilot,
  "cursor agent": siCursor, opencode: siOpencode, ollama: siOllama, "qwen code": siQwen,
  // services and hardware
  postgresql: siPostgresql, redis: siRedis, mongodb: siMongodb, mysql: siMysql, django: siDjango, flask: siFlask,
  amd: siAmd, nvidia: siNvidia, intel: siIntel,
}

export function brandFor(key: string): Icon | null {
  const k = key.toLowerCase()
  if (MAP[k]) return MAP[k]
  // "Python 3.13", "postgres.exe", "PostgreSQL 16"
  for (const [name, icon] of Object.entries(MAP)) if (name.length > 2 && k.startsWith(name)) return icon
  if (k.startsWith("postgres")) return siPostgresql
  return null
}

/** Very dark marks (Next.js, Bun's outline, Rust) vanish on a dark surface;
    those draw in the text colour instead of their brand colour. */
function tooDark(hex: string) {
  const n = parseInt(hex, 16)
  const r = (n >> 16) & 255
  const g = (n >> 8) & 255
  const b = n & 255
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 60
}

export function BrandIcon({ name, size = 16, className, mono }: { name: string; size?: number; className?: string; mono?: boolean }) {
  const icon = brandFor(name)
  if (!icon) {
    const letters = name.replace(/[^A-Za-z0-9.+]/g, " ").trim().split(/\s+/).map((w) => w[0]).join("").slice(0, 2)
    return (
      <span
        aria-hidden
        className={cn("inline-grid shrink-0 place-items-center rounded-[5px] bg-track font-semibold text-muted", className)}
        style={{ width: size, height: size, fontSize: size * 0.45 }}
      >
        {letters.toUpperCase() || "?"}
      </span>
    )
  }
  const color = mono || tooDark(icon.hex) ? "currentColor" : `#${icon.hex}`
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden className={cn("shrink-0", className)} fill={color}>
      <path d={icon.path} />
    </svg>
  )
}
