//! Keeps every window, and the app and AI agents, in step.
//!
//! Each window holds its own copy of the workspace in memory. When one window
//! saves a change it also hands the new data to the others. Changes written by
//! another process (the MCP server used by Aven or Claude) are noticed through
//! SQLite's `data_version` and reloaded everywhere.

use crate::model::{Block, Event, Id, Page, Space};
use crate::store::{self, Loaded};
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub enum SyncMsg {
    Page(Page),
    Blocks(Id, Vec<Block>),
    Removed(Vec<Id>),
    Theme(String),
    Spaces(Vec<Space>),
    Event(Event),
    EventRemoved(Id),
    /// Every event as it is on disk, after an outside change.
    Events(Vec<Event>),
    Look { glass: u8, blur: u8, tint: String, accent: String },
    /// Everything as it is on disk, after an outside change.
    Reload(Rc<Loaded>),
}

thread_local! {
    static PEERS: RefCell<Vec<(u64, UnboundedSender<SyncMsg>)>> = const { RefCell::new(Vec::new()) };
    static WAITING: RefCell<HashMap<u64, UnboundedReceiver<SyncMsg>>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
    static SEEN_VERSION: Cell<Option<i64>> = const { Cell::new(None) };
}

/// Register a window. Its messages wait until the window collects them.
pub fn join() -> u64 {
    let key = NEXT.with(|n| {
        let k = n.get();
        n.set(k + 1);
        k
    });
    let (tx, rx) = unbounded();
    PEERS.with(|p| p.borrow_mut().push((key, tx)));
    WAITING.with(|w| w.borrow_mut().insert(key, rx));
    key
}

pub fn take_receiver(key: u64) -> Option<UnboundedReceiver<SyncMsg>> {
    WAITING.with(|w| w.borrow_mut().remove(&key))
}

pub fn leave(key: u64) {
    PEERS.with(|p| p.borrow_mut().retain(|(k, _)| *k != key));
    WAITING.with(|w| w.borrow_mut().remove(&key));
    crate::drawing::forget_window(key);
}

pub fn window_count() -> usize {
    PEERS.with(|p| p.borrow().len())
}

/// Tell every other window.
pub fn others(from: u64, msg: SyncMsg) {
    PEERS.with(|p| {
        for (k, tx) in p.borrow().iter() {
            if *k != from {
                let _ = tx.unbounded_send(msg.clone());
            }
        }
    });
}

fn everyone(msg: SyncMsg) {
    PEERS.with(|p| {
        for (_, tx) in p.borrow().iter() {
            let _ = tx.unbounded_send(msg.clone());
        }
    });
}

/// Whether another program wrote to the workspace since the windows last
/// took in what is on disk (the timer below will reload it soon).
pub fn outside_change_pending() -> bool {
    let Some(version) = store::with(|s| Ok(s.data_version())) else { return false };
    SEEN_VERSION.with(|v| v.get()).is_some_and(|seen| seen != version)
}

/// Everything again as it is on disk, in every window: for a change that
/// the app wrote to the notes file itself, round the windows' own copies
/// (it takes back what an AI did, see mcp.rs).
pub fn reload_now() {
    if let Some(loaded) = store::with(|s| s.load()) {
        everyone(SyncMsg::Reload(Rc::new(loaded)));
    }
    if let Some(events) = store::with(|s| s.load_events()) {
        everyone(SyncMsg::Events(events));
    }
}

/// Called on a timer by every window. The first to notice that another
/// process wrote to the database reloads it and tells all windows.
pub fn check_outside_changes() {
    let Some(version) = store::with(|s| Ok(s.data_version())) else { return };
    let seen = SEEN_VERSION.with(|v| v.replace(Some(version)));
    if seen.is_some_and(|s| s != version) {
        if let Some(loaded) = store::with(|s| s.load()) {
            everyone(SyncMsg::Reload(Rc::new(loaded)));
        }
        if let Some(events) = store::with(|s| s.load_events()) {
            everyone(SyncMsg::Events(events));
        }
    }
}
