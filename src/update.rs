//! The app updates itself.
//!
//! Every build made by `scripts/bundle.sh` carries a build number (the time
//! it was built, as YYYYMMDDHHMMSS): compiled into the program, and written in
//! the app's Info.plist as its version. An installed copy (say in
//! /Applications) looks at the *update source*, the app that the build script
//! last made, and when that has a higher number it replaces itself with a
//! copy of it and starts again. With "Update automatically" on this happens
//! as the app opens; while it is open, the sidebar says an update is ready.
//!
//! The new copy is staged beside the app first and checked (whole, and
//! signed by the same developer through a certificate from Apple), then the
//! two change places in one step (`swap`), so there is never a half-written
//! app nor a moment with no app. The old copy is kept hidden beside it until
//! the new one has opened its window (`started`). Until then a small watcher
//! (the old program, `--update-watch`) looks on: when the new copy ends
//! before its window opens, the watcher puts the old copy back, opens it,
//! and that build is not offered again.
//!
//! `notion2 --update-check` and `--update-install` do the same from Terminal,
//! without opening a window or restarting.
//!
//! On every Mac but the one that builds the app there is no build folder.
//! There the app asks the published releases for the newest version (one
//! small request a day, or "Check now"; nothing of the person's is sent),
//! brings a newer one to this Mac, and installs it the same way. A download
//! must also match the checksum the releases give, and pass Apple's check
//! (notarization), as a download that the person opened by hand would.

use std::path::{Path, PathBuf};

/// This build's number; none for a plain `cargo build` (which never updates).
pub const BUILD: Option<&str> = option_env!("N2_BUILD_STAMP");
/// The commit this build was made from, when the build script knew it.
pub const COMMIT: Option<&str> = option_env!("N2_BUILD_COMMIT");
/// Where the build script puts the app (on the Mac that built this copy).
pub const DEFAULT_SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/release/Rune.app");

/// The newest published release (GitHub's answer is JSON).
pub const RELEASES: &str = "https://api.github.com/repos/capi-git/Rune/releases/latest";
/// Where a release's download must come from.
const RELEASE_FILES: &str = "https://github.com/capi-git/Rune/releases/download/";
/// The page to send a person to who must get a release by hand.
pub const RELEASES_PAGE: &str = "https://github.com/capi-git/Rune/releases/latest";
const ASSET: &str = "Rune.zip";
/// How often the releases are asked without "Check now".
const ASK_EVERY_MS: i64 = 20 * 60 * 60 * 1000;

const BUNDLE_ID: &str = "com.jackhagan.notion2";
const PROGRAM: &str = "Contents/MacOS/notion2";

/// A newer build that can be installed.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    /// The app to copy.
    pub source: PathBuf,
    /// Its build number.
    pub build: u64,
}

fn number(s: &str) -> Option<u64> {
    s.trim().parse().ok().filter(|n| *n > 0)
}

/// This build's number.
pub fn own_build() -> Option<u64> {
    BUILD.and_then(number)
}

/// "9 Nov 2026, 13:31" for a build number.
pub fn build_label(build: u64) -> String {
    let s = build.to_string();
    match chrono::NaiveDateTime::parse_from_str(&s, "%Y%m%d%H%M%S") {
        Ok(t) => t.format("%-d %b %Y, %H:%M").to_string(),
        Err(_) => s,
    }
}

/// The value of a string key in an Info.plist.
fn plist_value(plist: &str, key: &str) -> Option<String> {
    let after = plist.split(&format!("<key>{key}</key>")).nth(1)?;
    let value = after.split("<string>").nth(1)?.split("</string>").next()?;
    Some(value.trim().to_string())
}

/// The build number of an app on disk, if it is a build of this app.
pub fn build_of(app: &Path) -> Option<u64> {
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    if plist_value(&plist, "CFBundleIdentifier").as_deref() != Some(BUNDLE_ID) || !app.join(PROGRAM).is_file() {
        return None;
    }
    number(&plist_value(&plist, "CFBundleVersion")?)
}

/// The app this program runs from; none when it was started straight from a
/// build folder (not inside an .app).
pub fn own_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension().is_some_and(|e| e == "app") && app.join(PROGRAM) == exe).then(|| app.to_path_buf())
}

/// Where newer builds come from: the setting, else where the build script
/// puts them.
pub fn source() -> PathBuf {
    crate::store::setting("update_source").filter(|s| !s.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_SOURCE))
}

pub fn set_source(path: Option<&Path>) {
    crate::store::set_setting("update_source", &path.map(|p| p.display().to_string()).unwrap_or_default());
}

/// Whether a newer build is installed as the app opens (Settings).
pub fn automatic() -> bool {
    crate::store::setting("update_auto").as_deref() != Some("off")
}

pub fn set_automatic(on: bool) {
    crate::store::set_setting("update_auto", if on { "on" } else { "off" });
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The update `source` holds for the app at `own` whose build is `build`.
pub fn available_at(own: &Path, build: u64, source: &Path) -> Option<Update> {
    if same_place(own, source) {
        return None;
    }
    let newer = build_of(source)?;
    (newer > build).then(|| Update { source: source.to_path_buf(), build: newer })
}

/// The update `source` holds, unless that build was tried and did not start.
fn offered_at(own: &Path, build: u64, source: &Path) -> Option<Update> {
    available_at(own, build, source).filter(|u| !failed(u.build))
}

/// The update waiting for this copy of the app, if any: in the build
/// folder, or a release that was brought to this Mac before.
pub fn available() -> Option<Update> {
    let (own, build) = (own_app()?, own_build()?);
    offered_at(&own, build, &source()).or_else(|| downloaded().and_then(|app| offered_at(&own, build, &app)))
}

// ----- the published releases (for every Mac but the one that builds) ------

/// Whether the app may ask the published releases for a newer version (Settings).
pub fn online() -> bool {
    crate::store::setting("update_online").as_deref() != Some("off")
}

pub fn set_online(on: bool) {
    crate::store::set_setting("update_online", if on { "on" } else { "off" });
}

/// Whether this copy gets its updates from the published releases: it has
/// no build folder to look in (and none was chosen in Settings).
pub fn from_releases() -> bool {
    !source().exists()
}

/// "v0.1.3" or "0.1.3" as numbers.
fn version_numbers(s: &str) -> Option<Vec<u64>> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let parts: Option<Vec<u64>> = s.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

/// Whether version `theirs` is later than `ours`.
pub fn newer_version(theirs: &str, ours: &str) -> bool {
    match (version_numbers(theirs), version_numbers(ours)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

/// The newest release, as the releases describe it.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Its name, as "v0.1.4".
    pub version: String,
    /// Where its download is.
    pub url: String,
    /// The SHA-256 of the download (64 hex digits), when the releases give it.
    pub sha256: Option<String>,
}

/// The newest release in what the releases answered.
pub fn release_in(answer: &str) -> Option<Release> {
    let json: serde_json::Value = serde_json::from_str(answer).ok()?;
    if json.get("draft").and_then(|d| d.as_bool()) == Some(true) || json.get("prerelease").and_then(|d| d.as_bool()) == Some(true) {
        return None;
    }
    let version = json.get("tag_name")?.as_str()?.to_string();
    let asset = json.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET))?;
    let url = asset.get("browser_download_url")?.as_str()?.to_string();
    // GitHub gives "sha256:<hex>"; anything else is left unchecked.
    let sha256 = asset
        .get("digest")
        .and_then(|d| d.as_str())
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()));
    version_numbers(&version)?;
    Some(Release { version, url, sha256 })
}

/// Where releases that were brought to this Mac are kept until they are
/// installed, with the updater's notes (`NOTION2_UPDATE_DIR` names another
/// folder, for a test).
fn downloads() -> Option<PathBuf> {
    // A unit test keeps its notes in a folder of its own, never the person's.
    #[cfg(test)]
    return Some(tests::DIR.with(|d| d.borrow().clone()));
    #[allow(unreachable_code)]
    if let Some(dir) = std::env::var_os("NOTION2_UPDATE_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches/Rune Updates"))
}

/// A note of the updater, in the downloads folder.
fn note_path(name: &str) -> Option<PathBuf> {
    Some(downloads()?.join(name))
}

fn write_note(name: &str, text: &str) {
    if let Some(path) = note_path(name) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")));
        let _ = std::fs::write(path, text);
    }
}

/// One line in the updater's log (only the updater's own steps; read when
/// an update went wrong).
fn log(line: &str) {
    use std::io::Write;
    let Some(path) = note_path("update.log") else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")));
    // Kept small: a long log starts again.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 64 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// Whether build `build` was installed before and did not start.
pub fn failed(build: u64) -> bool {
    note_path("failed").and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|text| text.lines().any(|l| number(l) == Some(build)))
}

fn mark_failed(build: u64) {
    let mut text = note_path("failed").and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    text.push_str(&format!("{build}\n"));
    write_note("failed", &text);
}

/// The newest release that is on this Mac already.
fn downloaded() -> Option<PathBuf> {
    let mut found: Vec<(u64, PathBuf)> = std::fs::read_dir(downloads()?)
        .ok()?
        .flatten()
        .map(|e| e.path().join("Rune.app"))
        .filter_map(|app| build_of(&app).map(|b| (b, app)))
        .filter(|(b, _)| !failed(*b))
        .collect();
    found.sort();
    found.pop().map(|(_, app)| app)
}

/// Remove the releases on this Mac that are not newer than this copy (the
/// updater's notes stay).
fn tidy_downloads() {
    let (Some(dir), Some(build)) = (downloads(), own_build()) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !folder.is_dir() {
            continue;
        }
        if !build_of(&folder.join("Rune.app")).is_some_and(|b| b > build && !failed(b)) {
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
}

/// Whether `app` is a release that was brought to this Mac from the internet.
fn from_internet(app: &Path) -> bool {
    downloads().is_some_and(|d| app.starts_with(d))
}

/// The SHA-256 of a file, as 64 hex digits.
fn sha256_of(file: &Path) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/shasum").arg("-a").arg("256").arg(file).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let hex = text.split_whitespace().next()?.to_ascii_lowercase();
    (out.status.success() && hex.len() == 64).then_some(hex)
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("/usr/bin/curl")
        .args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https,file", "--user-agent", "Rune-updater"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("couldn't ask for the newest version ({e})"))?;
    if !out.status.success() {
        return Err("couldn't reach the releases (is this Mac on the internet?)".into());
    }
    Ok(out.stdout)
}

/// Ask `feed` for the newest release; when it is later than this version,
/// bring it to this Mac for the app at `own`. Gives the app to install
/// from, or none when this copy is the newest. A download that does not
/// match its checksum, is not a build of this app, is not signed by the
/// developer of `own`, or does not pass Apple's check is removed again.
/// Blocks: call it off the UI thread.
pub fn fetch_release(feed: &str, own: &Path) -> Result<Option<PathBuf>, String> {
    // A feed that is not the real one (a test) may name a file on this Mac.
    let real = feed == RELEASES;
    let answer = curl(&["--max-time", "20", "-H", "Accept: application/vnd.github+json", feed])?;
    let Some(release) = release_in(&String::from_utf8_lossy(&answer)) else { return Ok(None) };
    if !newer_version(&release.version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    if real && !release.url.starts_with(RELEASE_FILES) {
        return Err("the newest release has its download in a place that is not expected".into());
    }
    let name: String = release.version.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
    let folder = downloads().ok_or("this Mac has no home folder")?.join(name);
    let app = folder.join("Rune.app");
    if let Some(build) = build_of(&app) {
        // Brought here before (and checked then; it is checked again as it is installed).
        return Ok((!failed(build)).then_some(app));
    }
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| format!("couldn't make room for the download ({e})"))?;
    let zip = folder.join(ASSET);
    log(&format!("download {} from {}", release.version, release.url));
    let checked = curl(&["--max-time", "600", "--output", &zip.display().to_string(), &release.url]).and_then(|_| match &release.sha256 {
        Some(want) if sha256_of(&zip).as_deref() != Some(want.as_str()) => Err("the download is damaged (its checksum does not match)".to_string()),
        _ => Ok(()),
    });
    let opened = checked.and_then(|_| {
        let done = std::process::Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(&zip).arg(&folder).status();
        if done.is_ok_and(|s| s.success()) { Ok(()) } else { Err("couldn't open the download".to_string()) }
    });
    let _ = std::fs::remove_file(&zip);
    let result = opened.and_then(|_| match build_of(&app) {
        None => Err("the download is not a build of this app".to_string()),
        Some(build) if failed(build) => Ok(None),
        Some(_) => trusted(own, &app, true).map(|_| Some(app.clone())),
    });
    match &result {
        Ok(Some(_)) => log(&format!("downloaded {}", release.version)),
        Ok(None) => log(&format!("{} did not start before; not offered", release.version)),
        Err(why) => log(&format!("download of {} refused: {why}", release.version)),
    }
    if !matches!(result, Ok(Some(_))) {
        let _ = std::fs::remove_dir_all(&folder);
    }
    result
}

/// Whether it is time to ask the releases again by itself.
fn time_to_ask(now: i64) -> bool {
    let last = crate::store::setting("update_asked_at").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    now - last >= ASK_EVERY_MS
}

/// The developer team an app is signed by, if it is signed by one.
fn team_of(app: &Path) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/codesign").arg("-dv").arg(app).output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    let team = text.lines().find_map(|l| l.strip_prefix("TeamIdentifier="))?.trim().to_string();
    (!team.is_empty() && team != "not set").then_some(team)
}

/// The code requirement a build must meet to take the place of an app
/// signed by `team`: signed through a certificate that Apple issued, to
/// that developer team. (A certificate that someone made for themselves
/// can carry any team name; this one can't.)
pub fn requirement(team: &str) -> Option<String> {
    // Team ids are ten letters and digits; anything else is not trusted.
    (!team.is_empty() && team.len() <= 16 && team.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| format!("anchor apple generic and certificate leaf[subject.OU] = \"{team}\""))
}

/// Whether Apple's check (notarization) lets `app` open on this Mac, as it
/// would a download that the person opened by hand.
fn passes_apple_check(app: &Path) -> bool {
    std::process::Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute"])
        .arg(app)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether a new build may take the place of the app at `own`: when the app
/// is signed by a developer team, the build must be whole (its signature
/// holds for every file in it) and signed by the same team, through a
/// certificate from Apple. So a bundle that some other program put where
/// builds are looked for is not installed. A build that came from the
/// internet (`internet`) must also pass Apple's check, and is never taken by
/// an app with no developer signature. An app that is not signed by a team
/// (a build made without a certificate) takes any build from its build folder.
fn trusted(own: &Path, new: &Path, internet: bool) -> Result<(), String> {
    let Some(team) = team_of(own) else {
        return if internet { Err("this copy is not signed by its developer, so it takes no build from the internet".into()) } else { Ok(()) };
    };
    let rule = requirement(&team).ok_or("this copy's signature names no developer team")?;
    let whole = std::process::Command::new("/usr/bin/codesign").arg("--verify").arg("--deep").arg("--strict").arg(format!("-R={rule}")).arg(new).output();
    if !whole.is_ok_and(|o| o.status.success()) {
        return Err("the new build is not whole or not signed by the developer of this app".into());
    }
    if internet && !passes_apple_check(new) {
        return Err("the new build does not pass Apple's check (notarization)".into());
    }
    Ok(())
}

fn beside(app: &Path, tag: &str) -> Option<PathBuf> {
    let name = app.file_name()?.to_str()?;
    Some(app.parent()?.join(format!(".{name}.{tag}")))
}

/// Let `a` and `b` change places in one step (both must exist). Where the
/// disk can't do that (not APFS), an error, and nothing has moved.
fn swap(a: &Path, b: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn renamex_np(from: *const libc::c_char, to: *const libc::c_char, flags: libc::c_uint) -> libc::c_int;
    }
    const RENAME_SWAP: libc::c_uint = 0x2;
    let a = std::ffi::CString::new(a.as_os_str().as_bytes())?;
    let b = std::ffi::CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both are valid C strings that live through the call.
    if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Put `staged` in the place of `own`, and the app that was there at `old`.
fn put_in_place(own: &Path, staged: &Path, old: &Path) -> Result<(), String> {
    if swap(staged, own).is_ok() {
        // `staged` now holds the old app.
        if let Err(e) = std::fs::rename(staged, old) {
            log(&format!("the old app stays at {} ({e})", staged.display()));
        }
        return Ok(());
    }
    // Two renames: for a moment there is no app at `own`.
    std::fs::rename(own, old).map_err(|e| format!("couldn't move the old app aside ({e})"))?;
    if let Err(e) = std::fs::rename(staged, own) {
        // Put things back as they were.
        let _ = std::fs::rename(old, own);
        let _ = std::fs::remove_dir_all(staged);
        return Err(format!("couldn't put the new app in place ({e})"));
    }
    Ok(())
}

/// What to say when the app's own folder can't be written to.
fn locked_folder(own: &Path) -> String {
    let folder = own.parent().map(|p| p.display().to_string()).unwrap_or_default();
    format!("Rune can't write to the folder it is in ({folder}). Download the new version by hand from {RELEASES_PAGE}")
}

/// Replace the app at `own` with a copy of `update.source`. The copy is made
/// beside it and checked first, then the two change places; the old app is
/// left hidden beside it until the new one runs (see `started`). A program
/// running from `own` keeps running until it quits.
pub fn install_at(own: &Path, update: &Update) -> Result<(), String> {
    let staged = beside(own, "new").ok_or("the app is in a place with no name")?;
    let old = beside(own, "old").ok_or("the app is in a place with no name")?;
    for leftover in [&staged, &old] {
        if leftover.exists() {
            std::fs::remove_dir_all(leftover).map_err(|_| locked_folder(own))?;
        }
    }
    let copied = std::process::Command::new("/usr/bin/ditto").arg(&update.source).arg(&staged).stderr(std::process::Stdio::null()).status();
    if !copied.is_ok_and(|s| s.success()) {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(if update.source.exists() { locked_folder(own) } else { "the new build is gone".into() });
    }
    if build_of(&staged) != Some(update.build) {
        let _ = std::fs::remove_dir_all(&staged);
        return Err("the copy of the new build is not complete".into());
    }
    if let Err(why) = trusted(own, &staged, from_internet(&update.source)) {
        let _ = std::fs::remove_dir_all(&staged);
        log(&format!("install of {} refused: {why}", update.build));
        return Err(why);
    }
    put_in_place(own, &staged, &old)?;
    log(&format!("installed {} at {}", update.build, own.display()));
    Ok(())
}

/// Remove what an earlier update left beside the app at `own`.
pub fn tidy_at(own: &Path) {
    for tag in ["old", "new"] {
        if let Some(left) = beside(own, tag).filter(|p| p.exists()) {
            let _ = std::fs::remove_dir_all(left);
        }
    }
}

/// Settings a watcher passes on to the app it opens (for a test on a
/// throwaway workspace; none are set in normal use).
const PASSED_ON: [&str; 3] = ["NOTION2_DB", "NOTION2_UPDATE_DIR", "NOTION2_UPDATE_FEED"];

/// `open` the app at `app` as a new copy; with `wait`, the program ends when
/// the app does.
fn open_app(app: &Path, wait: bool) -> std::io::Result<std::process::Child> {
    let mut open = std::process::Command::new("/usr/bin/open");
    open.arg("-n");
    if wait {
        open.arg("-W");
    }
    for key in PASSED_ON {
        if let Some(value) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            open.arg("--env").arg(format!("{key}={}", value.to_string_lossy()));
        }
    }
    open.arg(app).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()
}

/// Start the app at `own` afresh, now build `build`, and end this program.
/// The old program (kept beside the app) watches the new one start, and
/// puts itself back if it does not (see `watch`).
pub fn relaunch(own: &Path, build: u64) -> ! {
    let watcher = beside(own, "old").map(|old| old.join(PROGRAM)).filter(|p| p.is_file());
    let watching = watcher.and_then(|program| {
        std::process::Command::new(program)
            .arg("--update-watch")
            .arg(own)
            .arg(build.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
    });
    if watching.is_none() {
        // No watcher: open the new copy after a moment, so this copy has
        // gone before the new one looks for another running copy of itself.
        let _ = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 0.6; /usr/bin/open -n \"$0\"")
            .arg(own)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    std::process::exit(0);
}

/// What the watcher saw.
#[derive(Clone, Debug, PartialEq)]
pub enum Watched {
    /// The new copy opened its window.
    Started,
    /// The new copy still runs, but has not opened its window yet; the old
    /// copy stays beside it until it does.
    StillRunning,
    /// The new copy ended before its window opened; the old one is back.
    WentBack,
    /// It ended, and the old one could not be put back.
    Stuck(String),
}

/// Remove the old copy kept beside `own` once build `build` runs there;
/// not when that copy has installed a later build in the meantime (the
/// copy beside it is then that update's old copy, kept for its own watcher).
fn tidy_after(own: &Path, build: u64) {
    if build_of(own) == Some(build) {
        tidy_at(own);
    }
}

/// Whether the copy at `own`, build `build`, has said that it started.
fn has_started(own: &Path, build: u64) -> bool {
    note_path("started").and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|t| t.trim() == format!("{build}\t{}", own.display()))
}

/// Watch the new copy at `own` (build `build`) start: `open` starts it, and
/// gives a program that ends when it does. When the copy says it started
/// (its window is open), done. When it ends first, open it once or twice
/// more if it never got going (macOS can refuse an open that comes too
/// soon), else put the old copy back. Gives up looking after `wait`.
pub fn watch(own: &Path, build: u64, open: &dyn Fn(&Path) -> std::io::Result<std::process::Child>, wait: std::time::Duration) -> Watched {
    let begin = std::time::Instant::now();
    let pause = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
    let mut tries = 1;
    let mut running = open(own).ok();
    loop {
        if has_started(own, build) {
            if let Some(mut c) = running.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            tidy_after(own, build);
            return Watched::Started;
        }
        let ended = match running.as_mut() {
            None => Some(false),
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => Some(status.success()),
                Ok(None) => None,
                Err(_) => Some(false),
            },
        };
        if let Some(fine) = ended {
            // Its note may land just after it ends (a quit right after the window opened).
            pause(300);
            if has_started(own, build) {
                tidy_after(own, build);
                return Watched::Started;
            }
            if !fine && tries < 3 && begin.elapsed() < std::time::Duration::from_secs(10) {
                tries += 1;
                pause(1500);
                running = open(own).ok();
                continue;
            }
            log(&format!("build {build} ended before its window opened"));
            return match roll_back(own, build) {
                Ok(()) => Watched::WentBack,
                Err(why) => {
                    log(&format!("couldn't go back: {why}"));
                    Watched::Stuck(why)
                }
            };
        }
        if begin.elapsed() > wait {
            return Watched::StillRunning;
        }
        pause(250);
    }
}

/// Put the old copy kept beside `own` back in its place, mark build `build`
/// (the copy that did not start) so it is not offered again, and remove it.
fn roll_back(own: &Path, build: u64) -> Result<(), String> {
    let old = beside(own, "old").ok_or("the app is in a place with no name")?;
    if build_of(own) != Some(build) {
        return Err("the app was changed in the meantime".into());
    }
    if build_of(&old).is_none() {
        return Err("there is no earlier copy to go back to".into());
    }
    put_in_place(own, &old, &beside(own, "broken").ok_or("no name")?)?;
    let _ = std::fs::remove_dir_all(beside(own, "broken").unwrap_or_default());
    mark_failed(build);
    write_note("went-back", &build.to_string());
    // The download of that build is of no more use.
    if let Some(dir) = downloads() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if build_of(&entry.path().join("Rune.app")) == Some(build) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    log(&format!("went back from {build} to {}", build_of(own).map(|b| b.to_string()).unwrap_or_default()));
    Ok(())
}

/// `notion2 --update-watch <app> <build>`: started by `relaunch` from the
/// old program (kept beside the app), to open the new copy and watch it start.
pub fn watch_command(args: &[String]) -> i32 {
    let at = args.iter().position(|a| a == "--update-watch").unwrap_or(0);
    let (Some(own), Some(build)) = (args.get(at + 1).map(PathBuf::from), args.get(at + 2).and_then(|b| number(b))) else {
        eprintln!("Usage: notion2 --update-watch <app> <build>");
        return 2;
    };
    // A moment's wait, so the copy that started this has gone before the
    // new one looks for another running copy of itself.
    std::thread::sleep(std::time::Duration::from_millis(600));
    log(&format!("open {build} and watch it start"));
    let seen = watch(&own, build, &|app| open_app(app, true), std::time::Duration::from_secs(180));
    log(&format!("watcher: {seen:?}"));
    match seen {
        Watched::WentBack => {
            let _ = open_app(&own, false);
            0
        }
        Watched::Stuck(_) => 1,
        _ => 0,
    }
}

/// The app's first window is open, so this build runs: say so to a watcher,
/// remove the old copy kept beside it, and give what to tell the person
/// (true for a problem): that it was updated, or that an update did not
/// start and the app went back. Call it on the UI thread.
pub fn started() -> Option<(bool, String)> {
    let (own, build) = (own_app()?, own_build()?);
    write_note("started", &format!("{build}\t{}", own.display()));
    tidy_at(&own);
    let went_back = note_path("went-back").and_then(|p| {
        let text = std::fs::read_to_string(&p).ok();
        let _ = std::fs::remove_file(&p);
        text
    });
    let last = crate::store::setting("update_last_build").and_then(|s| number(&s));
    if last != Some(build) {
        crate::store::set_setting("update_last_build", &build.to_string());
    }
    if let Some(bad) = went_back.as_deref().and_then(number) {
        return Some((true, format!("The update to the build of {} did not start, so Rune went back to the version you had. That update will not be offered again.", build_label(bad))));
    }
    match last {
        Some(before) if before < build => Some((false, format!("Rune is now version {} (built {})", env!("CARGO_PKG_VERSION"), build_label(build)))),
        _ => None,
    }
}

/// As the app opens: clear up after the last update, and with "Update
/// automatically" on, install a newer build and start again. An update that
/// was tried and did not take is not tried again on its own (so a broken one
/// can't restart the app for ever); it stays on offer in the app.
pub fn at_launch() {
    let Some(own) = own_app() else { return };
    // A half-made copy of an earlier try goes; the old copy stays until
    // this one has opened its window (`started`).
    if let Some(staged) = beside(&own, "new").filter(|p| p.exists()) {
        let _ = std::fs::remove_dir_all(staged);
    }
    tidy_downloads();
    if !automatic() {
        return;
    }
    let Some(update) = available() else { return };
    let tried = update.build.to_string();
    if crate::store::setting("update_tried").as_deref() == Some(tried.as_str()) {
        return;
    }
    crate::store::set_setting("update_tried", &tried);
    match install_at(&own, &update) {
        Ok(()) => {
            // This copy got this far, so it runs: a watcher of its own start
            // must not take it for one that did not.
            if let Some(build) = own_build() {
                write_note("started", &format!("{build}\t{}", own.display()));
            }
            relaunch(&own, update.build)
        }
        Err(e) => eprintln!("[update] {e}"),
    }
}

/// `available` off the calling thread; safe to await from the UI. A copy
/// that gets its updates from the published releases also asks them: when
/// `now` is true ("Check now"), or once a day by itself. The second value
/// says why the releases could not be asked, when they could not.
pub async fn check(now: bool) -> (Option<Update>, Option<String>) {
    let (tx, rx) = futures_channel::oneshot::channel();
    // The settings are read here, on the UI thread, where the workspace lives.
    let (own, build, source) = (own_app(), own_build(), source());
    let at = crate::model::now_ms();
    let feed = std::env::var("NOTION2_UPDATE_FEED").ok().filter(|f| !f.is_empty());
    let ask = from_releases() && (feed.is_some() || online()) && (now || time_to_ask(at));
    if ask {
        crate::store::set_setting("update_asked_at", &at.to_string());
    }
    crate::ai::runtime().spawn_blocking(move || {
        let (Some(own), Some(build)) = (own, build) else {
            let _ = tx.send((None, None));
            return;
        };
        let mut found = offered_at(&own, build, &source).or_else(|| downloaded().and_then(|app| offered_at(&own, build, &app)));
        let mut problem = None;
        // Only a copy signed by its developer takes a build from the
        // internet: the signature is what says the download is the real one.
        if found.is_none() && ask && team_of(&own).is_some() {
            match fetch_release(feed.as_deref().unwrap_or(RELEASES), &own) {
                Ok(Some(app)) => found = offered_at(&own, build, &app),
                Ok(None) => {}
                Err(why) => problem = Some(why),
            }
        }
        let _ = tx.send((found, problem));
    });
    rx.await.unwrap_or((None, None))
}

/// Install `update` over this copy (off the UI thread). On success the
/// caller restarts with `relaunch` (the app, and the build now in it).
pub async fn install(update: Update) -> Result<(PathBuf, u64), String> {
    let own = own_app().ok_or("This copy runs from the build folder, so there is nothing to replace.")?;
    let (tx, rx) = futures_channel::oneshot::channel();
    let (target, build) = (own.clone(), update.build);
    crate::ai::runtime().spawn_blocking(move || {
        let _ = tx.send(install_at(&target, &update));
    });
    rx.await.unwrap_or_else(|_| Err("the update stopped".into())).map(|()| (own, build))
}

/// One line on what this copy is, for Settings and `--update-check`.
pub fn describe() -> String {
    let version = env!("CARGO_PKG_VERSION");
    match own_build() {
        Some(build) => match COMMIT.filter(|c| !c.is_empty()) {
            Some(commit) => format!("Version {version}, built {} ({commit})", build_label(build)),
            None => format!("Version {version}, built {}", build_label(build)),
        },
        None => format!("Version {version}, a development build"),
    }
}

/// `notion2 --update-check` / `--update-install`: say what is on offer, and
/// with `install`, put it in place (no restart).
pub fn command_line(install: bool) -> i32 {
    println!("{}", describe());
    let source = PathBuf::from(DEFAULT_SOURCE);
    let (Some(own), Some(build)) = (own_app(), own_build()) else {
        println!("Update source: {}", source.display());
        println!("This copy does not update itself (it is not an installed build).");
        return 0;
    };
    // NOTION2_UPDATE_FEED names another feed than the published releases
    // (for a test), and has this copy use it even beside a build folder.
    let test_feed = std::env::var("NOTION2_UPDATE_FEED").ok().filter(|f| !f.is_empty());
    let mut found = None;
    if source.exists() && test_feed.is_none() {
        println!("Update source: {}", source.display());
        found = offered_at(&own, build, &source);
    } else {
        // No build folder on this Mac: the published releases.
        let feed = test_feed.unwrap_or_else(|| RELEASES.to_string());
        println!("Update source: the published releases ({feed})");
        if team_of(&own).is_none() {
            println!("This copy is not signed by its developer, so it takes no build from the internet.");
        } else {
            match fetch_release(&feed, &own) {
                Ok(Some(app)) => found = offered_at(&own, build, &app),
                Ok(None) => {}
                Err(why) => println!("Couldn't ask the releases: {why}."),
            }
        }
    }
    match found {
        None => {
            println!("Up to date.");
            0
        }
        Some(update) if !install => {
            println!("A newer build is ready: {}.", build_label(update.build));
            0
        }
        Some(update) => match install_at(&own, &update) {
            Ok(()) => {
                println!("Installed the build of {}.", build_label(update.build));
                0
            }
            Err(e) => {
                println!("Couldn't update: {e}.");
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_are_compared_as_numbers() {
        assert!(newer_version("v0.1.3", "0.1.2"));
        assert!(newer_version("0.2", "0.1.9"));
        assert!(newer_version("v0.1.10", "0.1.9"), "ten is more than nine");
        assert!(newer_version("1.0.0.1", "1.0"));
        assert!(!newer_version("v0.1.2", "0.1.2"));
        assert!(!newer_version("0.1", "0.1.0"));
        assert!(!newer_version("v0.1.1", "0.1.2"));
        // What is not a version is never newer.
        for odd in ["", "latest", "v1.x", "1..2", "1.2.3.4.5"] {
            assert!(!newer_version(odd, "0.1.0"), "{odd}");
        }
    }

    #[test]
    fn the_newest_release_is_read_from_the_answer() {
        let answer = r#"{"tag_name":"v0.1.3","draft":false,"prerelease":false,"assets":[
            {"name":"notes.txt","browser_download_url":"https://github.com/capi-git/Rune/releases/download/v0.1.3/notes.txt"},
            {"name":"Rune.zip","browser_download_url":"https://github.com/capi-git/Rune/releases/download/v0.1.3/Rune.zip"}]}"#;
        let url = "https://github.com/capi-git/Rune/releases/download/v0.1.3/Rune.zip".to_string();
        assert_eq!(release_in(answer), Some(Release { version: "v0.1.3".into(), url: url.clone(), sha256: None }));
        // The checksum GitHub gives with a download.
        let sum = "24F60DE53B18D365D0F2D92DCB0A493A7B13897DC061C7B941283C67F77C1DE6";
        let with = answer.replace(r#""name":"Rune.zip","#, &format!(r#""name":"Rune.zip","digest":"sha256:{sum}","#));
        assert_eq!(release_in(&with).unwrap().sha256, Some(sum.to_ascii_lowercase()));
        let odd = answer.replace(r#""name":"Rune.zip","#, r#""name":"Rune.zip","digest":"md5:abc","#);
        assert_eq!(release_in(&odd).unwrap().sha256, None, "a checksum of another kind is not used");
        // No download for this app, a draft, a name that is no version, or no JSON: nothing.
        assert_eq!(release_in(r#"{"tag_name":"v0.1.3","assets":[]}"#), None);
        assert_eq!(release_in(&answer.replace("\"draft\":false", "\"draft\":true")), None);
        assert_eq!(release_in(&answer.replace("v0.1.3\",\"draft", "newest\",\"draft")), None);
        assert_eq!(release_in("<html>rate limit</html>"), None);
    }

    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// The updater's folder for this test (each test runs on a thread of its own).
        pub(super) static DIR: RefCell<PathBuf> = RefCell::new(std::env::temp_dir().join(format!("notion2-update-notes-{}", std::process::id())));
    }

    /// Give this test an updater's folder of its own.
    fn own_notes(dir: &Path) {
        DIR.with(|d| *d.borrow_mut() = dir.join("notes"));
    }

    /// A stand-in app on disk: an Info.plist with a build number, and a program.
    fn fake_app(dir: &Path, name: &str, build: &str, program: &str, id: &str) -> PathBuf {
        let app = dir.join(name);
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!("<plist><dict>\n  <key>CFBundleIdentifier</key><string>{id}</string>\n  <key>CFBundleVersion</key><string>{build}</string>\n</dict></plist>"),
        )
        .unwrap();
        std::fs::write(app.join(PROGRAM), program).unwrap();
        app
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("notion2-update-{name}-{}", crate::model::now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_a_newer_build_of_this_app_is_an_update() {
        let dir = temp("offer");
        let own = fake_app(&dir.join("Applications"), "Notion 2.0.app", "20261109120000", "old", BUNDLE_ID);
        let newer = fake_app(&dir.join("build"), "Notion 2.0.app", "20261109130000", "new", BUNDLE_ID);
        assert_eq!(build_of(&newer), Some(20261109130000));
        assert_eq!(available_at(&own, 20261109120000, &newer).map(|u| u.build), Some(20261109130000));
        assert_eq!(available_at(&own, 20261109130000, &newer), None, "the same build");
        assert_eq!(available_at(&own, 20261109140000, &newer), None, "an older build");
        assert_eq!(available_at(&own, 1, &own), None, "the app is never its own update");
        let other = fake_app(&dir.join("other"), "Notion 2.0.app", "20991231000000", "x", "com.example.other");
        assert_eq!(available_at(&own, 20261109120000, &other), None, "another app");
        assert_eq!(available_at(&own, 20261109120000, &dir.join("missing.app")), None);
        let dev = fake_app(&dir.join("dev"), "Notion 2.0.app", "0.1.0", "x", BUNDLE_ID);
        assert_eq!(build_of(&dev), None, "a build with no build number");
        assert_eq!(build_label(20261109133153), "9 Nov 2026, 13:31");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_swaps_the_app_and_tidies_up_later() {
        let dir = temp("swap");
        let own = fake_app(&dir.join("Applications"), "Notion 2.0.app", "20261109120000", "old program", BUNDLE_ID);
        let newer = fake_app(&dir.join("build"), "Notion 2.0.app", "20261109130000", "new program", BUNDLE_ID);
        let update = available_at(&own, 20261109120000, &newer).unwrap();
        install_at(&own, &update).unwrap();
        assert_eq!(std::fs::read_to_string(own.join(PROGRAM)).unwrap(), "new program");
        assert_eq!(build_of(&own), Some(20261109130000));
        assert_eq!(std::fs::read_to_string(newer.join(PROGRAM)).unwrap(), "new program", "the source is left as it was");
        let old = beside(&own, "old").unwrap();
        assert_eq!(std::fs::read_to_string(old.join(PROGRAM)).unwrap(), "old program", "the old app waits beside it");
        assert!(!beside(&own, "new").unwrap().exists());
        tidy_at(&own);
        assert!(!old.exists());
        // Installing again over leftovers of an earlier try works too.
        std::fs::create_dir_all(beside(&own, "new").unwrap()).unwrap();
        let newest = fake_app(&dir.join("build2"), "Notion 2.0.app", "20261109140000", "newest program", BUNDLE_ID);
        install_at(&own, &available_at(&own, 20261109130000, &newest).unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(own.join(PROGRAM)).unwrap(), "newest program");
        // A source that is gone fails and leaves the app as it is.
        let gone = Update { source: dir.join("nowhere.app"), build: 20261109150000 };
        assert!(install_at(&own, &gone).is_err());
        assert_eq!(std::fs::read_to_string(own.join(PROGRAM)).unwrap(), "newest program");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_build_must_come_from_apple_issued_certificates_of_this_developer() {
        assert_eq!(requirement("L54FM345MU").as_deref(), Some(r#"anchor apple generic and certificate leaf[subject.OU] = "L54FM345MU""#));
        // A team name that could change the meaning of the rule is not trusted.
        for odd in ["", "L54\" or true", "a b", "x".repeat(40).as_str()] {
            assert_eq!(requirement(odd), None, "{odd}");
        }
    }

    #[test]
    fn two_folders_change_places_in_one_step() {
        let dir = temp("swap-step");
        let a = fake_app(&dir, "A.app", "1", "a", BUNDLE_ID);
        let b = fake_app(&dir, "B.app", "2", "b", BUNDLE_ID);
        swap(&a, &b).unwrap();
        assert_eq!(std::fs::read_to_string(a.join(PROGRAM)).unwrap(), "b");
        assert_eq!(std::fs::read_to_string(b.join(PROGRAM)).unwrap(), "a");
        assert!(swap(&a, &dir.join("missing.app")).is_err(), "both must be there");
        assert_eq!(std::fs::read_to_string(a.join(PROGRAM)).unwrap(), "b", "and then nothing moved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An app at `own` just updated to build 20261111130000 from 20261111120000
    /// (kept beside it), as `install_at` leaves it.
    fn just_updated(dir: &Path) -> PathBuf {
        own_notes(dir);
        let own = fake_app(&dir.join("Applications"), "Rune.app", "20261111120000", "old program", BUNDLE_ID);
        let newer = fake_app(&dir.join("build"), "Rune.app", "20261111130000", "new program", BUNDLE_ID);
        install_at(&own, &available_at(&own, 20261111120000, &newer).unwrap()).unwrap();
        assert_eq!(build_of(&own), Some(20261111130000));
        own
    }

    fn program(app: &Path) -> String {
        std::fs::read_to_string(app.join(PROGRAM)).unwrap_or_default()
    }

    fn sh(script: &str) -> std::io::Result<std::process::Child> {
        std::process::Command::new("/bin/sh").arg("-c").arg(script).spawn()
    }

    const NEW: u64 = 20261111130000;
    const LONG: std::time::Duration = std::time::Duration::from_secs(20);

    #[test]
    fn a_new_copy_that_ends_before_its_window_opens_is_taken_back() {
        let dir = temp("went-back");
        let own = just_updated(&dir);
        // Downloaded before: it goes with the bad build.
        let kept = fake_app(&dir.join("notes/v0.1.9"), "Rune.app", &NEW.to_string(), "new program", BUNDLE_ID);
        let seen = watch(&own, NEW, &|_| sh("exit 0"), LONG);
        assert_eq!(seen, Watched::WentBack);
        assert_eq!(program(&own), "old program", "the old copy is back in its place");
        assert_eq!(build_of(&own), Some(20261111120000));
        assert!(!beside(&own, "old").unwrap().exists() && !beside(&own, "broken").unwrap().exists(), "nothing is left beside it");
        assert!(!kept.exists(), "the download of the bad build is removed");
        assert!(failed(NEW) && !failed(20261111120000));
        assert_eq!(std::fs::read_to_string(note_path("went-back").unwrap()).unwrap(), NEW.to_string(), "the old copy will say what happened");
        // That build is never offered again, from anywhere.
        let again = fake_app(&dir.join("build3"), "Rune.app", &NEW.to_string(), "new program", BUNDLE_ID);
        assert!(available_at(&own, 20261111120000, &again).is_some());
        assert_eq!(offered_at(&own, 20261111120000, &again), None);
        // Nothing to go back to: it says so and leaves the app alone.
        let lone = fake_app(&dir.join("Lone"), "Rune.app", &NEW.to_string(), "lone", BUNDLE_ID);
        assert!(matches!(watch(&lone, NEW, &|_| sh("exit 0"), LONG), Watched::Stuck(_)));
        assert_eq!(program(&lone), "lone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_copy_that_opens_its_window_is_kept_and_the_old_one_goes() {
        let dir = temp("started");
        let own = just_updated(&dir);
        // The note of another copy (the build folder's, with the same build) does not count.
        write_note("started", &format!("{NEW}\t/elsewhere/Rune.app"));
        let opened = std::cell::Cell::new(0);
        let own_line = format!("{NEW}\t{}", own.display());
        let seen = watch(
            &own,
            NEW,
            &|_| {
                opened.set(opened.get() + 1);
                write_note("started", &own_line);
                sh("sleep 30")
            },
            LONG,
        );
        assert_eq!(seen, Watched::Started);
        assert_eq!(opened.get(), 1);
        assert_eq!(program(&own), "new program");
        assert!(!beside(&own, "old").unwrap().exists(), "the old copy goes once the new one runs");
        assert!(!failed(NEW));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_copy_that_updates_again_at_once_keeps_the_copy_its_own_watcher_needs() {
        let dir = temp("chain");
        let own = just_updated(&dir);
        let own_line = format!("{NEW}\t{}", own.display());
        let later = fake_app(&dir.join("build4"), "Rune.app", "20261111140000", "later program", BUNDLE_ID);
        let seen = watch(
            &own,
            NEW,
            &|app| {
                // As `at_launch` does: install the later build, then say this one ran.
                install_at(app, &available_at(app, NEW, &later).unwrap()).unwrap();
                write_note("started", &own_line);
                sh("exit 0")
            },
            LONG,
        );
        assert_eq!(seen, Watched::Started);
        assert_eq!(program(&own), "later program");
        assert_eq!(program(&beside(&own, "old").unwrap()), "new program", "kept for the watcher of the later build");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_that_macos_refused_is_tried_again() {
        let dir = temp("open-again");
        let own = just_updated(&dir);
        let opened = std::cell::Cell::new(0);
        let own_line = format!("{NEW}\t{}", own.display());
        let seen = watch(
            &own,
            NEW,
            &|_| {
                opened.set(opened.get() + 1);
                if opened.get() == 1 {
                    return sh("exit 1");
                }
                write_note("started", &own_line);
                sh("sleep 30")
            },
            LONG,
        );
        assert_eq!((seen, opened.get()), (Watched::Started, 2));
        assert_eq!(program(&own), "new program");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_slow_new_copy_keeps_the_old_one_beside_it() {
        let dir = temp("slow");
        let own = just_updated(&dir);
        let seen = watch(&own, NEW, &|_| sh("sleep 30"), std::time::Duration::from_millis(700));
        assert_eq!(seen, Watched::StillRunning);
        assert_eq!(program(&own), "new program");
        assert_eq!(program(&beside(&own, "old").unwrap()), "old program", "kept until the new copy opens its window");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A release feed on this Mac: `Rune.zip` of an app with build `build`,
    /// and the answer of the releases with `tag` and checksum `sum` (the
    /// zip's own when none).
    fn feed(dir: &Path, build: &str, tag: &str, sum: Option<&str>) -> String {
        let made = fake_app(&dir.join("made"), "Rune.app", build, "released program", BUNDLE_ID);
        let zip = dir.join("Rune.zip");
        let _ = std::fs::remove_file(&zip);
        assert!(std::process::Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(&made).arg(&zip).status().unwrap().success());
        let sum = sum.map(str::to_string).unwrap_or_else(|| sha256_of(&zip).unwrap());
        let answer = dir.join("latest.json");
        std::fs::write(
            &answer,
            format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"assets":[{{"name":"Rune.zip","digest":"sha256:{sum}","browser_download_url":"file://{}"}}]}}"#, zip.display()),
        )
        .unwrap();
        format!("file://{}", answer.display())
    }

    #[test]
    fn a_download_is_checked_before_it_is_offered() {
        let dir = temp("download");
        own_notes(&dir);
        let own = fake_app(&dir.join("Applications"), "Rune.app", "20261111120000", "old program", BUNDLE_ID);
        let folder = dir.join("notes/v99.0.0");
        // Not later than this version: nothing is brought here.
        assert_eq!(fetch_release(&feed(&dir, "20261111130000", &format!("v{}", env!("CARGO_PKG_VERSION")), None), &own), Ok(None));
        // A damaged download is removed.
        let damaged = fetch_release(&feed(&dir, "20261111130000", "v99.0.0", Some(&"0".repeat(64))), &own);
        assert!(damaged.as_ref().is_err_and(|e| e.contains("checksum")), "{damaged:?}");
        assert!(!folder.exists());
        // A whole one is opened, and then must be signed by this app's
        // developer; the stand-in app here is not signed, so it takes none.
        let unsigned = fetch_release(&feed(&dir, "20261111130000", "v99.0.0", None), &own);
        assert!(unsigned.as_ref().is_err_and(|e| e.contains("not signed")), "{unsigned:?}");
        assert!(!folder.exists(), "what is refused is not kept");
        // A build that did not start before is not brought here again.
        mark_failed(20261111130000);
        assert_eq!(fetch_release(&feed(&dir, "20261111130000", "v99.0.0", None), &own), Ok(None));
        assert!(!folder.exists());
        // A build from the internet always needs the developer's signature.
        assert!(trusted(&own, &own, true).is_err());
        assert!(trusted(&own, &own, false).is_ok(), "an unsigned copy takes its own build folder's builds, as before");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
