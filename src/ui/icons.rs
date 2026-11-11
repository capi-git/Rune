//! The app's own icon set: original line icons drawn on a 24×24 grid with a
//! 1.75 stroke, plus the palette of page and callout icons and their colours.
//!
//! Page icons are stored as `"name"` or `"name:color"` (for example
//! `"rocket:purple"`). Older workspaces stored emoji; [`parse_icon`] maps those
//! to the closest icon so nothing shows up blank.

use dioxus::prelude::*;

/// Icons used by the interface itself (checked by the tests).
#[cfg_attr(not(test), allow(dead_code))]
pub const UI_ICONS: &[&str] = &[
    "search",
    "home",
    "sparkles",
    "settings",
    "trash",
    "plus",
    "chevron-right",
    "chevron-left",
    "chevron-down",
    "chevron-up",
    "more-horizontal",
    "star",
    "star-filled",
    "link",
    "duplicate",
    "file",
    "sidebar",
    "close",
    "grip",
    "clock",
    "history",
    "mic",
    "speaker",
    "repeat",
    "check",
    "triangle-right",
    "text",
    "heading-1",
    "heading-2",
    "heading-3",
    "list-bullet",
    "list-numbered",
    "checkbox",
    "toggle-list",
    "quote",
    "callout",
    "code",
    "divider",
    "bold",
    "italic",
    "strikethrough",
    "inline-code",
    "undo",
    "redo",
    "minus",
    "arrow-up-right",
    "hash",
    "gauge",
    "choice",
    "chart-bars",
    "ring",
    "days",
    "sliders",
];

/// Icons offered for pages and callouts, in picker order.
pub const PAGE_ICONS: &[&str] = &[
    "file",
    "note",
    "book",
    "bookmark",
    "folder",
    "calendar",
    "check-circle",
    "lightbulb",
    "rocket",
    "target",
    "star",
    "flame",
    "leaf",
    "briefcase",
    "home",
    "plane",
    "palette",
    "music",
    "flask",
    "wrench",
    "chart",
    "chat",
    "coffee",
    "globe",
    "lock",
    "heart",
    "smile",
    "bot",
    "code",
    "warning",
    "info",
    "pin",
    "bell",
    "inbox",
    "map-pin",
    "camera",
    "gift",
    "trophy",
    "sparkles",
];

/// Icon colours: (id, label). The CSS defines `--icon-<id>` and `--icon-<id>-soft`.
pub const ICON_COLORS: &[(&str, &str)] = &[
    ("gray", "Gray"),
    ("brown", "Brown"),
    ("orange", "Orange"),
    ("yellow", "Yellow"),
    ("green", "Green"),
    ("blue", "Blue"),
    ("purple", "Purple"),
    ("pink", "Pink"),
    ("red", "Red"),
];

/// Colour used when a stored icon has none.
pub const DEFAULT_COLOR: &str = "gray";

/// Emoji that earlier versions stored as page or callout icons.
const LEGACY_EMOJI: &[(&str, &str, &str)] = &[
    ("👋", "smile", "yellow"),
    ("💡", "lightbulb", "yellow"),
    ("📚", "book", "blue"),
    ("📄", "file", "gray"),
    ("📝", "note", "gray"),
    ("📌", "pin", "red"),
    ("📎", "link", "gray"),
    ("🗂", "folder", "blue"),
    ("🗓", "calendar", "red"),
    ("✅", "check-circle", "green"),
    ("🚀", "rocket", "purple"),
    ("🎯", "target", "red"),
    ("⭐", "star", "yellow"),
    ("🔥", "flame", "orange"),
    ("🌱", "leaf", "green"),
    ("🧠", "sparkles", "pink"),
    ("💼", "briefcase", "brown"),
    ("🏠", "home", "blue"),
    ("✈", "plane", "blue"),
    ("🎨", "palette", "pink"),
    ("🎵", "music", "purple"),
    ("🧪", "flask", "green"),
    ("🛠", "wrench", "gray"),
    ("📈", "chart", "green"),
    ("💬", "chat", "blue"),
    ("🍳", "coffee", "brown"),
    ("🏃", "rocket", "orange"),
    ("🌍", "globe", "blue"),
    ("🔒", "lock", "gray"),
    ("❤", "heart", "red"),
    ("😀", "smile", "yellow"),
    ("🤖", "bot", "purple"),
    ("🦀", "code", "orange"),
    ("✨", "sparkles", "purple"),
    ("⚠", "warning", "orange"),
    ("❗", "warning", "red"),
    ("ℹ", "info", "blue"),
    ("🔍", "search", "gray"),
    ("⚙", "settings", "gray"),
    ("🗑", "trash", "gray"),
    ("🔗", "link", "blue"),
    ("📖", "book", "brown"),
    ("🏆", "trophy", "yellow"),
    ("🎁", "gift", "red"),
    ("📷", "camera", "gray"),
    ("🔔", "bell", "yellow"),
    ("📥", "inbox", "blue"),
    ("📍", "map-pin", "red"),
    ("🔖", "bookmark", "red"),
    ("📁", "folder", "blue"),
    ("☕", "coffee", "brown"),
];

/// Inner SVG markup for an icon, or `None` if the name is unknown.
pub fn paths(name: &str) -> Option<&'static str> {
    Some(match name {
        "bell" => {
            r#"<path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 1.75h-15z"/><path d="M10 20.75c.45.5 1.15.75 2 .75s1.55-.25 2-.75"/>"#
        }
        "bold" => {
            r#"<path d="M7.25 5.25h5.5a3.38 3.38 0 0 1 0 6.75h-5.5zM7.25 12h6.5a3.38 3.38 0 0 1 0 6.75h-6.5z" stroke-width="2.25"/>"#
        }
        "book" => {
            r#"<path d="M19 16.5V4H7.25C6.01 4 5 5.01 5 6.25v12.5"/><path d="M19 16.5H7.25C6.01 16.5 5 17.51 5 18.75S6.01 21 7.25 21H19z"/><path d="M9 8h6"/>"#
        }
        "bookmark" => {
            r#"<path d="M7 5.25C7 4.56 7.56 4 8.25 4h7.5c.69 0 1.25.56 1.25 1.25V20l-5-3.75L7 20z"/>"#
        }
        "bot" => {
            r#"<rect x="4.5" y="8" width="15" height="11" rx="3"/><path d="M12 8V5.25M9.5 12.5v1.5M14.5 12.5v1.5M2.5 12.5v2.5M21.5 12.5v2.5"/><circle cx="12" cy="4.25" r="1.25" fill="currentColor" stroke="none"/>"#
        }
        "briefcase" => {
            r#"<rect x="3.5" y="7.5" width="17" height="12" rx="2"/><path d="M9 7.5V5.75c0-.41.34-.75.75-.75h4.5c.41 0 .75.34.75.75V7.5M3.5 12.5h17"/>"#
        }
        "calendar" => {
            r#"<rect x="4" y="5.5" width="16" height="14.5" rx="2.25"/><path d="M4 10h16M8.5 3.5v4M15.5 3.5v4"/><circle cx="8.5" cy="14" r="1.15" fill="currentColor" stroke="none"/><circle cx="12" cy="14" r="1.15" fill="currentColor" stroke="none"/>"#
        }
        "callout" => {
            r#"<rect x="3.5" y="5" width="17" height="14" rx="2.5"/><circle cx="8.25" cy="12" r="1.75"/><path d="M12.25 10.5h5M12.25 13.5h3.5"/>"#
        }
        "graph" => {
            r#"<circle cx="6.25" cy="7" r="2.25"/><circle cx="17.75" cy="8.25" r="2.25"/><circle cx="11" cy="17.5" r="2.25"/><path d="M8.5 7.25l7 .75M7.2 9.05l2.9 6.4M16.4 10.05l-4.1 5.65"/>"#
        }
        "image" => {
            r#"<rect x="3.75" y="4.75" width="16.5" height="14.5" rx="2"/><circle cx="8.75" cy="9.5" r="1.5"/><path d="M4.5 17.25l4.75-4.75 3.5 3.25 2.75-2.5 4.25 4"/>"#
        }
        "table" => {
            r#"<rect x="3.75" y="4.75" width="16.5" height="14.5" rx="2"/><path d="M3.75 9.75h16.5M3.75 14.5h16.5M9.5 4.75v14.5"/>"#
        }
        "camera" => {
            r#"<path d="M4 8.75C4 7.78 4.78 7 5.75 7h2.1l1.4-2h5.5l1.4 2h2.1c.97 0 1.75.78 1.75 1.75v9.5c0 .97-.78 1.75-1.75 1.75H5.75C4.78 20 4 19.22 4 18.25z"/><circle cx="12" cy="13" r="3.5"/>"#
        }
        "chart" => {
            r#"<path d="M4 4v15.25c0 .41.34.75.75.75H20"/><path d="m7.5 15 3.5-4 3 2.5 5-6.5"/>"#
        }
        "chat" => {
            r#"<path d="M5.75 4.5h12.5c.97 0 1.75.78 1.75 1.75v8.5c0 .97-.78 1.75-1.75 1.75H10.5L6.25 20v-3.5h-.5c-.97 0-1.75-.78-1.75-1.75v-8.5c0-.97.78-1.75 1.75-1.75z"/>"#
        }
        "check" => r#"<path d="m5 12.5 4.5 4.5L19 7.5"/>"#,
        "hash" => r#"<path d="M9.5 4 7.5 20M16.5 4l-2 16M4.5 9h15.5M4 15h15.5"/>"#,
        "gauge" => r#"<path d="M4.5 17a7.5 7.5 0 1 1 15 0"/><path d="m12 17 3.75-4.75"/>"#,
        "choice" => r#"<rect x="3.5" y="7.5" width="17" height="9" rx="4.5"/><path d="M8 12h5"/>"#,
        "chart-bars" => r#"<path d="M4 20h16"/><path d="M7 16.5V12M12 16.5V6.5M17 16.5V9.5"/>"#,
        "ring" => {
            r#"<circle cx="12" cy="12" r="7.75" stroke-opacity=".4"/><path d="M12 4.25A7.75 7.75 0 0 1 19.75 12" stroke-width="2.5"/>"#
        }
        "days" => {
            r#"<rect x="4" y="4" width="4.5" height="4.5" rx="1"/><rect x="9.75" y="4" width="4.5" height="4.5" rx="1" fill="currentColor"/><rect x="15.5" y="4" width="4.5" height="4.5" rx="1"/><rect x="4" y="9.75" width="4.5" height="4.5" rx="1" fill="currentColor"/><rect x="9.75" y="9.75" width="4.5" height="4.5" rx="1"/><rect x="15.5" y="9.75" width="4.5" height="4.5" rx="1" fill="currentColor"/><rect x="4" y="15.5" width="4.5" height="4.5" rx="1"/><rect x="9.75" y="15.5" width="4.5" height="4.5" rx="1" fill="currentColor"/><rect x="15.5" y="15.5" width="4.5" height="4.5" rx="1"/>"#
        }
        "sliders" => r#"<path d="M4 7.5h9M17 7.5h3M4 16.5h3M11 16.5h9"/><circle cx="15" cy="7.5" r="2"/><circle cx="9" cy="16.5" r="2"/>"#,
        "check-circle" => {
            r#"<circle cx="12" cy="12" r="8.5"/><path d="m8.25 12.25 2.5 2.5 5-5.25"/>"#
        }
        "checkbox" => {
            r#"<rect x="4" y="4" width="16" height="16" rx="3.5"/><path d="m8.25 12.25 2.5 2.5 5-5.25"/>"#
        }
        "chevron-down" => r#"<path d="m6 9.5 6 6 6-6"/>"#,
        "chevron-up" => r#"<path d="m6 14.5 6-6 6 6"/>"#,
        "chevron-left" => r#"<path d="m14.5 6-6 6 6 6"/>"#,
        "chevron-right" => r#"<path d="m9.5 6 6 6-6 6"/>"#,
        "clock" => r#"<circle cx="12" cy="12" r="8.25"/><path d="M12 7.5V12l3 2"/>"#,
        "repeat" => r#"<path d="m16.5 3.5 3 3-3 3"/><path d="M4.5 11.5v-1a4 4 0 0 1 4-4h11"/><path d="m7.5 20.5-3-3 3-3"/><path d="M19.5 12.5v1a4 4 0 0 1-4 4h-11"/>"#,
        "speaker" => r#"<path d="M4.5 9.75v4.5h3.25l4.25 3.5V6.25l-4.25 3.5H4.5z"/><path d="M15.25 9.25a4 4 0 0 1 0 5.5"/><path d="M17.75 6.75a7.5 7.5 0 0 1 0 10.5"/>"#,
        "minus" => r#"<path d="M6.5 12h11"/>"#,
        // Tables: kinds of column, views, and what their menus do.
        "choices" => r#"<rect x="3.5" y="4.5" width="13" height="6.5" rx="3.25"/><rect x="7.5" y="13" width="13" height="6.5" rx="3.25"/>"#,
        "mail" => r#"<rect x="3.5" y="5.5" width="17" height="13" rx="2"/><path d="m4 7 8 6 8-6"/>"#,
        "phone" => r#"<rect x="7" y="3" width="10" height="18" rx="2"/><path d="M11 17.5h2"/>"#,
        "paperclip" => r#"<path d="m20 11.5-8.2 8.2a5 5 0 0 1-7-7L13 4.5a3.3 3.3 0 0 1 4.7 4.7L9.5 17.4a1.7 1.7 0 0 1-2.4-2.4l7.5-7.5"/>"#,
        "pie" => r#"<path d="M12 3.75a8.25 8.25 0 1 0 8.25 8.25H12z"/><path d="M15 3.5A6.5 6.5 0 0 1 20.5 9H15z"/>"#,
        "status" => r#"<path d="M12 3.75a8.25 8.25 0 1 1-5.83 2.42"/><circle cx="12" cy="12" r="3"/>"#,
        "sigma" => r#"<path d="M17 5H7l6 7-6 7h10"/>"#,
        "money" => r#"<path d="M12 3v18M16.5 7.5c-.6-1.5-2.3-2.3-4.5-2.3-2.5 0-4.3 1.2-4.3 3.2 0 4.4 9 2.2 9 6.8 0 2-1.9 3.3-4.6 3.3-2.4 0-4.2-1-4.8-2.7"/>"#,
        "board" => r#"<rect x="4" y="5" width="4.5" height="14" rx="1"/><rect x="9.8" y="5" width="4.5" height="9" rx="1"/><rect x="15.5" y="5" width="4.5" height="12" rx="1"/>"#,
        "gallery" => r#"<rect x="4" y="5" width="7" height="6" rx="1"/><rect x="13" y="5" width="7" height="6" rx="1"/><rect x="4" y="13" width="7" height="6" rx="1"/><rect x="13" y="13" width="7" height="6" rx="1"/>"#,
        "side" => r#"<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="M14 5v14"/>"#,
        "expand" => r#"<path d="M14 4h6v6M20 4l-7 7M10 20H4v-6M4 20l7-7"/>"#,
        "chevrons-right" => r#"<path d="m6 6 6 6-6 6M12 6l6 6-6 6"/>"#,
        "new-tab" => r#"<rect x="4" y="6" width="16" height="13" rx="2"/><path d="M4 10h16"/>"#,
        "eye" => r#"<path d="M3 12c.5-2.4 4-7 9-7s8.5 4.6 9 7c-.5 2.4-4 7-9 7s-8.5-4.6-9-7z"/><circle cx="12" cy="12" r="2.5"/>"#,
        "eye-off" => r#"<path d="m4 4 16 16M9.5 5.4A9.6 9.6 0 0 1 12 5c5 0 8.5 4.6 9 7-.3 1.2-1.2 2.7-2.6 4M6.2 7.3C4.5 8.6 3.3 10.4 3 12c.5 2.4 4 7 9 7 1.400 0 2.700-.4 3.900-1"/>"#,
        "filter" => r#"<path d="M4 7h16M7 12h10M10 17h4"/>"#,
        "sort" => r#"<path d="M7 5v14M4 16l3 3 3-3M17 19V5M14 8l3-3 3 3"/>"#,
        "sort-up" => r#"<path d="M7 19V5M4 8l3-3 3 3M13 7h4M13 12h6M13 17h8"/>"#,
        "sort-down" => r#"<path d="M7 5v14M4 16l3 3 3-3M13 7h8M13 12h6M13 17h4"/>"#,
        "group" => r#"<rect x="4" y="4" width="16" height="6" rx="1.5"/><rect x="4" y="14" width="16" height="6" rx="1.5"/>"#,
        "insert-left" => r#"<path d="M5 4v16M20 12H10M14 8l-4 4 4 4"/>"#,
        "insert-right" => r#"<path d="M19 4v16M4 12h10M10 8l4 4-4 4"/>"#,
        "freeze" => r#"<path d="M9 4v16"/><rect x="4" y="4" width="16" height="16" rx="2"/>"#,
        "indent" => r#"<path d="M4 6h16M10 12h10M10 18h10M4 11l3 3.500-3 3.500"/>"#,
        "outdent" => r#"<path d="M4 6h16M10 12h10M10 18h10M7 11l-3 3.500 3 3.500"/>"#,
        "template" => r#"<rect x="4.5" y="3.500" width="15" height="17" rx="2"/><path d="M8 8h8M8 12h8M8 16h4"/>"#,
        "arrow-up-right" => r#"<path d="M8 16 16.5 7.5M9.75 7.5h6.75v6.75"/>"#,
        "mic" => r#"<rect x="9" y="3.25" width="6" height="11" rx="3"/><path d="M5.75 11.5a6.25 6.25 0 0 0 12.5 0"/><path d="M12 17.75v3"/>"#,
        "history" => r#"<path d="M4.1 13.5A8.25 8.25 0 1 0 6.2 6.1"/><path d="M5.6 2.9 6.2 6.1 3 6.7"/><path d="M12 8v4l2.75 1.75"/>"#,
        "close" => r#"<path d="m6.5 6.5 11 11M17.5 6.5l-11 11"/>"#,
        "code" => r#"<path d="M8.5 7.5 4 12l4.5 4.5M15.5 7.5 20 12l-4.5 4.5M13.5 5l-3 14"/>"#,
        "coffee" => {
            r#"<path d="M5 9h11v5.5a4.5 4.5 0 0 1-4.5 4.5h-2A4.5 4.5 0 0 1 5 14.5z"/><path d="M16 10.5h1.25a2.25 2.25 0 0 1 0 4.5H16"/><path d="M8.5 3.5c-.5.75-.5 1.5 0 2.25M12 3.5c-.5.75-.5 1.5 0 2.25"/>"#
        }
        "divider" => r#"<path d="M3.5 12h17"/><path d="M7 7h10M7 17h10" stroke-opacity=".45"/>"#,
        "duplicate" => {
            r#"<rect x="4" y="8" width="12" height="12" rx="2"/><path d="M8 5.75C8 4.78 8.78 4 9.75 4h8.5C19.22 4 20 4.78 20 5.75v8.5c0 .97-.78 1.75-1.75 1.75H16"/>"#
        }
        "file" => {
            r#"<path d="M13.5 3.75H7.25c-.97 0-1.75.78-1.75 1.75v13c0 .97.78 1.75 1.75 1.75h9.5c.97 0 1.75-.78 1.75-1.75V8.75z"/><path d="M13.5 3.75v3.75c0 .69.56 1.25 1.25 1.25h3.75"/>"#
        }
        "flame" => {
            r#"<path d="M12 20.75c-3.45 0-5.75-2.4-5.75-5.5 0-4.1 3.8-5.75 4.5-10.5 2.4 1.55 3.3 3.9 3.15 6.1.95-.55 1.6-1.55 1.85-2.75 1.35 1.55 2 3.5 2 5.6 0 4.2-2.3 7.05-5.75 7.05z"/><path d="M12 20.75c-1.4 0-2.35-.95-2.35-2.25 0-1.6 1.3-2.45 2.35-3.9 1.05 1.45 2.35 2.3 2.35 3.9 0 1.3-.95 2.25-2.35 2.25z"/>"#
        }
        "flask" => {
            r#"<path d="M9.5 3.75h5M10.25 3.75v5.5L5.1 17.9c-.7 1.15.13 2.6 1.47 2.6h10.86c1.34 0 2.17-1.45 1.47-2.6L13.75 9.25v-5.5"/><path d="M7.6 14h8.8"/>"#
        }
        "folder" => {
            r#"<path d="M3.5 7.25c0-.97.78-1.75 1.75-1.75h4.1c.46 0 .9.18 1.24.51L12 7.5h6.75c.97 0 1.75.78 1.75 1.75v8.5c0 .97-.78 1.75-1.75 1.75H5.25c-.97 0-1.75-.78-1.75-1.75z"/>"#
        }
        "gift" => {
            r#"<rect x="3.5" y="7.5" width="17" height="4" rx="1.25"/><path d="M5 11.5v7.75c0 .97.78 1.75 1.75 1.75h10.5c.97 0 1.75-.78 1.75-1.75V11.5M12 7.5V21"/><path d="M12 7.5C11 5 7.5 4 7.5 6.25c0 1.25 2 1.25 4.5 1.25zM12 7.5c1-2.5 4.5-3.5 4.5-1.25 0 1.25-2 1.25-4.5 1.25z"/>"#
        }
        "globe" => {
            r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 3.5c-2.3 2.3-3.5 5.2-3.5 8.5s1.2 6.2 3.5 8.5c2.3-2.3 3.5-5.2 3.5-8.5S14.3 5.8 12 3.5zM3.5 12h17"/>"#
        }
        "grip" => {
            r#"<circle cx="9" cy="6" r="1.5" fill="currentColor" stroke="none"/><circle cx="15" cy="6" r="1.5" fill="currentColor" stroke="none"/><circle cx="9" cy="12" r="1.5" fill="currentColor" stroke="none"/><circle cx="15" cy="12" r="1.5" fill="currentColor" stroke="none"/><circle cx="9" cy="18" r="1.5" fill="currentColor" stroke="none"/><circle cx="15" cy="18" r="1.5" fill="currentColor" stroke="none"/>"#
        }
        "heading-1" => r#"<path d="M4 6v12M12 6v12M4 12h8"/><path d="m15.75 9 3-2.25V18"/>"#,
        "heading-2" => {
            r#"<path d="M4 6v12M12 6v12M4 12h8"/><path d="M15.25 9.5c.2-1.5 1.35-2.5 2.85-2.5 1.65 0 2.9 1.1 2.9 2.65 0 1.15-.6 2-1.55 2.95L15.25 18h5.75"/>"#
        }
        "heading-3" => {
            r#"<path d="M4 6v12M12 6v12M4 12h8"/><path d="M15.4 8.4C15.9 7.5 16.85 7 17.95 7c1.6 0 2.8 1 2.8 2.4s-1.15 2.35-2.6 2.35h-.55"/><path d="M17.6 11.75h.6c1.65 0 2.95 1.05 2.95 2.6 0 1.65-1.35 2.9-3.2 2.9-1.3 0-2.35-.6-2.85-1.6"/>"#
        }
        "heart" => {
            r#"<path d="M12 19.5s-7.75-4.5-7.75-10A4.25 4.25 0 0 1 12 7a4.25 4.25 0 0 1 7.75 2.5c0 5.5-7.75 10-7.75 10z"/>"#
        }
        "home" => {
            r#"<path d="M3.75 11.25 12 4.5l8.25 6.75"/><path d="M6 9.75v9c0 .69.56 1.25 1.25 1.25H10v-5h4v5h2.75c.69 0 1.25-.56 1.25-1.25v-9"/>"#
        }
        "inbox" => {
            r#"<path d="M3.5 13.5 6 5.75c.2-.6.75-1 1.4-1h9.2c.65 0 1.2.4 1.4 1l2.5 7.75v5c0 .97-.78 1.75-1.75 1.75H5.25c-.97 0-1.75-.78-1.75-1.75z"/><path d="M3.5 13.5h4.25l1.25 2.5h6l1.25-2.5h4.25"/>"#
        }
        "info" => {
            r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.25"/><circle cx="12" cy="7.9" r="1.15" fill="currentColor" stroke="none"/>"#
        }
        "inline-code" => r#"<path d="M9 7 4 12l5 5M15 7l5 5-5 5"/>"#,
        "italic" => r#"<path d="M10.5 5h7.5M6 19h7.5M14.25 5l-4.5 14"/>"#,
        "leaf" => {
            r#"<path d="M6.5 17.5C5 10 10.5 4.75 19.25 4.75c0 8.75-5.25 14.25-12.75 12.75z"/><path d="m4.5 19.5 2-2M6.5 17.5 13 11"/>"#
        }
        "lightbulb" => {
            r#"<path d="M9.25 17.5v-1.6c0-.3-.15-.56-.4-.75C7.1 13.95 6 12.1 6 10a6 6 0 0 1 12 0c0 2.1-1.1 3.95-2.85 5.15-.25.19-.4.45-.4.75v1.6z"/><path d="M10 20.75h4"/>"#
        }
        "link" => {
            r#"<path d="M10.25 13.75a4 4 0 0 0 5.66 0l2.83-2.83a4 4 0 0 0-5.66-5.66l-1.06 1.06"/><path d="M13.75 10.25a4 4 0 0 0-5.66 0l-2.83 2.83a4 4 0 0 0 5.66 5.66l1.06-1.06"/>"#
        }
        "list-bullet" => {
            r#"<circle cx="5" cy="7" r="1.35" fill="currentColor" stroke="none"/><circle cx="5" cy="12" r="1.35" fill="currentColor" stroke="none"/><circle cx="5" cy="17" r="1.35" fill="currentColor" stroke="none"/><path d="M9.5 7h10M9.5 12h10M9.5 17h10"/>"#
        }
        "list-numbered" => {
            r#"<path d="M3.75 5.25 5.5 4v5.25" stroke-width="1.5"/><path d="M3.6 14.6c.2-.85.85-1.35 1.7-1.35.95 0 1.65.6 1.65 1.5 0 .6-.3 1.05-.85 1.6L3.6 19.25h3.45" stroke-width="1.5"/><path d="M10.5 7h9.5M10.5 16.5h9.5"/>"#
        }
        "lock" => {
            r#"<rect x="5" y="10.5" width="14" height="10" rx="2.25"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5M12 14.5v2"/>"#
        }
        "map-pin" => {
            r#"<path d="M12 21s-6.5-5.6-6.5-11a6.5 6.5 0 0 1 13 0c0 5.4-6.5 11-6.5 11z"/><circle cx="12" cy="10" r="2.25"/>"#
        }
        "more-horizontal" => {
            r#"<circle cx="5.5" cy="12" r="1.45" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.45" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.45" fill="currentColor" stroke="none"/>"#
        }
        "music" => {
            r#"<path d="M9 17.5v-12l10-2v12"/><circle cx="6.75" cy="17.5" r="2.25"/><circle cx="16.75" cy="15.5" r="2.25"/>"#
        }
        "note" => {
            r#"<rect x="5" y="3.75" width="14" height="16.5" rx="2.25"/><path d="M8.75 8.5h6.5M8.75 12h6.5M8.75 15.5h4"/>"#
        }
        "palette" => {
            r#"<path d="M12 3.75a8.25 8.25 0 0 0 0 16.5c1.1 0 1.75-.7 1.75-1.6 0-.45-.2-.85-.45-1.15-.25-.3-.45-.7-.45-1.15 0-.9.75-1.6 1.65-1.6h1.9c2.15 0 3.85-1.75 3.85-3.9C20.25 7.1 16.55 3.75 12 3.75z"/><circle cx="7.75" cy="11.25" r="1.2" fill="currentColor" stroke="none"/><circle cx="9.75" cy="7.75" r="1.2" fill="currentColor" stroke="none"/><circle cx="14" cy="7.5" r="1.2" fill="currentColor" stroke="none"/>"#
        }
        "drawing" => {
            r#"<rect x="3.75" y="3.75" width="7" height="7" rx="1.5"/><circle cx="16.75" cy="7.25" r="3.5"/><path d="M3.75 18.5c2-2.75 4-2.75 5.5 0s3.75 2.75 5.5 0 3.75-2.75 5.5-.75"/>"#
        }
        "pin" => r#"<path d="M9 3.75h6M10 3.75l-.5 6L6.5 13v1.5h11V13l-3-3.25-.5-6M12 14.5v6"/>"#,
        "plane" => {
            r#"<path d="M20.5 3.5 3.5 10.25l6.75 3.25 3.25 6.75z"/><path d="M10.25 13.5 20.5 3.5"/>"#
        }
        "plus" => r#"<path d="M12 5v14M5 12h14"/>"#,
        "arrow-up" => r#"<path d="M12 19.5V5M5.75 11.25 12 5l6.25 6.25"/>"#,
        "quote" => {
            r#"<circle cx="7.75" cy="9.5" r="2.45" fill="currentColor" stroke="none"/><circle cx="16.25" cy="9.5" r="2.45" fill="currentColor" stroke="none"/><path d="M10.2 9.5c0 3.6-1.35 6.1-3.95 7.25M18.7 9.5c0 3.6-1.35 6.1-3.95 7.25"/>"#
        }
        "redo" => r#"<path d="m15 14 4.5-4.5L15 5"/><path d="M19.5 9.5h-10a5 5 0 0 0 0 10H13"/>"#,
        "rocket" => {
            r#"<path d="M12 3.5c3 2 4.5 5.25 4.5 9v3.5h-9v-3.5c0-3.75 1.5-7 4.5-9z"/><path d="M7.5 12.75 5 15.25V18h2.5M16.5 12.75l2.5 2.5V18h-2.5"/><circle cx="12" cy="9.75" r="1.5"/><path d="M10.5 19c.3 1 .8 1.75 1.5 2 .7-.25 1.2-1 1.5-2"/>"#
        }
        "search" => r#"<circle cx="10.5" cy="10.5" r="6.25"/><path d="M15.25 15.25 20 20"/>"#,
        "settings" => {
            r#"<path d="M10.92 5.54L11.11 3.3A8.75 8.75 0 0 1 12.89 3.3L13.08 5.54A6.55 6.55 0 0 1 15.81 6.67L17.52 5.21A8.75 8.75 0 0 1 18.79 6.48L17.33 8.19A6.55 6.55 0 0 1 18.46 10.92L20.7 11.11A8.75 8.75 0 0 1 20.7 12.89L18.46 13.08A6.55 6.55 0 0 1 17.33 15.81L18.79 17.52A8.75 8.75 0 0 1 17.52 18.79L15.81 17.33A6.55 6.55 0 0 1 13.08 18.46L12.89 20.7A8.75 8.75 0 0 1 11.11 20.7L10.92 18.46A6.55 6.55 0 0 1 8.19 17.33L6.48 18.79A8.75 8.75 0 0 1 5.21 17.52L6.67 15.81A6.55 6.55 0 0 1 5.54 13.08L3.3 12.89A8.75 8.75 0 0 1 3.3 11.11L5.54 10.92A6.55 6.55 0 0 1 6.67 8.19L5.21 6.48A8.75 8.75 0 0 1 6.48 5.21L8.19 6.67A6.55 6.55 0 0 1 10.92 5.54z"/><circle cx="12" cy="12" r="2.75"/>"#
        }
        "sidebar" => {
            r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M9.5 4.5v15"/><path d="M5.75 8h1.5M5.75 11h1.5"/>"#
        }
        "smile" => {
            r#"<circle cx="12" cy="12" r="8.5"/><circle cx="9" cy="10" r="1.15" fill="currentColor" stroke="none"/><circle cx="15" cy="10" r="1.15" fill="currentColor" stroke="none"/><path d="M8.5 14c.9 1.3 2.1 2 3.5 2s2.6-.7 3.5-2"/>"#
        }
        "sparkles" => {
            r#"<path d="M9.5 4.25c.62 4.15 2.6 6.13 6.75 6.75-4.15.62-6.13 2.6-6.75 6.75-.62-4.15-2.6-6.13-6.75-6.75 4.15-.62 6.13-2.6 6.75-6.75z"/><path d="M17.75 14.25c.3 1.95 1.05 2.7 3 3-1.95.3-2.7 1.05-3 3-.3-1.95-1.05-2.7-3-3 1.95-.3 2.7-1.05 3-3z"/>"#
        }
        "star" => {
            r#"<path d="M12 3.8L14.38 9.12L20.18 9.74L15.85 13.65L17.05 19.36L12 16.45L6.95 19.36L8.15 13.65L3.82 9.74L9.62 9.12z"/>"#
        }
        "star-filled" => {
            r#"<path d="M12 3.8L14.38 9.12L20.18 9.74L15.85 13.65L17.05 19.36L12 16.45L6.95 19.36L8.15 13.65L3.82 9.74L9.62 9.12z" fill="currentColor"/>"#
        }
        "strikethrough" => {
            r#"<path d="M16.6 7.6c-.55-1.6-2.2-2.6-4.4-2.6-2.6 0-4.5 1.35-4.5 3.35 0 1.5.95 2.45 2.75 3.05"/><path d="M14.4 13.1c1.6.6 2.35 1.55 2.35 2.95 0 2.1-1.95 3.45-4.65 3.45-2.4 0-4.2-1.05-4.8-2.8"/><path d="M4.5 12.25h15"/>"#
        }
        "target" => {
            r#"<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="4.75"/><circle cx="12" cy="12" r="1.5" fill="currentColor" stroke="none"/>"#
        }
        "text" => {
            r#"<path d="M3.5 18 8 6l4.5 12M5.2 14h5.6"/><circle cx="16.75" cy="15.25" r="2.75"/><path d="M19.5 12.5V18"/>"#
        }
        "toggle-list" => {
            r#"<path d="M4.5 5.6v4.8c0 .5.56.8.97.52l3.3-2.4a.65.65 0 0 0 0-1.04l-3.3-2.4c-.41-.29-.97.01-.97.52z" fill="currentColor" stroke="none"/><path d="M11.5 8h8.5M11.5 13h8.5M11.5 18h5.5"/>"#
        }
        "trash" => {
            r#"<path d="M4.5 6.75h15"/><path d="M9.5 6.75v-1.5c0-.69.56-1.25 1.25-1.25h2.5c.69 0 1.25.56 1.25 1.25v1.5"/><path d="m6.25 6.75.85 12.05c.07.96.86 1.7 1.82 1.7h6.16c.96 0 1.75-.74 1.82-1.7l.85-12.05"/><path d="M10.25 10.75v5.5M13.75 10.75v5.5"/>"#
        }
        "triangle-right" => {
            r#"<path d="M9 6.62v10.76c0 .62.71.97 1.2.6l7.12-5.38a.75.75 0 0 0 0-1.2L10.2 6.02c-.49-.37-1.2-.02-1.2.6z" fill="currentColor" stroke="none"/>"#
        }
        "trophy" => {
            r#"<path d="M7.5 4.5h9v5a4.5 4.5 0 0 1-9 0z"/><path d="M7.5 6H5.25c0 2.6 1 4 3 4.5M16.5 6h2.25c0 2.6-1 4-3 4.5M12 14v4M8.5 20h7"/>"#
        }
        "undo" => r#"<path d="M9 14 4.5 9.5 9 5"/><path d="M4.5 9.5h10a5 5 0 0 1 0 10H11"/>"#,
        "warning" => {
            r#"<path d="M10.48 4.62c.67-1.16 2.37-1.16 3.04 0l7.07 12.25c.67 1.16-.17 2.63-1.52 2.63H4.93c-1.35 0-2.19-1.47-1.52-2.63z"/><path d="M12 9.5v4"/><circle cx="12" cy="16.6" r="1.15" fill="currentColor" stroke="none"/>"#
        }
        "wrench" => {
            r#"<path d="M14.75 6.25a4 4 0 0 0-5.2 5.2l-5.05 5.05a1.41 1.41 0 0 0 2 2l5.05-5.05a4 4 0 0 0 5.2-5.2l-2.3 2.3-2.05-.45-.45-2.05z"/>"#
        }
        _ => return None,
    })
}

/// A complete `<svg>` element for an icon at `size` pixels. Unknown names draw
/// the plain page icon.
pub fn svg(name: &str, size: u32) -> String {
    let inner = paths(name).or_else(|| paths("file")).unwrap_or_default();
    format!(
        "<svg viewBox=\"0 0 24 24\" width=\"{size}\" height=\"{size}\" fill=\"none\" stroke=\"currentColor\" \
         stroke-width=\"1.75\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\">{inner}</svg>"
    )
}

fn is_color(color: &str) -> bool {
    ICON_COLORS.iter().any(|(id, _)| *id == color)
}

/// Split a stored icon into (name, colour). Accepts `"name"`, `"name:color"`
/// and legacy emoji; anything unrecognised becomes the plain page icon.
pub fn parse_icon(stored: &str) -> (String, String) {
    let cleaned: String = stored
        .trim()
        .chars()
        .filter(|c| !matches!(*c, '\u{FE0E}' | '\u{FE0F}' | '\u{200D}'))
        .collect();
    let (name, color) = match cleaned.split_once(':') {
        Some((n, c)) => (n, c),
        None => (cleaned.as_str(), ""),
    };
    if paths(name).is_some() {
        let color = if is_color(color) {
            color
        } else {
            DEFAULT_COLOR
        };
        return (name.to_string(), color.to_string());
    }
    if let Some((_, n, c)) = LEGACY_EMOJI.iter().find(|(e, _, _)| *e == cleaned) {
        return (n.to_string(), c.to_string());
    }
    ("file".to_string(), DEFAULT_COLOR.to_string())
}

/// Store an icon choice as `"name:color"`.
pub fn format_icon(name: &str, color: &str) -> String {
    format!("{name}:{color}")
}

/// An interface icon. It takes the surrounding text colour.
#[component]
pub fn Icon(
    #[props(into)] name: String,
    #[props(default = 18)] size: u32,
    #[props(default, into)] class: String,
) -> Element {
    let markup = svg(&name, size);
    rsx! {
        span { class: "ico {class}", dangerous_inner_html: "{markup}" }
    }
}

/// A page or callout icon in its colour. `large` draws it on a soft tile of
/// that colour, for the top of a page. No icon shows a muted page glyph.
#[component]
pub fn PageIcon(
    #[props(!optional)] icon: Option<String>,
    #[props(default = 18)] size: u32,
    #[props(default)] large: bool,
) -> Element {
    let size = if large && size == 18 { 40 } else { size };
    let (name, color, class) = match icon.as_deref() {
        Some(stored) if !stored.trim().is_empty() => {
            let (name, color) = parse_icon(stored);
            (
                name,
                Some(color),
                if large { "page-ico large" } else { "page-ico" },
            )
        }
        _ => (
            "file".to_string(),
            None,
            if large {
                "page-ico large none"
            } else {
                "page-ico none"
            },
        ),
    };
    let style = match &color {
        Some(c) => format!("--c: var(--icon-{c}); --c-soft: var(--icon-{c}-soft);"),
        None => String::new(),
    };
    let markup = svg(&name, size);
    rsx! {
        span { class: "{class}", style: "{style}", dangerous_inner_html: "{markup}" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_icon_has_paths() {
        for name in UI_ICONS.iter().chain(PAGE_ICONS) {
            assert!(paths(name).is_some(), "missing icon {name}");
        }
        assert!(PAGE_ICONS.len() >= 32);
    }

    #[test]
    fn svg_falls_back_to_file() {
        assert_eq!(svg("no-such-icon", 16), svg("file", 16));
        assert!(svg("search", 20).contains("width=\"20\""));
    }

    #[test]
    fn stored_icons_round_trip() {
        for name in PAGE_ICONS {
            for (color, _) in ICON_COLORS {
                assert_eq!(
                    parse_icon(&format_icon(name, color)),
                    (name.to_string(), color.to_string())
                );
            }
        }
        assert_eq!(parse_icon("rocket"), ("rocket".into(), "gray".into()));
        assert_eq!(parse_icon("rocket:teal"), ("rocket".into(), "gray".into()));
        assert_eq!(parse_icon("nonsense:blue"), ("file".into(), "gray".into()));
        assert_eq!(parse_icon(""), ("file".into(), "gray".into()));
    }

    #[test]
    fn legacy_emoji_map_to_icons() {
        assert_eq!(parse_icon("👋"), ("smile".into(), "yellow".into()));
        assert_eq!(parse_icon("💡"), ("lightbulb".into(), "yellow".into()));
        assert_eq!(parse_icon("📚"), ("book".into(), "blue".into()));
        assert_eq!(parse_icon("🦀"), ("code".into(), "orange".into()));
        // With and without the emoji variation selector.
        assert_eq!(parse_icon("✈️"), ("plane".into(), "blue".into()));
        assert_eq!(parse_icon("✈"), ("plane".into(), "blue".into()));
        assert_eq!(parse_icon("⚠️"), ("warning".into(), "orange".into()));
        assert_eq!(parse_icon("❤️"), ("heart".into(), "red".into()));
        assert_eq!(parse_icon("ℹ️"), ("info".into(), "blue".into()));
        assert_eq!(parse_icon("🐙"), ("file".into(), "gray".into()));
        for (emoji, name, color) in LEGACY_EMOJI {
            assert!(paths(name).is_some(), "{emoji} maps to missing icon {name}");
            assert!(is_color(color), "{emoji} maps to unknown colour {color}");
        }
    }
}
