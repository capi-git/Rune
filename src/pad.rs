//! The meeting pad: a small card on the screen, outside the app's window,
//! for the time of a call.
//!
//! When the meeting watcher sees a call start (meeting.rs: Teams, Zoom, a
//! call in a browser…), the app is behind it. So the question "Listen and write the notes?" is asked on a
//! card of its own, at the corner of the screen; the card then becomes a
//! notepad (what is typed goes under "Notes" of the meeting's page, and a
//! line says the last words heard), can be made small, and at the end asks
//! which page to keep the notes in. Then it goes.
//!
//! The first window of the app owns all of it: the watcher, the listener and
//! the pages. The pad is a window with nothing of its own: it draws the
//! `View` the first window hands it and sends back a `Command` for each
//! click. The two talk through this module (they are on one thread).
//!
//! The pad's window is a panel that does not make the app the active one:
//! macOS shows such a window beside another program's full screen, and a
//! click on it leaves that program in front. tao makes plain windows only, so
//! the class of the window is changed right after tao has made it and before
//! the web view is put in it (see `make_panel`).

use crate::model::{Block, BlockKind, Id};
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use std::cell::{Cell, RefCell};

/// The question at the start becomes the small form after this long.
pub const ASK_MS: i64 = 30_000;
/// The question at the end goes away after this long (the page still asks).
pub const END_MS: i64 = 120_000;
/// A card that only says where the notes are: this long.
pub const TOLD_MS: i64 = 8_000;
/// "Kept in …" is said for this long.
pub const KEPT_MS: i64 = 1_300;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    /// "Listen and write the notes?"
    #[default]
    Ask,
    /// The notepad.
    Open,
    /// A pill: the name, and the time while it listens.
    Small,
    /// The meeting is over: where to keep the notes.
    End,
    /// "Kept in …", for a moment.
    Kept,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Ask => "ask",
            Form::Open => "open",
            Form::Small => "small",
            Form::End => "end",
            Form::Kept => "kept",
        }
    }
}

/// The pad of the meeting in progress, as the first window keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    /// The notes page of the meeting.
    pub page: Id,
    pub form: Form,
    /// When it took this form (two of them go away by themselves).
    pub since: i64,
    /// The lines typed in the pad.
    pub lines: Vec<String>,
    /// The blocks of the page those lines are in, each with the line as the
    /// pad wrote it (see `put_lines`).
    pub written: Vec<(Id, String)>,
    /// The person listened or typed: at the end the pad asks where to keep
    /// the notes. An unused pad just goes.
    pub used: bool,
    /// It listened in this meeting.
    pub heard: bool,
    /// At the end: what is said about the notes ("Writing the notes…").
    pub status: String,
    /// The notes are still being written: the card waits for them.
    pub busy: bool,
    /// Where the notes were kept ("" for the top of the list).
    pub kept: String,
    /// Why it does not listen, when the listener said so.
    pub problem: String,
    /// When the meeting started, and its time on the calendar if it has one.
    pub started: i64,
    pub span: Option<(i64, i64)>,
    /// When it was over.
    pub ended: Option<i64>,
    /// The form it had before its last card: it goes back to that one if
    /// the meeting goes on after all.
    pub was: Form,
}

impl Pad {
    pub fn new(page: &str, form: Form, now: i64, span: Option<(i64, i64)>) -> Pad {
        Pad {
            page: page.to_string(),
            form,
            since: now,
            lines: Vec::new(),
            written: Vec::new(),
            used: false,
            heard: false,
            status: String::new(),
            busy: false,
            kept: String::new(),
            problem: String::new(),
            started: now,
            span,
            ended: None,
            was: Form::Small,
        }
    }

    pub fn turn(&mut self, form: Form, now: i64) {
        self.form = form;
        self.since = now;
    }
}

/// What the pad does when nobody answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByItself {
    /// The question at the start becomes the small form.
    Fold,
    /// The card goes away.
    Go,
}

/// What a pad that took `form` at `since` does by itself at `now`.
/// `asks_place`: the card at the end has a question on it. `busy`: the notes
/// are still being written.
pub fn by_itself(form: Form, since: i64, now: i64, asks_place: bool, busy: bool) -> Option<ByItself> {
    let age = now - since;
    match form {
        Form::Ask if age >= ASK_MS => Some(ByItself::Fold),
        Form::End if !busy && age >= if asks_place { END_MS } else { TOLD_MS } => Some(ByItself::Go),
        Form::Kept if age >= KEPT_MS => Some(ByItself::Go),
        _ => None,
    }
}

/// Put the lines typed in the pad into the page, as bullets under "Notes".
///
/// `written` is what the pad put there before, in order: (block, line). A
/// line that changed is written again in its block; a new line gets a block
/// after the pad's last one (the first: at the end of "Notes", in place of
/// the empty bullet the template leaves); a line that is gone takes its
/// block with it, unless the block was changed in the page since. A block
/// the person took out of the page is made again only if its line changes.
/// Says whether the page changed.
pub fn put_lines(page: &mut Vec<Block>, written: &mut Vec<(Id, String)>, lines: &[String]) -> bool {
    let lines: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let mut changed = false;
    let mut now: Vec<(Id, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let stored = crate::inline::escape(line);
        let before = written.get(i);
        let block = before.and_then(|(id, _)| page.iter().position(|b| &b.id == id));
        match (before, block) {
            (Some((id, old)), Some(at)) => {
                if old != line {
                    page[at].text = stored;
                    changed = true;
                }
                now.push((id.clone(), line.to_string()));
            }
            // Taken out of the page by hand, and the line is as it was: it stays out.
            (Some((id, old)), None) if old == line => now.push((id.clone(), line.to_string())),
            _ => {
                let block = Block::new(BlockKind::Bulleted, stored);
                let id = block.id.clone();
                let after = now.iter().rev().find_map(|(id, _)| page.iter().position(|b| &b.id == id));
                match after {
                    Some(at) => page.insert(at + 1, block),
                    None => crate::voice::add_under(page, "Notes", vec![block]),
                }
                now.push((id, line.to_string()));
                changed = true;
            }
        }
    }
    for (id, old) in written.iter().skip(lines.len()) {
        let as_written = crate::inline::escape(old);
        if let Some(at) = page.iter().position(|b| &b.id == id && b.text == as_written) {
            page.remove(at);
            changed = true;
        }
    }
    *written = now;
    changed
}

/// "12:04" for seconds of listening.
pub fn clock(seconds: i64) -> String {
    format!("{}:{:02}", seconds.max(0) / 60, seconds.max(0) % 60)
}

// ---------------------------------------------------------------------------
// What the pad draws, and what it asks for
// ---------------------------------------------------------------------------

/// While the app listens: the time, the last words, and whether it is finishing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hears {
    pub clock: String,
    pub latest: String,
    pub stopping: bool,
}

/// A page the notes can be kept in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Place {
    /// What `AppState::keep_page_in` takes: a page, a space (the top of its
    /// list), or "" (the top of the list, with one space only).
    pub id: Id,
    pub title: String,
    pub depth: usize,
    pub icon: Option<String>,
    /// On the row of a space: its badge (its first letter, its colour).
    pub badge: Option<(String, String)>,
}

/// Everything the pad draws, worked out by the first window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub form: Form,
    /// The name of the meeting, and its time ("10:00 AM – 10:30 AM",
    /// "Started 10:02 AM", "Over · 28 min").
    pub title: String,
    pub when: String,
    pub icon: Option<String>,
    /// The app's theme class and its glass colours (see `glass::css_vars`),
    /// and its accent colour by name ("" for the app's blue).
    pub theme: String,
    pub glass: String,
    pub accent: String,
    /// This copy of the app can listen, and is not listening to something else.
    pub can_listen: bool,
    pub has_ai: bool,
    /// Under the question: to tell the others, and where the sound goes
    /// (`listen::where_speech_goes`: not every language stays on the Mac).
    pub sound: String,
    pub hears: Option<Hears>,
    pub lines: Vec<String>,
    pub status: String,
    pub kept: String,
    pub problem: String,
    /// At the end: the card asks where to keep the notes. When it does not
    /// (they are where the person chose before), `told` says where they are.
    pub asks_place: bool,
    pub told: String,
    /// Where the notes are now, the name of its space (with several
    /// spaces), the places they can go to, and the pages they were kept in
    /// before at this time ("Last time:").
    pub here: Place,
    pub here_space: String,
    pub places: Vec<Place>,
    pub before: Vec<(Id, String)>,
}

/// A click in the pad.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Listen,
    NotesOnly,
    /// The × of the question, and of the small form: put the pad away.
    NotNow,
    Small,
    Open,
    Stop,
    /// The lines of the notepad, as they are now.
    Lines(Vec<String>),
    /// Show the meeting's page in the app.
    OpenPage,
    /// Keep the notes in this place (see `Place::id`).
    Keep(Id),
    Done,
}

/// What the first window sends the pad's window.
pub enum ToPad {
    Show(Box<View>),
    /// (For the self-test: a script to run in the pad's own page.)
    Run(String),
    /// (For the self-test: press the mouse on the thing this selector finds.)
    Press(String),
    Close,
}

thread_local! {
    static TO_PAD: RefCell<Option<(u64, UnboundedSender<ToPad>)>> = const { RefCell::new(None) };
    static LATEST: RefCell<Option<View>> = const { RefCell::new(None) };
    static TO_OWNER: RefCell<Option<UnboundedSender<Command>>> = const { RefCell::new(None) };
    /// A window was asked for (it may still be opening).
    static WINDOW: Cell<bool> = const { Cell::new(false) };
    static WINDOWS: Cell<u64> = const { Cell::new(0) };
    /// A test run draws the pad in the first window, not in one of its own.
    static INLINE: Cell<bool> = const { Cell::new(false) };
    /// A test run that asked for the pad's real window (NOTION2_REAL_PAD).
    static REAL: Cell<bool> = const { Cell::new(false) };
    /// The pad's window while it is open (its NSWindow), for `window_facts`.
    static OPEN_WINDOW: Cell<usize> = const { Cell::new(0) };
    /// (For the self-test.) What the last script run in the pad's page gave back.
    static RESULT: RefCell<serde_json::Value> = const { RefCell::new(serde_json::Value::Null) };
}

/// The first window: take what the pad asks for from here.
pub fn owner() -> UnboundedReceiver<Command> {
    let (tx, rx) = unbounded();
    TO_OWNER.with(|t| *t.borrow_mut() = Some(tx));
    rx
}

/// The pad's window: ask the first window for something.
pub fn ask(command: Command) {
    TO_OWNER.with(|t| {
        if let Some(tx) = t.borrow().as_ref() {
            let _ = tx.unbounded_send(command);
        }
    });
}

/// The first window: this is what the pad shows now. Says whether a window
/// must be opened for it (none is open, and none is on its way).
pub fn show(view: View) -> bool {
    if LATEST.with(|l| l.borrow().as_ref() == Some(&view)) {
        return false;
    }
    LATEST.with(|l| *l.borrow_mut() = Some(view.clone()));
    TO_PAD.with(|t| {
        if let Some((_, tx)) = t.borrow().as_ref() {
            let _ = tx.unbounded_send(ToPad::Show(Box::new(view)));
        }
    });
    !WINDOW.with(|w| w.replace(true))
}

/// The first window: the pad goes away.
pub fn close() {
    LATEST.with(|l| *l.borrow_mut() = None);
    WINDOW.with(|w| w.set(false));
    TO_PAD.with(|t| {
        if let Some((_, tx)) = t.borrow_mut().take() {
            let _ = tx.unbounded_send(ToPad::Close);
        }
    });
}

/// The pad's window, when it opens: what to draw now (None: the pad went
/// away while the window was opening, so it closes again), and where the
/// later views arrive. The number names this window to `detach`.
pub fn attach() -> (u64, Option<View>, UnboundedReceiver<ToPad>) {
    let (tx, rx) = unbounded();
    let number = WINDOWS.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    let view = LATEST.with(|l| l.borrow().clone());
    if view.is_some() {
        TO_PAD.with(|t| *t.borrow_mut() = Some((number, tx)));
    }
    (number, view, rx)
}

/// The pad's window, as it closes. (A newer window is left alone.)
pub fn detach(number: u64) {
    TO_PAD.with(|t| {
        let mut t = t.borrow_mut();
        if t.as_ref().is_some_and(|(n, _)| *n == number) {
            *t = None;
            WINDOW.with(|w| w.set(false));
            LATEST.with(|l| *l.borrow_mut() = None);
        }
    });
}

/// Whether a pad is shown when a meeting starts (Settings). Never in a test
/// run, unless the test draws it in the first window.
pub fn enabled() -> bool {
    if inline() || REAL.with(|r| r.get()) {
        return true;
    }
    std::env::var_os("NOTION2_SELFTEST").is_none() && crate::store::setting("meeting_pad").as_deref() != Some("off")
}

pub fn set_enabled(on: bool) {
    crate::store::set_setting("meeting_pad", if on { "on" } else { "off" });
}

pub fn inline() -> bool {
    INLINE.with(|i| i.get())
}

pub fn set_inline(on: bool) {
    INLINE.with(|i| i.set(on));
}

/// (For the self-test.) Let a test run open the pad's real window.
pub fn set_real_in_test(on: bool) {
    REAL.with(|r| r.set(on));
}

/// (For the self-test.) Run a script in the page of the pad's window.
pub fn run_in_window(script: &str) {
    TO_PAD.with(|t| {
        if let Some((_, tx)) = t.borrow().as_ref() {
            let _ = tx.unbounded_send(ToPad::Run(script.to_string()));
        }
    });
}

/// (For the self-test.) Press the mouse on a thing in the pad's window: a
/// true mouse event, as from the person's hand.
pub fn press_in_window(selector: &str) {
    TO_PAD.with(|t| {
        if let Some((_, tx)) = t.borrow().as_ref() {
            let _ = tx.unbounded_send(ToPad::Press(selector.to_string()));
        }
    });
}

/// (For the self-test.) What the last script run in the pad's page gave back.
pub fn set_result(value: serde_json::Value) {
    RESULT.with(|r| *r.borrow_mut() = value);
}

pub fn result() -> serde_json::Value {
    RESULT.with(|r| r.borrow().clone())
}

/// The form of the pad as the first window last said it.
pub fn latest_form() -> Option<Form> {
    LATEST.with(|l| l.borrow().as_ref().map(|v| v.form))
}

/// The pad's window opened (`Some`) or is closing (`None`).
pub fn window_is(window: Option<&dioxus::desktop::tao::window::Window>) {
    use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
    OPEN_WINDOW.with(|w| w.set(window.map_or(0, |w| w.ns_window() as usize)));
}

/// What macOS says about the pad's window: whether it is a panel, is shown,
/// has the keyboard, and whether the app is the active one. None when no pad
/// window is open. (For the self-test, and for `in_view`.)
pub fn window_facts() -> Option<serde_json::Value> {
    let window = OPEN_WINDOW.with(|w| w.get());
    (window != 0).then(|| native::facts(window))
}

/// Whether the person can see the pad now: its window is shown, on the space
/// in view, and not fully covered. It is not when macOS keeps it off another
/// program's full screen (the window could not be made a panel, or a macOS
/// to come no longer lets a panel in): the app then says with a notification
/// that the notes are ready, as it did before there was a pad.
pub fn in_view() -> bool {
    window_facts().is_some_and(|facts| ["shown", "on_space", "uncovered"].iter().all(|fact| facts[*fact] == true))
}

// ---------------------------------------------------------------------------
// Where the pad sits
// ---------------------------------------------------------------------------

/// The room between the pad and the corner of the screen, when the person
/// has not put it somewhere else.
pub const CORNER: (f64, f64) = (20.0, 20.0);

/// The top left of a pad `size` big whose bottom right corner is `gap` from
/// the bottom right of `area` (left, top, wide, high). A gap that would put
/// the pad out of the area (another screen than the last time) is the usual one.
pub fn place_in(area: (f64, f64, f64, f64), size: (f64, f64), gap: (f64, f64)) -> (f64, f64) {
    let (left, top, wide, high) = area;
    let fits = gap.0 >= 0.0 && gap.1 >= 0.0 && gap.0 + size.0 <= wide && gap.1 + size.1 <= high;
    let gap = if fits { gap } else { CORNER };
    ((left + wide - gap.0 - size.0).max(left), (top + high - gap.1 - size.1).max(top))
}

/// The gap the person chose by dragging the pad, kept between meetings.
pub fn saved_gap() -> (f64, f64) {
    crate::store::setting("pad_gap").and_then(|s| serde_json::from_str::<(f64, f64)>(&s).ok()).unwrap_or(CORNER)
}

pub fn save_gap(gap: (f64, f64)) {
    crate::store::set_setting("pad_gap", &serde_json::to_string(&gap).unwrap_or_default());
}

/// Whether the pad is kept out of a screen share and a recording (it is,
/// unless the "pad_shared" setting says "on": the notes are the person's own).
pub fn hidden_from_shares() -> bool {
    crate::store::setting("pad_shared").as_deref() != Some("on")
}

// ---------------------------------------------------------------------------
// The window: a panel that does not make the app the active one
// ---------------------------------------------------------------------------

pub use native::{bring_out, first_click_acts, leave_keyboard, main_facts, make_panel, press, screen_area, take_keyboard};

mod native {
    use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
    use dioxus::desktop::tao::window::Window;
    use objc2::encode::{Encode, Encoding};
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{class, msg_send, sel};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Once;

    #[link(name = "objc")]
    extern "C" {
        fn object_setClass(object: *mut c_void, class: *const c_void) -> *const c_void;
        fn object_getClass(object: *const c_void) -> *const c_void;
        fn class_getInstanceSize(class: *const c_void) -> usize;
        fn class_getInstanceMethod(class: *const c_void, name: Sel) -> *mut c_void;
        fn method_setImplementation(method: *mut c_void, implementation: *const c_void) -> *const c_void;
    }

    /// Over normal windows (NSFloatingWindowLevel).
    const LEVEL: isize = 3;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        wide: f64,
        high: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        at: Point,
        size: Size,
    }
    // SAFETY: laid out as CGPoint, CGSize and CGRect are (doubles, in this order).
    unsafe impl Encode for Point {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
    }
    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
    }
    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
    }

    fn object(window: &Window) -> *mut AnyObject {
        window.ns_window() as *mut AnyObject
    }

    /// The pad may take the keyboard only while it is the notepad (see
    /// `PadRoot`): tao's own window class keeps that in a field "focusable"
    /// (`set_focusable`), and this class has the same field at the same
    /// place. A window that cannot have the keyboard gets every press of the
    /// mouse at once, which is what the buttons of the other forms need.
    extern "C" fn can_take_keyboard(this: &AnyObject, _sel: Sel) -> Bool {
        // SAFETY: the class has the field, a BOOL (see `panel_class`).
        #[allow(deprecated)]
        unsafe {
            *this.get_ivar::<Bool>("focusable")
        }
    }

    extern "C" fn never(_this: &AnyObject, _sel: Sel) -> Bool {
        Bool::NO
    }

    fn panel_class() -> &'static AnyClass {
        if let Some(class) = AnyClass::get(c"RunePadPanel") {
            return class;
        }
        let mut builder = ClassBuilder::new(c"RunePadPanel", class!(NSPanel)).expect("a new class");
        builder.add_ivar::<Bool>(c"focusable");
        // SAFETY: both selectors take no arguments and return BOOL, as the functions do.
        unsafe {
            builder.add_method(sel!(canBecomeKeyWindow), can_take_keyboard as extern "C" fn(_, _) -> _);
            builder.add_method(sel!(canBecomeMainWindow), never as extern "C" fn(_, _) -> _);
        }
        builder.register()
    }

    /// Turn the window tao made into a panel that does not make the app the
    /// active one. Call it from Dioxus's `Config::with_on_window`: after tao
    /// has made the window, before the web view is put in it. (WebKit
    /// watches the window it is in by key-value observing, which changes the
    /// window's class itself; a class change after that loses the observers
    /// and the app then fails when the web view leaves the window.) Nothing
    /// is changed if the two classes are not laid out the same, or if
    /// something observes the window already: the pad is then a plain
    /// window, which works everywhere but beside a full-screen program.
    pub fn make_panel(window: &Window) -> bool {
        let window = object(window);
        if window.is_null() {
            return false;
        }
        // SAFETY: the live window, on the main thread; the class is changed
        // only to one of the same size, with the same one field.
        unsafe {
            let said: *const AnyClass = msg_send![window, class];
            let real = object_getClass(window.cast());
            let new = panel_class() as *const AnyClass;
            if said.cast::<c_void>() != real || class_getInstanceSize(real) != class_getInstanceSize(new.cast()) {
                eprintln!("The meeting pad stays a plain window (its class could not be changed safely).");
                return false;
            }
            object_setClass(window.cast(), new.cast());
            let mask: usize = msg_send![window, styleMask];
            // NSWindowStyleMaskNonactivatingPanel.
            let _: () = msg_send![window, setStyleMask: mask | (1usize << 7)];
            // The style alone did not reach the window server, because it
            // is given after the window was made: a press on the pad made
            // the app the active one, and when the pad then closed (its ×),
            // the app's own window came in front of the program the person
            // was in. The window says it to the window server itself.
            let tells: bool = msg_send![window, respondsToSelector: objc2::sel!(_setPreventsActivation:)];
            if tells {
                let _: () = msg_send![window, _setPreventsActivation: true];
            }
            if server_keeps_app_inactive(window) == Some(false) {
                tell_server_no_activation(window);
            }
            let _: () = msg_send![window, setFloatingPanel: true];
            let _: () = msg_send![window, setHidesOnDeactivate: false];
            let _: () = msg_send![window, setBecomesKeyOnlyIfNeeded: false];
            true
        }
    }

    /// (For the self-test.) Whether the window server itself knows the
    /// window as one that does not make its app the active one when it is
    /// pressed. The style of the window says so too, but the window server
    /// decides at a press, and a style given after the window was made did
    /// not reach it. None: this macOS does not tell.
    fn server_keeps_app_inactive(window: *mut AnyObject) -> Option<bool> {
        type Connection = unsafe extern "C" fn() -> i32;
        type Tags = unsafe extern "C" fn(i32, u32, *mut u32, i32) -> i32;
        // SAFETY: two functions of the window server's library, looked up by
        // name (so that a macOS without them is no fault); the shapes are
        // theirs, and the tags are two words of 32 bits.
        unsafe {
            let connection = libc::dlsym(libc::RTLD_DEFAULT, c"CGSMainConnectionID".as_ptr());
            let tags = libc::dlsym(libc::RTLD_DEFAULT, c"CGSGetWindowTags".as_ptr());
            if connection.is_null() || tags.is_null() {
                return None;
            }
            let connection: Connection = std::mem::transmute(connection);
            let tags: Tags = std::mem::transmute(tags);
            let number: isize = msg_send![window, windowNumber];
            let mut out = [0u32; 2];
            if tags(connection(), number as u32, out.as_mut_ptr(), 64) != 0 {
                return None;
            }
            // (The tag "prevents activation".)
            Some(out[0] & (1 << 16) != 0)
        }
    }

    /// Say to the window server that a press on this window does not make
    /// the app the active one, when the window's own way did not (see
    /// `make_panel`). Nothing happens on a macOS without the function.
    fn tell_server_no_activation(window: *mut AnyObject) {
        type Connection = unsafe extern "C" fn() -> i32;
        type Tags = unsafe extern "C" fn(i32, u32, *const u32, i32) -> i32;
        // SAFETY: as in `server_keeps_app_inactive`.
        unsafe {
            let connection = libc::dlsym(libc::RTLD_DEFAULT, c"CGSMainConnectionID".as_ptr());
            let tags = libc::dlsym(libc::RTLD_DEFAULT, c"CGSSetWindowTags".as_ptr());
            if connection.is_null() || tags.is_null() {
                return;
            }
            let connection: Connection = std::mem::transmute(connection);
            let tags: Tags = std::mem::transmute(tags);
            let number: isize = msg_send![window, windowNumber];
            let set = [1u32 << 16, 0];
            tags(connection(), number as u32, set.as_ptr(), 64);
        }
    }

    static FIRST_CLICK: Once = Once::new();
    static WAS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn first_click(this: &AnyObject, sel: Sel, event: *mut AnyObject) -> Bool {
        // SAFETY: `this` is a live view; `window` is a plain getter. The
        // earlier function has this same shape.
        unsafe {
            let window: *mut AnyObject = msg_send![this, window];
            if !window.is_null() {
                let ours: bool = msg_send![window, isKindOfClass: panel_class()];
                if ours {
                    return Bool::YES;
                }
            }
            match WAS.load(Ordering::Relaxed) {
                0 => Bool::NO,
                was => {
                    let was: extern "C" fn(&AnyObject, Sel, *mut AnyObject) -> Bool = std::mem::transmute(was);
                    was(this, sel, event)
                }
            }
        }
    }

    /// A click on the pad acts at once, though the pad does not have the
    /// keyboard (a web view by itself spends the first click on taking it).
    /// The web view's answer to "does the first click act?" becomes yes in a
    /// pad and stays what it was in every other window.
    pub fn first_click_acts() {
        FIRST_CLICK.call_once(|| {
            let Some(class) = AnyClass::get(c"WryWebView") else { return };
            // SAFETY: the class is wry's own and defines this method itself
            // (so nothing of WebKit's is changed); the new function has the
            // method's shape: (id, SEL, NSEvent *) -> BOOL.
            unsafe {
                let method = class_getInstanceMethod((class as *const AnyClass).cast(), sel!(acceptsFirstMouse:));
                if method.is_null() {
                    return;
                }
                let new = first_click as extern "C" fn(&AnyObject, Sel, *mut AnyObject) -> Bool;
                let was = method_setImplementation(method, new as *const c_void);
                WAS.store(was as usize, Ordering::Relaxed);
            }
        });
    }

    /// (For the self-test.) What macOS says about a window of the app that
    /// is not the pad: whether the app is the active one, whether the window
    /// has the keyboard, and its place among all windows on the screen,
    /// counted from the front (0: in front of all).
    pub fn main_facts(window: &Window) -> serde_json::Value {
        let window = object(window);
        if window.is_null() {
            return serde_json::json!({});
        }
        // SAFETY: the live window and the app; the messages take plain
        // integers or nothing, and the list holds numbers.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let active: bool = msg_send![app, isActive];
            let key: bool = msg_send![window, isKeyWindow];
            let number: isize = msg_send![window, windowNumber];
            // NSWindowNumberListAllApplications: the windows of every program, front to back.
            let all: *mut AnyObject = msg_send![class!(NSWindow), windowNumbersWithOptions: 1usize];
            let count: usize = msg_send![all, count];
            let mut order: i64 = -1;
            for i in 0..count {
                let n: *mut AnyObject = msg_send![all, objectAtIndex: i];
                let v: isize = msg_send![n, integerValue];
                if v == number {
                    order = i as i64;
                    break;
                }
            }
            serde_json::json!({ "active": active, "key": key, "order": order })
        }
    }

    /// Show the pad: on every desktop and beside a full-screen window, over
    /// normal windows, without the keyboard. `hidden`: not in a screen share
    /// or a recording (as far as macOS and the sharing program honour it).
    pub fn bring_out(window: &Window, hidden: bool) {
        window.set_content_protection(hidden);
        let window = object(window);
        if window.is_null() {
            return;
        }
        // SAFETY: the live window; the messages take plain integers or nothing.
        unsafe {
            // CanJoinAllSpaces | FullScreenAuxiliary, and only those: "managed"
            // (which Dioxus gives every window) cannot go with them.
            let _: () = msg_send![window, setCollectionBehavior: (1usize << 0) | (1usize << 8)];
            let _: () = msg_send![window, setLevel: LEVEL];
            let _: () = msg_send![window, invalidateShadow];
            let _: () = msg_send![window, orderFrontRegardless];
        }
    }

    /// (For the self-test.) A press and a release of the left mouse button
    /// at a place in the window (counted from its top left, in points),
    /// handed to the app as macOS hands it the events of the person's hand.
    /// It goes the whole way a real click goes: the window decides whether
    /// a click on a window without the keyboard acts (`first_click_acts`).
    pub fn press(window: &Window, x: f64, y: f64) {
        let window = object(window);
        if window.is_null() {
            return;
        }
        // The height of the window itself. (Not tao's `inner_size`: that is
        // the size of the view tao made first, which the web view replaced;
        // it stays what it was when the window is made bigger or smaller.)
        // SAFETY: the live window; `frame` returns a rectangle.
        let high = unsafe {
            let frame: Rect = msg_send![window, frame];
            frame.size.high
        };
        // SAFETY: the live window; the class method takes these nine values
        // (the event's kind, place, flags, time, window number, no context,
        // number, click count, pressure) and returns an event or nil.
        unsafe {
            let number: isize = msg_send![window, windowNumber];
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            let now: f64 = msg_send![info, systemUptime];
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let at = Point { x, y: high - y };
            // NSEventTypeLeftMouseDown, then NSEventTypeLeftMouseUp.
            for (kind, when) in [(1usize, now), (2usize, now + 0.05)] {
                let event: *mut AnyObject = msg_send![
                    class!(NSEvent),
                    mouseEventWithType: kind,
                    location: at,
                    modifierFlags: 0usize,
                    timestamp: when,
                    windowNumber: number,
                    context: std::ptr::null::<AnyObject>(),
                    eventNumber: 0isize,
                    clickCount: 1isize,
                    pressure: 1.0f32
                ];
                if !event.is_null() {
                    let _: () = msg_send![app, sendEvent: event];
                }
            }
        }
    }

    /// (For the self-test.) What macOS says about a pad window.
    pub fn facts(window: usize) -> serde_json::Value {
        let window = window as *mut AnyObject;
        // SAFETY: the live pad window (its number is forgotten as it closes); plain getters.
        unsafe {
            let panel: bool = msg_send![window, isKindOfClass: class!(NSPanel)];
            let mask: usize = msg_send![window, styleMask];
            let shown: bool = msg_send![window, isVisible];
            let on_space: bool = msg_send![window, isOnActiveSpace];
            // NSWindowOcclusionStateVisible.
            let occlusion: usize = msg_send![window, occlusionState];
            let key: bool = msg_send![window, isKeyWindow];
            let can_key: bool = msg_send![window, canBecomeKeyWindow];
            let level: isize = msg_send![window, level];
            let spaces: usize = msg_send![window, collectionBehavior];
            let sharing: usize = msg_send![window, sharingType];
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let active: bool = msg_send![app, isActive];
            // The view a press in the middle of the window lands on, and
            // whether it says that a first press acts.
            let content: *mut AnyObject = msg_send![window, contentView];
            let frame: Rect = msg_send![content, frame];
            let middle = Point { x: frame.size.wide / 2.0, y: frame.size.high / 2.0 };
            let hit: *mut AnyObject = msg_send![content, hitTest: middle];
            let (hit_class, first_press) = if hit.is_null() {
                (String::new(), false)
            } else {
                let class: *const AnyClass = msg_send![hit, class];
                let acts: bool = msg_send![hit, acceptsFirstMouse: std::ptr::null::<AnyObject>()];
                ((*class).name().to_string_lossy().into_owned(), acts)
            };
            let key_if_needed: bool = msg_send![window, becomesKeyOnlyIfNeeded];
            serde_json::json!({
                "hit": hit_class,
                "first_press": first_press,
                "key_if_needed": key_if_needed,
                "panel": panel,
                "nonactivating": mask & (1 << 7) != 0,
                "shown": shown,
                "on_space": on_space,
                "uncovered": occlusion & 2 != 0,
                "key": key,
                "can_key": can_key,
                "level": level,
                "all_spaces": spaces & 1 != 0,
                "beside_full_screen": spaces & (1 << 8) != 0,
                "hidden_from_shares": sharing == 0,
                "app_active": active,
                "server_nonactivating": server_keeps_app_inactive(window),
            })
        }
    }

    /// Give the pad the keyboard (the person pressed in its notepad). The
    /// app does not become the active one: that is what a panel of this
    /// kind is for.
    pub fn take_keyboard(window: &Window) {
        let window = object(window);
        if window.is_null() {
            return;
        }
        // SAFETY: the live window; the message takes nothing and returns nothing.
        unsafe {
            let _: () = msg_send![window, makeKeyWindow];
        }
    }

    /// Give the keyboard back to the program in front, if the pad has it
    /// (the notepad was made small, or the meeting is over).
    pub fn leave_keyboard(window: &Window) {
        let window = object(window);
        if window.is_null() {
            return;
        }
        // SAFETY: the live window; a panel that leaves the screen hands the
        // keyboard back, and comes back without it.
        unsafe {
            let has: bool = msg_send![window, isKeyWindow];
            if has {
                let _: () = msg_send![window, orderOut: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![window, orderFrontRegardless];
            }
        }
    }

    /// The part of a screen that windows may use (not the menu bar, not the
    /// Dock): left, top, wide, high, counted from the top left of the main
    /// screen as tao counts. Which screen: the one `at` is on (a point
    /// counted the same way), or with no point the one the pointer is on;
    /// the main screen if the point is on none.
    pub fn screen_area(at: Option<(f64, f64)>) -> Option<(f64, f64, f64, f64)> {
        // SAFETY: plain AppKit getters on the main thread.
        unsafe {
            let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
            if screens.is_null() {
                return None;
            }
            let count: usize = msg_send![screens, count];
            if count == 0 {
                return None;
            }
            // The first one is the main screen: AppKit counts heights from its bottom.
            let main: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
            let main_frame: Rect = msg_send![main, frame];
            let main_high = main_frame.size.high;
            let point = match at {
                Some((x, y)) => Point { x, y: main_high - y },
                None => msg_send![class!(NSEvent), mouseLocation],
            };
            let mut found: Rect = msg_send![main, visibleFrame];
            for i in 0..count {
                let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
                let frame: Rect = msg_send![screen, frame];
                let inside = point.x >= frame.at.x
                    && point.x < frame.at.x + frame.size.wide
                    && point.y >= frame.at.y
                    && point.y < frame.at.y + frame.size.high;
                if inside {
                    found = msg_send![screen, visibleFrame];
                    break;
                }
            }
            Some((found.at.x, main_high - (found.at.y + found.size.high), found.size.wide, found.size.high))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting_page() -> Vec<Block> {
        vec![
            Block::text("Nov 10, 2026"),
            Block::new(BlockKind::Heading2, "Notes"),
            Block::new(BlockKind::Bulleted, ""),
            Block::new(BlockKind::Heading2, "Action items"),
            Block::new(BlockKind::Todo, ""),
        ]
    }

    fn texts(page: &[Block]) -> Vec<String> {
        page.iter().map(|b| format!("{}:{}", b.kind.as_str(), b.text)).collect()
    }

    fn lines(list: &[&str]) -> Vec<String> {
        list.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn typed_lines_go_under_notes_and_follow_what_is_typed() {
        let mut page = meeting_page();
        let mut written = Vec::new();
        // The first line takes the place of the empty bullet of the template.
        assert!(put_lines(&mut page, &mut written, &lines(&["Beta moves to Friday"])));
        assert_eq!(texts(&page)[1..4], ["heading2:Notes", "bulleted:Beta moves to Friday", "heading2:Action items"]);
        // A second line comes after it; an empty line and spaces are nothing.
        assert!(put_lines(&mut page, &mut written, &lines(&["Beta moves to Friday", "  ", " Sam writes the notes "])));
        assert_eq!(texts(&page)[2..4], ["bulleted:Beta moves to Friday", "bulleted:Sam writes the notes"]);
        let first = page[2].id.clone();
        // The same lines again change nothing.
        assert!(!put_lines(&mut page, &mut written, &lines(&["Beta moves to Friday", "Sam writes the notes"])));
        // A line typed on is written in its own block.
        assert!(put_lines(&mut page, &mut written, &lines(&["Beta moves to Friday 14th", "Sam writes the notes"])));
        assert_eq!(page[2].id, first);
        assert_eq!(page[2].text, "Beta moves to Friday 14th");
        // A line taken out takes its block with it.
        assert!(put_lines(&mut page, &mut written, &lines(&["Sam writes the notes"])));
        assert_eq!(texts(&page)[1..4], ["heading2:Notes", "bulleted:Sam writes the notes", "heading2:Action items"]);
        assert_eq!(written.len(), 1);
        // All of them out: the heading stays, with nothing of the pad under it.
        assert!(put_lines(&mut page, &mut written, &[]));
        assert_eq!(texts(&page)[1..3], ["heading2:Notes", "heading2:Action items"]);
    }

    #[test]
    fn what_the_person_does_in_the_page_is_kept() {
        let mut page = meeting_page();
        let mut written = Vec::new();
        put_lines(&mut page, &mut written, &lines(&["One", "Two"]));
        // A line of their own, written in the page between the pad's two.
        page.insert(3, Block::new(BlockKind::Bulleted, "Mine"));
        // The pad's second line was changed in the page: when the pad drops
        // that line, the block stays (it is no longer what the pad wrote).
        page[4].text = "Two, and more".into();
        put_lines(&mut page, &mut written, &lines(&["One"]));
        assert_eq!(texts(&page)[2..5], ["bulleted:One", "bulleted:Mine", "bulleted:Two, and more"]);
        // A new line of the pad comes after the pad's last one, not at the end.
        put_lines(&mut page, &mut written, &lines(&["One", "Three"]));
        assert_eq!(texts(&page)[2..6], ["bulleted:One", "bulleted:Three", "bulleted:Mine", "bulleted:Two, and more"]);
        // A pad line the person took out of the page stays out while it is not typed on…
        page.remove(3);
        assert!(!put_lines(&mut page, &mut written, &lines(&["One", "Three"])));
        assert_eq!(texts(&page)[2..4], ["bulleted:One", "bulleted:Mine"]);
        // …and is made again when it is.
        assert!(put_lines(&mut page, &mut written, &lines(&["One", "Three!"])));
        assert_eq!(texts(&page)[2..4], ["bulleted:One", "bulleted:Three!"]);
    }

    #[test]
    fn signs_are_kept_as_typed_and_a_page_with_no_notes_part_gets_one() {
        let mut page = vec![Block::text("Just a page")];
        let mut written = Vec::new();
        put_lines(&mut page, &mut written, &lines(&["2 * 3 = 6, see [this] and _that_"]));
        assert_eq!(page[1].plain_text(), "Notes");
        assert_eq!(page[2].plain_text(), "2 * 3 = 6, see [this] and _that_");
    }

    #[test]
    fn the_style_file_closes_every_block_and_the_pad_rules_stand_alone() {
        // A block left open (a merge did it once) puts every later rule
        // inside it, where it applies to nothing: the pad then has no look.
        let css = include_str!("../assets/style.css");
        let mut depth = 0i32;
        let mut in_comment = false;
        let mut pad_rules = 0;
        for (n, line) in css.lines().enumerate() {
            if !in_comment && depth == 0 && (line.starts_with(".pad-card") || line.starts_with(".padwin")) {
                pad_rules += 1;
            }
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                match (in_comment, c, chars.peek()) {
                    (false, '/', Some('*')) => in_comment = true,
                    (true, '*', Some('/')) => {
                        in_comment = false;
                        chars.next();
                    }
                    (false, '{', _) => depth += 1,
                    (false, '}', _) => depth -= 1,
                    _ => {}
                }
            }
            assert!(depth >= 0, "a block is closed too often at line {}", n + 1);
            assert!(depth <= 3, "blocks are nested too deep at line {} (one was left open before it)", n + 1);
            if !in_comment && line.starts_with(".pad-") {
                assert_eq!(depth - line.matches('{').count() as i32, 0, "the rule at line {} is inside another rule's block", n + 1);
            }
        }
        assert_eq!(depth, 0, "a block of the style file is left open");
        assert!(pad_rules >= 2, "the pad's rules are at the top level of the style file");
    }

    #[test]
    fn the_questions_go_away_by_themselves() {
        // The first card: the small form after half a minute, never gone.
        assert_eq!(by_itself(Form::Ask, 0, ASK_MS - 1, false, false), None);
        assert_eq!(by_itself(Form::Ask, 0, ASK_MS, false, false), Some(ByItself::Fold));
        // The notepad and the small form stay for the whole meeting.
        assert_eq!(by_itself(Form::Open, 0, 10 * END_MS, false, false), None);
        assert_eq!(by_itself(Form::Small, 0, 10 * END_MS, false, false), None);
        // The last card: two minutes with a question, a few seconds without,
        // and not while the notes are still being written.
        assert_eq!(by_itself(Form::End, 0, END_MS - 1, true, false), None);
        assert_eq!(by_itself(Form::End, 0, END_MS, true, false), Some(ByItself::Go));
        assert_eq!(by_itself(Form::End, 0, TOLD_MS, false, false), Some(ByItself::Go));
        assert_eq!(by_itself(Form::End, 0, 10 * END_MS, true, true), None);
        assert_eq!(by_itself(Form::Kept, 0, KEPT_MS, false, false), Some(ByItself::Go));
    }

    #[test]
    fn the_pad_sits_at_the_corner_or_where_it_was_put() {
        let screen = (0.0, 25.0, 1440.0, 800.0);
        assert_eq!(place_in(screen, (328.0, 150.0), CORNER), (1440.0 - 20.0 - 328.0, 825.0 - 20.0 - 150.0));
        // Where the person put it: the same corner of the pad stays there as it grows.
        assert_eq!(place_in(screen, (328.0, 270.0), (300.0, 200.0)), (1440.0 - 300.0 - 328.0, 825.0 - 200.0 - 270.0));
        // A place that is not on this screen: the usual corner.
        assert_eq!(place_in(screen, (328.0, 150.0), (2000.0, 40.0)), place_in(screen, (328.0, 150.0), CORNER));
        assert_eq!(place_in(screen, (328.0, 150.0), (-30.0, 40.0)), place_in(screen, (328.0, 150.0), CORNER));
        // A second screen, to the left of the main one.
        assert_eq!(place_in((-1920.0, 0.0, 1920.0, 1080.0), (328.0, 150.0), CORNER), (-20.0 - 328.0, 1080.0 - 20.0 - 150.0));
        assert_eq!(clock(724), "12:04");
        assert_eq!(clock(-3), "0:00");
    }

    #[test]
    fn the_first_window_and_the_pad_window_stay_in_step() {
        close();
        let view = |title: &str| View { title: title.into(), ..View::default() };
        // The first view asks for a window; the same view again asks for nothing.
        assert!(show(view("Apollo standup")));
        assert!(!show(view("Apollo standup")));
        // A view that changes while the window is opening: no second window.
        assert!(!show(view("Apollo standup, later")));
        // The window opens: it draws the newest view, and gets the next ones.
        let (number, first, mut later) = attach();
        assert_eq!(first.map(|v| v.title), Some("Apollo standup, later".to_string()));
        assert!(!show(View { form: Form::Open, ..view("Apollo standup, later") }));
        assert!(matches!(later.try_next(), Ok(Some(ToPad::Show(v))) if v.form == Form::Open));
        // The pad goes: the window is told, and the next meeting gets a new one.
        close();
        assert!(matches!(later.try_next(), Ok(Some(ToPad::Close))));
        detach(number);
        assert!(show(view("Project A review")));
        // That pad goes before its window has opened: the window finds nothing to draw.
        close();
        let (_, nothing, _) = attach();
        assert!(nothing.is_none());
        // What the pad asks reaches the first window.
        let mut asked = owner();
        ask(Command::Listen);
        ask(Command::Keep("page-1".into()));
        assert_eq!(asked.try_next().ok().flatten(), Some(Command::Listen));
        assert_eq!(asked.try_next().ok().flatten(), Some(Command::Keep("page-1".into())));
    }
}
