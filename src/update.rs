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
//! The swap is two renames in the app's own folder (the new copy is staged
//! beside it first), so there is never a half-written app; the old copy is
//! left hidden beside it and removed the next time the app opens.
//!
//! `notion2 --update-check` and `--update-install` do the same from Terminal,
//! without opening a window or restarting.
//!
//! On every Mac but the one that builds the app there is no build folder.
//! There the app asks the published releases for the newest version (one
//! small request a day, or "Check now"; nothing of the person's is sent),
//! brings a newer one to this Mac, and installs it the same way: the copy
//! must be whole and signed by the same developer.

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

/// The update waiting for this copy of the app, if any: in the build
/// folder, or a release that was brought to this Mac before.
pub fn available() -> Option<Update> {
    let (own, build) = (own_app()?, own_build()?);
    available_at(&own, build, &source()).or_else(|| downloaded().and_then(|app| available_at(&own, build, &app)))
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

/// The newest release in what the releases answered: its version, and
/// where its download is.
pub fn release_in(answer: &str) -> Option<(String, String)> {
    let json: serde_json::Value = serde_json::from_str(answer).ok()?;
    if json.get("draft").and_then(|d| d.as_bool()) == Some(true) || json.get("prerelease").and_then(|d| d.as_bool()) == Some(true) {
        return None;
    }
    let version = json.get("tag_name")?.as_str()?.to_string();
    let url = json.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET))?.get("browser_download_url")?.as_str()?.to_string();
    version_numbers(&version)?;
    Some((version, url))
}

/// Where releases that were brought to this Mac are kept until they are installed.
fn downloads() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches/Rune Updates"))
}

/// The newest release that is on this Mac already.
fn downloaded() -> Option<PathBuf> {
    let mut found: Vec<(u64, PathBuf)> = std::fs::read_dir(downloads()?)
        .ok()?
        .flatten()
        .map(|e| e.path().join("Rune.app"))
        .filter_map(|app| build_of(&app).map(|b| (b, app)))
        .collect();
    found.sort();
    found.pop().map(|(_, app)| app)
}

/// Remove the releases on this Mac that are not newer than this copy.
fn tidy_downloads() {
    let (Some(dir), Some(build)) = (downloads(), own_build()) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !build_of(&folder.join("Rune.app")).is_some_and(|b| b > build) {
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
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
/// bring it to this Mac. Gives the app to install from, or none when this
/// copy is the newest. Blocks: call it off the UI thread.
pub fn fetch_release(feed: &str) -> Result<Option<PathBuf>, String> {
    // A feed that is not the real one (a test) may name a file on this Mac.
    let real = feed == RELEASES;
    let answer = curl(&["--max-time", "20", "-H", "Accept: application/vnd.github+json", feed])?;
    let Some((version, url)) = release_in(&String::from_utf8_lossy(&answer)) else { return Ok(None) };
    if !newer_version(&version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    if real && !url.starts_with(RELEASE_FILES) {
        return Err("the newest release has its download in a place that is not expected".into());
    }
    let name: String = version.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
    let folder = downloads().ok_or("this Mac has no home folder")?.join(name);
    let app = folder.join("Rune.app");
    if build_of(&app).is_some() {
        return Ok(Some(app));
    }
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| format!("couldn't make room for the download ({e})"))?;
    let zip = folder.join(ASSET);
    let fetched = curl(&["--max-time", "600", "--output", &zip.display().to_string(), &url]);
    let opened = fetched.and_then(|_| {
        let done = std::process::Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(&zip).arg(&folder).status();
        if done.is_ok_and(|s| s.success()) { Ok(()) } else { Err("couldn't open the download".to_string()) }
    });
    let _ = std::fs::remove_file(&zip);
    if let Err(why) = opened {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(why);
    }
    if build_of(&app).is_none() {
        let _ = std::fs::remove_dir_all(&folder);
        return Err("the download is not a build of this app".into());
    }
    Ok(Some(app))
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

/// Whether a new build may take the place of the app at `own`: when the app
/// is signed by a developer team, the build must be whole (its signature
/// holds for every file in it) and signed by the same team. So a bundle
/// that some other program put where builds are looked for is not installed.
/// An app that is not signed by a team (a build made without a certificate)
/// takes any build, as before.
fn trusted(own: &Path, new: &Path) -> Result<(), String> {
    let Some(team) = team_of(own) else { return Ok(()) };
    let whole = std::process::Command::new("/usr/bin/codesign").arg("--verify").arg("--deep").arg("--strict").arg(new).output();
    if !whole.is_ok_and(|o| o.status.success()) {
        return Err("the new build's signature does not hold".into());
    }
    if team_of(new).as_deref() != Some(team.as_str()) {
        return Err("the new build is not signed by the developer of this app".into());
    }
    Ok(())
}

fn beside(app: &Path, tag: &str) -> Option<PathBuf> {
    let name = app.file_name()?.to_str()?;
    Some(app.parent()?.join(format!(".{name}.{tag}")))
}

/// Replace the app at `own` with a copy of `update.source`. The copy is made
/// beside it first, then swapped in; the old app is left hidden beside it
/// (see `tidy`). A program running from `own` keeps running until it quits.
pub fn install_at(own: &Path, update: &Update) -> Result<(), String> {
    let staged = beside(own, "new").ok_or("the app is in a place with no name")?;
    let old = beside(own, "old").ok_or("the app is in a place with no name")?;
    for leftover in [&staged, &old] {
        if leftover.exists() {
            std::fs::remove_dir_all(leftover).map_err(|e| format!("couldn't clear an earlier update ({e})"))?;
        }
    }
    let copied = std::process::Command::new("/usr/bin/ditto").arg(&update.source).arg(&staged).status();
    if !copied.is_ok_and(|s| s.success()) {
        let _ = std::fs::remove_dir_all(&staged);
        return Err("couldn't copy the new build (is the Applications folder locked?)".into());
    }
    if build_of(&staged) != Some(update.build) {
        let _ = std::fs::remove_dir_all(&staged);
        return Err("the copy of the new build is not complete".into());
    }
    if let Err(why) = trusted(own, &staged) {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(why);
    }
    std::fs::rename(own, &old).map_err(|e| format!("couldn't move the old app aside ({e})"))?;
    if let Err(e) = std::fs::rename(&staged, own) {
        // Put things back as they were.
        let _ = std::fs::rename(&old, own);
        let _ = std::fs::remove_dir_all(&staged);
        return Err(format!("couldn't put the new app in place ({e})"));
    }
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

/// Start the app at `own` afresh and end this program.
pub fn relaunch(own: &Path) -> ! {
    // A moment's wait, so this copy has gone before the new one looks for
    // another running copy of itself.
    let _ = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.6; /usr/bin/open -n \"$0\"")
        .arg(own)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    std::process::exit(0);
}

/// As the app opens: clear up after the last update, and with "Update
/// automatically" on, install a newer build and start again. An update that
/// was tried and did not take is not tried again on its own (so a broken one
/// can't restart the app for ever); it stays on offer in the app.
pub fn at_launch() {
    let Some(own) = own_app() else { return };
    tidy_at(&own);
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
        Ok(()) => relaunch(&own),
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
        let mut found = available_at(&own, build, &source).or_else(|| downloaded().and_then(|app| available_at(&own, build, &app)));
        let mut problem = None;
        // Only a copy signed by its developer takes a build from the
        // internet: the signature is what says the download is the real one.
        if found.is_none() && ask && team_of(&own).is_some() {
            match fetch_release(feed.as_deref().unwrap_or(RELEASES)) {
                Ok(Some(app)) => found = available_at(&own, build, &app),
                Ok(None) => {}
                Err(why) => problem = Some(why),
            }
        }
        let _ = tx.send((found, problem));
    });
    rx.await.unwrap_or((None, None))
}

/// Install `update` over this copy (off the UI thread). On success the
/// caller restarts with `relaunch`.
pub async fn install(update: Update) -> Result<PathBuf, String> {
    let own = own_app().ok_or("This copy runs from the build folder, so there is nothing to replace.")?;
    let (tx, rx) = futures_channel::oneshot::channel();
    let target = own.clone();
    crate::ai::runtime().spawn_blocking(move || {
        let _ = tx.send(install_at(&target, &update));
    });
    rx.await.unwrap_or_else(|_| Err("the update stopped".into())).map(|()| own)
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
    tidy_at(&own);
    // NOTION2_UPDATE_FEED names another feed than the published releases
    // (for a test), and has this copy use it even beside a build folder.
    let test_feed = std::env::var("NOTION2_UPDATE_FEED").ok().filter(|f| !f.is_empty());
    let mut found = None;
    if source.exists() && test_feed.is_none() {
        println!("Update source: {}", source.display());
        found = available_at(&own, build, &source);
    } else {
        // No build folder on this Mac: the published releases.
        let feed = test_feed.unwrap_or_else(|| RELEASES.to_string());
        println!("Update source: the published releases ({feed})");
        if team_of(&own).is_none() {
            println!("This copy is not signed by its developer, so it takes no build from the internet.");
        } else {
            match fetch_release(&feed) {
                Ok(Some(app)) => found = available_at(&own, build, &app),
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
        assert_eq!(release_in(answer), Some(("v0.1.3".to_string(), "https://github.com/capi-git/Rune/releases/download/v0.1.3/Rune.zip".to_string())));
        // No download for this app, a draft, a name that is no version, or no JSON: nothing.
        assert_eq!(release_in(r#"{"tag_name":"v0.1.3","assets":[]}"#), None);
        assert_eq!(release_in(&answer.replace("\"draft\":false", "\"draft\":true")), None);
        assert_eq!(release_in(&answer.replace("v0.1.3\",\"draft", "newest\",\"draft")), None);
        assert_eq!(release_in("<html>rate limit</html>"), None);
    }

    use super::*;

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
}
