//! Rune: a local-first workspace of notes and a calendar, written in Rust.
//! (It was called Notion 2.0 at first. The names inside keep that: the
//! program is `notion2`, the notes folder and the page links too, so that
//! nothing a person had set up stops working.)

mod ai;
mod attach;
mod aven;
mod backup;
mod chart;
mod dictation;
mod repeat;
mod transfer;
mod update;
mod voice;
mod drawing;
mod files;
mod formula;
mod glass;
mod history;
mod inline;
mod layout;
mod listen;
mod maccal;
mod markdown;
mod mcp;
mod meeting;
mod model;
mod notify;
mod pad;
mod player;
mod quickadd;
mod slots;
mod state;
mod store;
mod sync;
mod devsync;
mod table;
mod ui;
mod video;
mod when;

use dioxus::desktop::muda::accelerator::Accelerator;
use dioxus::desktop::muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use dioxus::desktop::tao::platform::macos::WindowBuilderExtMacOS;
use dioxus::desktop::{Config, LogicalPosition, LogicalSize, WindowBuilder};

const CSS: &str = include_str!("../assets/style.css");
const ICON_CSS: &str = include_str!("../assets/icons.css");
const TABLE_CSS: &str = include_str!("../assets/tables.css");

thread_local! {
    /// One menu bar for the whole app, shared by every window.
    static MENU: Menu = build_menu();
}

fn main() {
    // `notion2 --mcp`: serve the workspace to AI agents over stdio, no window.
    if std::env::args().skip(1).any(|arg| arg == "--mcp") {
        std::process::exit(mcp::run_stdio());
    }
    // `notion2 --meeting-probe`: print which programs use audio now, and
    // whether that counts as a Teams meeting (for checking during a real one).
    // `notion2 --update-check` / `--update-install`: what the updater sees,
    // and the swap itself, with no window and no restart.
    // `notion2 --update-watch <app> <build>`: the old program, after an
    // update, opens the new copy and puts itself back if that does not start.
    if std::env::args().skip(1).any(|arg| arg == "--update-watch") {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::exit(update::watch_command(&args));
    }
    if let Some(flag) = std::env::args().skip(1).find(|arg| arg == "--update-check" || arg == "--update-install") {
        std::process::exit(update::command_line(flag == "--update-install"));
    }
    // `notion2 --listen-test <report> …`: run the real listener (see listen::test).
    if std::env::args().skip(1).any(|arg| arg == "--listen-test") {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::exit(listen::test(&args));
    }
    // `notion2 --transcript-into <page id> <lines file> …`: put a transcript
    // kept outside the app into a page (see listen::into_page).
    if std::env::args().skip(1).any(|arg| arg == "--transcript-into") {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::exit(listen::into_page(&args));
    }
    if std::env::args().skip(1).any(|arg| arg == "--meeting-probe") {
        print!("{}", meeting::probe());
        return;
    }

    let store = match store::Store::open(store::Store::default_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Couldn't open the workspace database ({e}); changes will not be saved.");
            // The window says so too: without this the app looks normal and
            // everything written is gone when it closes.
            store::runs_without_a_file();
            store::Store::open_in_memory().expect("in-memory database")
        }
    };
    if store.from_newer_program() {
        store::tell("These notes come from a newer Rune. Update this app before you change them.");
    }
    store::install(store);
    meeting::load_setting();
    // Sync with the iPhone, when it was turned on in Settings.
    if std::env::var_os("NOTION2_SELFTEST").is_none() {
        devsync::start_from_settings();
    }
    // A newer build waiting? Install it and start again (never in a test run).
    if std::env::var_os("NOTION2_SELFTEST").is_none() {
        update::at_launch();
    }

    let mut config = window_config();
    // A test run stays behind the person's windows and leaves the keyboard
    // to them: the program does not come to the front as it starts.
    if quiet_test() {
        use dioxus::desktop::tao::event_loop::EventLoopBuilder;
        use dioxus::desktop::tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        let mut event_loop = EventLoopBuilder::with_user_event().build();
        event_loop.set_activate_ignoring_other_apps(false);
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        config = config.with_event_loop(event_loop);
    }
    dioxus::LaunchBuilder::desktop().with_cfg(config).launch(ui::App);
}

/// A developer self-test that stays out of the person's way: its window is
/// behind the others and does not take the keyboard. (NOTION2_SELFTEST_FRONT=1
/// gives the test its window in front, as it was before.)
pub fn quiet_test() -> bool {
    std::env::var_os("NOTION2_SELFTEST").is_some() && std::env::var_os("NOTION2_SELFTEST_FRONT").is_none()
}

/// Settings for every window: see-through glass behind the sidebar and top
/// row, a hidden title bar with the traffic lights centred in the 40px top row
/// (as in Aven), the app's styles and the shared menu bar.
pub fn window_config() -> Config {
    let window = WindowBuilder::new()
        .with_title("Rune")
        .with_inner_size(LogicalSize::new(1320.0, 860.0))
        .with_min_inner_size(LogicalSize::new(800.0, 520.0))
        .with_transparent(true)
        .with_titlebar_transparent(true)
        .with_title_hidden(true)
        .with_fullsize_content_view(true)
        .with_traffic_light_inset(LogicalPosition::new(12.0, 13.0))
        // (A quiet test run shows its window itself, at the back.)
        .with_focused(!quiet_test())
        .with_visible(!quiet_test());
    Config::new()
        .with_window(window)
        .with_background_color((0, 0, 0, 0))
        // The page template names no text encoding; without this, symbols in
        // the styles (…, ⌘) come out garbled.
        .with_custom_head(format!("<meta charset=\"utf-8\"><style>{CSS}\n{ICON_CSS}\n{TABLE_CSS}</style>"))
        .with_menu(MENU.with(|m| m.clone()))
        .with_disable_drag_drop_handler(true)
        .with_custom_protocol(drawing::SCHEME, drawing::serve)
        .with_custom_protocol(files::SCHEME, files::serve)
}

/// Settings for the window of the meeting pad (see pad.rs): a small
/// see-through window with no frame, over the other windows. It is made
/// out of view and without the keyboard; the pad shows it once it knows how
/// big it is. Its class is changed to a panel right after tao has made it,
/// before the web view goes in (the only moment that is safe: see
/// `pad::make_panel`).
pub fn pad_window_config() -> Config {
    let window = WindowBuilder::new()
        .with_title("Rune meeting pad")
        .with_inner_size(LogicalSize::new(328.0, 150.0))
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top(true)
        .with_has_shadow(true)
        .with_focusable(false)
        .with_focused(false)
        .with_visible(false);
    Config::new()
        .with_window(window)
        .with_background_color((0, 0, 0, 0))
        .with_custom_head(format!("<meta charset=\"utf-8\"><style>{CSS}\n{ICON_CSS}</style>"))
        // (No menu of its own: the app's menu bar stays as the first window set it.)
        .with_disable_drag_drop_handler(true)
        .with_on_window(|window, _dom| {
            pad::make_panel(&window);
        })
}

/// Menu bar modelled on the desktop app's (File, Edit, View, History, Window).
/// Item ids are the command names handled by `AppState::command`.
fn build_menu() -> Menu {
    let item = |id: &str, label: &str, shortcut: Option<&str>| {
        let accelerator = shortcut.map(|s| s.parse::<Accelerator>().expect("valid shortcut"));
        MenuItem::with_id(id, label, true, accelerator)
    };

    let app = Submenu::with_items(
        "Rune",
        true,
        &[
            &PredefinedMenuItem::about(Some("About Rune"), None),
            &PredefinedMenuItem::separator(),
            &item("settings", "Settings…", Some("CmdOrCtrl+,")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    )
    .expect("app menu");

    let file = Submenu::with_items(
        "File",
        true,
        &[
            &item("new_page", "New Page", Some("CmdOrCtrl+N")),
            &item("meeting_notes", "New Meeting Notes", Some("CmdOrCtrl+Shift+M")),
            &item("voice", "New Page from Speech…", Some("CmdOrCtrl+Shift+D")),
            &item("new_window", "New Window", Some("CmdOrCtrl+Shift+N")),
            &item("new_tab", "New Tab", Some("CmdOrCtrl+T")),
            &PredefinedMenuItem::separator(),
            &item("close_tab", "Close Tab", Some("CmdOrCtrl+W")),
            &item("reopen_tab", "Reopen Last Closed Tab", Some("CmdOrCtrl+Shift+T")),
            &PredefinedMenuItem::separator(),
            &item("copy_markdown", "Copy Page as Markdown", None),
            &item("export_page", "Export Page as Markdown…", None),
            &item("export_all", "Export Workspace…", None),
            &item("import", "Import…", None),
            &PredefinedMenuItem::separator(),
            &item("trash", "Trash", None),
        ],
    )
    .expect("file menu");

    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            // The app's own undo covers the whole page (see history.rs); in a
            // plain text field it falls back to the field's own undo.
            &item("undo", "Undo", Some("CmdOrCtrl+Z")),
            &item("redo", "Redo", Some("CmdOrCtrl+Shift+Z")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
            &PredefinedMenuItem::separator(),
            &item("copy_link", "Copy Link to Current Page", Some("CmdOrCtrl+L")),
            &item("copy_name", "Copy Name of Current Page", Some("CmdOrCtrl+Alt+L")),
            &PredefinedMenuItem::separator(),
            &item("find", "Find in Page…", Some("CmdOrCtrl+F")),
            &item("find_next", "Find Next", Some("CmdOrCtrl+G")),
            &item("find_prev", "Find Previous", Some("CmdOrCtrl+Shift+G")),
            &PredefinedMenuItem::separator(),
            &item("quick_find", "Search…", Some("CmdOrCtrl+K")),
            &item("ai", "Ask AI…", Some("CmdOrCtrl+J")),
            &item("ask_notes", "Ask Your Notes…", Some("CmdOrCtrl+Shift+J")),
        ],
    )
    .expect("edit menu");

    let format = Submenu::with_items(
        "Format",
        true,
        &[
            &item("bold", "Bold", Some("CmdOrCtrl+B")),
            &item("italic", "Italic", Some("CmdOrCtrl+I")),
            &item("strike", "Strikethrough", Some("CmdOrCtrl+Shift+S")),
            &item("code", "Inline Code", Some("CmdOrCtrl+E")),
            &PredefinedMenuItem::separator(),
            &item("link", "Add Link… (⌘K with text selected)", None),
        ],
    )
    .expect("format menu");

    let view = Submenu::with_items(
        "View",
        true,
        &[
            &item("toggle_sidebar", "Show/Hide Sidebar", Some("CmdOrCtrl+\\")),
            &item("calendar", "Calendar", Some("CmdOrCtrl+Shift+C")),
            &item("read_aloud", "Read Page Aloud / Stop", Some("CmdOrCtrl+Shift+R")),
            &item("close_split", "Close Pane", None),
            &item("dark_mode", "Toggle Dark Mode", Some("CmdOrCtrl+Shift+L")),
            &PredefinedMenuItem::separator(),
            &item("zoom_in", "Zoom In", Some("CmdOrCtrl+=")),
            &item("zoom_out", "Zoom Out", Some("CmdOrCtrl+-")),
            &item("zoom_reset", "Actual Size", Some("CmdOrCtrl+0")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::fullscreen(None),
        ],
    )
    .expect("view menu");

    let history = Submenu::with_items(
        "History",
        true,
        &[
            &item("back", "Back", Some("CmdOrCtrl+[")),
            &item("forward", "Forward", Some("CmdOrCtrl+]")),
        ],
    )
    .expect("history menu");

    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::separator(),
            &item("next_tab", "Show Next Tab", Some("CmdOrCtrl+Shift+]")),
            &item("prev_tab", "Show Previous Tab", Some("CmdOrCtrl+Shift+[")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ],
    )
    .expect("window menu");

    let menu = Menu::new();
    menu.append_items(&[&app, &file, &edit, &format, &view, &history, &window]).expect("menu bar");
    menu
}
