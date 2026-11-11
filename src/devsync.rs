//! Sync between devices: the Mac and the iPhone keep the same notes with no
//! server of ours (docs/sync-format.md, docs/sync-wifi.md).
//!
//! The Mac writes its sync files in a folder of its own, always
//! (`<notes folder>/Sync/sync-v1/`), and also copies them into the folder
//! in iCloud Drive when the iPhone app has made that folder. It reads the
//! other devices' files from both. Over Wi-Fi it serves its own folder to a
//! paired iPhone (wifi.rs). Both ways are off until they are turned on in
//! Settings; nothing in the notes file changes before that.

pub mod engine;
pub mod format;
pub mod wifi;

use std::path::{Path, PathBuf};

/// The folder of the Mac's own sync state, beside the notes file: the
/// certificate, the paired iPhones, and `sync-v1/`.
pub fn state_dir(db: &Path) -> PathBuf {
    if std::env::var_os("NOTION2_DB").is_some() {
        let mut name = db.to_path_buf().into_os_string();
        name.push(".sync");
        return PathBuf::from(name);
    }
    db.parent().unwrap_or_else(|| Path::new(".")).join("Sync")
}

/// The Mac's own sync folder.
pub fn own_folder(db: &Path) -> PathBuf {
    state_dir(db).join(format::FOLDER)
}

/// The folder in iCloud Drive, when the iPhone app has made it (the Mac
/// never makes it). `NOTION2_ICLOUD_SYNC` names another folder (tests).
pub fn icloud_folder() -> Option<PathBuf> {
    let root = match std::env::var_os("NOTION2_ICLOUD_SYNC").filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => dirs::home_dir()?.join("Library/Mobile Documents/iCloud~com~jackhagan~notion2/Documents"),
    };
    let folder = root.join(format::FOLDER);
    folder.is_dir().then_some(folder)
}

// ----- running -----------------------------------------------------------------

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};

/// The settings that turn the two ways on ("on" or nothing).
const ICLOUD_SETTING: &str = "sync_icloud";
const WIFI_SETTING: &str = "sync_wifi";
/// How often the engine looks for changes when nothing nudges it.
const EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// What Settings shows about sync.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub icloud: bool,
    pub wifi: bool,
    /// The folder in iCloud Drive is there (the iPhone app made it).
    pub icloud_ready: bool,
    /// When the engine last ran without a fault (ms since 1970).
    pub last_ms: i64,
    pub fault: Option<String>,
    /// Files are listed that iCloud has not brought yet.
    pub waiting: bool,
    /// A device writes a newer format: Rune must be updated.
    pub needs_update: bool,
    /// The other devices, as their device files say.
    pub devices: Vec<format::DeviceFile>,
}

enum Msg {
    Nudge,
    Stop,
}

struct Runtime {
    tx: Sender<Msg>,
    thread: Option<std::thread::JoinHandle<()>>,
    wifi: Option<wifi::Server>,
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
static STATUS: Mutex<Option<Status>> = Mutex::new(None);

/// The notes file sync works on (none when the app runs without one).
fn db() -> Option<PathBuf> {
    crate::store::db_path()
}

/// This Mac's name, as others see it ("MacBook Pro").
pub fn mac_name() -> String {
    wifi::computer_name()
}

pub fn status() -> Status {
    let mut status = STATUS.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
    status.icloud = crate::store::setting(ICLOUD_SETTING).as_deref() == Some("on");
    status.wifi = crate::store::setting(WIFI_SETTING).as_deref() == Some("on");
    status.icloud_ready = icloud_folder().is_some();
    status
}

/// At launch: start what Settings turned on.
pub fn start_from_settings() {
    let s = status();
    if s.icloud || s.wifi {
        if let Err(e) = restart(s.icloud, s.wifi) {
            set_fault(Some(e));
        }
    }
}

/// Turn the two ways on or off (from Settings).
pub fn set(icloud: bool, wifi: bool) -> Result<(), String> {
    crate::store::set_settings(&[(ICLOUD_SETTING, if icloud { "on" } else { "" }.to_string()), (WIFI_SETTING, if wifi { "on" } else { "" }.to_string())]);
    let result = restart(icloud, wifi);
    set_fault(result.clone().err());
    result
}

fn set_fault(fault: Option<String>) {
    if let Ok(mut s) = STATUS.lock() {
        s.get_or_insert_with(Status::default).fault = fault;
    }
}

/// Run the engine soon (a file came over Wi-Fi).
pub fn nudge() {
    if let Ok(rt) = RUNTIME.lock() {
        if let Some(rt) = rt.as_ref() {
            let _ = rt.tx.send(Msg::Nudge);
        }
    }
}

/// Stop the engine after one last run (the app quits).
pub fn stop() {
    let rt = RUNTIME.lock().ok().and_then(|mut rt| rt.take());
    if let Some(mut rt) = rt {
        let _ = rt.tx.send(Msg::Stop);
        if let Some(server) = rt.wifi.take() {
            server.stop();
        }
        if let Some(thread) = rt.thread.take() {
            let _ = thread.join();
        }
    }
}

fn restart(icloud: bool, wifi_on: bool) -> Result<(), String> {
    stop();
    if !icloud && !wifi_on {
        return Ok(());
    }
    let db = db().ok_or("Sync needs the notes file, and Rune runs without one now.")?;
    let state = state_dir(&db);
    let own = own_folder(&db);
    std::fs::create_dir_all(&own).map_err(|e| e.to_string())?;
    let name = mac_name();
    // Open once here, so a fault shows in Settings at once.
    let engine = engine::Engine::open(&db, &state, &own, crate::files::dir(), &name)?;
    let device = engine.device().to_string();
    let (tx, rx) = channel::<Msg>();
    let thread = std::thread::Builder::new()
        .name("rune-sync".into())
        .spawn(move || {
            let mut engine = engine;
            loop {
                let mut folders = vec![own.clone()];
                if icloud {
                    folders.extend(icloud_folder());
                }
                let outcome = engine.run(&folders);
                if let Ok(mut s) = STATUS.lock() {
                    let s = s.get_or_insert_with(Status::default);
                    match outcome {
                        Ok(report) => {
                            s.last_ms = engine::now_ms();
                            s.fault = None;
                            s.waiting = report.waiting;
                            s.needs_update = report.needs_update;
                            s.devices = report.devices;
                        }
                        Err(e) => s.fault = Some(e),
                    }
                }
                match rx.recv_timeout(EVERY) {
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Msg::Nudge) | Err(RecvTimeoutError::Timeout) => {}
                }
            }
            // One last run, so a change made just before the end goes out.
            let mut folders = vec![own.clone()];
            if icloud {
                folders.extend(icloud_folder());
            }
            let _ = engine.run(&folders);
        })
        .map_err(|e| e.to_string())?;
    let wifi = if wifi_on {
        let on_put: Arc<dyn Fn() + Send + Sync> = Arc::new(nudge);
        let config = wifi::Config { folder: own_folder(&db), state: state.clone(), device, name };
        // A test run listens on this Mac only: macOS then asks nothing.
        if std::env::var_os("NOTION2_SELFTEST").is_some() {
            Some(wifi::Server::start_local(config, 0, on_put)?)
        } else {
            Some(wifi::Server::start(config, on_put)?)
        }
    } else {
        None
    };
    if let Ok(mut rt) = RUNTIME.lock() {
        *rt = Some(Runtime { tx, thread: Some(thread), wifi });
    }
    Ok(())
}

/// A new code to connect an iPhone, with its QR code.
pub fn offer() -> Result<wifi::Offer, String> {
    let rt = RUNTIME.lock().map_err(|e| e.to_string())?;
    rt.as_ref().and_then(|rt| rt.wifi.as_ref()).ok_or_else(|| "Turn on Sync over Wi-Fi first.".to_string())?.offer()
}

/// The code on screen is closed.
pub fn withdraw() {
    if let Ok(rt) = RUNTIME.lock() {
        if let Some(server) = rt.as_ref().and_then(|rt| rt.wifi.as_ref()) {
            server.withdraw();
        }
    }
}

/// The iPhones paired over Wi-Fi.
pub fn paired() -> Vec<wifi::Paired> {
    db().map(|db| wifi::paired(&state_dir(&db))).unwrap_or_default()
}

pub fn forget(device: &str) -> Result<(), String> {
    let db = db().ok_or("No notes file.")?;
    wifi::forget(&state_dir(&db), device)
}
