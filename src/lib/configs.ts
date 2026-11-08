// Client for the Configs page: back up developer tool settings to a zip,
// restore one, and copy settings between tools. Mirrors src-tauri/src/configs.rs.
import { invoke } from "@tauri-apps/api/core"

export type ConfigKind = "file" | "dir" | "extensions"

export type ConfigItem = {
  id: string
  tool: string
  label: string
  path: string
  kind: ConfigKind
  size: number
  exists: boolean
  sensitive: boolean
  /** What secret the item may hold. */
  secret: string
  /** "too large", "42 extensions"... */
  note: string
  defaultSelected: boolean
}
export type ConfigTool = { id: string; name: string; size: number; items: ConfigItem[] }

export type ConfigProgress = { done: number; total: number; current: string }
export type ExportResult = { path: string; items: number; bytes: number; skipped: string[] }

export type BundleItem = {
  id: string
  tool: string
  toolName: string
  label: string
  kind: ConfigKind
  sensitive: boolean
  target: string
  replaces: boolean
  supported: boolean
  note: string
}
export type BundleInfo = { path: string; createdAt: string; sourceOs: string; hostname: string; items: BundleItem[] }
export type ImportResult = {
  backupId: string
  restored: number
  failed: { label: string; error: string }[]
  extensionsInstalled: number
  extensionsFailed: string[]
}
export type BackupInfo = { id: string; createdAt: number; reason: string; count: number; replaced: number; undone: boolean; targets: string[] }
export type UndoResult = { restored: number; failed: string[] }

export type Endpoint = { id: string; name: string; path: string; found: boolean; detail: string }
export type Migration = { id: string; title: string; about: string; sources: Endpoint[]; targets: Endpoint[]; extensions: boolean }
export type MigrateOptions = { from?: string; to: string[]; overwrite: boolean; extensions: boolean }
export type PlanAction = "copy" | "overwrite" | "convert" | "add" | "install" | "skip"
export type PlanStep = { from: string; to: string; action: PlanAction; detail: string }
export type MigrationResult = {
  backupId: string | null
  done: number
  skipped: number
  failed: string[]
  extensionsInstalled: number
  extensionsFailed: string[]
}

export const configsApi = {
  listSources: () => invoke<ConfigTool[]>("list_config_sources"),
  exportConfigs: (itemIds: string[], destPath: string) => invoke<ExportResult>("export_configs", { itemIds, destPath }),
  readBundle: (zipPath: string) => invoke<BundleInfo>("read_bundle", { zipPath }),
  importConfigs: (zipPath: string, itemIds: string[]) => invoke<ImportResult>("import_configs", { zipPath, itemIds }),
  listBackups: () => invoke<BackupInfo[]>("list_config_backups"),
  undoImport: (backupId: string) => invoke<UndoResult>("undo_config_import", { backupId }),
  listMigrations: () => invoke<Migration[]>("list_migrations"),
  planMigration: (id: string, options: MigrateOptions) => invoke<PlanStep[]>("plan_migration", { id, options }),
  runMigration: (id: string, options: MigrateOptions) => invoke<MigrationResult>("run_migration", { id, options }),
}

/** Name BrandIcon understands for a tool or endpoint id. */
export function brandOf(id: string): string {
  const m: Record<string, string> = {
    vscode: "VS Code",
    vscodium: "VSCodium",
    cursor: "cursor",
    windsurf: "windsurf",
    zed: "zed",
    sublime: "sublime text",
    neovim: "neovim",
    jetbrains: "jetbrains ides",
    wt: "Windows Terminal",
    git: "git",
    ssh: "SSH",
    pwsh: "PowerShell",
    shell: "Shell",
    claude: "claude code",
    "claude-desktop": "claude code",
    "claude-skills": "claude code",
    codex: "Codex",
    gemini: "gemini cli",
    copilot: "github copilot cli",
    opencode: "opencode",
    aider: "Aider",
    continue: "Continue",
    amp: "Amp",
    goose: "Goose",
    agents: "Agent skills",
  }
  return m[id] ?? id
}

export function backupFileName(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0")
  return `dev-cleaner-configs-${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}.zip`
}
