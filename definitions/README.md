# Definitions

What Dev Cleaner knows, kept as data: where caches live and how to clean
them, which folders inside projects are build output, what ending a process
means, what usually listens on a port, where AI apps keep models, and which
editor and coding-tool config files to back up.

The app's code is the engine (measuring, deleting, running a tool's own clean
command, elevating, walking drives). These files tell the engine what to look
for. A definition can only name engine actions; it never carries a command of
its own.

| File | What it holds |
| --- | --- |
| `index.json` | The set's version (date-based, `2026.11.09.1`), its format (`schema`), the oldest app that can read it (`minApp`), and the file list. |
| `caches.json` | Disk cleanup, language caches, AI/ML caches and System junk. |
| `projects.json` | Build and dependency folders inside projects (`node_modules`, `target`, `.venv`…). |
| `processes.json` | Verdicts for ending a process: system, save first, safe. |
| `ports.json` | Ports listed even when free, and names for what listens on them. |
| `models.json` | Folders AI apps keep models in. |
| `configs.json` | Config files offered for backup, and where cross-tool copies write. |
| `startup.json` | Verdicts for programs that start at sign-in: keep, or fine to turn off. |
| `services.json` | Services: protected (never changed), dev servers, optional. |

## How updates reach the app

Settings → Definitions → **Refresh** downloads `index.json` from this folder on
GitHub's `main` branch, and, when its `version` is newer than the set in use,
every file it lists. The set is checked exactly like a folder copy and written
to the data folder only if it passes; otherwise the current set stays. The app
also does this once a day when update checks are on. So:

- **Adding or fixing a rule** needs no app release: edit the JSON, bump
  `version` in `index.json`, push to `main`.
- **A rule that uses something only a newer app has** (a new field, expander or
  action) gets `"minApp": "X.Y.Z"` on the rule. Older apps skip just that rule
  and use the rest. An older app also skips, rule by rule, anything it can't
  parse or check.
- **`minApp` in `index.json`** shuts older apps out of the whole set. Raise it
  only when the format of a file other than `caches.json` changes.

## Where the app reads them from

1. `<app data>/definitions/` (where downloads go), when it holds a complete
   set (`index.json` plus every file it lists) that parses, passes the checks,
   uses a schema this app reads, doesn't need a newer app, and is newer than
   the built-in set.
2. Otherwise the copy compiled into the app.

Settings → Definitions shows which one is in use, its version, the folder path,
and why a folder copy was rejected. **Refresh** reads them again; the next scan
uses them.

## A cache rule

```json
{
  "id": "gradle-wrapper",
  "name": "Gradle wrapper distributions",
  "eco": "Java",
  "risk": "slow",
  "about": "Every Gradle version ./gradlew has downloaded…",
  "paths": ["{home}/.gradle/wrapper/dists"],
  "before": "stop-gradle"
}
```

| Field | Meaning |
| --- | --- |
| `id` | Unique per OS. Two rules may share an id when their `os` lists don't overlap. |
| `risk` | `safe` (rebuilt on demand), `slow` (the next build re-downloads), `data` (yours; never preselected). |
| `about` | The tooltip: what it is, what is deleted, what happens next. |
| `paths` | Templates, expanders or per-OS choices (below). Paths that don't exist are simply not shown. |
| `how` | Default `{"kind": "delete"}` (empty the folders). Also `olderThan` (`hours`), `tool` (`tool`, `args`, optional `perPathArgs` with `{path}`), `command`, `docker`, `recycleBin`, `admin` (`days` and/or a named `action`). |
| `os` | `["windows"]`, `["macos"]`, `["linux"]`, or any mix. All when missing. |
| `minApp` | The oldest Dev Cleaner that understands the rule. Older apps skip it and keep the rest of the set. |
| `junk` | Listed on System junk instead of Disk cleanup. |
| `realOnly` | A system location: never offered under the test sandbox. |
| `requires` | Offered only when this tool is on PATH. |
| `ask` | Ask the tool where its cache really is: `[{"tool": "npm", "args": ["config", "get", "cache"]}]`. |
| `before` | An engine step first. Today: `stop-gradle`. |

### Path templates

`{home}`, `{local}` (`%LOCALAPPDATA%`, `~/Library/Application Support`,
`~/.local/share`), `{roaming}` (`%APPDATA%`, `~/Library/Application Support`,
`~/.config`), `{cache}` (`%LOCALAPPDATA%`, `~/Library/Caches`, `~/.cache`),
`{temp}`, `{xdgCache}`, `{xdgData}`, `{localLow}`, `{user}`, `{systemRoot}`,
`{systemDrive}`, `{programData}`, and `{env:NAME|fallback}` (the fallback may
itself use templates). An unknown variable makes the whole set invalid.

Per OS: `{"windows": "…", "unix": "…"}` or `{"windows": …, "macos": …, "linux": …}`.

### Expanders

| `expand` | Does |
| --- | --- |
| `children` | Sub-folders of `dir`, each joined with every `tail` (`""` = the sub-folder itself). `match: "versioned"` keeps names with a digit; `except` and `exceptPrefix` drop names. `contains` keeps only names containing one of the given strings. |
| `old-entries` | Files and folders in `dir` except the newest `keep` (default 1) by modified time: old versions a tool never removes. A symlink in `dir`, and whatever it points at (the version in use), is never returned. `prefix` limits it to names starting with one of the given strings; `except` drops names. |
| `chromium-profiles` | The cache folders of every Chromium profile under a `User Data` folder, never cookies, history or logins. |
| `files` | Files in `dir` with one of the extensions `ext`, optionally starting with one of `prefix`. |
| `drive-walk` | What the walk of the chosen drives found: `partials`, `thumbs`, `shader-caches`. |
| `drive-roots` | `tail` at the root of every chosen drive (pnpm's per-drive store). |

### Admin actions

`admin` items run together in one elevated script. Without an `action` they
delete the paths (entries older than `days` when given). Named actions are
built into the app: `stop-update-services`, `delivery-optimization`,
`dism-component-cleanup`, `cleanmgr-autoclean`, `thin-time-machine-snapshots`,
`journal-vacuum`, `apt-clean`, `dnf-clean`, `paccache`, `snap-remove-disabled`.

## Checking a change

`cargo test` in `src-tauri` loads the built-in set and runs the same checks the
app runs on a folder copy. To try a change against your own machine, copy this
folder to the data folder shown in Settings, edit it there, and press Refresh.
