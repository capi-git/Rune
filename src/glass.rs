//! The window's glass, done the way Aven and Warden do it: the window itself
//! is see-through, the page paints its own translucent colours on it (the
//! frame, and a little less see-through, the page card), and the window
//! server softens whatever is behind the window by a radius chosen in Settings.
//!
//! That softening is `CGSSetWindowBackgroundBlurRadius`, a call macOS has but
//! doesn't document (terminal apps have leaned on it for years). It's looked
//! up by name when first needed; if a macOS ever drops it, the window falls
//! back to the documented frosted material, which blurs by a fixed amount.

use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
use dioxus::desktop::tao::window::Window;
use objc2::runtime::AnyObject;
use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use std::collections::HashSet;
use std::ffi::{c_void, CStr};
use std::ptr::NonNull;
use std::sync::Mutex;
use window_vibrancy::{apply_vibrancy, clear_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

/// How see-through the window starts out (0 solid … 100 clearest) and how
/// much it blurs what's behind (points). Aven's own defaults.
pub const GLASS_DEFAULT: u8 = 48;
pub const BLUR_DEFAULT: u8 = 16;
pub const BLUR_MAX: u8 = 64;

/// (connection, window number, radius) -> error code.
type SetBlur = unsafe extern "C" fn(i32, u32, usize) -> i32;
type Connection = unsafe extern "C" fn() -> i32;

/// A function of the window server, by name.
fn symbol(name: &CStr) -> Option<NonNull<c_void>> {
    // SAFETY: `dlsym` only reads the name; a missing symbol comes back null.
    NonNull::new(unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) })
}

fn ns_window(window: &Window) -> Option<NonNull<AnyObject>> {
    NonNull::new(window.ns_window() as *mut AnyObject)
}

/// The windows (by number) that have been blurred. Once a window has, it's
/// never asked for a radius of exactly 0 again: the window server can then
/// stop blurring that window for good.
static BLURRED: Mutex<Option<HashSet<isize>>> = Mutex::new(None);
/// The windows where the frosted material is standing in for the blur.
static FROSTED: Mutex<Option<HashSet<isize>>> = Mutex::new(None);

fn noted(list: &Mutex<Option<HashSet<isize>>>, window: isize) -> bool {
    list.lock().map(|l| l.as_ref().is_some_and(|l| l.contains(&window))).unwrap_or(false)
}

/// Note a window in a list, or take it off. Says whether it was there.
fn note(list: &Mutex<Option<HashSet<isize>>>, window: isize, on: bool) -> bool {
    let Ok(mut list) = list.lock() else { return false };
    let list = list.get_or_insert_with(HashSet::new);
    if on {
        !list.insert(window)
    } else {
        list.remove(&window)
    }
}

/// Blur what's behind the window by `radius` points (0 is sharp).
pub fn set_blur(window: &Window, radius: u8) {
    let radius = radius.min(BLUR_MAX) as usize;
    let Some(ns_window) = ns_window(window) else { return };
    // SAFETY: `ns_window` is this window's NSWindow; `windowNumber` takes no
    // arguments and returns an integer.
    let number: isize = unsafe { objc2::msg_send![ns_window.as_ptr(), windowNumber] };
    if server_blur(ns_window, number, radius) {
        if note(&FROSTED, number, false) {
            if let Some(view) = content_view(window) {
                let _ = clear_vibrancy(&view);
            }
        }
    } else {
        frost(window, number, radius > 0);
    }
}

fn server_blur(ns_window: NonNull<AnyObject>, number: isize, radius: usize) -> bool {
    let Some(set) = symbol(c"CGSSetWindowBackgroundBlurRadius") else { return false };
    let Some(connection) = symbol(c"CGSDefaultConnectionForThread").or_else(|| symbol(c"CGSMainConnectionID")) else { return false };
    let radius = if radius == 0 && noted(&BLURRED, number) { 1 } else { radius };
    // SAFETY: both are C functions of the window server with these
    // signatures (unchanged since they first appeared); the window is alive
    // and this is the main thread.
    let status = unsafe {
        let set: SetBlur = std::mem::transmute(set.as_ptr());
        let connection: Connection = std::mem::transmute(connection.as_ptr());
        // A window with no colour at all gets cut corners under its shadow;
        // the faintest tint avoids that.
        let clear: *mut AnyObject = objc2::msg_send![objc2::class!(NSColor), clearColor];
        let faint: *mut AnyObject = objc2::msg_send![clear, colorWithAlphaComponent: 0.01f64];
        let _: () = objc2::msg_send![ns_window.as_ptr(), setBackgroundColor: faint];
        let status = set(connection(), number as u32, radius);
        let _: () = objc2::msg_send![ns_window.as_ptr(), invalidateShadow];
        status
    };
    if status == 0 && radius > 0 {
        note(&BLURRED, number, true);
    }
    status == 0
}

/// The view currently filling the window, handed to the vibrancy library.
struct ContentView(NonNull<c_void>);

impl HasWindowHandle for ContentView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0));
        // SAFETY: the view belongs to a live window on the main thread and is
        // only used for the duration of this call.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

fn content_view(window: &Window) -> Option<ContentView> {
    let ns_window = ns_window(window)?;
    // SAFETY: `ns_window` is this window's NSWindow; `contentView` takes no
    // arguments and returns an NSView (or nil).
    let view: *mut AnyObject = unsafe { objc2::msg_send![ns_window.as_ptr(), contentView] };
    NonNull::new(view.cast::<c_void>()).map(ContentView)
}

/// The fallback: the documented frosted material under the page. The web
/// view replaces the window's first content view when it is created, so this
/// is only ever added after the page has loaded.
fn frost(window: &Window, number: isize, on: bool) {
    let Some(view) = content_view(window) else { return };
    let _ = clear_vibrancy(&view);
    note(&FROSTED, number, on);
    // Always active, so the glass doesn't turn solid grey when the window is
    // in the background.
    if on {
        if let Err(e) = apply_vibrancy(&view, NSVisualEffectMaterial::Sidebar, Some(NSVisualEffectState::Active), None) {
            eprintln!("Couldn't turn on the window glass: {e}");
        }
    }
}

/// The page's glass colours for a transparency of 0–100, as CSS variables
/// for `.app` (style.css falls back to the default when they're absent).
/// Solid at 0. In dark mode the frame keeps a fifth of its colour even at
/// its clearest, and the page card always shows less than the frame; in
/// light mode both stay more solid, or dark things behind the window would
/// turn the page grey. A tint (red, green, blue) colours the glass a little:
/// the frame most, the card least.
pub fn css_vars(glass: u8, tint: Option<(f64, f64, f64)>) -> String {
    let solid = 1.0 - f64::from(glass.min(100)) / 100.0 * 0.95;
    let (frame_d, card_d) = (0.2 + 0.8 * solid, solid.powf(1.22));
    let (frame_l, card_l) = (0.45 + 0.55 * solid, 0.7 + 0.3 * solid);
    // Things that float over the page (the tucked-away sidebar).
    let float = |frame: f64| (frame + 0.25).min(0.97);
    let rgba = |base: (f64, f64, f64), amount: f64, alpha: f64| {
        let (r, g, b) = match tint {
            Some(t) => (base.0 + (t.0 - base.0) * amount, base.1 + (t.1 - base.1) * amount, base.2 + (t.2 - base.2) * amount),
            None => base,
        };
        format!("rgba({r:.0}, {g:.0}, {b:.0}, {alpha:.3})")
    };
    format!(
        "--frame-l: {}; --frame-d: {}; --card-l: {}; --card-d: {}; --float-l: {}; --float-d: {};",
        rgba((243.0, 243.0, 244.0), 0.16, frame_l),
        rgba((14.0, 14.0, 15.0), 0.22, frame_d),
        rgba((255.0, 255.0, 255.0), 0.03, card_l),
        rgba((21.0, 21.0, 22.0), 0.06, card_d),
        rgba((247.0, 247.0, 248.0), 0.12, float(frame_l)),
        rgba((28.0, 28.0, 30.0), 0.18, float(frame_d)),
    )
}

/// Let a window show on every desktop, over full-screen apps too (the
/// developer self-test, which macOS would otherwise pause while out of view).
/// macOS does that only for the windows of an "accessory" program (one with
/// no Dock icon): a regular program's window stays on its own desktop when
/// the person works in a full-screen app. So the test copy becomes an
/// accessory program, which also keeps its icon out of the Dock.
pub fn show_over_full_screen(window: &Window, in_front: bool) {
    let ns_window = window.ns_window() as *mut AnyObject;
    if ns_window.is_null() {
        return;
    }
    // NSWindowCollectionBehaviorCanJoinAllSpaces | FullScreenAuxiliary.
    const BEHAVIOR: usize = (1 << 0) | (1 << 8);
    // NSApplicationActivationPolicyAccessory.
    const ACCESSORY: isize = 1;
    // SAFETY: `ns_window` is this window's NSWindow and `app` the shared
    // application; the messages take and return plain integers or nothing.
    unsafe {
        let app: *mut AnyObject = objc2::msg_send![objc2::class!(NSApplication), sharedApplication];
        if !app.is_null() {
            let _: bool = objc2::msg_send![app, setActivationPolicy: ACCESSORY];
        }
        // "Can go full screen itself" (1 << 7), "moves to the active desktop"
        // (1 << 1) and "managed" (1 << 2) cannot go together with these two:
        // with a clash macOS keeps none of them.
        const CLASH: usize = (1 << 7) | (1 << 1) | (1 << 2);
        let current: usize = objc2::msg_send![ns_window, collectionBehavior];
        let _: () = objc2::msg_send![ns_window, setCollectionBehavior: (current & !CLASH) | BEHAVIOR];
        if in_front {
            let _: () = objc2::msg_send![ns_window, orderFrontRegardless];
        }
    }
}

/// Show a window behind all the others, without the keyboard (the quiet
/// developer self-test).
pub fn show_behind(window: &Window) {
    let ns_window = window.ns_window() as *mut AnyObject;
    if ns_window.is_null() {
        return;
    }
    // SAFETY: `ns_window` is this window's NSWindow; the message takes the
    // sender (none) and returns nothing.
    unsafe {
        let _: () = objc2::msg_send![ns_window, orderBack: std::ptr::null::<AnyObject>()];
    }
}

/// Keep a web view drawing while its window is covered by others (the
/// developer self-test: macOS pauses a covered page, which stalled the test
/// whenever the window was not in view). Uses a WebKit switch that is not
/// part of its public interface, so it is asked for first.
pub fn draw_when_covered(webview: &dioxus::desktop::wry::WebView) {
    use dioxus::desktop::wry::WebViewExtMacOS;
    let view = webview.webview();
    let object = objc2::rc::Retained::as_ptr(&view) as *mut AnyObject;
    // SAFETY: `object` is the live WKWebView; the message is sent only if it
    // answers it, and takes one BOOL.
    unsafe {
        let switch = objc2::sel!(_setWindowOcclusionDetectionEnabled:);
        let answers: bool = objc2::msg_send![object, respondsToSelector: switch];
        if answers {
            let _: () = objc2::msg_send![object, _setWindowOcclusionDetectionEnabled: false];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_at_zero_and_clearer_as_it_rises() {
        let solid = css_vars(0, None);
        assert!(solid.contains("--frame-d: rgba(14, 14, 15, 1.000)") && solid.contains("--card-l: rgba(255, 255, 255, 1.000)"), "{solid}");
        // Aven's default: the frame about 0.63, the card under a half.
        let usual = css_vars(GLASS_DEFAULT, None);
        assert!(usual.contains("--frame-d: rgba(14, 14, 15, 0.635)") && usual.contains("--card-d: rgba(21, 21, 22, 0.476)"), "{usual}");
        // The page card always shows less than the frame, in light mode too.
        assert!(usual.contains("--frame-l: rgba(243, 243, 244, 0.749)") && usual.contains("--card-l: rgba(255, 255, 255, 0.863)"), "{usual}");
        // Never fully clear, and over the top is the same as the top.
        assert!(css_vars(100, None).contains("--frame-d: rgba(14, 14, 15, 0.240)"));
        assert_eq!(css_vars(200, None), css_vars(100, None));
    }

    #[test]
    fn a_tint_colours_the_glass_but_not_how_clear_it_is() {
        let plain = css_vars(GLASS_DEFAULT, None);
        let blue = css_vars(GLASS_DEFAULT, Some((58.0, 125.0, 210.0)));
        assert_ne!(plain, blue);
        // Same alphas, in the same order.
        let alphas = |s: &str| s.split(';').filter_map(|v| v.rsplit(", ").next().map(str::to_string)).collect::<Vec<_>>();
        assert_eq!(alphas(&plain), alphas(&blue));
        assert!(blue.contains("--frame-d: rgba(24, 38, 58, 0.635)"), "{blue}");
    }
}
