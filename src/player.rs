//! A YouTube video played inside the app: a small player in a corner of the
//! window (see `PlayerDock` in ui/mod.rs for the frame around it).
//!
//! The pages of the app's own web view cannot hold YouTube's player (the
//! framework hands every web address to the browser, a frame's too). So the
//! player is a second web view, a child of the window, put over the frame
//! that the page draws for it. That second view:
//!
//! - shows one small page, served to it from this Mac only (127.0.0.1): the
//!   page holds YouTube's player. YouTube refuses a page with no web address
//!   of its own ("error 153"), which is why the page is served and not given
//!   as text.
//! - cannot talk to the app. The only words it can send are a few fixed
//!   ones ("ready", "error:150"); anything else is dropped. Video sites run
//!   other people's code, and that code must have no way into the notes.
//! - goes nowhere else: a link that is clicked in it opens in the browser.

use crate::video::Video;
use dioxus::desktop::wry::dpi::{LogicalPosition, LogicalSize};
use dioxus::desktop::wry::{NewWindowResponse, Rect, WebView, WebViewBuilder};
use raw_window_handle::HasWindowHandle;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;

/// Where in the window the player goes (points from the top left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Place {
    fn rect(self) -> Rect {
        Rect { position: LogicalPosition::new(self.x, self.y).into(), size: LogicalSize::new(self.w.max(1.0), self.h.max(1.0)).into() }
    }
}

thread_local! {
    /// The player on screen, with the video it plays ("id@start").
    static PLAYER: RefCell<Option<(String, WebView)>> = const { RefCell::new(None) };
}

/// The page that holds YouTube's player for one video. It tells the app
/// when the player is ready and when it cannot play (see `words`).
pub fn page(video: &Video) -> String {
    // A test run that plays a real video plays it without sound.
    let mute = u8::from(std::env::var_os("NOTION2_SELFTEST").is_some());
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="referrer" content="strict-origin-when-cross-origin">
<title>Video</title>
<style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}#p{{position:absolute;inset:0;width:100%;height:100%;border:0}}</style>
</head><body><div id="p"></div>
<script>
function say(m){{try{{window.ipc.postMessage(m)}}catch(e){{}}}}
function onYouTubeIframeAPIReady(){{
  new YT.Player('p',{{videoId:'{id}',width:'100%',height:'100%',
    playerVars:{{autoplay:1,start:{start},playsinline:1,rel:0,mute:{mute}}},
    events:{{onReady:function(e){{say('ready');try{{e.target.playVideo()}}catch(x){{}}}},onError:function(e){{say('error:'+e.data)}}}}}});
}}
var s=document.createElement('script');s.src='https://www.youtube.com/iframe_api';s.onerror=function(){{say('error:offline')}};document.head.appendChild(s);
</script></body></html>"#,
        id = video.id,
        start = video.start,
        mute = mute
    )
}

/// What a message from the player's page means, when it is one of the few
/// it may send: `Some(None)` for "ready", `Some(Some(reason))` for a video
/// that cannot play. Anything else is not from our page, or not for us.
pub fn words(message: &str) -> Option<Option<&'static str>> {
    match message {
        "ready" => Some(None),
        "error:101" | "error:150" => Some(Some("The owner of this video does not let it play outside YouTube.")),
        "error:100" => Some(Some("This video is not on YouTube any more, or it is private.")),
        "error:153" => Some(Some("YouTube did not accept this player.")),
        "error:offline" => Some(Some("YouTube could not be reached.")),
        m if m.starts_with("error:") && m.len() <= 12 => Some(Some("This video did not play.")),
        _ => None,
    }
}

/// The port the player's page is served on, on this Mac only. The server
/// starts the first time a video is played.
fn port() -> Option<u16> {
    static PORT: OnceLock<Option<u16>> = OnceLock::new();
    *PORT.get_or_init(|| {
        let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
        let port = listener.local_addr().ok()?.port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answer(stream, port);
            }
        });
        Some(port)
    })
}

/// The video a request asks for: only `GET /p?v=<id>&t=<seconds>`, asked of
/// this Mac by its own name (a page of another site cannot pass that).
pub fn asked_for(request: &str, port: u16) -> Option<Video> {
    let mut lines = request.lines();
    let first = lines.next()?;
    let path = first.strip_prefix("GET ")?.split(' ').next()?;
    let host_ok = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .is_some_and(|(_, host)| host.trim() == format!("127.0.0.1:{port}"));
    let query = path.strip_prefix("/p?")?;
    let param = |name: &str| query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v);
    let id = param("v")?;
    let start = param("t").and_then(|t| t.parse::<u32>().ok()).unwrap_or(0);
    // The id goes into the page as it is: only what a YouTube id can hold.
    let safe = (6..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    (host_ok && safe).then(|| Video { id: id.to_string(), start })
}

fn answer(mut stream: TcpStream, port: u16) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut buffer = [0u8; 2048];
    let n = stream.read(&mut buffer).unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..n]);
    let reply = match asked_for(&request, port) {
        Some(video) => {
            let body = page(&video);
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
    };
    let _ = stream.write_all(reply.as_bytes());
}

/// Where the player's view may go: its own page, and the sites YouTube's
/// player is made of. Everything else stays out of the app.
fn may_load(url: &str, port: u16) -> bool {
    if url == "about:blank" || url.starts_with(&format!("http://127.0.0.1:{port}/p?")) {
        return true;
    }
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("").to_ascii_lowercase();
    ["youtube.com", "youtube-nocookie.com", "google.com", "googlevideo.com", "ytimg.com", "ggpht.com", "gstatic.com", "googleapis.com", "doubleclick.net", "googlesyndication.com"]
        .iter()
        .any(|site| host == *site || host.ends_with(&format!(".{site}")))
}

/// Put the player of `video` over this part of the window: made the first
/// time (or for another video), moved after that. `heard` gets what the
/// player says of itself, in the words of `words`.
pub fn show(window: &impl HasWindowHandle, video: &Video, place: Place, heard: impl Fn(Option<&'static str>) + 'static) -> Result<(), String> {
    let key = format!("{}@{}", video.id, video.start);
    let moved = PLAYER.with(|p| match p.borrow().as_ref() {
        Some((playing, view)) if *playing == key => {
            let _ = view.set_bounds(place.rect());
            true
        }
        _ => false,
    });
    if moved {
        return Ok(());
    }
    hide();
    let port = port().ok_or("The player could not start on this Mac.")?;
    let view = WebViewBuilder::new()
        .with_url(format!("http://127.0.0.1:{port}/p?v={}&t={}", video.id, video.start))
        .with_bounds(place.rect())
        .with_background_color((0, 0, 0, 255))
        .with_autoplay(true)
        .with_focused(false)
        .with_navigation_handler(move |url| may_load(&url, port))
        // A link clicked in the player (the YouTube mark, a channel) opens in the browser.
        .with_new_window_req_handler(|url, _| {
            if url.starts_with("https://") {
                let _ = std::process::Command::new("open").arg(&url).spawn();
            }
            NewWindowResponse::Deny
        })
        .with_ipc_handler(move |request| {
            if let Some(said) = words(request.body()) {
                heard(said);
            }
        })
        .build_as_child(window)
        .map_err(|e| format!("The player could not open. ({e})"))?;
    PLAYER.with(|p| *p.borrow_mut() = Some((key, view)));
    Ok(())
}

/// Take the player off the screen (the video stops).
pub fn hide() {
    PLAYER.with(|p| p.borrow_mut().take());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_page_is_served_only_for_a_plain_request_from_this_mac() {
        let ask = |first: &str, host: &str| asked_for(&format!("{first}\r\nHost: {host}\r\nAccept: */*\r\n\r\n"), 5000);
        assert_eq!(ask("GET /p?v=jNQXAC9IVRw&t=42 HTTP/1.1", "127.0.0.1:5000"), Some(Video { id: "jNQXAC9IVRw".into(), start: 42 }));
        assert_eq!(ask("GET /p?v=jNQXAC9IVRw HTTP/1.1", "127.0.0.1:5000").map(|v| v.start), Some(0));
        // Another name for this Mac, another path, another verb, an id that is not one.
        assert_eq!(ask("GET /p?v=jNQXAC9IVRw HTTP/1.1", "evil.example:5000"), None);
        assert_eq!(ask("GET /p?v=jNQXAC9IVRw HTTP/1.1", "localhost:5000"), None);
        assert_eq!(ask("GET /other?v=jNQXAC9IVRw HTTP/1.1", "127.0.0.1:5000"), None);
        assert_eq!(ask("POST /p?v=jNQXAC9IVRw HTTP/1.1", "127.0.0.1:5000"), None);
        assert_eq!(ask("GET /p?v=%27%3Balert(1)// HTTP/1.1", "127.0.0.1:5000"), None);
        assert_eq!(ask("GET /p?v=a HTTP/1.1", "127.0.0.1:5000"), None);
        // The page holds the id and the start, and nothing else from the request.
        let html = page(&Video { id: "jNQXAC9IVRw".into(), start: 42 });
        assert!(html.contains("videoId:'jNQXAC9IVRw'") && html.contains("start:42") && html.contains("youtube.com/iframe_api"));
    }

    #[test]
    fn the_player_says_only_a_few_fixed_things_and_goes_only_to_youtube() {
        assert_eq!(words("ready"), Some(None));
        assert!(words("error:150").unwrap().unwrap().contains("owner"));
        assert!(words("error:5").unwrap().is_some());
        // Not ours: dropped.
        assert_eq!(words("{\"method\":\"user_event\"}"), None);
        assert_eq!(words("error:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), None);
        assert_eq!(words(""), None);
        assert!(may_load("http://127.0.0.1:5000/p?v=jNQXAC9IVRw&t=0", 5000));
        assert!(may_load("https://www.youtube.com/embed/jNQXAC9IVRw?autoplay=1", 5000));
        assert!(may_load("https://rr2---sn-abc.googlevideo.com/videoplayback?x=1", 5000));
        assert!(!may_load("http://127.0.0.1:5001/p?v=jNQXAC9IVRw", 5000));
        assert!(!may_load("https://example.com/", 5000));
        assert!(!may_load("https://youtube.com.evil.example/", 5000));
        assert!(!may_load("http://www.youtube.com/", 5000));
        assert!(!may_load("file:///etc/passwd", 5000));
        assert!(!may_load("notion2://page/abc", 5000));
    }
}
