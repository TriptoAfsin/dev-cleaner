//! Developer tool settings: find them, back them up to one zip, restore a
//! zip on this or another machine, and copy settings between tools (editor
//! settings, agent skills, commands, subagents, instruction files and MCP
//! servers). Every write is preceded by a snapshot of what it replaces, so
//! any restore or copy can be undone.

use crate::util::{self, home};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

type Res<T> = Result<T, String>;

/// Any single folder bigger than this is listed but left out of a backup.
const DIR_CAP: u64 = 200 * 1024 * 1024;
/// Never copied into a backup or between tools (skill folders sometimes
/// carry their own dependencies or a git checkout).
const SKIP_DIRS: &[&str] = &["node_modules", ".git"];
const FORMAT_VERSION: u32 = 1;

fn es<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ------------------------------------------------------------------ paths

/// The three roots every stored path is relative to.
#[derive(Clone)]
struct Bases {
    home: PathBuf,
    roaming: PathBuf,
    local: PathBuf,
}

impl Bases {
    fn current() -> Self {
        Bases { home: home(), roaming: util::roaming_dir(), local: util::local_dir() }
    }
}

/// "%APPDATA%/Code/User" -> a real path on this machine.
fn expand(t: &str, b: &Bases) -> PathBuf {
    let (base, rest) = if let Some(r) = t.strip_prefix("%APPDATA%") {
        (b.roaming.clone(), r)
    } else if let Some(r) = t.strip_prefix("%LOCALAPPDATA%") {
        (b.local.clone(), r)
    } else if let Some(r) = t.strip_prefix('~') {
        (b.home.clone(), r)
    } else {
        return PathBuf::from(t);
    };
    let mut p = base;
    for part in rest.split(['/', '\\']).filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

/// A real path -> "~/.gitconfig", "%APPDATA%/Code/User/settings.json", so it
/// maps onto another machine or OS. The deepest matching root wins (Roaming
/// lives inside the home folder on Windows).
fn templatize(p: &Path, b: &Bases) -> String {
    let mut cands = [("%APPDATA%", &b.roaming), ("%LOCALAPPDATA%", &b.local), ("~", &b.home)];
    cands.sort_by_key(|(_, base)| std::cmp::Reverse(base.components().count()));
    for (tok, base) in cands {
        if let Ok(rest) = p.strip_prefix(base) {
            let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            return if parts.is_empty() { tok.to_string() } else { format!("{tok}/{}", parts.join("/")) };
        }
    }
    p.to_string_lossy().replace('\\', "/")
}

/// How a template reads on each OS, for the agent instructions.
fn windows_form(t: &str) -> String {
    let t = if let Some(r) = t.strip_prefix('~') { format!("%USERPROFILE%{r}") } else { t.to_string() };
    t.replace('/', "\\")
}

fn mac_form(t: &str) -> String {
    for tok in ["%APPDATA%", "%LOCALAPPDATA%"] {
        if let Some(r) = t.strip_prefix(tok) {
            return format!("~/Library/Application Support{r}");
        }
    }
    t.to_string()
}

/// Join an archive-relative path under `base`, refusing anything that could
/// climb out of it (zip-slip): `..`, absolute paths, drive letters.
fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') || rel.contains('\0') {
        return None;
    }
    let mut out = base.to_path_buf();
    let mut pushed = false;
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        out.push(part);
        pushed = true;
    }
    pushed.then_some(out)
}

/// A plain file or folder name: no separators, no leading dot.
fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() < 120
        && !n.starts_with('.')
        && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn slug(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
        .collect();
    let out = out.trim_matches('_').to_string();
    if out.is_empty() || out == "." || out == ".." { "item".into() } else { out }
}

// ------------------------------------------------------------------ files

/// Visit every file under `root` with its '/'-separated relative path.
/// Symlinks are followed (skills are often linked in), with a depth limit
/// against loops. Return false from `f` to stop.
fn walk_files(root: &Path, skip: bool, f: &mut dyn FnMut(&Path, &str, u64) -> bool) -> bool {
    fn go(dir: &Path, rel: &str, depth: u32, skip: bool, f: &mut dyn FnMut(&Path, &str, u64) -> bool) -> bool {
        if depth > 40 {
            return true;
        }
        let Ok(rd) = fs::read_dir(dir) else { return true };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let Ok(meta) = fs::metadata(&path) else { continue };
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if meta.is_dir() {
                if skip && SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                if !go(&path, &r, depth + 1, skip, f) {
                    return false;
                }
            } else if meta.is_file() && !f(&path, &r, meta.len()) {
                return false;
            }
        }
        true
    }
    go(root, "", 0, skip, f)
}

/// Size of a folder, stopping early once past `cap`. Returns (size, over).
fn measure(p: &Path, cap: u64) -> (u64, bool) {
    let mut total = 0u64;
    let done = walk_files(p, true, &mut |_, _, len| {
        total += len;
        total <= cap
    });
    (total, !done)
}

fn copy_tree(src: &Path, dst: &Path, skip: bool) -> Res<u64> {
    fs::create_dir_all(dst).map_err(es)?;
    let mut n = 0;
    let mut err: Option<String> = None;
    walk_files(src, skip, &mut |abs, rel, _| {
        let Some(to) = safe_join(dst, rel) else { return true };
        let r = to.parent().map(fs::create_dir_all).unwrap_or(Ok(())).and_then(|_| fs::copy(abs, &to));
        match r {
            Ok(_) => {
                n += 1;
                true
            }
            Err(e) => {
                err = Some(format!("{}: {e}", abs.display()));
                false
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(n),
    }
}

fn remove_path(p: &Path) -> std::io::Result<()> {
    let m = fs::symlink_metadata(p)?;
    if m.file_type().is_symlink() {
        fs::remove_file(p).or_else(|_| fs::remove_dir(p))
    } else if m.is_dir() {
        util::remove_tree(p)
    } else {
        fs::remove_file(p)
    }
}

fn write_file(to: &Path, bytes: &[u8]) -> Res<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(es)?;
    }
    fs::write(to, bytes).map_err(|e| format!("{}: {e}", to.display()))
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

// ------------------------------------------------------------------ time

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

fn iso(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn stamp(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

// ------------------------------------------------------------------ editor CLIs

fn editor_cli(tool: &str) -> Option<&'static str> {
    match tool {
        "vscode" => Some("code"),
        "cursor" => Some("cursor"),
        "windsurf" => Some("windsurf"),
        "vscodium" => Some("codium"),
        _ => None,
    }
}

/// Find an editor's command-line launcher. Under the e2e fake home only
/// `<fake>/bin` is searched, so tests never touch real editors.
fn cli(name: &str) -> Option<PathBuf> {
    if let Some(f) = util::fake_home() {
        let p = f.join("bin").join(name);
        return [p.clone(), p.with_extension("cmd")].into_iter().find(|p| p.is_file());
    }
    if let Some(p) = util::which(name) {
        return Some(p);
    }
    let local = util::local_dir();
    let cands: Vec<PathBuf> = if cfg!(windows) {
        match name {
            "code" => vec![
                local.join("Programs/Microsoft VS Code/bin/code.cmd"),
                PathBuf::from(r"C:\Program Files\Microsoft VS Code\bin\code.cmd"),
            ],
            "cursor" => vec![local.join("Programs/cursor/resources/app/bin/cursor.cmd")],
            "windsurf" => vec![local.join("Programs/Windsurf/bin/windsurf.cmd")],
            "codium" => vec![local.join("Programs/VSCodium/bin/codium.cmd")],
            _ => vec![],
        }
    } else {
        let app = match name {
            "code" => "Visual Studio Code",
            "cursor" => "Cursor",
            "windsurf" => "Windsurf",
            "codium" => "VSCodium",
            _ => return None,
        };
        vec![PathBuf::from(format!("/Applications/{app}.app/Contents/Resources/app/bin/{name}"))]
    };
    cands.into_iter().find(|p| p.is_file())
}

fn is_extension_id(s: &str) -> bool {
    let id = s.split('@').next().unwrap_or("");
    id.contains('.')
        && !s.contains(char::is_whitespace)
        && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn list_extensions(cli: &Path) -> Option<Vec<String>> {
    let text = util::run_capture(cli, &["--list-extensions", "--show-versions"], Duration::from_secs(40))?;
    Some(text.lines().map(str::trim).filter(|l| is_extension_id(l)).map(String::from).collect())
}

fn strip_version(id: &str) -> String {
    id.split('@').next().unwrap_or(id).to_string()
}

/// Install each extension, returning the ones that failed.
fn install_extensions(cli: &Path, ids: &[String], mut on: impl FnMut(usize, &str)) -> (u32, Vec<String>) {
    let mut ok = 0;
    let mut failed = vec![];
    for (i, id) in ids.iter().enumerate() {
        if !is_extension_id(id) {
            continue;
        }
        on(i, id);
        if util::run_ok(cli, &["--install-extension", id, "--force"], Duration::from_secs(180)) {
            ok += 1;
        } else {
            failed.push(id.clone());
        }
    }
    (ok, failed)
}

// ------------------------------------------------------------------ catalogue

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    File,
    Dir,
    Extensions,
}

const TOOLS: &[(&str, &str)] = &[
    ("vscode", "VS Code"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("vscodium", "VSCodium"),
    ("zed", "Zed"),
    ("sublime", "Sublime Text"),
    ("neovim", "Neovim"),
    ("jetbrains", "JetBrains IDEs"),
    ("wt", "Windows Terminal"),
    ("git", "Git"),
    ("ssh", "SSH"),
    ("pwsh", "PowerShell"),
    ("shell", "Shell"),
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("gemini", "Gemini CLI"),
    ("copilot", "GitHub Copilot CLI"),
    ("opencode", "OpenCode"),
    ("aider", "Aider"),
    ("continue", "Continue"),
    ("amp", "Amp"),
    ("goose", "Goose"),
    ("claude-desktop", "Claude Desktop"),
    ("agents", "Shared agent skills"),
];

fn tool_name(id: &str) -> String {
    TOOLS.iter().find(|(t, _)| *t == id).map(|(_, n)| n.to_string()).unwrap_or_else(|| id.to_string())
}

const MCP_SECRET: &str = "MCP server definitions often carry API keys or tokens in their env or headers.";
const LOGIN: &str = "A login token. Anyone with this file can use your account.";
const API_KEYS: &str = "Usually holds model provider API keys.";

#[derive(Clone)]
struct Def {
    id: String,
    tool: &'static str,
    label: String,
    kind: Kind,
    win: Option<String>,
    mac: Option<String>,
    cli: Option<&'static str>,
    sensitive: bool,
    secret: &'static str,
    default_on: bool,
}

impl Def {
    fn template(&self) -> Option<&str> {
        if cfg!(windows) { self.win.as_deref() } else { self.mac.as_deref() }
    }
    fn path(&self, b: &Bases) -> Option<PathBuf> {
        self.template().map(|t| expand(t, b))
    }
    fn secret(mut self, why: &'static str) -> Self {
        self.sensitive = true;
        self.secret = why;
        self
    }
    fn off(mut self) -> Self {
        self.default_on = false;
        self
    }
}

fn d(tool: &'static str, slug: &str, label: &str, kind: Kind, win: &str, mac: &str) -> Def {
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
    Def {
        id: format!("{tool}:{slug}"),
        tool,
        label: label.into(),
        kind,
        win: opt(win),
        mac: opt(mac),
        cli: None,
        sensitive: false,
        secret: "",
        default_on: true,
    }
}

fn both(tool: &'static str, slug: &str, label: &str, kind: Kind, t: &str) -> Def {
    d(tool, slug, label, kind, t, t)
}

fn static_defs() -> Vec<Def> {
    use Kind::*;
    let mut v = Vec::new();
    for (tool, dir) in [("vscode", "Code"), ("cursor", "Cursor"), ("windsurf", "Windsurf"), ("vscodium", "VSCodium")] {
        let u = format!("%APPDATA%/{dir}/User");
        v.push(both(tool, "settings", "settings.json", File, &format!("{u}/settings.json")));
        v.push(both(tool, "keybindings", "keybindings.json", File, &format!("{u}/keybindings.json")));
        v.push(both(tool, "snippets", "snippets", Dir, &format!("{u}/snippets")));
        if tool == "vscode" {
            v.push(both(tool, "profiles", "profiles", Dir, &format!("{u}/profiles")));
            v.push(both(tool, "mcp", "mcp.json", File, &format!("{u}/mcp.json")).secret(MCP_SECRET));
        }
        let mut e = d(tool, "extensions", "Extensions", Extensions, "", "");
        e.cli = editor_cli(tool);
        v.push(e);
    }
    v.push(both("cursor", "mcp", "mcp.json", File, "~/.cursor/mcp.json").secret(MCP_SECRET));
    v.push(both("cursor", "rules", "rules", Dir, "~/.cursor/rules"));
    v.push(both("cursor", "skills", "skills", Dir, "~/.cursor/skills"));
    v.push(both("windsurf", "mcp", "mcp_config.json", File, "~/.codeium/windsurf/mcp_config.json").secret(MCP_SECRET));
    v.push(both("windsurf", "global-rules", "global_rules.md", File, "~/.codeium/windsurf/memories/global_rules.md"));
    v.push(d("zed", "settings", "settings.json", File, "%APPDATA%/Zed/settings.json", "~/.config/zed/settings.json"));
    v.push(d("zed", "keymap", "keymap.json", File, "%APPDATA%/Zed/keymap.json", "~/.config/zed/keymap.json"));
    v.push(both("sublime", "user", "Packages/User", Dir, "%APPDATA%/Sublime Text/Packages/User"));
    v.push(d("neovim", "config", "nvim", Dir, "%LOCALAPPDATA%/nvim", "~/.config/nvim"));
    v.push(d(
        "wt",
        "settings",
        "settings.json",
        File,
        "%LOCALAPPDATA%/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
        "",
    ));
    v.push(both("git", "gitconfig", ".gitconfig", File, "~/.gitconfig"));
    v.push(both("git", "ignore", "global ignore", File, "~/.config/git/ignore"));
    v.push(
        both("ssh", "config", "config", File, "~/.ssh/config")
            .secret("Host names, user names and which key opens which server. No keys themselves."),
    );
    v.push(d(
        "pwsh",
        "profile",
        "Microsoft.PowerShell_profile.ps1",
        File,
        "~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1",
        "~/.config/powershell/Microsoft.PowerShell_profile.ps1",
    ));
    v.push(d(
        "pwsh",
        "profile-legacy",
        "Windows PowerShell profile",
        File,
        "~/Documents/WindowsPowerShell/Microsoft.PowerShell_profile.ps1",
        "",
    ));
    for f in [".zshrc", ".bashrc", ".bash_profile", ".zprofile"] {
        v.push(both("shell", f.trim_start_matches('.'), f, File, &format!("~/{f}")));
    }

    // Claude Code: only settings and authored content. projects/, history,
    // debug/, cache/, todos/, statsig/, logs/ and shell snapshots are
    // transcripts and caches and are never listed.
    v.push(both("claude", "settings", "settings.json", File, "~/.claude/settings.json"));
    v.push(both("claude", "claude-md", "CLAUDE.md", File, "~/.claude/CLAUDE.md"));
    for dir in ["rules", "skills", "agents", "commands", "output-styles"] {
        v.push(both("claude", dir, dir, Dir, &format!("~/.claude/{dir}")));
    }
    v.push(both("claude", "keybindings", "keybindings.json", File, "~/.claude/keybindings.json"));
    v.push(both("claude", "plugins", "installed_plugins.json", File, "~/.claude/plugins/installed_plugins.json"));
    v.push(
        both("claude", "claude-json", ".claude.json", File, "~/.claude.json")
            .secret("Your MCP servers (with any API keys in their env) and account details."),
    );
    v.push(both("claude", "credentials", ".credentials.json", File, "~/.claude/.credentials.json").secret(LOGIN));

    v.push(
        both("codex", "config", "config.toml", File, "~/.codex/config.toml")
            .secret("MCP servers in here can carry API keys in their env."),
    );
    v.push(both("codex", "agents-md", "AGENTS.md", File, "~/.codex/AGENTS.md"));
    v.push(both("codex", "skills", "skills", Dir, "~/.codex/skills"));
    v.push(both("codex", "prompts", "prompts", Dir, "~/.codex/prompts"));
    v.push(both("codex", "auth", "auth.json", File, "~/.codex/auth.json").secret(LOGIN));

    v.push(both("gemini", "settings", "settings.json", File, "~/.gemini/settings.json"));
    v.push(both("gemini", "gemini-md", "GEMINI.md", File, "~/.gemini/GEMINI.md"));
    v.push(both("gemini", "commands", "commands", Dir, "~/.gemini/commands"));
    v.push(both("gemini", "extensions", "extensions", Dir, "~/.gemini/extensions"));
    v.push(both("gemini", "env", ".env", File, "~/.gemini/.env").secret(API_KEYS));
    v.push(both("gemini", "oauth", "oauth_creds.json", File, "~/.gemini/oauth_creds.json").secret(LOGIN));
    v.push(
        both("gemini", "accounts", "google_accounts.json", File, "~/.gemini/google_accounts.json")
            .secret("The Google accounts you signed in with."),
    );

    v.push(both("copilot", "settings", "settings.json", File, "~/.copilot/settings.json"));
    v.push(both("copilot", "mcp", "mcp-config.json", File, "~/.copilot/mcp-config.json").secret(MCP_SECRET));
    v.push(both("copilot", "agents", "agents", Dir, "~/.copilot/agents"));
    v.push(both("copilot", "skills", "skills", Dir, "~/.copilot/skills"));
    v.push(both("copilot", "instructions", "copilot-instructions.md", File, "~/.copilot/copilot-instructions.md"));
    v.push(both("copilot", "config", "config.json", File, "~/.copilot/config.json").secret(LOGIN));

    v.push(both("opencode", "config", "opencode.json", File, "~/.config/opencode/opencode.json"));
    v.push(both("opencode", "configc", "opencode.jsonc", File, "~/.config/opencode/opencode.jsonc"));
    v.push(both("opencode", "agents-md", "AGENTS.md", File, "~/.config/opencode/AGENTS.md"));
    v.push(both("opencode", "agents", "agents", Dir, "~/.config/opencode/agents"));
    v.push(both("opencode", "commands", "commands", Dir, "~/.config/opencode/commands"));

    v.push(both("aider", "conf", ".aider.conf.yml", File, "~/.aider.conf.yml").secret(API_KEYS));
    v.push(both("continue", "yaml", "config.yaml", File, "~/.continue/config.yaml").secret(API_KEYS));
    v.push(both("continue", "json", "config.json", File, "~/.continue/config.json").secret(API_KEYS));
    v.push(both("amp", "settings", "settings.json", File, "~/.config/amp/settings.json"));
    v.push(d("goose", "config", "config.yaml", File, "%APPDATA%/Block/goose/config/config.yaml", "~/.config/goose/config.yaml"));
    v.push(
        d("goose", "secrets", "secrets.yaml", File, "%APPDATA%/Block/goose/config/secrets.yaml", "~/.config/goose/secrets.yaml")
            .secret(API_KEYS),
    );
    v.push(
        both("claude-desktop", "config", "claude_desktop_config.json", File, "%APPDATA%/Claude/claude_desktop_config.json")
            .secret(MCP_SECRET),
    );
    v.push(both("agents", "skills", "skills", Dir, "~/.agents/skills"));
    v
}

fn jetbrains_def(dir: &str) -> Option<Def> {
    valid_name(dir).then(|| {
        both("jetbrains", dir, &format!("{dir} options"), Kind::Dir, &format!("%APPDATA%/JetBrains/{dir}/options"))
    })
}

fn ssh_key_def(name: &str) -> Option<Def> {
    if !name.starts_with("id_") || !valid_name(name) {
        return None;
    }
    let def = both("ssh", &format!("key:{name}"), name, Kind::File, &format!("~/.ssh/{name}"));
    Some(if name.ends_with(".pub") {
        def
    } else {
        def.secret("A private SSH key: it opens every server and Git host that trusts it.").off()
    })
}

/// JetBrains keeps one folder per product version (IntelliJIdea2025.2); only
/// the newest of each product is offered.
fn jetbrains_defs(b: &Bases) -> Vec<Def> {
    let Ok(rd) = fs::read_dir(b.roaming.join("JetBrains")) else { return vec![] };
    let mut best: HashMap<String, (Vec<u32>, String)> = HashMap::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !e.path().join("options").is_dir() {
            continue;
        }
        let Some(pos) = name.find(|c: char| c.is_ascii_digit()) else { continue };
        if pos == 0 {
            continue;
        }
        let ver: Vec<u32> = name[pos..].split('.').map(|s| s.parse().unwrap_or(0)).collect();
        let product = name[..pos].to_string();
        if best.get(&product).is_none_or(|(v, _)| *v < ver) {
            best.insert(product, (ver, name));
        }
    }
    let mut out: Vec<Def> = best.into_values().filter_map(|(_, n)| jetbrains_def(&n)).collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

fn ssh_key_defs(b: &Bases) -> Vec<Def> {
    let Ok(rd) = fs::read_dir(b.home.join(".ssh")) else { return vec![] };
    let mut names: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.iter().filter_map(|n| ssh_key_def(n)).collect()
}

fn all_defs(b: &Bases) -> Vec<Def> {
    let mut v = static_defs();
    v.extend(jetbrains_defs(b));
    v.extend(ssh_key_defs(b));
    v
}

/// Item ids are the only thing that decides where a restore writes.
fn def_for_id(id: &str) -> Option<Def> {
    if let Some(d) = static_defs().into_iter().find(|d| d.id == id) {
        return Some(d);
    }
    if let Some(dir) = id.strip_prefix("jetbrains:") {
        return jetbrains_def(dir);
    }
    if let Some(name) = id.strip_prefix("ssh:key:") {
        return ssh_key_def(name);
    }
    None
}

// ------------------------------------------------------------------ discovery

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConfigItem {
    pub id: String,
    pub tool: String,
    pub label: String,
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub exists: bool,
    pub sensitive: bool,
    /// What secret the item may hold, for the tooltip.
    pub secret: String,
    /// "too large", "42 extensions"...
    pub note: String,
    pub default_selected: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConfigTool {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub items: Vec<ConfigItem>,
}

fn probe(d: &Def, b: &Bases) -> Option<ConfigItem> {
    let mut item = ConfigItem {
        id: d.id.clone(),
        tool: d.tool.into(),
        label: d.label.clone(),
        path: String::new(),
        kind: d.kind,
        size: 0,
        exists: true,
        sensitive: d.sensitive,
        secret: d.secret.into(),
        note: String::new(),
        default_selected: d.default_on,
    };
    match d.kind {
        Kind::Extensions => {
            let exe = cli(d.cli?)?;
            let exts = list_extensions(&exe)?;
            if exts.is_empty() {
                return None;
            }
            item.size = exts.iter().map(|e| e.len() as u64 + 1).sum();
            item.note = format!("{} extensions", exts.len());
            item.path = format!("{} --list-extensions", exe.display());
        }
        Kind::File => {
            let p = d.path(b)?;
            let m = fs::metadata(&p).ok()?;
            if !m.is_file() {
                return None;
            }
            item.size = m.len();
            item.path = p.to_string_lossy().into_owned();
        }
        Kind::Dir => {
            let p = d.path(b)?;
            if !p.is_dir() {
                return None;
            }
            let (size, over) = measure(&p, DIR_CAP);
            if size == 0 && !over {
                return None;
            }
            item.size = size;
            if over {
                item.note = "too large".into();
                item.default_selected = false;
            }
            item.path = p.to_string_lossy().into_owned();
        }
    }
    Some(item)
}

pub fn list_sources() -> Vec<ConfigTool> {
    let b = Bases::current();
    let defs = all_defs(&b);
    let items: Vec<ConfigItem> = defs.par_iter().filter_map(|d| probe(d, &b)).collect();
    TOOLS
        .iter()
        .filter_map(|(id, name)| {
            let items: Vec<ConfigItem> = items.iter().filter(|i| i.tool == *id).cloned().collect();
            (!items.is_empty()).then(|| ConfigTool {
                id: id.to_string(),
                name: name.to_string(),
                size: items.iter().map(|i| i.size).sum(),
                items,
            })
        })
        .collect()
}

// ------------------------------------------------------------------ export

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManifestItem {
    pub id: String,
    pub tool: String,
    pub label: String,
    pub kind: Kind,
    pub original_path: String,
    pub archive_path: String,
    pub sensitive: bool,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub windows_path: Option<String>,
    #[serde(default)]
    pub mac_path: Option<String>,
    #[serde(default)]
    pub cli: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub app: String,
    pub format_version: u32,
    pub created_at: String,
    pub source_os: String,
    pub hostname: String,
    pub items: Vec<ManifestItem>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub items: usize,
    pub bytes: u64,
    pub skipped: Vec<String>,
}

type Zw = zip::ZipWriter<fs::File>;

fn zip_opts() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn zip_put(z: &mut Zw, name: &str, bytes: &[u8]) -> Res<()> {
    z.start_file(name, zip_opts()).map_err(es)?;
    z.write_all(bytes).map_err(es)
}

/// Add one item; returns (original path template, bytes stored).
fn add_item(z: &mut Zw, d: &Def, b: &Bases, ap: &str) -> Res<(String, u64)> {
    match d.kind {
        Kind::Extensions => {
            let name = d.cli.ok_or("No CLI")?;
            let exe = cli(name).ok_or_else(|| format!("`{name}` command not found"))?;
            let exts = list_extensions(&exe).ok_or("Could not list extensions")?;
            let text = exts.join("\n") + "\n";
            zip_put(z, ap, text.as_bytes())?;
            Ok((name.to_string(), text.len() as u64))
        }
        Kind::File => {
            let p = d.path(b).ok_or("Not used on this OS")?;
            let bytes = fs::read(&p).map_err(es)?;
            zip_put(z, ap, &bytes)?;
            Ok((templatize(&p, b), bytes.len() as u64))
        }
        Kind::Dir => {
            let p = d.path(b).ok_or("Not used on this OS")?;
            let (_, over) = measure(&p, DIR_CAP);
            if over {
                return Err("larger than 200 MB".into());
            }
            let mut total = 0u64;
            let mut err = None;
            walk_files(&p, true, &mut |abs, rel, _| match fs::read(abs) {
                Ok(bytes) => match zip_put(z, &format!("{ap}/{rel}"), &bytes) {
                    Ok(()) => {
                        total += bytes.len() as u64;
                        true
                    }
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                },
                // A locked file inside a folder should not sink the backup.
                Err(_) => true,
            });
            match err {
                Some(e) => Err(e),
                None => Ok((templatize(&p, b), total)),
            }
        }
    }
}

pub fn export(app: &AppHandle, ids: &[String], dest: &str) -> Res<ExportResult> {
    if ids.is_empty() {
        return Err("Nothing selected".into());
    }
    let b = Bases::current();
    let dest = PathBuf::from(dest);
    let file = fs::File::create(&dest).map_err(|e| format!("Could not create {}: {e}", dest.display()))?;
    let mut z = zip::ZipWriter::new(file);
    let mut items = vec![];
    let mut skipped = vec![];
    let mut used = HashSet::new();
    let mut bytes = 0u64;
    for (i, id) in ids.iter().enumerate() {
        let Some(d) = def_for_id(id) else {
            skipped.push(format!("{id}: unknown item"));
            continue;
        };
        let _ = app.emit(
            "config-export-progress",
            Progress { done: i, total: ids.len(), current: format!("{} · {}", tool_name(d.tool), d.label) },
        );
        let mut ap = if d.kind == Kind::Extensions {
            format!("files/{}/extensions.txt", d.tool)
        } else {
            format!("files/{}/{}", d.tool, slug(&d.label))
        };
        let base = ap.clone();
        let mut n = 2;
        while !used.insert(ap.clone()) {
            ap = format!("{base}-{n}");
            n += 1;
        }
        match add_item(&mut z, &d, &b, &ap) {
            Ok((original, size)) => {
                bytes += size;
                items.push(ManifestItem {
                    id: d.id.clone(),
                    tool: d.tool.into(),
                    label: d.label.clone(),
                    kind: d.kind,
                    original_path: original,
                    archive_path: ap,
                    sensitive: d.sensitive,
                    tool_name: tool_name(d.tool),
                    windows_path: d.win.clone(),
                    mac_path: d.mac.clone(),
                    cli: d.cli.map(String::from),
                });
            }
            Err(e) => skipped.push(format!("{} {}: {e}", tool_name(d.tool), d.label)),
        }
    }
    if items.is_empty() {
        drop(z);
        let _ = fs::remove_file(&dest);
        return Err(format!("Nothing could be backed up. {}", skipped.join("; ")));
    }
    let manifest = Manifest {
        app: "dev-cleaner".into(),
        format_version: FORMAT_VERSION,
        created_at: iso(now_secs()),
        source_os: std::env::consts::OS.into(),
        hostname: sysinfo::System::host_name().unwrap_or_default(),
        items,
    };
    zip_put(&mut z, "manifest.json", serde_json::to_string_pretty(&manifest).map_err(es)?.as_bytes())?;
    zip_put(&mut z, "AGENT-RESTORE.md", agent_doc(&manifest).as_bytes())?;
    z.finish().map_err(es)?;
    let _ = app.emit("config-export-progress", Progress { done: ids.len(), total: ids.len(), current: String::new() });
    Ok(ExportResult { path: dest.to_string_lossy().into(), items: manifest.items.len(), bytes, skipped })
}

/// Plain instructions an AI coding agent can follow to restore the bundle
/// without Dev Cleaner. Generated from the manifest, so it lists exactly
/// what is in the zip.
fn agent_doc(m: &Manifest) -> String {
    let mut s = String::new();
    s += "# Restore developer configs\n\n";
    s += &format!(
        "This archive was made by Dev Cleaner on `{}` ({}) at {}. It holds {} item(s). \
         The archive paths below are relative to the root of the zip.\n\n",
        if m.hostname.is_empty() { "unknown host" } else { &m.hostname },
        m.source_os,
        m.created_at,
        m.items.len()
    );
    let secrets: Vec<&ManifestItem> = m.items.iter().filter(|i| i.sensitive).collect();
    if !secrets.is_empty() {
        s += "> **WARNING: this archive contains secrets in plain text** (login tokens, API keys or private keys):\n";
        for i in &secrets {
            s += &format!("> - {} — {} (`{}`)\n", i.tool_name, i.label, i.archive_path);
        }
        s += "> Do not commit, upload or share it. Delete the extracted copy when you are done.\n\n";
    }
    s += "## Instructions for the agent\n\n";
    s += "1. Detect the OS (Windows or macOS) and use the matching destination for each item.\n";
    s += "2. Paths: `~` is the home folder (`%USERPROFILE%` on Windows); `%APPDATA%` is the Roaming folder and \
           `%LOCALAPPDATA%` the Local folder on Windows; on macOS both are `~/Library/Application Support`.\n";
    s += "3. **Back up the existing file or folder first**: copy it next to itself as `<name>.bak-<date>` before writing.\n";
    s += "4. Files: copy the archive file to the destination, creating parent folders. \
           Folders: copy the archive folder's contents into the destination (same-named files are replaced, others kept).\n";
    s += "5. Do not edit the contents. Skip items whose destination says \"not used on this OS\".\n";
    s += "6. Ask the user before restoring any item marked as containing secrets.\n\n";
    s += "## Items\n\n";
    for (n, i) in m.items.iter().enumerate() {
        s += &format!("### {}. {} — {}\n\n", n + 1, i.tool_name, i.label);
        if i.kind == Kind::Extensions {
            let exe = i.cli.clone().unwrap_or_else(|| "code".into());
            s += &format!("- Archive: `{}` (one extension id per line, `publisher.name@version`)\n", i.archive_path);
            s += &format!(
                "- Install: for each line run `{exe} --install-extension <id> --force`. Drop the `@version` part to get the latest.\n"
            );
            if exe != "code" {
                s += "- Note: this editor installs from Open VSX, which lacks some Microsoft-only extensions; \
                       report the ones that fail instead of stopping.\n";
            }
            s += "\n";
            continue;
        }
        let what = if i.kind == Kind::Dir { "folder" } else { "file" };
        s += &format!("- Archive: `{}` ({what})\n", i.archive_path);
        let win = i.windows_path.as_deref().map(windows_form);
        let mac = i.mac_path.as_deref().map(mac_form);
        if win.is_none() && mac.is_none() {
            s += &format!("- Original location: `{}`\n", i.original_path);
        } else {
            s += &format!("- Windows: {}\n", win.map(|p| format!("`{p}`")).unwrap_or("not used on this OS".into()));
            s += &format!("- macOS: {}\n", mac.map(|p| format!("`{p}`")).unwrap_or("not used on this OS".into()));
        }
        if i.sensitive {
            s += "- Contains secrets: yes\n";
        }
        s += "\n";
    }
    s += "## Verify\n\n";
    s += "- [ ] Every destination above exists and matches the archive (compare file sizes or hashes).\n";
    s += "- [ ] JSON settings files still parse (VS Code style `settings.json` allows comments).\n";
    if m.items.iter().any(|i| i.kind == Kind::Extensions) {
        s += "- [ ] `<cli> --list-extensions` shows the expected extensions; list any that failed.\n";
    }
    if m.items.iter().any(|i| i.tool == "ssh") {
        s += "- [ ] SSH files are private: on macOS `chmod 700 ~/.ssh && chmod 600 ~/.ssh/config ~/.ssh/id_*` \
               (keep `.pub` at 644); on Windows only your user should have access.\n";
    }
    s += "- [ ] The `.bak-<date>` copies are kept until the user confirms everything works.\n";
    s += "- [ ] Restart the restored tools and editors so they pick up the new settings.\n";
    s
}

// ------------------------------------------------------------------ backups

fn backups_root() -> PathBuf {
    util::local_dir().join("dev-cleaner").join("config-backups")
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BackupEntry {
    target: String,
    stored: Option<String>,
    is_dir: bool,
    existed: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    id: String,
    created_at: u64,
    reason: String,
    #[serde(default)]
    undone: bool,
    entries: Vec<BackupEntry>,
}

/// A copy of everything a restore or copy is about to replace.
struct Snapshot {
    m: BackupManifest,
    dir: PathBuf,
    seen: HashSet<PathBuf>,
}

impl Snapshot {
    fn new(reason: &str) -> Res<Self> {
        let root = backups_root();
        fs::create_dir_all(&root).map_err(es)?;
        let now = now_secs();
        let base = stamp(now);
        let mut id = base.clone();
        let mut n = 1;
        while root.join(&id).exists() {
            n += 1;
            id = format!("{base}-{n}");
        }
        let dir = root.join(&id);
        fs::create_dir_all(dir.join("files")).map_err(es)?;
        Ok(Self {
            m: BackupManifest { id, created_at: now, reason: reason.into(), undone: false, entries: vec![] },
            dir,
            seen: HashSet::new(),
        })
    }

    fn save(&mut self, target: &Path) -> Res<()> {
        if !self.seen.insert(target.to_path_buf()) {
            return Ok(());
        }
        let n = self.m.entries.len();
        let (stored, is_dir, existed) = match fs::metadata(target) {
            Err(_) => (None, false, false),
            Ok(meta) => {
                let to = self.dir.join("files").join(n.to_string());
                let fail = |e: String| format!("Could not back up {}: {e}", target.display());
                if meta.is_dir() {
                    copy_tree(target, &to, false).map_err(fail)?;
                } else {
                    fs::copy(target, &to).map_err(|e| fail(e.to_string()))?;
                }
                (Some(format!("files/{n}")), meta.is_dir(), true)
            }
        };
        self.m.entries.push(BackupEntry { target: target.to_string_lossy().into(), stored, is_dir, existed });
        Ok(())
    }

    fn finish(&self) -> Res<()> {
        write_file(&self.dir.join("backup.json"), serde_json::to_string_pretty(&self.m).map_err(es)?.as_bytes())
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub id: String,
    pub created_at: u64,
    pub reason: String,
    pub count: usize,
    pub replaced: usize,
    pub undone: bool,
    pub targets: Vec<String>,
}

pub fn list_backups() -> Vec<BackupInfo> {
    let b = Bases::current();
    let Ok(rd) = fs::read_dir(backups_root()) else { return vec![] };
    let mut out: Vec<BackupInfo> = rd
        .flatten()
        .filter_map(|e| {
            let text = fs::read_to_string(e.path().join("backup.json")).ok()?;
            let m: BackupManifest = serde_json::from_str(&text).ok()?;
            Some(BackupInfo {
                id: m.id,
                created_at: m.created_at,
                reason: m.reason,
                count: m.entries.len(),
                replaced: m.entries.iter().filter(|e| e.existed).count(),
                undone: m.undone,
                targets: m.entries.iter().map(|e| templatize(Path::new(&e.target), &b)).collect(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    out
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    pub restored: u32,
    pub failed: Vec<String>,
}

/// Put every target back the way the snapshot found it: replaced items are
/// copied back, items that did not exist before are removed.
pub fn undo(id: &str) -> Res<UndoResult> {
    if !valid_name(id) {
        return Err("Unknown backup".into());
    }
    let dir = backups_root().join(id);
    let text = fs::read_to_string(dir.join("backup.json")).map_err(|_| "Backup not found".to_string())?;
    let mut m: BackupManifest = serde_json::from_str(&text).map_err(es)?;
    if m.undone {
        return Err("This backup was already put back".into());
    }
    let mut restored = 0;
    let mut failed = vec![];
    for e in m.entries.iter().rev() {
        let target = PathBuf::from(&e.target);
        if fs::symlink_metadata(&target).is_ok() {
            if let Err(err) = remove_path(&target) {
                failed.push(format!("{}: {err}", e.target));
                continue;
            }
        }
        if !e.existed {
            restored += 1;
            continue;
        }
        let Some(src) = e.stored.as_deref().and_then(|s| safe_join(&dir, s)) else {
            failed.push(format!("{}: backup copy missing", e.target));
            continue;
        };
        let r = if e.is_dir {
            copy_tree(&src, &target, false).map(|_| ())
        } else {
            fs::read(&src).map_err(es).and_then(|bytes| write_file(&target, &bytes))
        };
        match r {
            Ok(()) => restored += 1,
            Err(err) => failed.push(format!("{}: {err}", e.target)),
        }
    }
    m.undone = true;
    write_file(&dir.join("backup.json"), serde_json::to_string_pretty(&m).map_err(es)?.as_bytes())?;
    Ok(UndoResult { restored, failed })
}

// ------------------------------------------------------------------ restore

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BundleItem {
    pub id: String,
    pub tool: String,
    pub tool_name: String,
    pub label: String,
    pub kind: Kind,
    pub sensitive: bool,
    pub target: String,
    pub replaces: bool,
    pub supported: bool,
    pub note: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BundleInfo {
    pub path: String,
    pub created_at: String,
    pub source_os: String,
    pub hostname: String,
    pub items: Vec<BundleItem>,
}

fn open_zip(path: &str) -> Res<zip::ZipArchive<fs::File>> {
    let f = fs::File::open(path).map_err(|e| format!("Could not open {path}: {e}"))?;
    zip::ZipArchive::new(f).map_err(|e| format!("Not a zip file: {e}"))
}

fn read_manifest(z: &mut zip::ZipArchive<fs::File>) -> Res<Manifest> {
    let mut text = String::new();
    z.by_name("manifest.json")
        .map_err(|_| "This zip was not made by Dev Cleaner (no manifest.json)".to_string())?
        .read_to_string(&mut text)
        .map_err(es)?;
    let m: Manifest = serde_json::from_str(&text).map_err(|e| format!("Bad manifest: {e}"))?;
    if m.app != "dev-cleaner" || m.format_version > FORMAT_VERSION {
        return Err("This backup was made by a newer or different app".into());
    }
    Ok(m)
}

/// Where an item would go on this machine, decided by its id alone.
fn resolve(it: &ManifestItem, b: &Bases) -> Res<(Def, Option<PathBuf>)> {
    let d = def_for_id(&it.id).ok_or("Not supported by this version")?;
    if d.kind != it.kind {
        return Err("Not supported by this version".into());
    }
    if d.kind == Kind::Extensions {
        return Ok((d, None));
    }
    let p = d.path(b).ok_or("Not used on this OS")?;
    Ok((d, Some(p)))
}

pub fn read_bundle(path: &str) -> Res<BundleInfo> {
    let b = Bases::current();
    let mut z = open_zip(path)?;
    let m = read_manifest(&mut z)?;
    let items = m
        .items
        .iter()
        .map(|it| {
            let mut bi = BundleItem {
                id: it.id.clone(),
                tool: it.tool.clone(),
                tool_name: tool_name(&it.tool),
                label: it.label.clone(),
                kind: it.kind,
                sensitive: it.sensitive,
                target: String::new(),
                replaces: false,
                supported: true,
                note: String::new(),
            };
            match resolve(it, &b) {
                Ok((d, Some(p))) => {
                    bi.replaces = p.exists();
                    bi.target = p.to_string_lossy().into();
                    bi.tool = d.tool.into();
                }
                Ok((d, None)) => {
                    let name = d.cli.unwrap_or("code");
                    bi.target = format!("{name} --install-extension");
                    if cli(name).is_none() {
                        bi.supported = false;
                        bi.note = format!("`{name}` command not found");
                    }
                }
                Err(e) => {
                    bi.supported = false;
                    bi.note = e;
                }
            }
            bi
        })
        .collect();
    Ok(BundleInfo { path: path.into(), created_at: m.created_at, source_os: m.source_os, hostname: m.hostname, items })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub label: String,
    pub error: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub backup_id: String,
    pub restored: u32,
    pub failed: Vec<Failure>,
    pub extensions_installed: u32,
    pub extensions_failed: Vec<String>,
}

fn zip_read(z: &mut zip::ZipArchive<fs::File>, name: &str) -> Res<Vec<u8>> {
    let mut f = z.by_name(name).map_err(|_| format!("{name} missing from the zip"))?;
    let mut buf = vec![];
    f.read_to_end(&mut buf).map_err(es)?;
    Ok(buf)
}

fn extract_dir(z: &mut zip::ZipArchive<fs::File>, names: &[String], ap: &str, target: &Path) -> Res<u32> {
    let prefix = format!("{ap}/");
    let mut n = 0;
    let mut unsafe_entries = 0;
    for name in names.iter().filter(|n| n.starts_with(&prefix) && !n.ends_with('/')) {
        let Some(to) = safe_join(target, &name[prefix.len()..]) else {
            unsafe_entries += 1;
            continue;
        };
        let bytes = zip_read(z, name)?;
        write_file(&to, &bytes)?;
        n += 1;
    }
    if unsafe_entries > 0 {
        return Err(format!("{unsafe_entries} unsafe path(s) in the zip were refused; {n} files written"));
    }
    Ok(n)
}

pub fn import(app: &AppHandle, zip_path: &str, ids: &[String]) -> Res<ImportResult> {
    let b = Bases::current();
    let mut z = open_zip(zip_path)?;
    let m = read_manifest(&mut z)?;
    let names: Vec<String> = z.file_names().map(String::from).collect();
    let mut failed = vec![];
    let mut work = vec![];
    for it in m.items.iter().filter(|i| ids.contains(&i.id)) {
        match resolve(it, &b) {
            Ok((d, p)) => work.push((it, d, p)),
            Err(e) => failed.push(Failure { label: format!("{} {}", tool_name(&it.tool), it.label), error: e }),
        }
    }
    if work.is_empty() {
        return Err("None of the selected items can be restored here".into());
    }
    let file = Path::new(zip_path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let mut snap = Snapshot::new(&format!("Restore from {file}"))?;
    for (_, _, p) in &work {
        if let Some(p) = p {
            snap.save(p)?;
        }
    }
    snap.finish()?;

    let total = work.len();
    let mut restored = 0;
    let mut ext_ok = 0;
    let mut ext_failed = vec![];
    for (i, (it, d, p)) in work.iter().enumerate() {
        let label = format!("{} {}", tool_name(d.tool), d.label);
        let _ = app.emit("config-import-progress", Progress { done: i, total, current: label.clone() });
        let r = match (d.kind, p) {
            (Kind::File, Some(p)) => zip_read(&mut z, &it.archive_path).and_then(|bytes| write_file(p, &bytes)),
            (Kind::Dir, Some(p)) => extract_dir(&mut z, &names, &it.archive_path, p).map(|_| ()),
            (Kind::Extensions, _) => {
                let name = d.cli.unwrap_or("code");
                match (cli(name), zip_read(&mut z, &it.archive_path)) {
                    (None, _) => Err(format!("`{name}` command not found; install the extensions by hand")),
                    (_, Err(e)) => Err(e),
                    (Some(exe), Ok(bytes)) => {
                        let ids: Vec<String> =
                            String::from_utf8_lossy(&bytes).lines().map(str::trim).map(strip_version).collect();
                        let (ok, bad) = install_extensions(&exe, &ids, |_, id| {
                            let _ = app.emit(
                                "config-import-progress",
                                Progress { done: i, total, current: format!("Installing {id}") },
                            );
                        });
                        ext_ok += ok;
                        ext_failed.extend(bad);
                        Ok(())
                    }
                }
            }
            _ => Err("Not used on this OS".into()),
        };
        match r {
            Ok(()) => restored += 1,
            Err(error) => failed.push(Failure { label, error }),
        }
    }
    let _ = app.emit("config-import-progress", Progress { done: total, total, current: String::new() });
    Ok(ImportResult {
        backup_id: snap.m.id.clone(),
        restored,
        failed,
        extensions_installed: ext_ok,
        extensions_failed: ext_failed,
    })
}

// ------------------------------------------------------------------ JSONC

/// Strip `//` and `/* */` comments and trailing commas so serde_json can
/// read VS Code-style settings. Returns the cleaned text and whether any
/// comments were found.
fn strip_jsonc(src: &str) -> (String, bool) {
    let chars: Vec<char> = src.trim_start_matches('\u{feff}').chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut had = false;
    let mut in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('"', _) => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            ('/', Some('/')) => {
                had = true;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                had = true;
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Trailing commas.
    let chars: Vec<char> = out.chars().collect();
    let mut res = String::with_capacity(chars.len());
    in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            res.push(c);
            if c == '\\' && i + 1 < chars.len() {
                res.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            res.push(c);
        } else if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if !matches!(chars.get(j), Some('}') | Some(']')) {
                res.push(c);
            }
        } else {
            res.push(c);
        }
        i += 1;
    }
    (res, had)
}

fn parse_jsonc(src: &str) -> Res<(Value, bool)> {
    let (clean, had) = strip_jsonc(src);
    if clean.trim().is_empty() {
        return Ok((json!({}), had));
    }
    serde_json::from_str(&clean).map(|v| (v, had)).map_err(es)
}

fn read_json_file(path: &Path) -> Res<(Value, bool)> {
    match fs::read_to_string(path) {
        Ok(text) => parse_jsonc(&text),
        Err(_) if !path.exists() => Ok((json!({}), false)),
        Err(e) => Err(e.to_string()),
    }
}

// ------------------------------------------------------------------ TOML bits

fn toml_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Body of a `"""` multi-line basic string.
fn toml_ml(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' | '\t' => o.push(c),
            '\r' => {}
            c if (c as u32) < 0x20 || c as u32 == 0x7f => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.replace("\"\"\"", "\"\"\\\"")
}

fn toml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        k.to_string()
    } else {
        toml_str(k)
    }
}

fn value_text(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| v.to_string())
}

fn toml_arr(xs: &[Value]) -> String {
    format!("[{}]", xs.iter().map(|v| toml_str(&value_text(v))).collect::<Vec<_>>().join(", "))
}

fn toml_inline(m: &Map<String, Value>) -> String {
    let parts: Vec<String> = m.iter().map(|(k, v)| format!("{} = {}", toml_key(k), toml_str(&value_text(v)))).collect();
    format!("{{ {} }}", parts.join(", "))
}

// ------------------------------------------------------------------ markdown conversions

/// Split YAML front matter into (key, raw value) pairs and the body.
/// Continuation lines (lists, folded text) stay with their key.
fn split_frontmatter(text: &str) -> (Vec<(String, String)>, String) {
    let t = text.replace("\r\n", "\n");
    let t = t.trim_start_matches('\u{feff}');
    if let Some(rest) = t.strip_prefix("---\n") {
        let end = if rest.starts_with("---") { Some(0) } else { rest.find("\n---").map(|e| e + 1) };
        if let Some(end) = end {
            let fm = &rest[..end];
            let after = &rest[end + 3..];
            let body = after.split_once('\n').map(|(_, b)| b).unwrap_or("");
            let mut pairs: Vec<(String, String)> = vec![];
            for line in fm.lines() {
                let cont = line.starts_with(' ') || line.starts_with('\t') || line.starts_with('-');
                if cont {
                    if let Some(last) = pairs.last_mut() {
                        last.1.push('\n');
                        last.1.push_str(line);
                    }
                    continue;
                }
                if let Some((k, v)) = line.split_once(':') {
                    pairs.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            return (pairs, body.to_string());
        }
    }
    (vec![], t.to_string())
}

fn fm_get(pairs: &[(String, String)], key: &str) -> Option<String> {
    let raw = &pairs.iter().find(|(k, _)| k == key)?.1;
    let mut lines = raw.lines();
    let first = lines.next().unwrap_or("").trim();
    let v = if first.starts_with('>') || first.starts_with('|') {
        lines.map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
    } else {
        let f = first;
        let unq = if f.len() >= 2 && ((f.starts_with('"') && f.ends_with('"')) || (f.starts_with('\'') && f.ends_with('\''))) {
            &f[1..f.len() - 1]
        } else {
            f
        };
        unq.to_string()
    };
    (!v.is_empty()).then_some(v)
}

/// YAML-safe scalar (a JSON string is valid YAML).
fn yq(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Claude command body -> Gemini syntax: `$ARGUMENTS` -> `{{args}}`,
/// ``!`cmd` `` -> `!{cmd}`.
fn gemini_body(body: &str) -> String {
    let body = body.replace("$ARGUMENTS", "{{args}}");
    let mut out = String::with_capacity(body.len());
    let mut rest = body.as_str();
    while let Some(pos) = rest.find("!`") {
        let after = &rest[pos + 2..];
        match after.find('`') {
            Some(end) if !after[..end].contains('\n') => {
                out.push_str(&rest[..pos]);
                out.push_str("!{");
                out.push_str(&after[..end]);
                out.push('}');
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..pos + 2]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn command_to_gemini(text: &str) -> String {
    let (fm, body) = split_frontmatter(text);
    let mut out = String::new();
    if let Some(desc) = fm_get(&fm, "description") {
        out.push_str(&format!("description = {}\n", toml_str(&desc)));
    }
    let body = gemini_body(body.trim_matches('\n'));
    out.push_str(&format!("prompt = \"\"\"\n{}\n\"\"\"\n", toml_ml(body.trim_end())));
    out
}

fn with_newline(mut s: String) -> String {
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn command_to_skill(name: &str, text: &str) -> String {
    let (fm, body) = split_frontmatter(text);
    let desc = fm_get(&fm, "description").unwrap_or_else(|| format!("Converted from the /{name} command"));
    let mut out = format!("---\nname: {name}\ndescription: {}\n", yq(&desc));
    if let Some((_, v)) = fm.iter().find(|(k, _)| k == "allowed-tools") {
        out.push_str(&format!("allowed-tools: {v}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(body.trim_start_matches('\n'));
    with_newline(out)
}

/// OpenCode subagent: keeps the description, adds `mode: subagent`, drops
/// Claude-only keys (tools, model names, colour).
fn agent_to_opencode(name: &str, text: &str) -> String {
    let (fm, body) = split_frontmatter(text);
    let desc = fm_get(&fm, "description").unwrap_or_else(|| format!("{name} subagent"));
    let mut out = format!("---\ndescription: {}\nmode: subagent\n", yq(&desc));
    out.push_str("---\n\n");
    out.push_str(body.trim_start_matches('\n'));
    with_newline(out)
}

fn agent_to_copilot(name: &str, text: &str) -> String {
    let (fm, body) = split_frontmatter(text);
    let desc = fm_get(&fm, "description").unwrap_or_else(|| format!("{name} agent"));
    let mut out = format!("---\nname: {name}\ndescription: {}\n---\n\n", yq(&desc));
    out.push_str(body.trim_start_matches('\n'));
    with_newline(out)
}

/// Claude's `@path` import lines are not understood by every tool: inline
/// the file when it exists, otherwise leave a comment.
fn inline_imports(text: &str, base: &Path, home: &Path) -> (String, usize) {
    let mut n = 0;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        let is_import = t.len() > 1
            && t.starts_with('@')
            && !t.contains(char::is_whitespace)
            && (t.contains('.') || t.contains('/'));
        if !is_import {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let rel = &t[1..];
        let p = if let Some(r) = rel.strip_prefix("~/") {
            home.join(r)
        } else if Path::new(rel).is_absolute() {
            PathBuf::from(rel)
        } else {
            base.join(rel)
        };
        match fs::metadata(&p).ok().filter(|m| m.is_file() && m.len() < 256 * 1024).and_then(|_| fs::read_to_string(&p).ok()) {
            Some(content) => {
                n += 1;
                out.push_str(&format!("<!-- inlined from {t} -->\n{}\n<!-- end of {t} -->\n", content.trim_end()));
            }
            None => out.push_str(&format!("<!-- removed import {t}: file not found -->\n")),
        }
    }
    (out, n)
}

// ------------------------------------------------------------------ MCP

fn mcp_location(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "claude" => ("Claude Code", "~/.claude.json", "mcpServers"),
        "claude-desktop" => ("Claude Desktop", "%APPDATA%/Claude/claude_desktop_config.json", "mcpServers"),
        "cursor" => ("Cursor", "~/.cursor/mcp.json", "mcpServers"),
        "gemini" => ("Gemini CLI", "~/.gemini/settings.json", "mcpServers"),
        "vscode" => ("VS Code", "%APPDATA%/Code/User/mcp.json", "servers"),
        "zed" => (
            "Zed",
            if cfg!(windows) { "%APPDATA%/Zed/settings.json" } else { "~/.config/zed/settings.json" },
            "context_servers",
        ),
        "codex" => ("Codex", "~/.codex/config.toml", "mcp_servers"),
        _ => return None,
    })
}

const MCP_SOURCES: [&str; 5] = ["claude", "claude-desktop", "cursor", "gemini", "vscode"];
const MCP_TARGETS: [&str; 7] = ["claude", "claude-desktop", "cursor", "gemini", "vscode", "zed", "codex"];

fn pick(o: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    let mut m = Map::new();
    for k in keys {
        if let Some(v) = o.get(*k) {
            m.insert(k.to_string(), v.clone());
        }
    }
    m
}

/// Any tool's server entry -> one shape: stdio `{command, args, env}` or
/// remote `{type: http|sse, url, headers}`.
fn normalize_mcp(src: &str, s: &Value) -> Option<Value> {
    let o = s.as_object()?;
    if o.get("command").and_then(Value::as_str).is_some() {
        return Some(Value::Object(pick(o, &["command", "args", "env"])));
    }
    let (url, ty) = if src == "gemini" {
        if let Some(u) = o.get("httpUrl") {
            (u.clone(), "http")
        } else {
            (o.get("url")?.clone(), "sse")
        }
    } else {
        let u = o.get("url").or_else(|| o.get("serverUrl"))?.clone();
        let ty = if o.get("type").and_then(Value::as_str) == Some("sse") { "sse" } else { "http" };
        (u, ty)
    };
    let mut c = Map::new();
    c.insert("type".into(), json!(ty));
    c.insert("url".into(), url);
    if let Some(h) = o.get("headers").filter(|h| h.is_object()) {
        c.insert("headers".into(), h.clone());
    }
    Some(Value::Object(c))
}

fn read_mcp(src: &str, b: &Bases) -> BTreeMap<String, Value> {
    let Some((_, tpl, key)) = mcp_location(src) else { return BTreeMap::new() };
    let Ok((v, _)) = read_json_file(&expand(tpl, b)) else { return BTreeMap::new() };
    v.get(key)
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(n, s)| normalize_mcp(src, s).map(|c| (n.clone(), c))).collect())
        .unwrap_or_default()
}

/// The canonical server in the shape `target` expects; None if it cannot
/// hold that kind of server.
fn mcp_for(target: &str, c: &Value) -> Option<Value> {
    let o = c.as_object()?;
    let remote = o.contains_key("url");
    let ty = o.get("type").and_then(Value::as_str).unwrap_or("http");
    let stdio = pick(o, &["command", "args", "env"]);
    let mut m = Map::new();
    match (target, remote) {
        ("claude-desktop", true) => return None,
        ("claude" | "vscode", false) => {
            m.insert("type".into(), json!("stdio"));
            m.extend(stdio);
        }
        ("zed", false) => {
            m.insert("source".into(), json!("custom"));
            m.extend(stdio);
        }
        (_, false) => m.extend(stdio),
        ("claude" | "vscode", true) => {
            m.insert("type".into(), json!(ty));
            m.extend(pick(o, &["url", "headers"]));
        }
        ("gemini", true) => {
            let key = if ty == "sse" { "url" } else { "httpUrl" };
            m.insert(key.into(), o.get("url")?.clone());
            m.extend(pick(o, &["headers"]));
        }
        (_, true) => m.extend(pick(o, &["url", "headers"])),
    }
    Some(Value::Object(m))
}

/// One `[mcp_servers.<name>]` table for Codex's config.toml.
fn mcp_to_codex(name: &str, c: &Value) -> Option<String> {
    let o = c.as_object()?;
    let mut s = format!("[mcp_servers.{}]\n", toml_key(name));
    if let Some(cmd) = o.get("command").and_then(Value::as_str) {
        s.push_str(&format!("command = {}\n", toml_str(cmd)));
        if let Some(args) = o.get("args").and_then(Value::as_array).filter(|a| !a.is_empty()) {
            s.push_str(&format!("args = {}\n", toml_arr(args)));
        }
        if let Some(env) = o.get("env").and_then(Value::as_object).filter(|e| !e.is_empty()) {
            s.push_str(&format!("env = {}\n", toml_inline(env)));
        }
    } else {
        let url = o.get("url").and_then(Value::as_str)?;
        s.push_str(&format!("url = {}\n", toml_str(url)));
        if let Some(h) = o.get("headers").and_then(Value::as_object).filter(|h| !h.is_empty()) {
            s.push_str(&format!("http_headers = {}\n", toml_inline(h)));
        }
    }
    Some(s)
}

/// The server a `[mcp_servers.x]` or `[mcp_servers.x.env]` header belongs to.
fn codex_header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with('[') || t.starts_with("[[") {
        return None;
    }
    let rest = t[1..].trim_start().strip_prefix("mcp_servers.")?;
    if let Some(r) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = r.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '"' => return Some(out),
                c => out.push(c),
            }
        }
        None
    } else {
        let end = rest.find(['.', ']']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

fn remove_codex_servers(text: &str, names: &HashSet<String>) -> String {
    let mut skipping = false;
    let mut out = vec![];
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            skipping = codex_header(line).is_some_and(|n| names.contains(&n));
        }
        if !skipping {
            out.push(line);
        }
    }
    out.join("\n")
}

// ------------------------------------------------------------------ migrations

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub path: String,
    pub found: bool,
    pub detail: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Migration {
    pub id: String,
    pub title: String,
    pub about: String,
    pub sources: Vec<Endpoint>,
    pub targets: Vec<Endpoint>,
    pub extensions: bool,
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct MigrateOptions {
    pub from: Option<String>,
    pub to: Vec<String>,
    pub overwrite: bool,
    pub extensions: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub from: String,
    pub to: String,
    /// copy | overwrite | convert | add | install | skip
    pub action: String,
    pub detail: String,
}

enum Op {
    Copy { from: PathBuf, to: PathBuf, replace_dir: bool },
    Write { to: PathBuf, content: String },
    Install { cli: PathBuf, ids: Vec<String>, label: String },
}

struct Plan {
    b: Bases,
    steps: Vec<PlanStep>,
    ops: Vec<Op>,
}

impl Plan {
    fn disp(&self, p: &Path) -> String {
        templatize(p, &self.b)
    }
    fn step(&mut self, from: String, to: String, action: &str, detail: &str) {
        self.steps.push(PlanStep { from, to, action: action.into(), detail: detail.into() });
    }
    fn copy(&mut self, from: &Path, to: &Path, overwrite: bool, replace_dir: bool) {
        let (f, t) = (self.disp(from), self.disp(to));
        if same_place(from, to) {
            return self.step(f, t, "skip", "Same folder (linked)");
        }
        let exists = to.exists();
        if exists && !overwrite {
            return self.step(f, t, "skip", "Already exists; turn on Overwrite to replace it");
        }
        self.step(f, t, if exists { "overwrite" } else { "copy" }, "");
        self.ops.push(Op::Copy { from: from.into(), to: to.into(), replace_dir });
    }
    fn write(&mut self, from: String, to: &Path, content: String, overwrite: bool, action: &str, detail: &str) {
        let t = self.disp(to);
        let exists = to.exists();
        if exists && !overwrite {
            return self.step(from, t, "skip", "Already exists; turn on Overwrite to replace it");
        }
        self.step(from, t, if exists { "overwrite" } else { action }, detail);
        self.ops.push(Op::Write { to: to.into(), content });
    }
}

const EDITORS: [(&str, &str, &str); 4] = [
    ("vscode", "VS Code", "%APPDATA%/Code/User"),
    ("cursor", "Cursor", "%APPDATA%/Cursor/User"),
    ("windsurf", "Windsurf", "%APPDATA%/Windsurf/User"),
    ("vscodium", "VSCodium", "%APPDATA%/VSCodium/User"),
];
const INSTRUCTIONS: [(&str, &str, &str); 5] = [
    ("claude", "Claude Code", "~/.claude/CLAUDE.md"),
    ("codex", "Codex", "~/.codex/AGENTS.md"),
    ("gemini", "Gemini CLI", "~/.gemini/GEMINI.md"),
    ("copilot", "GitHub Copilot CLI", "~/.copilot/copilot-instructions.md"),
    ("opencode", "OpenCode", "~/.config/opencode/AGENTS.md"),
];
const SKILL_TARGETS: [(&str, &str, &str); 3] = [
    ("agents", "Shared (Codex, OpenCode)", "~/.agents/skills"),
    ("copilot", "GitHub Copilot CLI", "~/.copilot/skills"),
    ("cursor", "Cursor", "~/.cursor/skills"),
];
const COMMAND_TARGETS: [(&str, &str, &str); 3] = [
    ("gemini", "Gemini CLI", "~/.gemini/commands"),
    ("opencode", "OpenCode", "~/.config/opencode/commands"),
    ("claude-skills", "Claude Code skills", "~/.claude/skills"),
];
const AGENT_TARGETS: [(&str, &str, &str); 2] =
    [("opencode", "OpenCode", "~/.config/opencode/agents"), ("copilot", "GitHub Copilot CLI", "~/.copilot/agents")];

fn tpl_of<'a>(table: &'a [(&str, &str, &'a str)], id: &str) -> Option<&'a str> {
    table.iter().find(|(i, _, _)| *i == id).map(|(_, _, t)| *t)
}

fn subdirs(p: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(p) else { return vec![] };
    let mut v: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter(|e| fs::metadata(e.path()).is_ok_and(|m| m.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    v.sort();
    v
}

/// Markdown files under `root` as (relative path without .md, absolute path).
fn md_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut v = vec![];
    if root.is_dir() {
        walk_files(root, true, &mut |abs, rel, _| {
            if let Some(stem) = rel.strip_suffix(".md") {
                v.push((stem.to_string(), abs.to_path_buf()));
            }
            true
        });
    }
    v
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn endpoint(id: &str, name: &str, path: PathBuf, found: bool, detail: String) -> Endpoint {
    Endpoint { id: id.into(), name: name.into(), path: path.to_string_lossy().into(), found, detail }
}

/// A destination counts as found when the tool's own folder exists.
fn target_eps(table: &[(&str, &str, &str)], b: &Bases) -> Vec<Endpoint> {
    table
        .iter()
        .map(|(id, name, t)| {
            let p = expand(t, b);
            let found = p.parent().is_some_and(Path::exists);
            endpoint(id, name, p, found, String::new())
        })
        .collect()
}

fn migrations(b: &Bases) -> Vec<Migration> {
    let editors: Vec<Endpoint> = EDITORS
        .iter()
        .map(|(id, name, t)| {
            let p = expand(t, b);
            let n = ["settings.json", "keybindings.json"].iter().filter(|f| p.join(f).is_file()).count()
                + usize::from(p.join("snippets").is_dir());
            let found = p.is_dir() || editor_cli(id).and_then(cli).is_some();
            endpoint(id, name, p, found, if n > 0 { plural(n, "settings item") } else { String::new() })
        })
        .collect();

    let skills_dir = expand("~/.claude/skills", b);
    let skills = subdirs(&skills_dir).len();
    let cmd_dir = expand("~/.claude/commands", b);
    let commands = md_files(&cmd_dir).len();
    let agent_dir = expand("~/.claude/agents", b);
    let agents = md_files(&agent_dir).len();

    let instructions: Vec<Endpoint> = INSTRUCTIONS
        .iter()
        .map(|(id, name, t)| {
            let p = expand(t, b);
            let found = p.is_file();
            let detail = fs::metadata(&p).map(|m| format!("{} bytes", m.len())).unwrap_or_default();
            endpoint(id, name, p, found, detail)
        })
        .collect();
    let instruction_targets: Vec<Endpoint> = INSTRUCTIONS
        .iter()
        .map(|(id, name, t)| {
            let p = expand(t, b);
            let found = p.parent().is_some_and(Path::exists);
            endpoint(id, name, p, found, String::new())
        })
        .collect();

    let mcp_sources: Vec<Endpoint> = MCP_SOURCES
        .iter()
        .map(|id| {
            let (name, t, _) = mcp_location(id).unwrap_or(("", "", ""));
            let n = read_mcp(id, b).len();
            endpoint(id, name, expand(t, b), n > 0, if n > 0 { plural(n, "server") } else { String::new() })
        })
        .collect();
    let mcp_targets: Vec<Endpoint> = MCP_TARGETS
        .iter()
        .map(|id| {
            let (name, t, _) = mcp_location(id).unwrap_or(("", "", ""));
            let p = expand(t, b);
            let found = p.exists() || p.parent().is_some_and(|d| d != b.home && d.exists());
            endpoint(id, name, p, found, String::new())
        })
        .collect();

    let claude = |path: PathBuf, n: usize, what: &str| {
        vec![endpoint("claude", "Claude Code", path, n > 0, if n > 0 { plural(n, what) } else { String::new() })]
    };

    vec![
        Migration {
            id: "editor".into(),
            title: "Editor settings".into(),
            about: "settings.json, keybindings.json and snippets between VS Code-family editors, plus extensions through each editor's CLI.".into(),
            sources: editors.clone(),
            targets: editors,
            extensions: true,
        },
        Migration {
            id: "skills".into(),
            title: "Skills".into(),
            about: "Claude Code skill folders into the folders other agents read. ~/.agents/skills is shared by Codex and OpenCode.".into(),
            sources: claude(skills_dir, skills, "skill"),
            targets: target_eps(&SKILL_TARGETS, b),
            extensions: false,
        },
        Migration {
            id: "commands".into(),
            title: "Slash commands".into(),
            about: "Claude Code commands converted to Gemini TOML commands, copied to OpenCode, or turned into Claude skills.".into(),
            sources: claude(cmd_dir, commands, "command"),
            targets: target_eps(&COMMAND_TARGETS, b),
            extensions: false,
        },
        Migration {
            id: "agents".into(),
            title: "Subagents".into(),
            about: "Claude Code subagents as OpenCode subagents and Copilot custom agents. Tool lists are dropped; each tool names tools differently.".into(),
            sources: claude(agent_dir, agents, "agent"),
            targets: target_eps(&AGENT_TARGETS, b),
            extensions: false,
        },
        Migration {
            id: "instructions".into(),
            title: "Global instructions".into(),
            about: "Your global CLAUDE.md / AGENTS.md / GEMINI.md as plain markdown. @file imports are inlined.".into(),
            sources: instructions,
            targets: instruction_targets,
            extensions: false,
        },
        Migration {
            id: "mcp".into(),
            title: "MCP servers".into(),
            about: "Merged by name into the other tool's config; servers already there are kept unless Overwrite is on.".into(),
            sources: mcp_sources,
            targets: mcp_targets,
            extensions: false,
        },
    ]
}

pub fn list_migrations() -> Vec<Migration> {
    migrations(&Bases::current())
}

fn build_plan(id: &str, o: &MigrateOptions) -> Res<(String, Plan)> {
    let b = Bases::current();
    let all = migrations(&b);
    let m = all.iter().find(|m| m.id == id).ok_or("Unknown copy")?;
    let from = match &o.from {
        Some(f) => f.clone(),
        None => m.sources.iter().find(|s| s.found).map(|s| s.id.clone()).ok_or("Nothing to copy from")?,
    };
    let src = m.sources.iter().find(|s| s.id == from).ok_or("Unknown source")?;
    if !src.found {
        return Err(format!("{} has nothing to copy", src.name));
    }
    let to: Vec<String> =
        o.to.iter().filter(|t| **t != from && m.targets.iter().any(|x| x.id == **t)).cloned().collect();
    if to.is_empty() {
        return Err("Choose at least one destination".into());
    }
    let name_of = |id: &str| m.targets.iter().chain(&m.sources).find(|e| e.id == id).map(|e| e.name.clone()).unwrap_or_default();
    let mut p = Plan { b: b.clone(), steps: vec![], ops: vec![] };
    match id {
        "editor" => {
            let root = |id: &str| expand(tpl_of(&EDITORS, id).unwrap_or(""), &b);
            let src = root(&from);
            let exts = if o.extensions { editor_cli(&from).and_then(cli).and_then(|c| list_extensions(&c)) } else { None };
            for t in &to {
                let dst = root(t);
                for f in ["settings.json", "keybindings.json"] {
                    if src.join(f).is_file() {
                        p.copy(&src.join(f), &dst.join(f), o.overwrite, false);
                    }
                }
                if src.join("snippets").is_dir() {
                    p.copy(&src.join("snippets"), &dst.join("snippets"), o.overwrite, false);
                }
                if !o.extensions {
                    continue;
                }
                let cli_name = editor_cli(t).unwrap_or("code");
                let label = format!("{} extensions", name_of(&from));
                match (&exts, cli(cli_name)) {
                    (None, _) => p.step(label, name_of(t), "skip", "Could not list extensions: the source editor's CLI was not found"),
                    (_, None) => p.step(label, name_of(t), "skip", &format!("`{cli_name}` command not found")),
                    (Some(list), Some(exe)) => {
                        let ids: Vec<String> = list.iter().map(|e| strip_version(e)).collect();
                        let note = if t == "vscode" { "" } else { "Installs from Open VSX; some Microsoft-only extensions may be missing" };
                        p.step(plural(ids.len(), "extension"), format!("{} ({cli_name})", name_of(t)), "install", note);
                        p.ops.push(Op::Install { cli: exe, ids, label: name_of(t) });
                    }
                }
            }
        }
        "skills" => {
            for (name, dir) in subdirs(&expand("~/.claude/skills", &b)) {
                for t in &to {
                    let root = expand(tpl_of(&SKILL_TARGETS, t).unwrap_or(""), &b);
                    p.copy(&dir, &root.join(&name), o.overwrite, true);
                }
            }
        }
        "commands" => {
            for (rel, file) in md_files(&expand("~/.claude/commands", &b)) {
                let Ok(text) = fs::read_to_string(&file) else { continue };
                let from_disp = p.disp(&file);
                for t in &to {
                    let root = expand(tpl_of(&COMMAND_TARGETS, t).unwrap_or(""), &b);
                    match t.as_str() {
                        "gemini" => {
                            let Some(dest) = safe_join(&root, &format!("{rel}.toml")) else { continue };
                            p.write(from_disp.clone(), &dest, command_to_gemini(&text), o.overwrite, "convert", "");
                        }
                        "opencode" => {
                            let Some(dest) = safe_join(&root, &format!("{rel}.md")) else { continue };
                            p.copy(&file, &dest, o.overwrite, false);
                        }
                        _ => {
                            let n = rel.replace('/', "-");
                            let dir = root.join(&n);
                            if dir.exists() && !o.overwrite {
                                let d = p.disp(&dir);
                                p.step(from_disp.clone(), d, "skip", "A skill with this name exists");
                            } else {
                                p.write(from_disp.clone(), &dir.join("SKILL.md"), command_to_skill(&n, &text), true, "convert", "");
                            }
                        }
                    }
                }
            }
        }
        "agents" => {
            for (rel, file) in md_files(&expand("~/.claude/agents", &b)) {
                let Ok(text) = fs::read_to_string(&file) else { continue };
                let n = rel.replace('/', "-");
                let from_disp = p.disp(&file);
                for t in &to {
                    let root = expand(tpl_of(&AGENT_TARGETS, t).unwrap_or(""), &b);
                    let (dest, content) = if t == "opencode" {
                        (root.join(format!("{n}.md")), agent_to_opencode(&n, &text))
                    } else {
                        (root.join(format!("{n}.agent.md")), agent_to_copilot(&n, &text))
                    };
                    p.write(from_disp.clone(), &dest, content, o.overwrite, "convert", "");
                }
            }
        }
        "instructions" => {
            let src = expand(tpl_of(&INSTRUCTIONS, &from).unwrap_or(""), &b);
            let text = fs::read_to_string(&src).map_err(es)?;
            let (content, n) = inline_imports(&text, src.parent().unwrap_or(&b.home), &b.home);
            let detail = if n > 0 { format!("{} inlined", plural(n, "@import")) } else { String::new() };
            let from_disp = p.disp(&src);
            for t in &to {
                let dest = expand(tpl_of(&INSTRUCTIONS, t).unwrap_or(""), &b);
                p.write(from_disp.clone(), &dest, content.clone(), o.overwrite, "copy", &detail);
            }
        }
        "mcp" => {
            let servers = read_mcp(&from, &b);
            for t in &to {
                if t == "codex" {
                    plan_mcp_codex(&mut p, &servers, o.overwrite);
                } else {
                    plan_mcp_json(&mut p, t, &servers, o.overwrite);
                }
            }
        }
        _ => return Err("Unknown copy".into()),
    }
    Ok((m.title.clone(), p))
}

fn plan_mcp_json(p: &mut Plan, t: &str, servers: &BTreeMap<String, Value>, overwrite: bool) {
    let Some((name, tpl, key)) = mcp_location(t) else { return };
    let path = expand(tpl, &p.b);
    let disp = p.disp(&path);
    let (mut root, had_comments) = match read_json_file(&path) {
        Ok(x) => x,
        Err(e) => return p.step(name.into(), disp, "skip", &format!("Could not read it: {e}")),
    };
    let Some(obj) = root.as_object_mut() else {
        return p.step(name.into(), disp, "skip", "Not a JSON object");
    };
    let container = obj.entry(key.to_string()).or_insert_with(|| json!({}));
    let Some(cont) = container.as_object_mut() else {
        return p.step(name.into(), disp, "skip", &format!("`{key}` is not an object"));
    };
    let note = if had_comments { "Comments in this file are not kept (a backup is made first)" } else { "" };
    let mut changed = false;
    let mut steps = vec![];
    for (n, c) in servers {
        let Some(shape) = mcp_for(t, c) else {
            steps.push((n.clone(), "skip", "Remote servers are added as connectors in Claude Desktop"));
            continue;
        };
        let exists = cont.contains_key(n);
        if exists && !overwrite {
            steps.push((n.clone(), "skip", "Already configured there"));
            continue;
        }
        cont.insert(n.clone(), shape);
        changed = true;
        steps.push((n.clone(), if exists { "overwrite" } else { "add" }, note));
    }
    for (n, action, detail) in steps {
        p.step(n, disp.clone(), action, detail);
    }
    if changed {
        if let Ok(text) = serde_json::to_string_pretty(&root) {
            p.ops.push(Op::Write { to: path, content: text + "\n" });
        }
    }
}

fn plan_mcp_codex(p: &mut Plan, servers: &BTreeMap<String, Value>, overwrite: bool) {
    let path = expand("~/.codex/config.toml", &p.b);
    let disp = p.disp(&path);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let existing: HashSet<String> = text.lines().filter_map(codex_header).collect();
    let mut remove = HashSet::new();
    let mut blocks = vec![];
    for (n, c) in servers {
        let exists = existing.contains(n);
        if exists && !overwrite {
            p.step(n.clone(), disp.clone(), "skip", "Already configured there");
            continue;
        }
        let Some(block) = mcp_to_codex(n, c) else {
            p.step(n.clone(), disp.clone(), "skip", "Unsupported server type");
            continue;
        };
        if exists {
            remove.insert(n.clone());
        }
        blocks.push(block);
        p.step(n.clone(), disp.clone(), if exists { "overwrite" } else { "add" }, "");
    }
    if blocks.is_empty() {
        return;
    }
    let mut out = remove_codex_servers(&text, &remove).trim_end().to_string();
    for block in blocks {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(block.trim_end());
    }
    out.push('\n');
    p.ops.push(Op::Write { to: path, content: out });
}

pub fn plan_migration(id: &str, o: &MigrateOptions) -> Res<Vec<PlanStep>> {
    build_plan(id, o).map(|(_, p)| p.steps)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub backup_id: Option<String>,
    pub done: u32,
    pub skipped: u32,
    pub failed: Vec<String>,
    pub extensions_installed: u32,
    pub extensions_failed: Vec<String>,
}

pub fn run_migration(app: &AppHandle, id: &str, o: &MigrateOptions) -> Res<MigrationResult> {
    let (title, plan) = build_plan(id, o)?;
    let skipped = plan.steps.iter().filter(|s| s.action == "skip").count() as u32;
    if plan.ops.is_empty() {
        return Ok(MigrationResult {
            backup_id: None,
            done: 0,
            skipped,
            failed: vec![],
            extensions_installed: 0,
            extensions_failed: vec![],
        });
    }
    // Snapshot every destination before anything is written.
    let mut backup_id = None;
    if plan.ops.iter().any(|op| !matches!(op, Op::Install { .. })) {
        let mut snap = Snapshot::new(&format!("Copy {}", title.to_lowercase()))?;
        for op in &plan.ops {
            match op {
                Op::Copy { to, .. } | Op::Write { to, .. } => snap.save(to)?,
                Op::Install { .. } => {}
            }
        }
        snap.finish()?;
        backup_id = Some(snap.m.id.clone());
    }
    let total = plan.ops.len();
    let mut done = 0;
    let mut failed = vec![];
    let mut ext_ok = 0;
    let mut ext_failed = vec![];
    for (i, op) in plan.ops.iter().enumerate() {
        let current = match op {
            Op::Copy { to, .. } | Op::Write { to, .. } => templatize(to, &plan.b),
            Op::Install { label, .. } => format!("Installing extensions in {label}"),
        };
        let _ = app.emit("config-migrate-progress", Progress { done: i, total, current: current.clone() });
        let r = match op {
            Op::Copy { from, to, replace_dir } => {
                if from.is_dir() {
                    let cleared = if *replace_dir && fs::symlink_metadata(to).is_ok() { remove_path(to).map_err(es) } else { Ok(()) };
                    cleared.and_then(|_| copy_tree(from, to, true).map(|_| ()))
                } else {
                    fs::read(from).map_err(es).and_then(|bytes| write_file(to, &bytes))
                }
            }
            Op::Write { to, content } => write_file(to, content.as_bytes()),
            Op::Install { cli, ids, label } => {
                let (ok, bad) = install_extensions(cli, ids, |_, ext| {
                    let _ = app.emit(
                        "config-migrate-progress",
                        Progress { done: i, total, current: format!("{label}: installing {ext}") },
                    );
                });
                ext_ok += ok;
                ext_failed.extend(bad);
                Ok(())
            }
        };
        match r {
            Ok(()) => done += 1,
            Err(e) => failed.push(format!("{current}: {e}")),
        }
    }
    let _ = app.emit("config-migrate-progress", Progress { done: total, total, current: String::new() });
    Ok(MigrationResult { backup_id, done, skipped, failed, extensions_installed: ext_ok, extensions_failed: ext_failed })
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn bases(root: &Path) -> Bases {
        Bases { home: root.to_path_buf(), roaming: root.join("AppData/Roaming"), local: root.join("AppData/Local") }
    }

    #[test]
    fn templates_round_trip() {
        let root = std::env::temp_dir().join("dc-tpl-home");
        let b = bases(&root);
        for t in ["~/.gitconfig", "%APPDATA%/Code/User/settings.json", "%LOCALAPPDATA%/nvim", "~/.claude/skills/x/SKILL.md"] {
            let real = expand(t, &b);
            assert_eq!(templatize(&real, &b), t, "{t}");
        }
        // Roaming lives inside home: the deeper root wins.
        assert_eq!(templatize(&root.join("AppData").join("Roaming").join("Zed"), &b), "%APPDATA%/Zed");
        // macOS: Roaming and Local are the same folder.
        let mac = Bases { home: root.clone(), roaming: root.join("Lib/AS"), local: root.join("Lib/AS") };
        assert_eq!(templatize(&root.join("Lib/AS/Code"), &mac), "%APPDATA%/Code");
        assert_eq!(windows_form("~/.ssh/config"), "%USERPROFILE%\\.ssh\\config");
        assert_eq!(mac_form("%APPDATA%/Code/User"), "~/Library/Application Support/Code/User");
    }

    #[test]
    fn refuses_zip_slip() {
        let base = Path::new("/safe/base");
        assert_eq!(safe_join(base, "a/b.txt"), Some(base.join("a").join("b.txt")));
        assert_eq!(safe_join(base, "../evil"), None);
        assert_eq!(safe_join(base, "a/../../evil"), None);
        assert_eq!(safe_join(base, "/etc/passwd"), None);
        assert_eq!(safe_join(base, "\\Windows\\x"), None);
        assert_eq!(safe_join(base, "C:/Windows/x"), None);
        assert_eq!(safe_join(base, "a\\..\\..\\x"), None);
        assert_eq!(safe_join(base, ""), None);
        assert_eq!(safe_join(base, "./"), None);
    }

    #[test]
    fn unknown_or_hostile_ids_have_no_target() {
        assert!(def_for_id("vscode:settings").is_some());
        assert!(def_for_id("jetbrains:../../x").is_none());
        assert!(def_for_id("ssh:key:../authorized_keys").is_none());
        assert!(def_for_id("ssh:key:known_hosts").is_none());
        assert!(def_for_id("claude:projects").is_none());
        let key = def_for_id("ssh:key:id_ed25519").unwrap();
        assert!(key.sensitive && !key.default_on);
        assert!(!def_for_id("ssh:key:id_ed25519.pub").unwrap().sensitive);
    }

    #[test]
    fn converts_commands_to_gemini_toml() {
        let md = "---\ndescription: Review the \"diff\"\nallowed-tools: Bash(git diff:*)\n---\n\nReview this: $ARGUMENTS\nDiff: !`git diff HEAD`\nPath C:\\x and \"\"\"quotes\"\"\"\n";
        let toml = command_to_gemini(md);
        assert_eq!(
            toml,
            "description = \"Review the \\\"diff\\\"\"\nprompt = \"\"\"\nReview this: {{args}}\nDiff: !{git diff HEAD}\nPath C:\\\\x and \"\"\\\"quotes\"\"\\\"\n\"\"\"\n"
        );
        // No front matter, CRLF line endings.
        assert_eq!(command_to_gemini("Do it\r\n"), "prompt = \"\"\"\nDo it\n\"\"\"\n");
    }

    #[test]
    fn converts_commands_and_agents_to_markdown() {
        let skill = command_to_skill("git-commit", "---\ndescription: Commit\n---\nBody\n");
        assert_eq!(skill, "---\nname: git-commit\ndescription: \"Commit\"\n---\n\nBody\n");
        let agent = "---\nname: reviewer\ndescription: Reviews code\ntools: Read, Grep\nmodel: sonnet\n---\n\nYou review.\n";
        assert_eq!(agent_to_opencode("reviewer", agent), "---\ndescription: \"Reviews code\"\nmode: subagent\n---\n\nYou review.\n");
        assert!(agent_to_copilot("reviewer", agent).starts_with("---\nname: reviewer\ndescription: \"Reviews code\"\n---"));
    }

    #[test]
    fn converts_mcp_json_to_codex_toml() {
        let stdio = json!({ "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"], "env": { "GITHUB_TOKEN": "ghp_\"x" } });
        assert_eq!(
            mcp_to_codex("github", &stdio).unwrap(),
            "[mcp_servers.github]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-github\"]\nenv = { GITHUB_TOKEN = \"ghp_\\\"x\" }\n"
        );
        let remote = normalize_mcp("claude", &json!({ "type": "http", "url": "https://x/mcp", "headers": { "Authorization": "Bearer t" } })).unwrap();
        assert_eq!(
            mcp_to_codex("my docs", &remote).unwrap(),
            "[mcp_servers.\"my docs\"]\nurl = \"https://x/mcp\"\nhttp_headers = { Authorization = \"Bearer t\" }\n"
        );
        // Gemini's httpUrl is remote http; written back to Gemini the same way.
        let g = normalize_mcp("gemini", &json!({ "httpUrl": "https://g/mcp" })).unwrap();
        assert_eq!(mcp_for("gemini", &g).unwrap(), json!({ "httpUrl": "https://g/mcp" }));
        assert_eq!(mcp_for("vscode", &g).unwrap(), json!({ "type": "http", "url": "https://g/mcp" }));
        assert!(mcp_for("claude-desktop", &g).is_none());

        // Merging replaces only the named server's tables.
        let existing = "model = \"o3\"\n\n[mcp_servers.github]\ncommand = \"old\"\n\n[mcp_servers.github.env]\nA = \"1\"\n\n[mcp_servers.keep]\ncommand = \"k\"\n";
        assert_eq!(codex_header("[mcp_servers.\"my docs\".env]").as_deref(), Some("my docs"));
        let names: HashSet<String> = ["github".to_string()].into();
        let out = remove_codex_servers(existing, &names);
        assert!(!out.contains("old") && !out.contains("A = \"1\""));
        assert!(out.contains("[mcp_servers.keep]") && out.contains("model = \"o3\""));
    }

    #[test]
    fn parses_jsonc() {
        let src = "\u{feff}{\n  // editor\n  \"a\": \"http://x // not a comment\", /* block\n */ \"b\": [1, 2,],\n  \"c\": { \"d\": \"\\\"/*\", },\n}\n";
        let (v, had) = parse_jsonc(src).unwrap();
        assert!(had);
        assert_eq!(v, json!({ "a": "http://x // not a comment", "b": [1, 2], "c": { "d": "\"/*" } }));
        assert_eq!(parse_jsonc("  ").unwrap().0, json!({}));
        assert!(!parse_jsonc("{\"x\": 1}").unwrap().1);
    }

    #[test]
    fn inlines_imports() {
        let dir = std::env::temp_dir().join(format!("dc-inline-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("RTK.md"), "rtk rules\n").unwrap();
        let (out, n) = inline_imports("# Me\n@RTK.md\n@missing.md\n@someone\n", &dir, &dir);
        assert_eq!(n, 1);
        assert!(out.contains("rtk rules") && out.contains("removed import @missing.md") && out.contains("@someone\n"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn formats_dates() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_709_210_096), "2024-02-29T12:34:56Z");
        assert_eq!(stamp(1_709_210_096), "20240229-123456");
    }

    #[test]
    fn agent_doc_lists_every_item_and_warns_on_secrets() {
        let m = Manifest {
            app: "dev-cleaner".into(),
            format_version: 1,
            created_at: iso(0),
            source_os: "windows".into(),
            hostname: "box".into(),
            items: vec![
                ManifestItem {
                    id: "vscode:settings".into(),
                    tool: "vscode".into(),
                    label: "settings.json".into(),
                    kind: Kind::File,
                    original_path: "%APPDATA%/Code/User/settings.json".into(),
                    archive_path: "files/vscode/settings.json".into(),
                    sensitive: false,
                    tool_name: "VS Code".into(),
                    windows_path: Some("%APPDATA%/Code/User/settings.json".into()),
                    mac_path: Some("%APPDATA%/Code/User/settings.json".into()),
                    cli: None,
                },
                ManifestItem {
                    id: "codex:auth".into(),
                    tool: "codex".into(),
                    label: "auth.json".into(),
                    kind: Kind::File,
                    original_path: "~/.codex/auth.json".into(),
                    archive_path: "files/codex/auth.json".into(),
                    sensitive: true,
                    tool_name: "Codex".into(),
                    windows_path: Some("~/.codex/auth.json".into()),
                    mac_path: Some("~/.codex/auth.json".into()),
                    cli: None,
                },
            ],
        };
        let doc = agent_doc(&m);
        assert!(doc.contains("contains secrets"));
        assert!(doc.contains("`%APPDATA%\\Code\\User\\settings.json`"));
        assert!(doc.contains("`~/Library/Application Support/Code/User/settings.json`"));
        assert!(doc.contains("`%USERPROFILE%\\.codex\\auth.json`"));
        assert!(doc.contains("Back up the existing file"));
    }
}
