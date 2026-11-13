//! Locked pages: a page whose lines are kept scrambled with a password of
//! its own.
//!
//! On disk a locked page has one block, of the kind `Locked`, whose data is
//! all its lines (as JSON) sealed with AES-256-GCM. The key comes from the
//! password by PBKDF2-HMAC-SHA256 with a salt of the page's own; the page's
//! id is bound into the seal, so a sealed block cannot be moved to another
//! page. The format, in the block's data:
//!
//! `rune-lock-1:<rounds>:<salt>:<nonce>:<sealed lines>` (the last three in
//! standard base64).
//!
//! Its title, icon, place and space stay as they are: only its lines are
//! sealed. Sync, backups, search, the connector for AI tools and page
//! history see only the sealed block. While the page is open in this app,
//! its key is held here (`KEYS`): the store seals the lines again at every
//! save (store.rs) and opens them when it reads them. The page locks again
//! when no tab shows it, and when the app quits (the keys live only in
//! memory). There is no way back without the password.

use crate::model::{Block, BlockKind, Id};
use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::Mutex;

const PREFIX: &str = "rune-lock-1";
/// PBKDF2 rounds for a new lock (about half a second on an M1).
pub const ROUNDS: u32 = 600_000;
/// More rounds than this in a sealed line are not tried: such a line comes
/// from a damaged file or another device, and "Opening…" would never end.
const MAX_ROUNDS: u32 = 10_000_000;
/// The shortest password Rune takes.
pub const MIN_PASSWORD: usize = 6;

/// A page's key, from its password: what is needed to seal it again.
#[derive(Clone)]
pub struct Key {
    key: [u8; 32],
    salt: Vec<u8>,
    rounds: u32,
}

// (The key never shows in a log.)
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

/// The keys of the locked pages that are open in this app now, with the
/// window that opened each (it locks the page again when it shows it no more).
static KEYS: Mutex<Option<HashMap<Id, (Key, u64)>>> = Mutex::new(None);
/// Pages whose password is being removed: their plain lines may take the
/// place of the sealed block once.
static UNSEALING: Mutex<Option<HashSet<Id>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LockState {
    /// No password.
    #[default]
    None,
    /// Locked, and its lines are not shown.
    Shut,
    /// Locked, and open in this app with its password.
    Open,
}

impl LockState {
    pub fn is_locked(self) -> bool {
        self != LockState::None
    }
    pub fn is_open(self) -> bool {
        self == LockState::Open
    }
}

#[derive(Debug, PartialEq)]
pub enum LockError {
    /// The password does not open the page.
    Wrong,
    /// The sealed block cannot be read (damaged, or from a newer Rune).
    Unreadable,
}

/// The sealed block of a page whose blocks are only that.
pub fn sealed(blocks: &[Block]) -> Option<&Block> {
    match blocks {
        [b] if b.kind == BlockKind::Locked => Some(b),
        // Rune 0.1.17 and older do not know the kind: they read the line
        // as text, and a save of the page there writes it back as text,
        // with the sealed lines still in it. Told by its kind alone, the
        // page then showed here as one empty line with no password box,
        // and the next save lost the sealed lines for good.
        [b] if b.kind == BlockKind::Text && b.data.strip_prefix(PREFIX).is_some_and(|rest| rest.starts_with(':')) => Some(b),
        _ => None,
    }
}

/// The page's key is held: it is open in this app.
pub fn is_open(page_id: &str) -> bool {
    KEYS.lock().ok().is_some_and(|k| k.as_ref().is_some_and(|k| k.contains_key(page_id)))
}

/// What a page's lines say must be kept from search and the like: they are
/// sealed, or the page is locked and open.
pub fn hides(blocks: &[Block], page_id: &str) -> bool {
    sealed(blocks).is_some() || is_open(page_id)
}

/// Whether a page is locked, and open, in this window's state.
pub fn page_lock(state: &crate::state::AppState, page_id: &str) -> LockState {
    use dioxus::prelude::*;
    if is_open(page_id) {
        return LockState::Open;
    }
    let blocks = state.blocks.read();
    match blocks.get(page_id).and_then(|bs| sealed(bs)) {
        Some(_) => LockState::Shut,
        None => LockState::None,
    }
}

/// The ids of the locked pages that are open.
pub fn open_pages() -> Vec<Id> {
    KEYS.lock().ok().and_then(|k| k.as_ref().map(|k| k.keys().cloned().collect())).unwrap_or_default()
}

/// The ids of the locked pages that a window opened.
fn opened_by(window: u64) -> Vec<Id> {
    KEYS.lock()
        .ok()
        .and_then(|k| k.as_ref().map(|k| k.iter().filter(|(_, (_, w))| *w == window).map(|(id, _)| id.clone()).collect()))
        .unwrap_or_default()
}

/// A window was closed: the pages it opened lock again, as they do when
/// its tabs show them no more. The other windows got their plain lines
/// when they were opened (`show_lines`): they get the sealed line back.
/// (The key stayed before, and the page was open in the other windows with
/// no password until the app quit.)
pub fn window_closed(window: u64) {
    let listening = crate::state::listening_in();
    for id in opened_by(window) {
        // Another window listens in this page: the page stays open for
        // it, and that window locks it again (see `relock_unshown`). With
        // the key forgotten, each write of the transcript was refused, and
        // the rest of the meeting was never written.
        if let Some((listener, _)) = listening.as_ref().filter(|(listener, page)| *listener != window && *page == id) {
            pass_to(&id, *listener);
            continue;
        }
        forget(&id);
        let sealed_lines = crate::store::with(|s| s.load_page_blocks(&id)).unwrap_or_default();
        crate::sync::others(window, crate::sync::SyncMsg::Blocks(id, sealed_lines));
    }
}

/// The page's key goes to another window: that one locks the page again.
fn pass_to(page_id: &str, window: u64) {
    if let Ok(mut k) = KEYS.lock() {
        if let Some((_, of)) = k.as_mut().and_then(|k| k.get_mut(page_id)) {
            *of = window;
        }
    }
}

pub fn hold(page_id: &str, key: Key, window: u64) {
    if let Ok(mut k) = KEYS.lock() {
        k.get_or_insert_with(HashMap::new).insert(page_id.to_string(), (key, window));
    }
}

pub fn forget(page_id: &str) {
    if let Ok(mut k) = KEYS.lock() {
        if let Some(k) = k.as_mut() {
            k.remove(page_id);
        }
    }
}

fn key_of(page_id: &str) -> Option<Key> {
    KEYS.lock().ok().and_then(|k| k.as_ref().and_then(|k| k.get(page_id).map(|(key, _)| key.clone())))
}

fn is_unsealing(page_id: &str) -> bool {
    UNSEALING.lock().ok().is_some_and(|u| u.as_ref().is_some_and(|u| u.contains(page_id)))
}

/// While `f` runs, the page's plain lines may be written over its sealed
/// block (its password is being removed).
pub fn unsealing<T>(page_id: &str, f: impl FnOnce() -> T) -> T {
    if let Ok(mut u) = UNSEALING.lock() {
        u.get_or_insert_with(HashSet::new).insert(page_id.to_string());
    }
    let out = f();
    if let Ok(mut u) = UNSEALING.lock() {
        if let Some(u) = u.as_mut() {
            u.remove(page_id);
        }
    }
    out
}

/// A plain write may take the place of the page's sealed block now.
pub fn may_unseal(page_id: &str) -> bool {
    is_open(page_id) || is_unsealing(page_id)
}

// ----- the store's side --------------------------------------------------------

/// What the store writes for a page: the same blocks, or (for a page that
/// is open with its key) the one sealed block in their place.
pub fn for_store_write(page_id: &str, blocks: &[Block]) -> Option<Block> {
    if sealed(blocks).is_some() || is_unsealing(page_id) {
        return None;
    }
    let key = key_of(page_id)?;
    Some(seal_with(&key, page_id, blocks))
}

/// What the store gives back for a page: its lines opened when its key is
/// held. A page that is no longer sealed (its password was removed on
/// another device) lets go of its key.
pub fn for_store_read(page_id: &str, blocks: Vec<Block>) -> Vec<Block> {
    let Some(key) = key_of(page_id) else { return blocks };
    let Some(block) = sealed(&blocks) else {
        if !is_unsealing(page_id) {
            forget(page_id);
        }
        return blocks;
    };
    match open_with_key(&key, page_id, &block.data) {
        Ok(plain) => plain,
        // Sealed with another password (changed on another device): the
        // page shows as locked again.
        Err(_) => {
            forget(page_id);
            blocks
        }
    }
}

// ----- sealing -------------------------------------------------------------------

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// The id of a page's sealed block (the same at every save).
pub fn sealed_id(page_id: &str) -> Id {
    format!("lock-{page_id}")
}

fn derive(password: &str, salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    let rounds = NonZeroU32::new(rounds.max(1)).expect("not zero");
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, password.as_bytes(), &mut key);
    key
}

/// A new key for a password, with a new salt. Slow on purpose (see
/// `ROUNDS`): call it off the UI thread.
pub fn new_key(password: &str) -> Key {
    new_key_with(password, ROUNDS)
}

fn new_key_with(password: &str, rounds: u32) -> Key {
    let mut salt = vec![0u8; 16];
    SystemRandom::new().fill(&mut salt).expect("random bytes");
    Key { key: derive(password, &salt, rounds), salt, rounds }
}

/// The page's lines sealed with its key, as its one block.
pub fn seal_with(key: &Key, page_id: &str, blocks: &[Block]) -> Block {
    let mut data = serde_json::to_vec(blocks).unwrap_or_else(|_| b"[]".to_vec());
    let mut nonce = [0u8; 12];
    SystemRandom::new().fill(&mut nonce).expect("random bytes");
    let sealer = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key.key).expect("a 32-byte key"));
    sealer
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(page_id.as_bytes()), &mut data)
        .expect("sealing does not fail for this size");
    let mut block = Block::new(BlockKind::Locked, "");
    block.id = sealed_id(page_id);
    block.data = format!("{PREFIX}:{}:{}:{}:{}", key.rounds, b64().encode(&key.salt), b64().encode(nonce), b64().encode(&data));
    block
}

struct Parts {
    rounds: u32,
    salt: Vec<u8>,
    nonce: [u8; 12],
    sealed: Vec<u8>,
}

fn parts(data: &str) -> Result<Parts, LockError> {
    let mut it = data.split(':');
    if it.next() != Some(PREFIX) {
        return Err(LockError::Unreadable);
    }
    let rounds: u32 = it.next().and_then(|r| r.parse().ok()).filter(|r| (1..=MAX_ROUNDS).contains(r)).ok_or(LockError::Unreadable)?;
    let mut next = || it.next().and_then(|p| b64().decode(p).ok()).ok_or(LockError::Unreadable);
    let salt = next()?;
    let nonce: [u8; 12] = next()?.try_into().map_err(|_| LockError::Unreadable)?;
    let sealed = next()?;
    Ok(Parts { rounds, salt, nonce, sealed })
}

fn open_parts(key: &[u8; 32], page_id: &str, p: &Parts) -> Result<Vec<Block>, LockError> {
    let opener = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| LockError::Unreadable)?);
    let mut buf = p.sealed.clone();
    let plain = opener
        .open_in_place(Nonce::assume_unique_for_key(p.nonce), Aad::from(page_id.as_bytes()), &mut buf)
        .map_err(|_| LockError::Wrong)?;
    serde_json::from_slice(plain).map_err(|_| LockError::Unreadable)
}

fn open_with_key(key: &Key, page_id: &str, data: &str) -> Result<Vec<Block>, LockError> {
    let p = parts(data)?;
    if p.salt != key.salt {
        return Err(LockError::Wrong);
    }
    open_parts(&key.key, page_id, &p)
}

/// Open a sealed block with a password: its key and its lines. Slow on
/// purpose (see `ROUNDS`): call it off the UI thread.
pub fn open(password: &str, page_id: &str, sealed: &Block) -> Result<(Key, Vec<Block>), LockError> {
    let p = parts(&sealed.data)?;
    let key = derive(password, &p.salt, p.rounds);
    let blocks = open_parts(&key, page_id, &p)?;
    Ok((Key { key, salt: p.salt, rounds: p.rounds }, blocks))
}

/// Run slow work (the password's key) on a thread of its own, and wait for
/// it without holding up the window.
async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.await.ok()
}

// ----- the app's side --------------------------------------------------------------

impl crate::state::AppState {
    /// Put the lines of a page in this window, and in the others, without
    /// saving them (they are on disk as they should be).
    fn show_lines(self, page_id: &str, blocks: Vec<Block>) {
        use dioxus::prelude::*;
        let mut sig = self.blocks;
        sig.write().insert(page_id.to_string(), blocks.clone());
        crate::sync::others(self.win.key, crate::sync::SyncMsg::Blocks(page_id.to_string(), blocks));
    }

    /// Lock a page with a password: its lines are sealed on disk, its
    /// earlier versions are forgotten, and it shows as locked.
    /// Rune listens in this page (in any window): it is not locked now.
    /// The transcript goes into it, and a page that is shut takes no
    /// lines. True, with a word to the person, when that is so.
    pub fn listens_in(self, page_id: &str) -> bool {
        let here = crate::state::listening_in().is_some_and(|(_, page)| page == page_id);
        if here {
            self.toast("Rune listens in this page. Stop listening first.");
        }
        here
    }

    pub async fn lock_with(self, page_id: String, password: String) {
        use dioxus::prelude::*;
        if self.listens_in(&page_id) {
            return;
        }
        if self.blocks.peek().get(&page_id).is_none_or(|blocks| sealed(blocks).is_some()) {
            return;
        }
        let Some(key) = off_thread(move || new_key(&password)).await else { return };
        // The lines as they are now: the key took half a second, and what
        // another window, a transcript or an AI tool wrote in that time was
        // written over by the lines of before (with no history to find it in).
        let on_disk = if crate::sync::outside_change_pending() { crate::store::with(|s| s.load_page_blocks(&page_id)).filter(|b| !b.is_empty()) } else { None };
        let Some(blocks) = on_disk.or_else(|| self.blocks.peek().get(&page_id).cloned()) else { return };
        if sealed(&blocks).is_some() {
            return;
        }
        hold(&page_id, key, self.win.key);
        let saved = crate::store::with(|s| s.lock_page(&page_id, &blocks, crate::model::now_ms()));
        forget(&page_id);
        if saved.is_none() {
            self.notice("The page could not be locked. Nothing was changed.");
            return;
        }
        self.shut(&page_id);
        self.toast("Page locked");
        // The log of changes held the plain lines: it is emptied. A program
        // that reads the notes file at this moment holds that up, so it is
        // tried again for a minute. (One try, its result not looked at,
        // left the plain lines in the log until some later change.)
        if crate::store::with(|s| Ok(s.checkpoint())) == Some(false) {
            dioxus::core::spawn_forever(async {
                for _ in 0..30 {
                    crate::ai::sleep_ms(2000).await;
                    if crate::store::with(|s| Ok(s.checkpoint())) != Some(false) {
                        break;
                    }
                }
            });
        }
    }

    /// Open a locked page with its password, for as long as a tab shows it.
    pub async fn unlock_with(self, page_id: String, password: String) -> Result<(), LockError> {
        use dioxus::prelude::*;
        let block = self.blocks.peek().get(&page_id).and_then(|bs| sealed(bs).cloned()).ok_or(LockError::Unreadable)?;
        let id = page_id.clone();
        let (key, blocks) = off_thread(move || open(&password, &id, &block)).await.ok_or(LockError::Unreadable)??;
        hold(&page_id, key, self.win.key);
        self.show_lines(&page_id, blocks);
        Ok(())
    }

    /// The lines as they are on disk, sealed, in place of the open ones.
    fn shut(mut self, page_id: &str) {
        use dioxus::prelude::*;
        let sealed_lines = crate::store::with(|s| s.load_page_blocks(page_id)).unwrap_or_default();
        self.history.write().forget(page_id);
        self.show_lines(page_id, sealed_lines);
    }

    /// Lock an open page again (no tab shows it, or "Lock page now").
    pub fn relock(self, page_id: &str) {
        if !is_open(page_id) || self.listens_in(page_id) {
            return;
        }
        forget(page_id);
        self.shut(page_id);
    }

    /// Take the password away from an open page: its lines are plain on
    /// disk again.
    pub fn remove_password(mut self, page_id: &str) {
        use dioxus::prelude::*;
        if !is_open(page_id) {
            return;
        }
        let Some(blocks) = self.blocks.peek().get(page_id).cloned() else { return };
        let saved = unsealing(page_id, || crate::store::with(|s| s.save_blocks_at(page_id, &blocks, crate::model::now_ms())));
        if saved.is_none() {
            self.notice("The password could not be removed.");
            return;
        }
        forget(page_id);
        self.history.write().forget(page_id);
        self.show_lines(page_id, blocks);
        self.toast("Password removed");
    }

    /// Lock again each page this window opened that no tab of it shows.
    pub fn relock_unshown(self) {
        use dioxus::prelude::*;
        let open = opened_by(self.win.key);
        if open.is_empty() {
            return;
        }
        let shown: HashSet<Id> = {
            let ui = self.ui.peek();
            ui.tabs
                .iter()
                .chain(ui.parked.values().flat_map(|set| set.0.iter()))
                .flat_map(|t| t.ordered())
                .filter_map(|p| match p.route() {
                    crate::state::Route::Page(id) => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        // A page Rune listens in stays open until the listening stops: its
        // transcript goes into it, and a page that is shut takes no lines.
        // The app listens for one window, which may not be this one: the
        // page is then that window's to lock again, when the listening
        // stops (it calls this) or its own tabs show the page no more.
        let listening = crate::state::listening_in();
        for id in open.iter().filter(|id| !shown.contains(*id)) {
            match &listening {
                Some((listener, page)) if page == id => pass_to(id, *listener),
                _ => self.relock(id),
            }
        }
    }
}

#[cfg(test)]
pub fn test_key(password: &str) -> Key {
    new_key_with(password, 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<Block> {
        let mut todo = Block::new(BlockKind::Todo, "Call the bank");
        todo.checked = true;
        let mut out = vec![Block::new(BlockKind::Heading1, "Private"), Block::text("My account is 1234"), todo];
        for (i, b) in out.iter_mut().enumerate() {
            b.id = format!("line-{i}");
        }
        out
    }

    #[test]
    fn a_sealed_page_opens_with_its_password_and_no_other() {
        let key = test_key("correct horse");
        let block = seal_with(&key, "p1", &lines());
        assert_eq!(block.kind, BlockKind::Locked);
        assert_eq!(block.id, "lock-p1");
        assert!(block.text.is_empty());
        assert!(!block.data.contains("bank") && !block.data.contains("1234"), "nothing of the lines shows: {}", block.data);
        let (_, back) = open("correct horse", "p1", &block).unwrap();
        assert_eq!(back, lines());
        assert_eq!(open("wrong horse", "p1", &block).unwrap_err(), LockError::Wrong);
    }

    #[test]
    fn a_sealed_block_does_not_open_on_another_page() {
        let key = test_key("pw-pw-pw");
        let block = seal_with(&key, "p1", &lines());
        assert_eq!(open("pw-pw-pw", "p2", &block).unwrap_err(), LockError::Wrong);
    }

    #[test]
    fn each_seal_is_new_and_a_damaged_one_is_unreadable() {
        let key = test_key("pw-pw-pw");
        let a = seal_with(&key, "p1", &lines());
        let b = seal_with(&key, "p1", &lines());
        assert_ne!(a.data, b.data, "a new nonce each time");
        let mut bad = a.clone();
        bad.data = "rune-lock-1:1000:xx".into();
        assert_eq!(open("pw-pw-pw", "p1", &bad).unwrap_err(), LockError::Unreadable);
        bad.data = "something else".into();
        assert_eq!(open("pw-pw-pw", "p1", &bad).unwrap_err(), LockError::Unreadable);
        // A count of rounds that would keep "Opening…" busy for hours is not tried.
        bad.data = a.data.replacen(":1000:", ":4294967295:", 1);
        assert_ne!(bad.data, a.data);
        assert_eq!(open("pw-pw-pw", "p1", &bad).unwrap_err(), LockError::Unreadable);
    }

    #[test]
    fn the_store_keeps_a_locked_page_sealed_and_refuses_plain_lines_over_it() {
        use crate::store::Store;
        let page = "store-lock-page";
        let store = Store::open_in_memory().unwrap();
        store.save_blocks(page, &lines()).unwrap();
        store.save_version(page, 1, "Private", &lines()).unwrap();
        // Locked: the lines go to disk sealed, and the history goes.
        hold(page, test_key("pw-pw-pw"), 0);
        store.lock_page(page, &lines(), 2).unwrap();
        assert_eq!(store.load_page_blocks(page).unwrap(), lines(), "open here: read back plain");
        store.save_version(page, 3, "Private", &lines()).unwrap();
        assert!(store.load_versions(page).unwrap().is_empty(), "no history while locked");
        forget(page);
        let on_disk = store.load_page_blocks(page).unwrap();
        assert!(sealed(&on_disk).is_some(), "shut: only the sealed block");
        // Plain lines (an AI tool, a meeting's notes) may not take its place.
        assert!(store.save_blocks(page, &[Block::text("overwritten")]).is_err());
        assert!(store.append_blocks(page, &[Block::text("added")], 4).is_err());
        assert!(sealed(&store.load_page_blocks(page).unwrap()).is_some());
        // Open, a line is added inside the seal.
        let (key, _) = open("pw-pw-pw", page, &on_disk[0]).unwrap();
        hold(page, key, 0);
        store.append_blocks(page, &[Block::text("added")], 5).unwrap();
        let back = store.load_page_blocks(page).unwrap();
        assert_eq!(back.last().unwrap().text, "added");
        // The password is removed: the lines are plain again.
        forget(page);
        unsealing(page, || store.save_blocks(page, &back)).unwrap();
        assert_eq!(store.load_page_blocks(page).unwrap(), back);
    }

    #[test]
    fn a_lock_that_cannot_remove_the_history_changes_nothing() {
        use crate::store::Store;
        let page = "half-lock-page";
        let store = Store::open_in_memory().unwrap();
        store.save_blocks(page, &lines()).unwrap();
        store.save_version(page, 1, "Private", &lines()).unwrap();
        // The history cannot be removed (as when another program holds the file).
        store.run_for_test("CREATE TRIGGER no_forgetting BEFORE DELETE ON page_versions BEGIN SELECT RAISE(ABORT, 'held'); END;");
        hold(page, test_key("pw-pw-pw"), 0);
        assert!(store.lock_page(page, &lines(), 2).is_err());
        forget(page);
        assert_eq!(store.load_page_blocks(page).unwrap(), lines(), "not sealed: nothing was changed");
        assert_eq!(store.load_versions(page).unwrap().len(), 1);
        // With nothing in the way: sealed, and the history gone, together.
        store.run_for_test("DROP TRIGGER no_forgetting;");
        hold(page, test_key("pw-pw-pw"), 0);
        store.lock_page(page, &lines(), 3).unwrap();
        forget(page);
        assert!(sealed(&store.load_page_blocks(page).unwrap()).is_some());
        assert!(store.load_versions(page).unwrap().is_empty());
        assert!(store.checkpoint(), "nothing reads the file: the log is emptied");
    }

    #[test]
    fn a_sealed_line_that_an_older_rune_saved_as_text_is_still_a_locked_page() {
        use crate::store::Store;
        let page = "older-app-page";
        let store = Store::open_in_memory().unwrap();
        store.save_blocks(page, &[seal_with(&test_key("pw-pw-pw"), page, &lines())]).unwrap();
        // Rune 0.1.17 read the kind it did not know as text, and saved the page so.
        store.run_for_test(&format!("UPDATE blocks SET kind = 'text' WHERE page_id = '{page}'"));
        let on_disk = store.load_page_blocks(page).unwrap();
        assert_eq!(on_disk[0].kind, BlockKind::Text);
        let found = sealed(&on_disk).expect("still a locked page: it asks for its password");
        // Plain lines may still not take its place.
        assert!(store.save_blocks(page, &[Block::text("overwritten")]).is_err());
        assert!(store.append_blocks(page, &[Block::text("added")], 1).is_err());
        // Its password opens it, and the next save writes a locked line again.
        let (key, back) = open("pw-pw-pw", page, found).unwrap();
        assert_eq!(back, lines());
        hold(page, key, 0);
        assert_eq!(store.load_page_blocks(page).unwrap(), lines());
        store.save_blocks(page, &lines()).unwrap();
        forget(page);
        assert_eq!(store.load_page_blocks(page).unwrap()[0].kind, BlockKind::Locked);
        // Words that look like the mark are words; so is a page with more lines.
        assert!(sealed(&[Block::text("rune-lock-1:600000:and so on")]).is_none());
        assert!(sealed(&[on_disk[0].clone(), Block::text("typed on the older app")]).is_none());
    }

    #[test]
    fn a_window_that_closes_locks_the_pages_it_opened() {
        hold("closed-window-page", test_key("pw-pw-pw"), 7001);
        hold("other-window-page", test_key("pw-pw-pw"), 7002);
        window_closed(7001);
        assert!(!is_open("closed-window-page"), "its key is gone with its window");
        assert!(is_open("other-window-page"), "a page another window opened stays open");
        forget("other-window-page");
    }

    #[test]
    fn a_page_the_app_listens_in_stays_open_when_the_window_that_opened_it_closes() {
        // Window 7101 opened the page with its password; window 7102 listens in it.
        hold("meeting-page", test_key("pw-pw-pw"), 7101);
        hold("another-page", test_key("pw-pw-pw"), 7101);
        crate::state::listen_for_test(Some((7102, "meeting-page".into())));
        window_closed(7101);
        assert!(is_open("meeting-page"), "the transcript still goes into it");
        assert_eq!(opened_by(7102), vec!["meeting-page".to_string()], "and the listening window locks it again");
        assert!(!is_open("another-page"));
        // The listening window itself closes: the page locks.
        window_closed(7102);
        assert!(!is_open("meeting-page"));
        crate::state::listen_for_test(None);
    }

    #[test]
    fn the_store_side_seals_an_open_page_and_opens_it_again() {
        let page = "store-side-page";
        assert!(for_store_write(page, &lines()).is_none(), "no key: written as it is");
        hold(page, test_key("pw-pw-pw"), 0);
        let block = for_store_write(page, &lines()).expect("sealed");
        assert!(for_store_write(page, std::slice::from_ref(&block)).is_none(), "a sealed block is not sealed twice");
        assert_eq!(for_store_read(page, vec![block.clone()]), lines());
        assert!(may_unseal(page));
        forget(page);
        assert_eq!(for_store_read(page, vec![block.clone()]), vec![block], "no key: stays sealed");
        assert!(!may_unseal(page));
        assert!(unsealing(page, || may_unseal(page)));
    }
}
