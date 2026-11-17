//! The pages that link to each page ("Linked from" at the foot of a page),
//! kept as an index, and which pages a change of the lines touched.
//!
//! Every write to a window's lines is counted (see `watch`). A write that
//! changed one page only says so (see `one_page`: a key typed in a line or
//! in a cell of a table). Then the index reads that page again and nothing
//! else, and a page in another pane is not drawn again. After any other
//! write, everything is read again, as it was before there was an index.

use crate::inline::{self, OLD_PAGE_URL, PAGE_URL};
use crate::model::{Block, BlockKind, Id, Page};
use dioxus::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

/// How many writes of one page are remembered. (A reader that last looked
/// before them reads everything again.)
const SPANS: usize = 256;

/// The writes to a window's lines, and the page that some of them touched.
#[derive(Default)]
pub struct Log {
    count: u64,
    /// (writes after this number, up to this one, all of this page)
    spans: VecDeque<(u64, u64, Id)>,
}

impl Log {
    pub fn bump(&mut self) {
        self.count += 1;
    }

    /// The writes since number `from` all touched `page` and no other.
    pub fn mark(&mut self, from: u64, page: &str) {
        if self.count > from {
            self.spans.push_back((from, self.count, page.to_string()));
            if self.spans.len() > SPANS {
                self.spans.pop_front();
            }
        }
    }

    /// The pages touched since write number `since`, or `None` when a write
    /// since then could have touched any page.
    pub fn touched(&self, since: u64) -> Option<HashSet<Id>> {
        let mut at = since;
        let mut out = HashSet::new();
        for (from, to, page) in &self.spans {
            if *to <= at {
                continue;
            }
            if *from > at {
                return None;
            }
            at = *to;
            out.insert(page.clone());
        }
        (at == self.count).then_some(out)
    }
}

/// The links of one page: to the pages its words link to, and to the pages
/// its link lines are.
#[derive(Default, Debug, PartialEq)]
struct Links {
    words: HashSet<Id>,
    lines: HashSet<Id>,
}

/// The pages each page links to, and the other way round.
#[derive(Default)]
pub struct Index {
    built: bool,
    /// The write the index is up to.
    seen: u64,
    out: HashMap<Id, Links>,
    into: HashMap<Id, HashSet<Id>>,
}

/// The pages a line's stored Markdown links to (as `inline::links_to_page`
/// finds them).
fn word_links(md: &str, out: &mut HashSet<Id>) {
    if !md.contains(PAGE_URL) && !md.contains(OLD_PAGE_URL) {
        return;
    }
    for run in inline::parse(md) {
        let Some(url) = run.link.as_deref() else { continue };
        let Some(id) = url.strip_prefix(PAGE_URL).or_else(|| url.strip_prefix(OLD_PAGE_URL)) else { continue };
        if md.contains(url) {
            out.insert(id.to_string());
        }
    }
}

fn links_of(blocks: &[Block]) -> Links {
    let mut links = Links::default();
    for b in blocks {
        match b.kind {
            BlockKind::PageLink => {
                if let Some(t) = &b.target {
                    links.lines.insert(t.clone());
                }
            }
            BlockKind::Code => {}
            _ => word_links(&b.text, &mut links.words),
        }
    }
    links
}

impl Index {
    fn read_page(&mut self, page: &str, blocks: Option<&Vec<Block>>) {
        if let Some(old) = self.out.remove(page) {
            for t in old.words.iter().chain(&old.lines) {
                if let Some(from) = self.into.get_mut(t) {
                    from.remove(page);
                    if from.is_empty() {
                        self.into.remove(t);
                    }
                }
            }
        }
        let Some(blocks) = blocks else { return };
        let links = links_of(blocks);
        if links.words.is_empty() && links.lines.is_empty() {
            return;
        }
        for t in links.words.iter().chain(&links.lines) {
            self.into.entry(t.clone()).or_default().insert(page.to_string());
        }
        self.out.insert(page.to_string(), links);
    }

    /// Bring the index up to the lines as they are now.
    pub fn refresh(&mut self, log: &Log, blocks: &HashMap<Id, Vec<Block>>) {
        match self.built.then(|| log.touched(self.seen)).flatten() {
            Some(pages) => {
                for p in pages {
                    self.read_page(&p, blocks.get(&p));
                }
            }
            None => {
                self.out.clear();
                self.into.clear();
                for (p, bs) in blocks {
                    self.read_page(p, Some(bs));
                }
                self.built = true;
            }
        }
        self.seen = log.count;
    }

    /// The same as `state::linked_from`, from the index.
    pub fn linked_from(&self, pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, id: &str) -> Vec<Page> {
        let parent = pages.get(id).and_then(|p| p.parent_id.clone());
        let mut out: Vec<Page> = self
            .into
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|from| pages.get(from))
            .filter(|p| !p.trashed && p.id != id)
            // (What a locked page holds is not told, also while it is open.)
            .filter(|p| blocks.get(&p.id).is_some_and(|bs| !crate::lock::hides(bs, &p.id)))
            .filter(|p| {
                self.out.get(&p.id).is_some_and(|l| l.words.contains(id) || (l.lines.contains(id) && parent.as_deref() != Some(p.id.as_str())))
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.display_title().to_lowercase().cmp(&b.display_title().to_lowercase()).then(a.created_at.cmp(&b.created_at)));
        out
    }
}

#[derive(Default)]
struct Window {
    log: Log,
    index: Index,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<u64, Window>> = RefCell::new(HashMap::new());
}

/// Count every write to a window's lines from now on (once, as the window
/// opens).
pub fn watch(key: u64, blocks: Signal<HashMap<Id, Vec<Block>>>) {
    WINDOWS.with(|w| w.borrow_mut().insert(key, Window::default()));
    let rc = dioxus::dioxus_core::ReactiveContext::new_with_callback(
        move || {
            WINDOWS.with(|w| {
                if let Some(win) = w.borrow_mut().get_mut(&key) {
                    win.log.bump();
                }
            })
        },
        dioxus::dioxus_core::current_scope_id(),
        std::panic::Location::caller(),
    );
    rc.run_in(|| {
        let _ = blocks.read();
    });
}

/// The window is gone.
pub fn forget(key: u64) {
    WINDOWS.with(|w| w.borrow_mut().remove(&key));
}

fn count(key: u64) -> u64 {
    WINDOWS.with(|w| w.borrow().get(&key).map_or(0, |win| win.log.count))
}

/// Run `f`, which writes the lines of `page` and of no other page.
pub fn one_page<R>(key: u64, page: &str, f: impl FnOnce() -> R) -> R {
    let from = count(key);
    let out = f();
    WINDOWS.with(|w| {
        if let Some(win) = w.borrow_mut().get_mut(&key) {
            win.log.mark(from, page);
        }
    });
    out
}

/// Could the lines of `page` have changed since write number `since`? And
/// the number of the last write.
pub fn changed(key: u64, page: &str, since: u64) -> (bool, u64) {
    WINDOWS.with(|w| match w.borrow().get(&key) {
        Some(win) => (win.log.touched(since).is_none_or(|t| t.contains(page)), win.log.count),
        None => (true, 0),
    })
}

/// The pages that link to a page (see `state::linked_from`).
pub fn linked_from(key: u64, pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, id: &str) -> Vec<Page> {
    WINDOWS.with(|w| match w.borrow_mut().get_mut(&key) {
        Some(win) => {
            win.index.refresh(&win.log, blocks);
            win.index.linked_from(pages, blocks, id)
        }
        None => crate::state::linked_from(pages, blocks, id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str, title: &str, parent: Option<&str>) -> Page {
        let mut p = Page::new(parent.map(str::to_string), 0.0);
        p.id = id.to_string();
        p.title = title.to_string();
        p
    }

    fn link_line(to: &str) -> Block {
        let mut b = Block::new(BlockKind::PageLink, "");
        b.target = Some(to.to_string());
        b
    }

    #[test]
    fn the_log_knows_which_page_a_write_touched() {
        let mut log = Log::default();
        assert_eq!(log.touched(0), Some(HashSet::new()));
        // A key typed in page a.
        let from = log.count;
        log.bump();
        log.mark(from, "a");
        assert_eq!(log.touched(0), Some(HashSet::from(["a".to_string()])));
        // A write that says nothing: any page.
        log.bump();
        assert_eq!(log.touched(0), None);
        assert_eq!(log.touched(1), None);
        // Seen since: only what came after counts.
        let from = log.count;
        log.bump();
        log.bump();
        log.mark(from, "b");
        assert_eq!(log.touched(2), Some(HashSet::from(["b".to_string()])));
        assert_eq!(log.touched(4), Some(HashSet::new()));
        // A write that wrote nothing marks nothing.
        log.mark(log.count, "c");
        assert_eq!(log.touched(4), Some(HashSet::new()));
        // Too long ago: everything.
        for _ in 0..SPANS + 1 {
            let from = log.count;
            log.bump();
            log.mark(from, "a");
        }
        assert_eq!(log.touched(4), None);
        assert!(log.touched(log.count - 1).is_some());
    }

    #[test]
    fn the_index_finds_what_a_scan_of_every_page_finds() {
        let mut pages: HashMap<Id, Page> = HashMap::new();
        for p in [page("t", "Target", Some("home")), page("home", "Home", None), page("a", "Apple", None), page("b", "banana", None), page("c", "Code", None), page("gone", "Gone", None)] {
            pages.insert(p.id.clone(), p);
        }
        pages.get_mut("gone").unwrap().trashed = true;
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        // The parent lists it as one of its pages: left out. A line in words links.
        blocks.insert("home".into(), vec![link_line("t")]);
        blocks.insert("a".into(), vec![Block::text("see [Target](rune://page/t)")]);
        blocks.insert("b".into(), vec![Block::text("plain"), link_line("t"), Block::text("[old](notion2://page/t)")]);
        blocks.insert("c".into(), vec![Block::new(BlockKind::Code, "[Target](rune://page/t)")]);
        blocks.insert("gone".into(), vec![Block::text("[Target](rune://page/t)")]);
        blocks.insert("t".into(), vec![Block::text("[me](rune://page/t)"), Block::text("[Home](rune://page/home)")]);
        let ids = |ps: Vec<Page>| ps.into_iter().map(|p| p.id).collect::<Vec<_>>();
        let mut log = Log::default();
        let mut index = Index::default();
        let check = |index: &mut Index, log: &Log, blocks: &HashMap<Id, Vec<Block>>| {
            index.refresh(log, blocks);
            for id in ["t", "home", "a", "b", "c", "gone", "nowhere"] {
                assert_eq!(ids(index.linked_from(&pages, blocks, id)), ids(crate::state::linked_from(&pages, blocks, id)), "{id}");
            }
        };
        check(&mut index, &log, &blocks);
        assert_eq!(ids(index.linked_from(&pages, &blocks, "t")), ["a", "b"]);
        // A key typed in page a takes its link away: only page a is read again.
        let from = log.count;
        blocks.get_mut("a").unwrap()[0].text = "see Target".into();
        log.bump();
        log.mark(from, "a");
        // (Page b changed too, but no one said so: the index does not see it.)
        blocks.get_mut("b").unwrap().truncate(1);
        index.refresh(&log, &blocks);
        assert_eq!(ids(index.linked_from(&pages, &blocks, "t")), ["b"]);
        // A write that says nothing: every page is read again.
        log.bump();
        check(&mut index, &log, &blocks);
        assert!(index.linked_from(&pages, &blocks, "t").is_empty());
        // A link typed in page c's words, and page a gone.
        let from = log.count;
        blocks.get_mut("c").unwrap().push(Block::text("[x](rune://page/t)"));
        log.bump();
        log.mark(from, "c");
        let from = log.count;
        blocks.remove("a");
        log.bump();
        log.mark(from, "a");
        check(&mut index, &log, &blocks);
        assert_eq!(ids(index.linked_from(&pages, &blocks, "t")), ["c"]);
    }
}
