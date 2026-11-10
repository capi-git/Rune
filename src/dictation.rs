//! macOS Dictation, for the voice box (see `VoiceModal`).
//!
//! The app does not listen to the microphone itself. It asks macOS to start
//! Dictation, the same as Edit ▸ Start Dictation…: macOS then hears the
//! speech and types it into the text box that has the cursor. So there is no
//! microphone permission for this app, and what is said stays with Apple's
//! Dictation (on this Mac where the language allows it).

use objc2::runtime::{AnyObject, Sel};
use objc2::{class, msg_send, sel, MainThreadMarker};

/// Send the app the action a menu item would.
fn send(action: Sel) -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    // SAFETY: on the main thread, `sharedApplication` is the running
    // NSApplication; it is asked whether it answers the action before the
    // action is sent, with no sender, as a menu item with no target does.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return false;
        }
        let answers: bool = msg_send![app, respondsToSelector: action];
        if answers {
            let _: *mut AnyObject = msg_send![app, performSelector: action, withObject: std::ptr::null_mut::<AnyObject>()];
        }
        answers
    }
}

/// Whether Dictation may really be started now: always, except in a test
/// run, which must not make the Mac listen (unless it asks for exactly that
/// with `NOTION2_REAL_DICTATION`, to prove the real thing).
pub fn allowed() -> bool {
    std::env::var_os("NOTION2_SELFTEST").is_none() || std::env::var_os("NOTION2_REAL_DICTATION").is_some()
}

/// Start Dictation for the text box in front. False if this macOS can't.
/// (If Dictation is turned off, macOS offers to turn it on.)
pub fn start() -> bool {
    send(sel!(startDictation:))
}

/// Stop Dictation, if it is going.
pub fn stop() -> bool {
    send(sel!(stopDictation:))
}

/// System Settings ▸ Keyboard, where Dictation is turned on and its language
/// and shortcut are set.
pub fn open_settings() {
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Keyboard-Settings.extension?Dictation").spawn();
}
