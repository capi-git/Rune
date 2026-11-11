//! YouTube links shown as a video in the page.
//!
//! A line that is only a YouTube link gets the video under it: its still
//! picture with a play button. A click plays the video in a small player in
//! the corner of the window (see player.rs for why it is not in the page).

use crate::inline;
use crate::model::BlockKind;

/// A YouTube video and where in it to begin (seconds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Video {
    pub id: String,
    pub start: u32,
}

/// A video id as YouTube writes them: letters, digits, `-` and `_`.
fn valid_id(id: &str) -> bool {
    (6..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// "90", "90s", "1m30s", "1h2m3s" as seconds.
fn seconds(text: &str) -> u32 {
    let mut total = 0u32;
    let mut number = 0u32;
    let mut any_unit = false;
    for c in text.chars() {
        match c {
            '0'..='9' => number = number.saturating_mul(10).saturating_add(c as u32 - '0' as u32),
            'h' | 'm' | 's' => {
                let each = match c {
                    'h' => 3600,
                    'm' => 60,
                    _ => 1,
                };
                total = total.saturating_add(number.saturating_mul(each));
                number = 0;
                any_unit = true;
            }
            _ => return 0,
        }
    }
    if any_unit { total.saturating_add(number) } else { number }
}

/// The video a YouTube address is for, if it is one: `youtube.com/watch?v=…`,
/// `youtu.be/…`, and the `/shorts/`, `/embed/`, `/live/` forms, with `t=` or
/// `start=` for where to begin.
pub fn youtube(url: &str) -> Option<Video> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    // The same part of the address as written (ids tell capitals from small letters).
    let rest = &url[url.len() - rest.len()..];
    let (host, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let host = ["www.", "m.", "music."].iter().find_map(|p| host.strip_prefix(p)).unwrap_or(&host);
    let tail = tail.split('#').next().unwrap_or("");
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let param = |name: &str| query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v);
    let id = match host {
        "youtu.be" => path.split('/').next()?.to_string(),
        "youtube.com" | "youtube-nocookie.com" => {
            let mut parts = path.split('/');
            match parts.next()? {
                "watch" => param("v")?.to_string(),
                "shorts" | "embed" | "live" | "v" => parts.next()?.to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };
    let start = param("t").or_else(|| param("start")).map(seconds).unwrap_or(0);
    valid_id(&id).then_some(Video { id, start })
}

/// The video to show under a line: when the line is only a YouTube link (the
/// address itself, or words that are all that one link). A link inside a
/// sentence stays a link.
/// What a line of text keeps (in the place where a drawing keeps its
/// scene) when the person put the picture of its video away.
pub const HIDDEN: &str = "no-video";

pub fn in_block(kind: BlockKind, text: &str) -> Option<Video> {
    if !matches!(kind, BlockKind::Text | BlockKind::Bulleted | BlockKind::Numbered | BlockKind::Todo) || !text.contains("youtu") {
        return None;
    }
    let runs = inline::parse(text);
    let shown: String = runs.iter().map(|r| r.text.as_str()).collect();
    let shown = shown.trim();
    let mut links = runs.iter().filter(|r| !r.text.trim().is_empty()).map(|r| r.link.as_deref());
    let url = match links.next()? {
        Some(url) if links.all(|l| l == Some(url)) => url.to_string(),
        None if !shown.contains(char::is_whitespace) && runs.iter().all(|r| r.link.is_none()) => shown.to_string(),
        _ => return None,
    };
    youtube(&url)
}

/// A test run shows no still picture and no player of YouTube's (nothing is
/// fetched), unless it asks for the real thing (`NOTION2_REAL_VIDEO`).
pub fn stand_in() -> bool {
    std::env::var_os("NOTION2_SELFTEST").is_some() && std::env::var_os("NOTION2_REAL_VIDEO").is_none()
}

/// The still picture of a video (from YouTube). Empty in a test run.
pub fn thumbnail(id: &str) -> String {
    if stand_in() || !valid_id(id) { String::new() } else { format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg") }
}

/// The address of the video on YouTube's own site.
pub fn watch_url(video: &Video) -> String {
    if video.start > 0 { format!("https://www.youtube.com/watch?v={}&t={}s", video.id, video.start) } else { format!("https://www.youtube.com/watch?v={}", video.id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, start: u32) -> Option<Video> {
        Some(Video { id: id.into(), start })
    }

    #[test]
    fn youtube_addresses_are_known_in_all_their_forms() {
        assert_eq!(youtube("https://www.youtube.com/watch?v=jNQXAC9IVRw"), v("jNQXAC9IVRw", 0));
        assert_eq!(youtube("https://youtube.com/watch?feature=share&v=jNQXAC9IVRw&t=1m30s"), v("jNQXAC9IVRw", 90));
        assert_eq!(youtube("  https://youtu.be/jNQXAC9IVRw?t=42 "), v("jNQXAC9IVRw", 42));
        assert_eq!(youtube("http://m.youtube.com/shorts/aB3-_dE9xYz"), v("aB3-_dE9xYz", 0));
        assert_eq!(youtube("https://www.youtube.com/embed/jNQXAC9IVRw?start=7"), v("jNQXAC9IVRw", 7));
        assert_eq!(youtube("https://www.youtube.com/live/jNQXAC9IVRw#chat"), v("jNQXAC9IVRw", 0));
        assert_eq!(youtube("HTTPS://WWW.YouTube.com/watch?v=jNQXAC9IVRw"), v("jNQXAC9IVRw", 0));
        assert_eq!(youtube("https://music.youtube.com/watch?v=jNQXAC9IVRw&t=1h2m3s"), v("jNQXAC9IVRw", 3723));
        // Not videos, or not YouTube.
        for other in [
            "https://www.youtube.com/",
            "https://www.youtube.com/@somebody",
            "https://www.youtube.com/watch?list=PL123",
            "https://www.youtube.com/watch?v=<script>",
            "https://vimeo.com/12345678",
            "https://notyoutube.com/watch?v=jNQXAC9IVRw",
            "youtube.com/watch?v=jNQXAC9IVRw",
            "ftp://youtu.be/jNQXAC9IVRw",
        ] {
            assert_eq!(youtube(other), None, "{other}");
        }
        assert_eq!((seconds("90"), seconds("90s"), seconds("2m"), seconds("soon")), (90, 90, 120, 0));
    }

    #[test]
    fn a_line_that_is_only_a_youtube_link_shows_its_video() {
        let text = BlockKind::Text;
        assert_eq!(in_block(text, "https://youtu.be/jNQXAC9IVRw"), v("jNQXAC9IVRw", 0));
        assert_eq!(in_block(text, "  https://www.youtube.com/watch?v=jNQXAC9IVRw&t=5  "), v("jNQXAC9IVRw", 5));
        // Words that are all the one link.
        assert_eq!(in_block(text, "[The first video](https://youtu.be/jNQXAC9IVRw)"), v("jNQXAC9IVRw", 0));
        assert_eq!(in_block(BlockKind::Bulleted, "[**Watch** this](https://youtu.be/jNQXAC9IVRw)"), v("jNQXAC9IVRw", 0));
        // An id with an underscore is kept escaped in the line.
        assert_eq!(in_block(text, &crate::inline::escape("https://youtu.be/aB3-_dE9xYz")), v("aB3-_dE9xYz", 0));
        // A link inside a sentence stays a link; so do two links, and other kinds of line.
        assert_eq!(in_block(text, "See [this](https://youtu.be/jNQXAC9IVRw) later"), None);
        assert_eq!(in_block(text, "Watch https://youtu.be/jNQXAC9IVRw"), None);
        assert_eq!(in_block(text, "[a](https://youtu.be/jNQXAC9IVRw) [b](https://youtu.be/aB3-_dE9xYz)"), None);
        assert_eq!(in_block(BlockKind::Code, "https://youtu.be/jNQXAC9IVRw"), None);
        assert_eq!(in_block(BlockKind::Heading1, "https://youtu.be/jNQXAC9IVRw"), None);
        assert_eq!(in_block(text, "[site](https://example.com/youtu)"), None);
        assert_eq!(in_block(text, ""), None);
    }

    #[test]
    fn a_video_s_own_address() {
        assert_eq!(watch_url(&Video { id: "jNQXAC9IVRw".into(), start: 90 }), "https://www.youtube.com/watch?v=jNQXAC9IVRw&t=90s");
        assert_eq!(watch_url(&Video { id: "jNQXAC9IVRw".into(), start: 0 }), "https://www.youtube.com/watch?v=jNQXAC9IVRw");
    }
}
