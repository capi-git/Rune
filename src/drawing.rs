//! Drawing blocks: Excalidraw, running in a frame of its own inside the page.
//!
//! The frame's files are built into the app and served from the `n2draw://`
//! address, so drawing works offline. Each drawing's scene is Excalidraw's own
//! JSON, kept in the block's `data` (never in its text, so search, AI context
//! and Markdown export don't see it).
//!
//! The frame saves as you draw. A scene is only sent back to a window's frames
//! when it changed somewhere else (another window, an agent), so the frame
//! you're drawing in never has its own work pushed back at it.

use dioxus::desktop::wry::http::{header, Request, Response, StatusCode};
use dioxus::desktop::wry::WebViewId;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// The address the frames load from.
pub const SCHEME: &str = "n2draw";

macro_rules! file {
    ($path:literal) => {
        ($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/excalidraw/", $path)) as &[u8])
    };
}

static FILES: &[(&str, &[u8])] = &[
    file!("index.html"),
    file!("frame.js"),
    file!("vendor/react.production.min.js"),
    file!("vendor/react-dom.production.min.js"),
    file!("vendor/excalidraw.production.min.js"),
    file!("vendor/excalidraw-assets/vendor-677e88ca78c86bddf13d.js"),
    file!("vendor/excalidraw-assets/Virgil.woff2"),
    file!("vendor/excalidraw-assets/Cascadia.woff2"),
    file!("vendor/excalidraw-assets/Assistant-Regular.woff2"),
    file!("vendor/excalidraw-assets/Assistant-Medium.woff2"),
    file!("vendor/excalidraw-assets/Assistant-SemiBold.woff2"),
    file!("vendor/excalidraw-assets/Assistant-Bold.woff2"),
];

/// The frame's address for one drawing.
pub fn frame_url(block_id: &str) -> String {
    let test = if std::env::var_os("NOTION2_SELFTEST").is_some() { "&test=1" } else { "" };
    format!("{SCHEME}://app/index.html?id={block_id}{test}")
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Answer a request from a drawing frame.
pub fn serve(_: WebViewId, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match FILES.iter().find(|(p, _)| *p == path) {
        Some((p, bytes)) => Response::builder()
            .header(header::CONTENT_TYPE, content_type(p))
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
            .body(Cow::Borrowed(*bytes))
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    let mut r = Response::new(Cow::Borrowed(&b"not found"[..]));
    *r.status_mut() = StatusCode::NOT_FOUND;
    r
}

thread_local! {
    /// What each window's frames already show, by (window, block): a hash of the scene.
    static SHOWN: RefCell<HashMap<(u64, String), u64>> = RefCell::new(HashMap::new());
}

fn hash(data: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

/// The window's frame just saved this scene, so it already shows it.
pub fn mark_shown(window: u64, block_id: &str, data: &str) {
    SHOWN.with(|s| s.borrow_mut().insert((window, block_id.to_string()), hash(data)));
}

/// Does the window need this scene sent to its frames? (Notes it as sent.)
pub fn needs_sending(window: u64, block_id: &str, data: &str) -> bool {
    let h = hash(data);
    SHOWN.with(|s| s.borrow_mut().insert((window, block_id.to_string()), h) != Some(h))
}

/// A window closed: nothing of its is shown any more.
pub fn forget_window(window: u64) {
    SHOWN.with(|s| s.borrow_mut().retain(|(w, _), _| *w != window));
}

/// JavaScript that hands a scene to the window's frames for this drawing.
pub fn load_script(block_id: &str, data: &str) -> String {
    let id = serde_json::to_string(block_id).unwrap_or_default();
    let json = serde_json::to_string(data).unwrap_or_default();
    format!(
        "window.__n2 ? window.__n2.drawingLoad({id}, {json}) : ((window.__n2_scenes = window.__n2_scenes || {{}})[{id}] = {json})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_every_listed_file() {
        for (path, bytes) in FILES {
            assert!(!bytes.is_empty(), "{path} is empty");
            let r = serve("w", Request::get(format!("{SCHEME}://app/{path}")).body(Vec::new()).unwrap());
            assert_eq!(r.status(), StatusCode::OK, "{path}");
        }
        let missing = serve("w", Request::get(format!("{SCHEME}://app/nope.js")).body(Vec::new()).unwrap());
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn a_scene_is_sent_once_per_window() {
        assert!(needs_sending(1, "b", "{}"));
        assert!(!needs_sending(1, "b", "{}"));
        assert!(needs_sending(2, "b", "{}"));
        mark_shown(1, "b", "{\"x\":1}");
        assert!(!needs_sending(1, "b", "{\"x\":1}"));
        assert!(needs_sending(1, "b", "{\"x\":2}"));
    }
}
