//! About and update checks: which version this is, how it was installed,
//! and whether GitHub has a newer release. Only a plain GET to GitHub's API,
//! with no identifiers; any failure just says the check didn't work.

use crate::util;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const REPO: &str = "TriptoAfsin/dev-cleaner";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    /// "scoop", "winget-or-installer", "portable", "homebrew", "flatpak",
    /// "appimage", "deb-or-rpm", "app-bundle" or "dev".
    pub installed_via: String,
    /// The command that updates this install, when there is one.
    pub upgrade_command: Option<String>,
    pub repo: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub url: String,
    pub published_at: String,
    /// Release notes, trimmed.
    pub notes: String,
}

pub fn app_info(version: &str) -> AppInfo {
    let (via, cmd) = installed_via();
    AppInfo {
        version: version.into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        installed_via: via.into(),
        upgrade_command: cmd.map(String::from),
        repo: format!("https://github.com/{REPO}"),
    }
}

fn installed_via() -> (&'static str, Option<&'static str>) {
    if cfg!(debug_assertions) {
        return ("dev", None);
    }
    if std::env::var_os("FLATPAK_ID").is_some() {
        return ("flatpak", Some("flatpak update dev.tripto.dev-cleaner"));
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return ("appimage", None);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let p = exe.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    if cfg!(windows) {
        if p.contains("/scoop/apps/") {
            return ("scoop", Some("scoop update dev-cleaner"));
        }
        if util::portable_dir().is_some() {
            return ("portable", None);
        }
        return ("winget-or-installer", Some("winget upgrade TriptoAfsin.DevCleaner"));
    }
    if cfg!(target_os = "macos") {
        let brew = ["/opt/homebrew/Caskroom/dev-cleaner", "/usr/local/Caskroom/dev-cleaner"];
        if brew.iter().any(|b| Path::new(b).is_dir()) {
            return ("homebrew", Some("brew upgrade --cask dev-cleaner"));
        }
        return ("app-bundle", None);
    }
    ("deb-or-rpm", None)
}

/// "v0.2.0" or "0.2.0-beta.1" to (0, 2, 0) plus whether it is a pre-release.
fn parse(v: &str) -> Option<((u64, u64, u64), bool)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((c, rest)) => (c, !rest.is_empty() && v.contains('-')),
        None => (v, false),
    };
    let mut it = core.split('.').map(|x| x.parse::<u64>().ok());
    let t = (it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0));
    Some((t, pre))
}

/// Whether `latest` is newer than `current` (a release beats its own
/// pre-releases).
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some((l, lp)), Some((c, cp))) => l > c || (l == c && cp && !lp),
        _ => false,
    }
}

fn field<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

pub fn check(current: &str) -> Result<UpdateInfo, String> {
    // Tests point this at a local server; under the e2e fake home with no
    // override, never reach the network.
    let url = match std::env::var("DEV_CLEANER_UPDATE_URL") {
        Ok(u) if !u.is_empty() => u,
        _ if util::fake_home().is_some() => return Err("Update checks are off in tests".into()),
        _ => format!("https://api.github.com/repos/{REPO}/releases/latest"),
    };
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build();
    let body = agent
        .get(&url)
        .set("User-Agent", "dev-cleaner")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(403 | 429, _) => "GitHub is rate-limiting update checks. Try again in a while.".to_string(),
            ureq::Error::Status(404, _) => "No release found on GitHub.".to_string(),
            ureq::Error::Status(c, _) => format!("GitHub answered with HTTP {c}."),
            ureq::Error::Transport(_) => "Couldn't reach GitHub. Check your connection.".to_string(),
        })?
        .into_string()
        .map_err(|_| "GitHub's answer couldn't be read.".to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| "GitHub's answer wasn't valid JSON.".to_string())?;
    let tag = field(&v, "tag_name");
    if tag.is_empty() {
        return Err("GitHub's answer had no version.".into());
    }
    // The checksums belong on the release page; the rest is shown as Markdown.
    let body = field(&v, "body");
    let notes: String = body.split("## SHA-256 checksums").next().unwrap_or("").trim_end().chars().take(8000).collect();
    Ok(UpdateInfo {
        current: current.into(),
        latest: tag.trim_start_matches('v').into(),
        newer: is_newer(tag, current),
        url: field(&v, "html_url").into(),
        published_at: field(&v, "published_at").into(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0"));
        assert!(!is_newer("nonsense", "0.1.0"));
    }
}
