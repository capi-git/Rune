//! Undo and redo for block edits.
//!
//! Each edit records the page's blocks before and after. Typing in one block is
//! grouped into a single step until there's a pause. Undo works on the page
//! you're looking at; side effects on other pages (a sub-page created by the
//! `/page` command, linked pages trashed with a deleted block) are undone too.

use crate::model::{utf16_len, Block, Caret, Id};

const GROUP_TYPING_MS: i64 = 1200;
const LIMIT: usize = 300;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub page: Id,
    pub before: Vec<Block>,
    pub after: Vec<Block>,
    /// Set while the step is plain typing in this block (so it can grow).
    typing: Option<Id>,
    at: i64,
    /// Pages moved to the trash by this step (restored on undo).
    pub trashed: Vec<Id>,
    /// Pages created by this step (moved to the trash on undo).
    pub created: Vec<Id>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn record(&mut self, page: &str, before: Vec<Block>, after: Vec<Block>, typing: Option<&str>, now: i64) {
        if before == after {
            return;
        }
        self.redo.retain(|e| e.page != page);
        if let (Some(t), Some(last)) = (typing, self.undo.last_mut()) {
            if last.page == page && last.typing.as_deref() == Some(t) && now - last.at < GROUP_TYPING_MS {
                last.after = after;
                last.at = now;
                return;
            }
        }
        self.undo.push(Entry {
            page: page.to_string(),
            before,
            after,
            typing: typing.map(str::to_string),
            at: now,
            trashed: Vec::new(),
            created: Vec::new(),
        });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    /// A drawing changed. Drawings have their own undo inside the canvas, so
    /// page steps carry the latest drawing rather than winding it back.
    pub fn set_data(&mut self, block: &str, data: &str) {
        for e in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            for b in e.before.iter_mut().chain(e.after.iter_mut()) {
                if b.id == block && b.data != data {
                    b.data = data.to_string();
                }
            }
        }
    }

    /// Note pages trashed or created as part of the latest step on `page`.
    pub fn attach(&mut self, page: &str, trashed: &[Id], created: &[Id]) {
        if let Some(last) = self.undo.last_mut().filter(|e| e.page == page) {
            last.trashed.extend_from_slice(trashed);
            last.created.extend_from_slice(created);
            last.typing = None;
        }
    }

    pub fn take_undo(&mut self, page: &str) -> Option<Entry> {
        let i = self.undo.iter().rposition(|e| e.page == page)?;
        Some(self.undo.remove(i))
    }

    pub fn take_redo(&mut self, page: &str) -> Option<Entry> {
        let i = self.redo.iter().rposition(|e| e.page == page)?;
        Some(self.redo.remove(i))
    }

    pub fn push_undo(&mut self, mut e: Entry) {
        e.typing = None;
        self.undo.push(e);
    }

    pub fn push_redo(&mut self, mut e: Entry) {
        e.typing = None;
        self.redo.push(e);
    }

    /// Someone else changed the page (another window, an AI agent): old steps
    /// would overwrite their work, so drop them.
    pub fn forget(&mut self, page: &str) {
        self.undo.retain(|e| e.page != page);
        self.redo.retain(|e| e.page != page);
    }

    /// Stop growing the current typing step (after Enter, a paste, etc.).
    pub fn seal(&mut self) {
        if let Some(last) = self.undo.last_mut() {
            last.typing = None;
        }
    }
}

/// The page after one step is taken back or done again. `from` is the page as
/// the step left it and `to` is the page the step goes to. Most times the page
/// is still `from`, and the answer is `to`. But a meeting's transcript and an
/// AI's text are written to the page outside the history: then only what the
/// step itself changed is put back, and every other line stays as it is now.
/// (A step that only moved lines changes nothing in that case.)
pub fn rebase(current: &[Block], from: &[Block], to: &[Block]) -> Vec<Block> {
    if current == from {
        return to.to_vec();
    }
    let in_from = |id: &str| from.iter().find(|b| b.id == id);
    let in_to = |id: &str| to.iter().find(|b| b.id == id);
    // Lines the step made are taken out; lines it changed get their other form.
    let mut out: Vec<Block> = current
        .iter()
        .filter(|b| !(in_from(&b.id).is_some() && in_to(&b.id).is_none()))
        .map(|b| match (in_from(&b.id), in_to(&b.id)) {
            (Some(f), Some(t)) if f != t => t.clone(),
            _ => b.clone(),
        })
        .collect();
    // Lines the step took out come back, each after the line that was before it.
    for (i, b) in to.iter().enumerate() {
        if in_from(&b.id).is_some() || out.iter().any(|x| x.id == b.id) {
            continue;
        }
        let at = to[..i]
            .iter()
            .rev()
            .find_map(|prev| out.iter().position(|x| x.id == prev.id))
            .map_or(0, |n| n + 1);
        out.insert(at, b.clone());
    }
    out
}

/// Where the caret should go after the page changes from `from` to `to`:
/// the first place that differs.
pub fn caret_between(from: &[Block], to: &[Block]) -> Option<Caret> {
    for b in to {
        match from.iter().find(|x| x.id == b.id) {
            None if b.kind.is_editable() => return Some(Caret::end(&b.id)),
            Some(old) if b.kind.is_editable() && (old.text != b.text || old.kind != b.kind) => {
                let (a, c) = (old.plain_text(), b.plain_text());
                let common: usize = a.chars().zip(c.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf16()).sum();
                let pos = if common >= utf16_len(&c) { usize::MAX } else { common };
                return Some(Caret { id: b.id.clone(), pos });
            }
            _ => {}
        }
    }
    // Only removals or moves: go to the block before the first one removed.
    for (i, b) in from.iter().enumerate() {
        if !to.iter().any(|x| x.id == b.id) {
            return from[..i]
                .iter()
                .rev()
                .find(|x| to.iter().any(|y| y.id == x.id && y.kind.is_editable()))
                .map(|x| Caret::end(&x.id));
        }
    }
    to.iter().zip(from).find(|(a, b)| a != b).filter(|(a, _)| a.kind.is_editable()).map(|(a, _)| Caret::end(&a.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_groups_until_a_pause() {
        let mut h = History::default();
        let a = vec![Block::text("")];
        let id = a[0].id.clone();
        let mut b = a.clone();
        b[0].text = "h".into();
        let mut c = b.clone();
        c[0].text = "hi".into();
        h.record("p", a.clone(), b.clone(), Some(&id), 0);
        h.record("p", b.clone(), c.clone(), Some(&id), 500);
        let e = h.take_undo("p").unwrap();
        assert_eq!((e.before, e.after), (a.clone(), c.clone()));
        h.record("p", a, b.clone(), Some(&id), 0);
        h.record("p", b, c, Some(&id), 5000);
        assert!(h.take_undo("p").is_some() && h.take_undo("p").is_some());
        assert!(h.take_undo("p").is_none());
    }

    #[test]
    fn undo_is_per_page_and_a_new_edit_clears_redo() {
        let mut h = History::default();
        let a = vec![Block::text("a")];
        let b = vec![Block::text("b")];
        h.record("p", a.clone(), b.clone(), None, 0);
        h.record("q", a.clone(), b.clone(), None, 0);
        let e = h.take_undo("p").unwrap();
        h.push_redo(e);
        assert!(h.take_undo("p").is_none());
        h.record("p", a, b, None, 10);
        assert!(h.take_redo("p").is_none());
        assert!(h.take_undo("q").is_some());
    }

    #[test]
    fn a_step_taken_back_leaves_what_was_written_outside_the_history() {
        let ids = |bs: &[Block]| bs.iter().map(|b| b.text.clone()).collect::<Vec<_>>();
        let (a, b) = (Block::text("one"), Block::text("two"));
        let before = vec![a.clone(), b.clone()];
        // The step: "two" is changed, and a new line is made after it.
        let mut changed = b.clone();
        changed.text = "two!".into();
        let made = Block::text("three");
        let after = vec![a.clone(), changed.clone(), made.clone()];
        // Nothing else wrote to the page: the answer is the page as it was.
        assert_eq!(rebase(&after, &after, &before), before);
        assert_eq!(rebase(&before, &before, &after), after);

        // A transcript line came in after the step.
        let heard = Block::text("Sam: hello");
        let now = vec![a.clone(), changed.clone(), made.clone(), heard.clone()];
        let undone = rebase(&now, &after, &before);
        assert_eq!(ids(&undone), ["one", "two", "Sam: hello"], "the heard line stays");
        // ...and the step can be done again on top of it.
        let again = rebase(&undone, &before, &after);
        assert_eq!(ids(&again), ["one", "two!", "three", "Sam: hello"]);

        // A step that took a line out puts it back after the line before it.
        let cut = vec![a.clone()];
        let now = vec![heard.clone(), a.clone(), Block::text("later")];
        assert_eq!(ids(&rebase(&now, &cut, &before)), ["Sam: hello", "one", "two", "later"]);
        // With nothing before it, it goes first.
        let now = vec![heard.clone()];
        assert_eq!(ids(&rebase(&now, &[], &before)), ["one", "two", "Sam: hello"]);
    }

    #[test]
    fn caret_goes_to_the_first_difference() {
        let before = vec![Block::text("hello world")];
        let mut after = before.clone();
        after[0].text = "hello".into();
        let extra = Block::text(" world");
        after.push(extra);
        // Undoing an Enter split puts the caret back where Enter was pressed.
        assert_eq!(caret_between(&after, &before), Some(Caret { id: before[0].id.clone(), pos: 5 }));
        // Redoing it lands in the new block.
        let c = caret_between(&before, &after).unwrap();
        assert_eq!(c.id, before[0].id);
    }
}
