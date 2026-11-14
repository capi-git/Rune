//! Application state and every action the UI can take.

use crate::ai::{self, AiAction, AiEvent};
use crate::glass;
use crate::history::{caret_between, History};
use crate::inline;
use crate::layout::{Edge, Layout};
use crate::markdown;
use crate::model::*;
use crate::repeat;
use crate::voice::{self, Directive};
// A calendar event, not the UI library's input event.
use crate::model::Event;
use crate::store;
use crate::sync::{self, SyncMsg};
use crate::when;
use dioxus::core::{spawn_forever, Task};
use dioxus::prelude::*;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Route {
    Home,
    Page(Id),
    Calendar,
    /// Every page as a dot, with a line for each link between two pages.
    Graph,
}

impl Route {
    /// A name for what a pane shows. The page script keeps the scroll place
    /// of each by it (see "a page keeps its scroll place" in bridge.js).
    pub fn place_key(&self) -> String {
        match self {
            Route::Home => "home".to_string(),
            Route::Page(id) => format!("page:{id}"),
            Route::Calendar => "calendar".to_string(),
            Route::Graph => "graph".to_string(),
        }
    }
}

/// One page slot in a tab, with its own back and forward history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub id: Id,
    pub history: Vec<Route>,
    pub cursor: usize,
}

/// How far back a pane remembers, and how many closed tabs can be reopened.
const TAB_HISTORY: usize = 50;
const CLOSED_TABS: usize = 20;

impl Pane {
    pub fn new(route: Route) -> Pane {
        Pane { id: new_id(), history: vec![route], cursor: 0 }
    }
    pub fn route(&self) -> &Route {
        &self.history[self.cursor.min(self.history.len() - 1)]
    }
    pub fn push(&mut self, route: Route) {
        if *self.route() == route {
            return;
        }
        self.history.truncate(self.cursor + 1);
        self.history.push(route);
        if self.history.len() > TAB_HISTORY {
            let extra = self.history.len() - TAB_HISTORY;
            self.history.drain(..extra);
        }
        self.cursor = self.history.len() - 1;
    }
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }
    /// Forget pages that are gone for good; an empty history goes Home.
    fn forget(&mut self, ids: &[Id]) {
        let gone = |r: &Route| matches!(r, Route::Page(p) if ids.contains(p));
        let before = self.history.iter().take(self.cursor).filter(|r| !gone(r)).count();
        self.history.retain(|r| !gone(r));
        if self.history.is_empty() {
            self.history.push(Route::Home);
        }
        self.cursor = before.min(self.history.len() - 1);
    }
    /// Keep in the history only what belongs with the page on show: Back
    /// then stays in its space. `fallback` is the space of a pane on Home or
    /// the calendar.
    fn keep_one_space(&mut self, pages: &HashMap<Id, Page>, fallback: &str) {
        let space_of = |r: &Route| match r {
            Route::Page(id) => pages.get(id).map(|p| p.space_id.clone()),
            _ => None,
        };
        let home = space_of(self.route()).unwrap_or_else(|| fallback.to_string());
        let at = self.cursor.min(self.history.len() - 1);
        let (mut i, mut cursor) = (0, 0);
        self.history.retain(|r| {
            let keep = i == at || space_of(r).is_none_or(|s| s == home);
            if keep && i < at {
                cursor += 1;
            }
            i += 1;
            keep
        });
        self.cursor = cursor;
    }
}

/// A tab: any number of pages, arranged side by side and on top of each
/// other by `layout` (see layout.rs). `focus` is the pane pages open in and
/// that back and forward act on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedTab")]
pub struct Tab {
    pub id: Id,
    pub panes: Vec<Pane>,
    pub layout: Layout,
    pub focus: Id,
    /// Panes folded to a slim rail (see `Layout::rails`). Each keeps its
    /// share in the layout, so it opens at the size it had.
    #[serde(default)]
    pub folded: Vec<Id>,
    /// The pane that shows the rows of tables opened at the side: the next
    /// row opened that way takes its place there. Not kept from run to run.
    #[serde(skip)]
    pub peek: Option<Id>,
    /// Pinned: it stands at the left of the row, shows its icon only, and
    /// does not close until it is unpinned. Written only when it is so: a
    /// tab that is not pinned is saved as it always was, and an older Rune
    /// reads a pinned one as a plain tab.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl Tab {
    pub fn new(route: Route) -> Tab {
        Tab::with_pane(Pane::new(route))
    }
    pub fn with_pane(pane: Pane) -> Tab {
        Tab { id: new_id(), layout: Layout::pane(&pane.id), focus: pane.id.clone(), panes: vec![pane], folded: Vec::new(), peek: None, pinned: false }
    }
    /// The panes shown as rails.
    pub fn rails(&self) -> Vec<Id> {
        self.layout.rails(&self.folded)
    }
    pub fn pane(&self, id: &str) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }
    pub fn focused(&self) -> &Pane {
        self.pane(&self.focus).unwrap_or(&self.panes[0])
    }
    pub fn focused_mut(&mut self) -> &mut Pane {
        let i = self.panes.iter().position(|p| p.id == self.focus).unwrap_or(0);
        &mut self.panes[i]
    }
    /// What the focused pane shows.
    pub fn route(&self) -> &Route {
        self.focused().route()
    }
    pub fn push(&mut self, route: Route) {
        self.focused_mut().push(route);
    }
    pub fn can_back(&self) -> bool {
        self.focused().can_back()
    }
    pub fn can_forward(&self) -> bool {
        self.focused().can_forward()
    }
    /// Whether the tab shows more than one page.
    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }
    /// Panes in reading order (left to right, top to bottom).
    pub fn ordered(&self) -> Vec<&Pane> {
        self.layout.panes().iter().filter_map(|id| self.pane(id)).collect()
    }
    /// The pane showing this, if any.
    pub fn pane_showing(&self, route: &Route) -> Option<Id> {
        self.ordered().into_iter().find(|p| p.route() == route).map(|p| p.id.clone())
    }
    /// Add a pane on `edge` of `target` (or of the focused pane); it takes focus.
    pub fn add_pane(&mut self, pane: Pane, target: Option<&str>, edge: Edge) {
        let target = target.filter(|t| self.layout.contains(t)).map(str::to_string).unwrap_or_else(|| self.focus.clone());
        self.layout.place(&target, Layout::pane(&pane.id), edge);
        self.focus = pane.id.clone();
        self.panes.push(pane);
    }
    /// Bring in all of another tab's pages, on `edge` of `target` (or along
    /// that side of everything).
    pub fn add_tab(&mut self, other: Tab, target: Option<&str>, edge: Edge) {
        match target.filter(|t| self.layout.contains(t)) {
            Some(t) => {
                self.layout.place(t, other.layout, edge);
            }
            None => self.layout.append(other.layout, edge),
        }
        self.focus = other.focus;
        self.panes.extend(other.panes);
    }
    /// Take a pane out of the tab (never its last one).
    pub fn take_pane(&mut self, id: &str) -> Option<Pane> {
        if !self.is_split() {
            return None;
        }
        let next = self.layout.neighbour(id);
        if !self.layout.remove(id) {
            return None;
        }
        let i = self.panes.iter().position(|p| p.id == id)?;
        let pane = self.panes.remove(i);
        if self.focus == id {
            self.focus = next.unwrap_or_else(|| self.panes[0].id.clone());
        }
        Some(pane)
    }
    /// Each page as a tab of its own, in reading order.
    pub fn split_apart(self) -> Vec<Tab> {
        let order = self.layout.panes();
        let mut panes = self.panes;
        let mut out = Vec::new();
        for id in order {
            if let Some(i) = panes.iter().position(|p| p.id == id) {
                out.push(Tab::with_pane(panes.remove(i)));
            }
        }
        if let Some(first) = out.first_mut() {
            first.id = self.id;
            first.pinned = self.pinned;
        }
        out
    }
    /// Whether "Duplicate Tab" can copy it: not a tab of Home, the calendar
    /// or the graph alone (there is one of each in a row of tabs).
    pub fn can_duplicate(&self) -> bool {
        view_tab(self).is_none()
    }
    /// A copy of the tab ("Duplicate Tab"): the same pages with their Back
    /// and Forward, in the same layout, with the same pane in focus and the
    /// same panes folded; new ids for the tab and each pane, and not
    /// pinned. Gives the copy, and each pane's id with the id of its copy.
    pub fn copied(&self) -> (Tab, Vec<(Id, Id)>) {
        let names: Vec<(Id, Id)> = self.panes.iter().map(|p| (p.id.clone(), new_id())).collect();
        let name = |id: &str| names.iter().find(|(old, _)| old == id).map(|(_, new)| new.clone()).unwrap_or_else(|| id.to_string());
        let panes = self.panes.iter().map(|p| Pane { id: name(&p.id), ..p.clone() }).collect();
        let tab = Tab {
            id: new_id(),
            panes,
            layout: self.layout.renamed(&name),
            focus: name(&self.focus),
            folded: self.folded.iter().map(|f| name(f)).collect(),
            peek: None,
            pinned: false,
        };
        (tab, names)
    }
}

/// A tab as saved, including saves from before tabs could hold any number of
/// pages (then: one page with its history, and maybe a second one beside it).
#[derive(Deserialize)]
struct SavedTab {
    id: Id,
    #[serde(default)]
    panes: Vec<Pane>,
    #[serde(default)]
    layout: Option<Layout>,
    #[serde(default)]
    focus: Option<Id>,
    #[serde(default)]
    history: Vec<Route>,
    #[serde(default)]
    cursor: usize,
    #[serde(default)]
    split: Option<Route>,
    #[serde(default)]
    folded: Vec<Id>,
    #[serde(default)]
    pinned: bool,
}

impl From<SavedTab> for Tab {
    fn from(s: SavedTab) -> Tab {
        let mut panes = s.panes;
        if panes.is_empty() {
            let history = if s.history.is_empty() { vec![Route::Home] } else { s.history };
            let cursor = s.cursor.min(history.len() - 1);
            panes.push(Pane { id: new_id(), history, cursor });
            if let Some(route) = s.split {
                panes.push(Pane::new(route));
            }
        }
        for p in panes.iter_mut() {
            if p.history.is_empty() {
                p.history.push(Route::Home);
            }
            p.cursor = p.cursor.min(p.history.len() - 1);
        }
        let ids: Vec<Id> = panes.iter().map(|p| p.id.clone()).collect();
        let layout = s.layout.filter(|l| l.fits(&ids)).unwrap_or_else(|| Layout::row(&ids));
        let focus = s.focus.filter(|f| ids.contains(f)).unwrap_or_else(|| ids[0].clone());
        let folded = s.folded.into_iter().filter(|f| ids.contains(f)).collect();
        Tab { id: s.id, panes, layout, focus, folded, peek: None, pinned: s.pinned }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
    pub fn parse(s: &str) -> Theme {
        match s {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlashState {
    pub page_id: Id,
    pub block_id: Id,
    /// Visible UTF-16 offset of the "/" (or of the "@" or "[[").
    pub slash_at: usize,
    /// Where what was typed for the menu ends: the caret, not the end of
    /// the line. What is after it stays in the line when a choice is made.
    pub end: usize,
    /// The line was made by the "+" beside a line, with its "/".
    pub added: bool,
    pub query: String,
    pub selected: usize,
    /// The menu lists pages to link to (after "@" or "[["), not commands.
    pub pages: bool,
    /// After "[[": a page with the typed name can be made as well.
    pub wiki: bool,
}

/// What the "Undo" in a message puts back, each by the app's own way for it.
#[derive(Clone, Debug, PartialEq)]
pub enum Back {
    /// A page out of the Trash (as "Restore" in the Trash does). `tab`:
    /// the tab it was the one page of, in front. The Trash left Home in
    /// its place there, and it shows again in that place: in that tab,
    /// not in a new one (see `Ui::put_page_back_in`).
    Page { id: Id, tab: Option<Id> },
    /// One step of a page's own Undo (rows, columns or lines that were
    /// deleted): the step with this number, and only while it is the step
    /// that Undo would take back (see `History::next_undo`).
    Step { page: Id, step: u64 },
    /// A deleted event, with the notes pages that its repeats had.
    Event { event: Event, pages: Vec<(String, Id)> },
    /// One repeat that was deleted: the day its repeating event skips.
    Repeat { series: Id, day: String },
    /// A closed tab, while it is the one "Reopen Closed Tab" would open.
    Tab(Id),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Popover {
    BlockMenu { block_id: Id },
    /// The ⋯ menu of a pane's page.
    PageMenu { pane: Id },
    IconPicker { page_id: Id },
    /// The covers to choose from, for a page.
    CoverPicker { page_id: Id },
    TabMenu { index: usize },
    SidebarMenu { page_id: Id },
    SpaceMenu,
    /// The days to choose from for the due day of a to-do; `was` is the
    /// day it had when the menu opened (see `set_reminder`).
    Due { block_id: Id, was: Option<chrono::NaiveDate> },
    /// The menu of a table's column: its kind, its total, its unit.
    TableColumn { block_id: Id, column: usize },
    /// The menu of a table: its view, its columns, its filters.
    TableMenu { block_id: Id },
    /// The kinds a new column of a table can be.
    TableKinds { block_id: Id },
    /// The icons a table can have.
    TableIcon { block_id: Id },
    /// The menu of some rows of a table. These rows are picked while it is open.
    TableRow { block_id: Id, rows: Vec<usize> },
    /// What a filter of a table asks (or, with `sort`, a sort), open to change.
    TableChip { block_id: Id, sort: bool, index: usize },
    /// The colour rules of a table.
    TablePaint { block_id: Id },
    /// The list of a cell: the options of a choice cell, or the rows of
    /// another table that a relation cell can name. `on_page`: it was opened
    /// from the values at the top of the row's page, not from the table (both
    /// can show at once, the row's page at the side of its table).
    TableCell { block_id: Id, row: usize, column: usize, on_page: bool },
    /// What a chart draws.
    Chart { block_id: Id },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Modal {
    QuickFind,
    Settings,
    Trash,
    /// Earlier versions of the page in `Ui::history`.
    History,
    /// The voice box: speak, and a page is made from what was said.
    Voice,
    /// Ask your notes: a question answered from the pages of this space.
    AskNotes,
    /// Every open to-do of this space, by page.
    Todos,
    /// A password for the page in front, to lock it (see lock.rs).
    LockPage,
}

/// The keys that open the AI from any app (Settings ▸ AI ▸ Voice).
pub const AI_SHORTCUT: &str = "ctrl+alt+space";

/// What the voice box does with what is said (see `Modal::Voice`).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum VoiceFor {
    /// A new page; or the calendar or the To-dos page, when that is what
    /// was asked for.
    #[default]
    NewPage,
    /// Written into a page, at a block (the microphone on an empty line).
    Into { page: Id, block: Id },
    /// The selected text of a block (from `start` to `end`) is changed the
    /// way the speaker says.
    Edit { page: Id, block: Id, start: usize, end: usize },
}

/// The tabs of one space, and which of them is in front.
pub type TabSet = (Vec<Tab>, usize);

/// The views that have one tab at most: Home, the calendar, the graph.
const ONE_TAB_VIEWS: [Route; 3] = [Route::Home, Route::Calendar, Route::Graph];

/// The view a tab shows alone (one pane, on Home, the calendar or the graph).
fn view_tab(tab: &Tab) -> Option<&Route> {
    (tab.panes.len() == 1).then(|| tab.panes[0].route()).filter(|r| ONE_TAB_VIEWS.contains(r))
}

/// A tab that only shows Home (one pane, on Home).
fn is_home_tab(tab: &Tab) -> bool {
    view_tab(tab) == Some(&Route::Home)
}

/// One tab at most for each of Home, the calendar and the graph in a row
/// of tabs: the one in front if it is one, else the first. Says whether a
/// tab went.
fn one_home_tab(tabs: &mut Vec<Tab>, active: &mut usize) -> bool {
    let mut any = false;
    for view in ONE_TAB_VIEWS {
        any |= one_tab_of(tabs, active, &view);
    }
    any
}

fn one_tab_of(tabs: &mut Vec<Tab>, active: &mut usize, view: &Route) -> bool {
    let homes: Vec<usize> = (0..tabs.len()).filter(|&i| view_tab(&tabs[i]) == Some(view)).collect();
    if homes.len() < 2 {
        return false;
    }
    let keep = if homes.contains(active) { *active } else { homes[0] };
    let front = tabs.get(*active).map(|t| t.id.clone());
    let kept = tabs[keep].id.clone();
    let mut i = 0;
    tabs.retain(|t| {
        let stays = !homes.contains(&i) || t.id == kept;
        i += 1;
        stays
    });
    *active = front.and_then(|id| tabs.iter().position(|t| t.id == id)).or_else(|| tabs.iter().position(|t| t.id == kept)).unwrap_or(0);
    true
}

/// Pinned tabs stand first in their row, in the order they have; the tab in
/// front stays the one in front. Says whether a tab moved.
fn pins_first(tabs: &mut Vec<Tab>, active: &mut usize) -> bool {
    if tabs.iter().skip_while(|t| t.pinned).all(|t| !t.pinned) {
        return false;
    }
    let front = tabs.get(*active).map(|t| t.id.clone());
    // (A stable sort: the pinned ones keep their order, and so do the others.)
    tabs.sort_by_key(|t| !t.pinned);
    *active = front.and_then(|id| tabs.iter().position(|t| t.id == id)).unwrap_or(0);
    true
}

/// The order in which a window's tabs were used, for Ctrl+Tab (see
/// `AppState::walk_tabs`). Kept in memory only, never saved.
#[derive(Default)]
struct TabUse {
    /// Tab ids, the one used last first. The tabs of every space of the
    /// window are in it; a walk takes those in front.
    order: Vec<Id>,
    /// While Control is held after Ctrl+Tab: the order as it was at the
    /// first press, and how far along it the walk is.
    walk: Option<(Vec<Id>, usize)>,
}

thread_local! {
    /// The order of use of each window's tabs, by the window's key.
    static TAB_USE: std::cell::RefCell<HashMap<u64, TabUse>> = std::cell::RefCell::new(HashMap::new());
}

/// This tab was used now: it comes first.
fn note_use(order: &mut Vec<Id>, id: &str) {
    order.retain(|o| o != id);
    order.insert(0, id.to_string());
}

/// The tabs of a row in the order of use: those used, the last first, then
/// the others in the order of the row.
fn used_first(order: &[Id], tabs: &[Tab]) -> Vec<Id> {
    let mut out: Vec<Id> = order.iter().filter(|id| tabs.iter().any(|t| &t.id == *id)).cloned().collect();
    out.extend(tabs.iter().filter(|t| !order.contains(&t.id)).map(|t| t.id.clone()));
    out
}

impl TabUse {
    /// One press of Ctrl+Tab (`by` 1) or Ctrl+Shift+Tab (-1), with `front`
    /// in front of the row `tabs`: the tab to bring forward. The first press
    /// takes the order as it is; the next ones, while Control is held, go on
    /// along that same order. A tab that closed meanwhile starts it again.
    fn walk(&mut self, front: &str, tabs: &[Tab], by: i64) -> Option<Id> {
        let (list, at) = match self.walk.take() {
            Some((list, at)) if list.get(at).is_some_and(|id| id == front) && list.iter().all(|id| tabs.iter().any(|t| &t.id == id)) => (list, at),
            _ => {
                note_use(&mut self.order, front);
                (used_first(&self.order, tabs), 0)
            }
        };
        if list.len() < 2 {
            return None;
        }
        let next = (at as i64 + by).rem_euclid(list.len() as i64) as usize;
        let to = list[next].clone();
        self.walk = Some((list, next));
        Some(to)
    }

    /// `front` is in front now (of `every` tab the window has, in all its
    /// spaces): it comes first. Not while Control is held after Ctrl+Tab
    /// (the order waits for it to be let go); a tab that came forward by
    /// another way meanwhile (a press on it) ends that walk. A tab that
    /// closed leaves the order.
    fn used(&mut self, front: &str, every: &[&Tab]) {
        if let Some((list, at)) = &self.walk {
            if list.get(*at).is_some_and(|id| id == front) {
                return;
            }
            self.walk = None;
        }
        note_use(&mut self.order, front);
        self.order.retain(|id| every.iter().any(|t| &t.id == id));
    }
}

/// The space of the page a tab shows. Home, the calendar and the graph are
/// in no space of their own.
fn tab_space(tab: &Tab, pages: &HashMap<Id, Page>) -> Option<Id> {
    match tab.route() {
        Route::Page(id) => pages.get(id).map(|p| p.space_id.clone()).filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// What a window keeps of its layout for the next start. The theme is one
/// for all windows. The rest is the first window's alone: a second window
/// read the folds of the sidebar, the recent pages and the zoom once, when
/// it opened, and wrote that old copy back over the first window's at its
/// next click on a tab.
fn ui_settings(ui: &Ui, primary: bool) -> Vec<(&'static str, String)> {
    let mut pairs = vec![("theme", ui.theme.as_str().to_string())];
    if primary {
        let json = |v: &dyn erased::Json| v.json();
        pairs.extend([
            ("expanded", json(&ui.expanded)),
            ("recent", json(&ui.recent)),
            ("sidebar_open", if ui.sidebar_open { "1".into() } else { "0".into() }),
            ("sidebar_width", ui.sidebar_width.to_string()),
            ("zoom", ui.zoom.to_string()),
            ("tabs", serde_json::to_string(&(&ui.tabs, ui.active)).unwrap_or_default()),
            ("space_tabs", serde_json::to_string(&ui.parked).unwrap_or_default()),
            ("folds", json(&ui.folds)),
        ]);
    }
    pairs
}

/// Spaces went: what was in them (pages in their Trash, events) is in
/// `into` from now on, as the notes file has it (`Store::delete_space`).
/// Without this a window kept the dead space on them and wrote it back at
/// their next change: after a restart such a page was in no space at all.
fn rehome(pages: &mut HashMap<Id, Page>, events: &mut HashMap<Id, Event>, gone: &[Id], into: &str) {
    for p in pages.values_mut().filter(|p| gone.contains(&p.space_id)) {
        p.space_id = into.to_string();
    }
    for e in events.values_mut().filter(|e| gone.contains(&e.space_id)) {
        e.space_id = into.to_string();
    }
}

/// Take pages that are gone out of saved tabs.
fn forget_gone(tabs: &mut [Tab], pages: &HashMap<Id, Page>) {
    for pane in tabs.iter_mut().flat_map(|t| t.panes.iter_mut()) {
        let gone: Vec<Id> = pane
            .history
            .iter()
            .filter_map(|r| match r {
                Route::Page(id) if !pages.get(id).is_some_and(|p| !p.trashed) => Some(id.clone()),
                _ => None,
            })
            .collect();
        pane.forget(&gone);
    }
}

/// Tabs saved when all spaces shared one row: each goes to the space of its
/// page. Gives the tabs of `space`, the one of them in front, and the others.
fn sort_tabs_by_space(tabs: Vec<Tab>, active: usize, space: &str, pages: &HashMap<Id, Page>) -> (Vec<Tab>, usize, HashMap<Id, TabSet>) {
    let front = tabs.get(active).map(|t| t.id.clone());
    let mut here = Vec::new();
    let mut others: HashMap<Id, TabSet> = HashMap::new();
    for mut tab in tabs {
        for pane in tab.panes.iter_mut() {
            pane.keep_one_space(pages, space);
        }
        match tab_space(&tab, pages).filter(|s| s != space) {
            Some(other) => {
                let set = others.entry(other).or_default();
                set.0.push(tab);
                set.1 = set.0.len() - 1;
            }
            None => here.push(tab),
        }
    }
    let active = front.and_then(|id| here.iter().position(|t: &Tab| t.id == id)).unwrap_or(0);
    (here, active, others)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ui {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub closed: Vec<Tab>,
    /// The tabs of the spaces that are not in front (`tabs` and `active` are
    /// those of the space in front).
    pub parked: HashMap<Id, TabSet>,
    pub sidebar_open: bool,
    /// Pinned sidebar width in pixels.
    pub sidebar_width: f64,
    pub expanded: HashSet<Id>,
    pub popover: Option<Popover>,
    pub modal: Option<Modal>,
    pub zoom: f64,
    pub theme: Theme,
    pub recent: Vec<Id>,
    /// The space shown in the sidebar, Home, search and Trash.
    pub space: Id,
    /// The page whose history is open (see `Modal::History`).
    pub history: Option<Id>,
    /// What the voice box is open for.
    pub voice: VoiceFor,
    /// Focus mode: only the page is shown, the lines around the caret are
    /// pale, and the line being written stays near the middle. Not kept.
    pub focus_mode: bool,
    /// The headings folded on each page (page id, then the ids of its
    /// folded headings): the lines under them are hidden, never changed
    /// (see `shown` in model.rs). Kept with the window's layout, not in the
    /// notes themselves.
    pub folds: HashMap<Id, HashSet<Id>>,
}

impl Ui {
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }
    fn tab_mut(&mut self) -> &mut Tab {
        let i = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[i]
    }
    /// Pin a tab or unpin it: a pinned one goes after the others that are
    /// pinned, an unpinned one before the others that are not.
    fn pin_tab(&mut self, index: usize, on: bool) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.pinned = on;
            pins_first(&mut self.tabs, &mut self.active);
        }
    }
    /// A copy of a tab, right after it and in front (after the pinned tabs,
    /// for a pinned one: the copy is not pinned). A tab that shows only
    /// Home, the calendar or the graph has none: there is one such tab.
    /// Gives each pane's id with the id of its copy.
    fn duplicate(&mut self, index: usize) -> Option<Vec<(Id, Id)>> {
        let tab = self.tabs.get(index).filter(|t| t.can_duplicate())?;
        let (copy, names) = tab.copied();
        self.tabs.insert(index + 1, copy);
        self.active = index + 1;
        pins_first(&mut self.tabs, &mut self.active);
        Some(names)
    }
    /// Close every tab but one, and but the pinned ones. Gives the tabs
    /// that went.
    fn close_others(&mut self, keep: usize) -> Vec<Tab> {
        let Some(kept) = self.tabs.get(keep).map(|t| t.id.clone()) else { return Vec::new() };
        let (stay, gone): (Vec<Tab>, Vec<Tab>) = std::mem::take(&mut self.tabs).into_iter().partition(|t| t.pinned || t.id == kept);
        self.tabs = stay;
        self.active = self.tabs.iter().position(|t| t.id == kept).unwrap_or(0);
        gone
    }
    /// Every open tab, also those of the spaces not in front.
    fn every_tab_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.tabs.iter_mut().chain(self.parked.values_mut().flat_map(|set| set.0.iter_mut()))
    }
    /// Whether a pane of any tab shows one of these pages or has one among
    /// the pages Back and Forward step through.
    fn remembers(&self, ids: &[Id]) -> bool {
        let held = |r: &Route| matches!(r, Route::Page(id) if ids.contains(id));
        self.tabs.iter().chain(self.parked.values().flat_map(|set| set.0.iter())).any(|t| t.panes.iter().any(|p| p.history.iter().any(held)))
    }
    /// Put the tabs in front away and bring up those of another space, as
    /// they were left. A space with no tabs gets one showing `first`; the
    /// answer says whether it did.
    fn show_tabs_of(&mut self, space: &str, pages: &HashMap<Id, Page>, first: Route) -> bool {
        let left = std::mem::replace(&mut self.space, space.to_string());
        let set = (std::mem::take(&mut self.tabs), self.active);
        self.parked.insert(left, set);
        let (tabs, active) = self.parked.remove(space).unwrap_or_default();
        let front = tabs.get(active).map(|t| t.id.clone());
        // A page moved to another space while its tab was put away: the tab
        // went with it.
        let mut kept = Vec::new();
        for tab in tabs {
            match tab_space(&tab, pages).filter(|s| !tab.is_split() && s != space) {
                Some(other) => self.parked.entry(other).or_default().0.push(tab),
                None => kept.push(tab),
            }
        }
        let fresh = kept.is_empty();
        if fresh {
            kept.push(Tab::new(first));
        }
        self.active = front.and_then(|id| kept.iter().position(|t| t.id == id)).unwrap_or(0);
        self.tabs = kept;
        fresh
    }
    /// Pages went (to the Trash, or for good): a tab that shows one goes
    /// Home, and no pane keeps one in the pages Back and Forward step
    /// through. (Back opened a page in the Trash again, to be typed in,
    /// with no word that it was there.)
    fn leave_pages(&mut self, ids: &[Id]) {
        for tab in self.every_tab_mut() {
            let gone: Vec<Id> =
                tab.panes.iter().filter(|p| matches!(p.route(), Route::Page(id) if ids.contains(id))).map(|p| p.id.clone()).collect();
            for pane in gone {
                // The pane closes; a tab's only page goes Home instead, and
                // is pinned no more: the pin was for the page. (It stayed
                // as a pinned Home tab that could not be closed.)
                if tab.take_pane(&pane).is_none() {
                    if let Some(p) = tab.panes.iter_mut().find(|p| p.id == pane) {
                        p.push(Route::Home);
                    }
                    tab.pinned = false;
                }
            }
            for pane in tab.panes.iter_mut() {
                pane.forget(ids);
            }
        }
        pins_first(&mut self.tabs, &mut self.active);
        for set in self.parked.values_mut() {
            pins_first(&mut set.0, &mut set.1);
        }
        self.recent.retain(|r| !ids.contains(r));
    }
    /// The "Undo" of a page's move to the Trash, for the tab the page was
    /// the one page of: the Trash left Home in its place there
    /// (`leave_pages`), and the page takes that place again, in front. No
    /// tab is made. False, and nothing changes, when that tab is gone or
    /// shows something else by now.
    fn put_page_back_in(&mut self, tab: &str, page: &str) -> bool {
        let Some(i) = self.tabs.iter().position(|t| t.id == tab && t.panes.len() == 1 && t.route() == &Route::Home) else { return false };
        let pane = self.tabs[i].focused_mut();
        let at = pane.cursor.min(pane.history.len() - 1);
        pane.history[at] = Route::Page(page.to_string());
        self.active = i;
        true
    }
    /// The tab in front goes to another space, which comes to the front
    /// with that tab showing.
    fn carry_tab_to(&mut self, space: &str, pages: &HashMap<Id, Page>) {
        let tab = self.tabs.remove(self.active.min(self.tabs.len() - 1));
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        let id = tab.id.clone();
        self.parked.entry(space.to_string()).or_default().0.push(tab);
        self.show_tabs_of(space, pages, Route::Home);
        self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
    }
}

// ---------------------------------------------------------------------------
// AI session shown inline under a block
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum AiStatus {
    Prompting,
    Streaming,
    Done,
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiSession {
    pub run: u64,
    pub page_id: Id,
    pub anchor: Id,
    pub action: AiAction,
    pub prompt: String,
    pub output: String,
    pub status: AiStatus,
    pub task: Option<Task>,
    /// What the AI does with the tools of Rune, a line for each step (the
    /// last one shows while it works).
    pub steps: Vec<String>,
    /// Where the tools write down what this run changes (see mcp.rs).
    pub journal: Option<std::path::PathBuf>,
    /// What the run changed with the tools, read when it ends.
    pub did: Vec<crate::mcp::Change>,
    /// The plan for a large job that the person said yes to: the next run
    /// does the work.
    pub plan: Option<String>,
    /// What the AI is writing for a tool now ("Writing the page “Plan” ·
    /// 2,400 characters"), and when this run started (for its clock).
    pub working: Option<String>,
    pub started: i64,
    /// When the AI last gave a sign (a word, a step): a card that waits
    /// says so, and does not say "Writing" for minutes with nothing new.
    pub heard: i64,
    /// Pictures that go with the request (screenshots pasted or dropped in).
    pub pictures: Vec<ai::Picture>,
    /// The page as it was given to the AI: a reply that is the whole page
    /// again is put in its place only while the page is still that.
    pub sent: String,
    /// What the person gave with the request besides pictures: files, pages
    /// of the notes, the words of a web page (see attach.rs).
    pub given: Vec<crate::attach::Attached>,
    /// The blocks of the page that were picked when the box opened: the
    /// request is about that part only, and a reply that writes the page
    /// again is that part as it must be, put in its place.
    pub part: Vec<Id>,
    /// A web address in the request that the person can ask the app to get.
    pub offer: Option<String>,
    /// The page was too long to go to the AI whole (see `ai::is_cut`): the
    /// card says so.
    pub cut: bool,
    /// The words of the line that a rewrite was asked for (Improve,
    /// Shorter, Translate…): the reply takes the line's place only while
    /// the line is still that.
    pub line_sent: String,
    /// After "Try again": how many of the changes in `did` are of the runs
    /// before this one (they stay in the list, so that Undo takes them back too).
    pub did_before: usize,
}

/// A page as the AI is given it (see `page_for_ai`), from these lines.
fn page_as_given(pages: &HashMap<Id, Page>, page_id: &str, blocks: &[Block]) -> String {
    let title = pages.get(page_id).map(|p| p.title.trim().to_string()).unwrap_or_default();
    let body = crate::mcp::render_body(pages, blocks);
    if title.is_empty() { body } else { format!("# {title}\n\n{body}") }
}

/// The picked part of a page as the AI is given it (see `part_for_ai`).
fn part_as_given(pages: &HashMap<Id, Page>, blocks: &[Block], part: &[Id]) -> String {
    let mut blocks: Vec<Block> = blocks.iter().filter(|b| part.contains(&b.id)).cloned().collect();
    // (From the edge: the step in that the whole part has is put back
    // when a reply takes its place, see `ai_revise`.)
    let least = blocks.iter().map(|b| b.indent).min().unwrap_or(0);
    for b in &mut blocks {
        b.indent -= least;
    }
    crate::mcp::render_body(pages, &blocks)
}

// ---------------------------------------------------------------------------
// Slash menu
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum SlashAction {
    Turn(BlockKind),
    SubPage,
    Ai(AiAction),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlashItem {
    pub section: &'static str,
    pub label: String,
    pub hint: &'static str,
    pub icon: &'static str,
    pub keywords: &'static str,
    pub action: SlashAction,
}

pub fn slash_items() -> Vec<SlashItem> {
    let turn = |kind: BlockKind, hint: &'static str, keywords: &'static str| SlashItem {
        section: "Basic blocks",
        label: kind.label().to_string(),
        hint,
        icon: kind.icon(),
        keywords,
        action: SlashAction::Turn(kind),
    };
    let ai = |action: AiAction, hint: &'static str, keywords: &'static str| SlashItem {
        section: "AI",
        label: action.label(),
        hint,
        icon: "sparkles",
        keywords,
        action: SlashAction::Ai(action),
    };
    vec![
        turn(BlockKind::Text, "Plain text", "paragraph plain"),
        turn(BlockKind::Heading1, "Big section heading", "h1 title #"),
        turn(BlockKind::Heading2, "Medium section heading", "h2 subtitle ##"),
        turn(BlockKind::Heading3, "Small section heading", "h3 ###"),
        turn(BlockKind::Bulleted, "Simple bulleted list", "ul bullet unordered -"),
        turn(BlockKind::Numbered, "List with numbering", "ol ordered numbered 1."),
        turn(BlockKind::Todo, "Track tasks with a checkbox", "todo task checkbox check []"),
        turn(BlockKind::Toggle, "Hide content inside", "toggle collapse >"),
        SlashItem {
            section: "Basic blocks",
            label: "Page".into(),
            hint: "Embed a sub-page",
            icon: "file",
            keywords: "page subpage new",
            action: SlashAction::SubPage,
        },
        turn(BlockKind::Quote, "Capture a quote", "quote blockquote \""),
        turn(BlockKind::Callout, "Make writing stand out", "callout note info tip"),
        turn(BlockKind::Code, "Capture a code snippet", "code snippet ```"),
        turn(BlockKind::Divider, "Visually divide blocks", "divider hr line ---"),
        turn(BlockKind::Image, "Add a picture or a screenshot", "image picture photo screenshot png jpg"),
        turn(BlockKind::Table, "Rows and columns: dates, numbers, text", "table grid rows columns cells spreadsheet log"),
        turn(BlockKind::Chart, "Bars, a line, a ring or days from a table", "chart graph plot bars line ring progress streak days"),
        turn(BlockKind::Drawing, "Sketch with Excalidraw", "drawing excalidraw sketch whiteboard diagram draw canvas"),
        ai(AiAction::Ask, "Ask anything about this page", "ai ask chat question"),
        ai(AiAction::Continue, "Keep writing from here", "ai continue write"),
        ai(AiAction::Summarize, "Summarize the whole page", "ai summary summarize tldr"),
        ai(AiAction::ActionItems, "Pull out to-dos", "ai action items todo tasks"),
        ai(AiAction::Improve, "Rewrite this block", "ai improve rewrite"),
        ai(AiAction::FixGrammar, "Fix this block", "ai fix spelling grammar"),
        ai(AiAction::Translate("English".into()), "Translate this block", "ai translate english"),
    ]
}

pub fn filter_slash(query: &str) -> Vec<SlashItem> {
    filter_slash_for(query, ai::shown())
}

/// The rows of the "/" menu for what is typed. Its AI rows are there only
/// with an AI tool connected (`ai_on`).
pub fn filter_slash_for(query: &str, ai_on: bool) -> Vec<SlashItem> {
    let q = query.to_lowercase();
    slash_items()
        .into_iter()
        .filter(|item| ai_on || !matches!(item.action, SlashAction::Ai(_)))
        .filter(|item| {
            q.is_empty()
                || item.label.to_lowercase().contains(&q)
                || item.keywords.split_whitespace().any(|k| k.starts_with(&q))
        })
        .collect()
}


// ---------------------------------------------------------------------------
// Messages from the JavaScript keyboard / drag-and-drop bridge
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Bridge {
    /// The text of a block after typing (inline Markdown; raw for code),
    /// and where the caret is in it (in the letters that show).
    Input {
        page: String,
        id: String,
        md: String,
        #[serde(default)]
        pos: Option<usize>,
    },
    Enter { page: String, id: String, pos: usize, value: String },
    Backspace { page: String, id: String, value: String },
    /// Delete (forward) with the caret at the very end of a block.
    Delete { page: String, id: String, value: String },
    Paste { page: String, id: String, pos: usize, value: String, text: String },
    Undo,
    Redo,
    OpenUrl { url: String },
    Drag,
    ZoomWindow,
    SidebarWidth { w: f64 },
    TabDrop { from: String, #[serde(default)] to: Option<String>, mode: String },
    /// Back (`back`) or Forward from a mouse's side button or a swipe of two
    /// fingers, in the pane under the pointer if it was over one.
    PaneStep { #[serde(default)] pane: Option<String>, back: bool },
    /// A drawing's frame saved its scene.
    DrawingSave { page: String, id: String, data: String },
    /// A picture pasted or dropped at a block (base64).
    ImageData { page: String, id: String, data: String },
    /// A picture pasted into, or dropped on, a picture cell of a table
    /// (block `id`, row `r`, column `c`; base64).
    CellPicture { page: String, id: String, r: usize, c: usize, data: String },
    /// A picture was dragged to a side of its page (the right one with
    /// `right`), `x` from that side's edge of the page's column, at the
    /// width `w`, level with the line `to` (after it, with `after`).
    ImagePlace { page: String, id: String, x: f64, w: f64, #[serde(default)] right: bool, #[serde(default)] to: Option<String>, #[serde(default)] after: bool },
    /// A picture was made wider or narrower.
    ImageSize { page: String, id: String, w: f64 },
    /// Pick every block of a page.
    PickAll { page: String },
    /// Pick one block and what is nested in it; with `extend`, all the blocks
    /// from those picked already up to it.
    Pick { page: String, id: String, #[serde(default)] extend: bool },
    /// Nothing is picked any more.
    PickClear,
    /// Take a picture out of its page (Delete while it is shown large).
    ImageDelete { page: String, id: String },
    /// A page of the sidebar dragged onto another page ("into"), between
    /// pages ("before" or "after" the page `target`), or below the list
    /// ("root"): see `plan_page_move`.
    PageMove { page: String, #[serde(default)] target: Option<String>, mode: String },
    /// A row or column of a table added or taken out ("add-row",
    /// "add-column", "remove-row", "remove-column"), at a cell; or a row
    /// opened ("open-row"), moved to before the row `column` ("move-row"),
    /// its menu asked for ("row-menu"), its nested rows folded ("fold-row").
    TableOp { page: String, id: String, op: String, row: usize, column: usize },
    /// A cell of a table given other words (a card dragged to another
    /// column of a board).
    TableSet { page: String, id: String, row: usize, column: usize, text: String },
    /// A cover picture was dragged to show another part.
    CoverY { page: String, y: f64 },
    /// Where the frame of the video player is in the window.
    PlayerPlace { x: f64, y: f64, w: f64, h: f64 },
    /// The person moved the video player or gave it another size.
    PlayerDock { h: String, v: String, x: f64, y: f64, w: f64 },
    /// The steps of the fingers on the sidebar and what was made of them
    /// (numbers only), when the recorder is on: see `swipe_trace`.
    SwipeTrace { lines: String },
    /// A picture pasted or dropped into the AI box (a data address).
    AiImage { data: String },
    /// A file that is no picture, dropped or pasted into the AI box: its
    /// name and its bytes in base64.
    AiFile { name: String, data: String },
    /// An event was dragged in the calendar: so many days and minutes later
    /// (or earlier), or its end so many minutes later.
    EventMove { id: String, days: i64, minutes: i64, grow: i64 },
    /// A stretch of empty time was dragged over in the calendar: a new event
    /// from one minute of that day to another.
    EventDraw { day: String, from: i64, to: i64 },
    /// A press on empty time in a column of the calendar: the quarter of an
    /// hour under the pointer (see `cal_press_minute`).
    CalPress { minute: i64 },
    /// "@" and some letters in a text box (a cell, the notes of an event,
    /// the Ask box): the pages with that name are asked for. `n` comes back
    /// with the answer, so that a late answer to earlier letters is not shown.
    PagesFind {
        q: String,
        n: u32,
        #[serde(default)]
        here: String,
    },
    /// The pointer rests on a link to a page: what the card over it says is
    /// asked for (see `preview_page`). `n` comes back with the answer.
    PagePreview { id: String, n: u32 },
    /// A page chosen from that list in the Ask box: it goes with the question.
    AskGivePage { id: String },
    /// ⌘C on an event of the calendar.
    EventCopy { id: String },
    /// ⌘V in the calendar: the copied event on `day`, from `minute` of that
    /// day (or at its own time, with a negative minute: a day of the month
    /// or the row of all-day events).
    EventPaste { day: String, minute: i64 },
    /// A to-do was dragged onto a time of a day in the calendar.
    TodoPlan { page: String, block: String, day: String, minutes: i64 },
    /// A drawing's frame opened and this window has no scene for it yet.
    DrawingReady { id: String },
    /// One page of a tab that shows several, dragged to the tab bar (see detach_pane).
    SegDrop { tab: String, pane: String, #[serde(default)] to: Option<String>, mode: String },
    /// Something dropped on the edge of a pane: a tab, a sidebar page, or one
    /// page of a tab (by its part of the tab or its bar).
    PlaceTab { from: String, target: String, edge: String },
    PlacePage { page: String, target: String, edge: String },
    PlacePane { tab: String, pane: String, target: String, edge: String },
    /// New shares for a row or column of pages (a divider was dragged).
    SplitSizes { split: String, sizes: Vec<f64> },
    /// The microphone on the format bar: change the selected text as told.
    SpeakEdit { page: String, id: String, start: usize, end: usize },
    /// "Copy picture" from the short menu of a picture: the kept picture of
    /// this name goes on the clipboard.
    CopyPicture { name: String },
    /// Copy (or Cut) from the short text menu: put this on the clipboard.
    CopyText { text: String },
    /// A pane folded to a rail (or opened again) by dragging its divider.
    FoldPane { pane: String, on: bool },
    /// A sidebar page dropped on the tab strip.
    PageStripDrop { page: String, #[serde(default)] to: Option<String>, mode: String },
    Block { page: String, id: String, action: String, #[serde(default)] pos: usize },
    Nav { page: String, id: String, dir: String },
    Menu { action: String },
    Move { page: String, from: String, to: String, after: bool },
    Shortcut { name: String },
    Focus { page: String, id: String },
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub const SIDEBAR_DEFAULT: f64 = 256.0;
pub const SIDEBAR_MIN: f64 = 190.0;
pub const SIDEBAR_MAX: f64 = 320.0;

// Typed tables and charts (state/tables.rs).
mod tables;
pub use tables::{stale_row_titles, written_row_pages, ChartSource, Open};

mod erased {
    pub trait Json {
        fn json(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> String {
            serde_json::to_string(self).unwrap_or_default()
        }
    }
}

/// Which window this state belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Win {
    /// Key in the cross-window sync registry.
    pub key: u64,
    /// The first window; only it remembers its tabs between launches.
    pub primary: bool,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub pages: Signal<HashMap<Id, Page>>,
    pub blocks: Signal<HashMap<Id, Vec<Block>>>,
    pub ui: Signal<Ui>,
    pub ai: Signal<Option<AiSession>>,
    pub history: Signal<History>,
    pub win: Win,
    /// The `/` menu, kept apart from `ui` so typing in it redraws only the page.
    pub slash: Signal<Option<SlashState>>,
    /// The block with the caret. Never read while drawing, so changing it
    /// redraws nothing.
    pub focus: Signal<Option<(Id, Id)>>,
    /// (id, text, leaving, what its "Undo" puts back): shown for a second
    /// (six with an "Undo"), then it fades out.
    pub toast_msg: Signal<Option<(u64, String, bool, Option<Back>)>>,
    /// How see-through the window is.
    pub look: Signal<Look>,
    /// A toggle that was just opened (page, block), so its children ease in.
    pub reveal: Signal<Option<(Id, Id)>>,
    pub spaces: Signal<Vec<Space>>,
    pub events: Signal<HashMap<Id, Event>>,
    pub cal: Signal<Cal>,
    /// A newer build that is ready to install (see update.rs).
    pub update: Signal<Option<crate::update::Update>>,
    /// A page is being read aloud.
    pub reading: Signal<bool>,
    /// The app is listening to a meeting (see listen.rs).
    pub listening: Signal<Option<Listening>>,
    /// The meeting page that asks "Listen and write the notes?".
    pub listen_offer: Signal<Option<Id>>,
    /// The blocks of a page picked as a whole (see `Picked`).
    pub picked: Signal<Option<Picked>>,
    /// The video in the player at the corner of the window, if one is open.
    pub player: Signal<Option<Playing>>,
    /// The calendars of this Mac, when the person has turned them on.
    pub mac_cal: Signal<MacCal>,
    /// The notes page of each repeat of a repeating event, by the repeat's
    /// id ("<event id>@<day>"), kept as the setting `REPEAT_PAGES` (see
    /// `repeat::page_of`).
    pub repeat_pages: Signal<HashMap<String, Id>>,
    /// The side the list of a space comes in from when the space changes:
    /// 1 from the right (a space after this one), -1 from the left, 0 none.
    pub space_slide: Signal<i8>,
    /// Pictures for the box that asks all the notes (see `ai_add_picture`).
    pub ask_pictures: Signal<Vec<ai::Picture>>,
    /// Files given with a question to all the notes (see attach.rs).
    pub ask_given: Signal<Vec<crate::attach::Attached>>,
    /// Is an AI tool connected? Every part of the AI shows only then
    /// (see `ai::shown`); Settings is where a person connects one.
    pub ai_on: Signal<bool>,
    /// What the AI did to a page last (it changed the page and made
    /// sub-pages): shown for a moment at the foot of the page, with Undo.
    pub ai_done: Signal<Option<AiDone>>,
    /// The part of Settings to open first, when something sends the person
    /// to one part of it (the name of the part, as its button says it).
    pub settings_first: Signal<Option<&'static str>>,
    /// Where the video player is in the window, and how wide (kept between runs).
    pub player_dock: Signal<DockPlace>,
    /// The pages that ask which page to keep them in, each with the time
    /// from which it asks (the notes of a meeting ask when it is over). Kept
    /// between runs of the app (the "place_asks" setting).
    pub place_asks: Signal<Vec<(Id, i64)>>,
    /// The page whose "Move to…" was chosen: it asks now, whatever it is.
    pub place_now: Signal<Option<Id>>,
    /// The notes page that shows beside the day in the calendar's day view,
    /// while it shows. It is a page on screen like one in a pane of its own.
    /// (Read with `peek`: nothing is drawn from it.)
    pub day_page: Signal<Option<Id>>,
    /// A new event still without a name, on the grid of the calendar: when
    /// it starts and ends (a click gives a quarter of an hour, a drag more).
    pub cal_draft: Signal<Option<(i64, i64)>>,
    /// When each page was last written to in this window. (The page's own
    /// record is not touched at every key: the sidebar would redraw.)
    pub edited: Signal<HashMap<Id, i64>>,
    /// The pad of the meeting in progress: a card of its own on the screen,
    /// outside this window (see pad.rs). The first window keeps it.
    pub pad: Signal<Option<crate::pad::Pad>>,
    /// While Control is held after Ctrl+Tab: the tabs in the order of use
    /// and the one reached, for the list in the middle of the window (see
    /// `walk_tabs`; the order itself is kept in `TAB_USE`).
    pub tab_walk: Signal<Option<(Vec<Id>, usize)>>,
}

/// Blocks of one page picked as a whole (Command-A a second time, or a press
/// on a block that is not text): they are copied, cut or deleted together.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Picked {
    pub page: Id,
    pub ids: Vec<Id>,
}

/// A to-do that is not done yet, wherever it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTodo {
    pub page: Id,
    pub page_title: String,
    pub block: Id,
    pub text: String,
    /// The day it is due, when it has one.
    pub due: Option<chrono::NaiveDate>,
}

/// A meeting being listened to.
#[derive(Clone, Debug, PartialEq)]
pub struct Listening {
    /// The page its transcript and notes go in.
    pub page: Id,
    pub started: i64,
    /// Seconds so far (kept up to date for the display).
    pub seconds: i64,
    /// The Mac's sound is heard too (the other people), not only the microphone.
    pub others: bool,
    pub words: usize,
    /// The last few words heard.
    pub latest: String,
    /// Asked to stop; the last words are being finished.
    pub stopping: bool,
    /// It hears a video in the player (only what the Mac plays), not a meeting.
    pub video: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalView {
    Month,
    Week,
    /// One day at the left, and the notes of one of its events at the right.
    Day,
}

impl CalView {
    fn as_str(self) -> &'static str {
        match self {
            CalView::Month => "month",
            CalView::Week => "week",
            CalView::Day => "day",
        }
    }

    fn parse(s: Option<&str>) -> CalView {
        match s {
            Some("month") => CalView::Month,
            Some("day") => CalView::Day,
            _ => CalView::Week,
        }
    }
}

/// What the calendar shows: month or week, around which day, and the event
/// open in the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct Cal {
    pub view: CalView,
    pub anchor: chrono::NaiveDate,
    pub selected: Option<Id>,
    /// In the day view an event's notes show without its panel (time,
    /// repeat, reminder); this is whether the panel is open too.
    pub details: bool,
    /// The calendar shows the events of every space, each marked with its
    /// space (the default), or only those of the space in front. Kept as the
    /// setting "cal_spaces" ("all" or "here").
    pub all_spaces: bool,
}

/// Glass settings: how see-through the window is, whether what's behind it is
/// blurred, and the colour it is tinted with ("" for neutral, or "#rrggbb").
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// How see-through the window is: 0 solid … 100 clearest.
    pub glass: u8,
    /// How much what's behind the window is blurred, in points (0 is sharp).
    pub blur: u8,
    pub tint: String,
    /// The colour of links, rings and switches: "" for the app's blue, or
    /// one of `ACCENTS`.
    pub accent: String,
}

/// The accent colours a person can pick, besides the app's blue (""):
/// (what it is called, its name in the settings and in style.css, the
/// colour of its swatch). Soft and few, so the app stays calm.
pub const ACCENTS: [(&str, &str, &str); 6] = [
    ("Blue", "", "#2f7ae5"),
    ("Grey", "grey", "#6b7279"),
    ("Green", "green", "#3c8f62"),
    ("Orange", "orange", "#c96d33"),
    ("Pink", "pink", "#c2568a"),
    ("Purple", "purple", "#7a5fc6"),
];

impl Look {
    pub fn load() -> Look {
        Look {
            glass: store::setting("glass").and_then(|v| v.parse().ok()).map(|v: u8| v.min(100)).unwrap_or(glass::GLASS_DEFAULT),
            blur: blur_setting(store::setting("glass_radius").as_deref(), store::setting("glass_blur").as_deref()),
            tint: store::setting("glass_tint").filter(|t| valid_hex(t)).unwrap_or_default(),
            accent: store::setting("accent").filter(|a| ACCENTS.iter().any(|(_, id, _)| id == a)).unwrap_or_default(),
        }
    }
}

/// The blur radius from what Settings saved: the radius itself, or (from
/// builds where Blur was a switch) off as none and on as the usual amount.
fn blur_setting(radius: Option<&str>, old_switch: Option<&str>) -> u8 {
    match (radius.and_then(|v| v.parse::<u8>().ok()), old_switch) {
        (Some(radius), _) => radius.min(glass::BLUR_MAX),
        (None, Some("0")) => 0,
        _ => glass::BLUR_DEFAULT,
    }
}

pub fn valid_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// How an edit to a page's blocks came about.
#[derive(Clone, Copy, PartialEq)]
enum Origin<'a> {
    /// Typing in this block; the page already shows `text`.
    Typing { block: &'a str, text: &'a str },
    /// Typing in one cell of this table. `group` names the cell: letters typed
    /// in the same cell without a pause are one step for Undo.
    Cell { block: &'a str, group: &'a str },
    Other,
}

/// Closed tabs that can be reopened: the most recent few.
fn trim_closed(closed: &mut Vec<Tab>) {
    if closed.len() > CLOSED_TABS {
        let extra = closed.len() - CLOSED_TABS;
        closed.drain(..extra);
    }
}

/// Note a page as recently visited and open its parents in the sidebar.
fn remember(ui: &mut Ui, pages: &HashMap<Id, Page>, route: &Route) {
    if let Route::Page(id) = route {
        ui.recent.retain(|r| r != id);
        ui.recent.insert(0, id.clone());
        ui.recent.truncate(20);
        // (The page of a row is not in the sidebar: its parent stays as it is.)
        if pages.get(id).is_none_or(|p| p.row_of.is_none()) {
            for a in ancestors(pages, id) {
                ui.expanded.insert(a.id);
            }
        }
    }
}

/// The names in a path typed in search ("Work / Plan" → Work, Plan); empty
/// parts are left out.
pub fn path_parts(typed: &str) -> Vec<String> {
    typed.split('/').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()
}

/// How far a path of page names goes in a space: the deepest page found,
/// and the names after it that have no page yet. The first name may be a
/// page anywhere in the space (one at the top first); each next name is a
/// page inside the one before. Names are matched without regard to case.
pub fn find_path(pages: &HashMap<Id, Page>, space: &str, names: &[String]) -> (Option<Id>, Vec<String>) {
    let same = |p: &Page, name: &str| p.title.trim().to_lowercase() == name.to_lowercase();
    let mut parent: Option<Id> = None;
    for (i, name) in names.iter().enumerate() {
        let found = if i == 0 {
            roots_in(pages, space).into_iter().find(|p| same(p, name)).or_else(|| {
                let mut any: Vec<&Page> =
                    pages.values().filter(|p| !p.trashed && p.row_of.is_none() && p.space_id == space && same(p, name)).collect();
                any.sort_by_key(|p| p.created_at);
                any.first().map(|p| (*p).clone())
            })
        } else {
            children_of(pages, parent.as_deref()).into_iter().find(|p| same(p, name))
        };
        match found {
            Some(p) => parent = Some(p.id),
            None => return (parent, names[i..].to_vec()),
        }
    }
    (parent, Vec::new())
}

/// Top-level pages of a space, not trashed, in sidebar order.
pub fn roots_in(pages: &HashMap<Id, Page>, space: &str) -> Vec<Page> {
    children_of(pages, None).into_iter().filter(|p| p.space_id == space).collect()
}

/// Pages under `parent` (or top level), not trashed, in sidebar order.
pub fn children_of(pages: &HashMap<Id, Page>, parent: Option<&str>) -> Vec<Page> {
    let mut out: Vec<Page> = pages
        .values()
        // (A page that is a row of a table opens from its row, not from here.)
        .filter(|p| !p.trashed && p.row_of.is_none() && p.parent_id.as_deref() == parent)
        .cloned()
        .collect();
    out.sort_by(|a, b| a.position.total_cmp(&b.position).then(a.created_at.cmp(&b.created_at)));
    out
}

/// Where a page goes when it is moved in the sidebar: its new parent, its
/// place among that parent's pages, and its space. `mode` is "into" (inside
/// the page `target`, after the pages there), "before" or "after" (beside
/// the page `target`), or "root" (the end of the top level of `space`).
/// `None` when the move makes no sense: onto itself, into or beside a page
/// that is inside it, or to where it already is.
pub fn plan_page_move(pages: &HashMap<Id, Page>, space: &str, id: &str, target: Option<&str>, mode: &str) -> Option<(Option<Id>, f64, String)> {
    let page = pages.get(id).filter(|p| !p.trashed)?;
    let inside = descendants(pages, id);
    let others = |list: Vec<Page>| -> Vec<Page> { list.into_iter().filter(|p| p.id != id).collect() };
    let after_last = |list: &[Page]| list.last().map(|p| p.position + 1.0).unwrap_or(1.0);
    let plan = match mode {
        "into" => {
            let t = pages.get(target?).filter(|t| !t.trashed && !inside.contains(&t.id))?;
            let kids = others(children_of(pages, Some(&t.id)));
            (Some(t.id.clone()), after_last(&kids), t.space_id.clone())
        }
        "before" | "after" => {
            let t = pages.get(target?).filter(|t| !t.trashed && !inside.contains(&t.id))?;
            let beside = others(match t.parent_id.as_deref() {
                Some(parent) => children_of(pages, Some(parent)),
                None => roots_in(pages, &t.space_id),
            });
            let at = beside.iter().position(|p| p.id == t.id)?;
            let position = if mode == "before" {
                let low = at.checked_sub(1).map(|i| beside[i].position).unwrap_or(t.position - 1.0);
                (low + t.position) / 2.0
            } else {
                let high = beside.get(at + 1).map(|p| p.position).unwrap_or(t.position + 2.0);
                (t.position + high) / 2.0
            };
            (t.parent_id.clone(), position, t.space_id.clone())
        }
        "root" => (None, after_last(&others(roots_in(pages, space))), space.to_string()),
        _ => return None,
    };
    // Already there: the same parent, and no other page between it and that place.
    if plan.0 == page.parent_id && plan.2 == page.space_id {
        let beside = match page.parent_id.as_deref() {
            Some(parent) => children_of(pages, Some(parent)),
            None => roots_in(pages, &page.space_id),
        };
        let (low, high) = (page.position.min(plan.1), page.position.max(plan.1));
        if !beside.iter().any(|p| p.id != id && p.position >= low && p.position <= high) {
            return None;
        }
    }
    Some(plan)
}

/// The rows of the list to choose where a page is kept. With several
/// spaces: each space, which is a choice itself (the top of its list), with
/// its pages under it. With one space there is nothing to tell apart: a row
/// for the top of the list, then the pages. Left out: the page itself, what
/// is inside it, the notes of meetings (`meetings`) and the Trash.
pub fn places_to_keep(pages: &HashMap<Id, Page>, spaces: &[Space], page: &str, meetings: &[Id]) -> Vec<crate::pad::Place> {
    use crate::pad::Place;
    type Kids<'a> = HashMap<Option<&'a str>, Vec<&'a Page>>;
    fn walk(kids: &Kids, list: &[&Page], depth: usize, page: &str, meetings: &[Id], out: &mut Vec<Place>) {
        for p in list.iter().filter(|p| p.id != page) {
            let shown = !meetings.contains(&p.id);
            if shown {
                out.push(Place { id: p.id.clone(), title: p.display_title().to_string(), depth, icon: p.icon.clone(), badge: None });
            }
            if let Some(inner) = kids.get(&Some(p.id.as_str())) {
                walk(kids, inner, depth + usize::from(shown), page, meetings, out);
            }
        }
    }
    let mut kids: Kids = HashMap::new();
    for p in pages.values().filter(|p| !p.trashed) {
        kids.entry(p.parent_id.as_deref()).or_default().push(p);
    }
    for list in kids.values_mut() {
        list.sort_by(|a, b| a.position.total_cmp(&b.position).then(a.created_at.cmp(&b.created_at)));
    }
    let many = spaces.len() > 1;
    let mut out = Vec::new();
    for space in spaces {
        out.push(if many {
            Place { id: space.id.clone(), title: space.display_name().to_string(), depth: 0, icon: None, badge: Some((space.initial(), space.color.clone())) }
        } else {
            Place { id: String::new(), title: NO_PAGE.to_string(), depth: 0, icon: None, badge: None }
        });
        let roots: Vec<&Page> = kids.get(&None).into_iter().flatten().copied().filter(|p| p.space_id == space.id).collect();
        walk(&kids, &roots, usize::from(many), page, meetings, &mut out);
    }
    out
}

/// The row that says where a page is kept now, and the name of its space
/// when there are several and the row does not say it.
pub fn place_of(pages: &HashMap<Id, Page>, spaces: &[Space], page: &str) -> (crate::pad::Place, String) {
    use crate::pad::Place;
    let this = pages.get(page);
    let space = this.and_then(|p| spaces.iter().find(|s| s.id == p.space_id));
    let many = spaces.len() > 1;
    if let Some(home) = this.and_then(|p| p.parent_id.as_ref()).and_then(|id| pages.get(id)) {
        let place = Place { id: home.id.clone(), title: home.display_title().to_string(), depth: 0, icon: home.icon.clone(), badge: None };
        return (place, space.filter(|_| many).map(|s| s.display_name().to_string()).unwrap_or_default());
    }
    match space.filter(|_| many) {
        Some(s) => (Place { id: s.id.clone(), title: s.display_name().to_string(), depth: 0, icon: None, badge: Some((s.initial(), s.color.clone())) }, String::new()),
        None => (Place { id: String::new(), title: NO_PAGE.to_string(), depth: 0, icon: None, badge: None }, String::new()),
    }
}

/// The name of the top of the list, when there is one space.
const NO_PAGE: &str = "No page (top of the list)";

/// One event on the line of the day (Home).
#[derive(Clone, Debug, PartialEq)]
pub struct RibbonEvent {
    pub id: Id,
    pub title: String,
    pub when: String,
    pub color: String,
    /// Percent from the left of the line, and percent wide.
    pub left: f64,
    pub width: f64,
    /// Percent of the line its name may take (up to the next event of its row).
    pub room: f64,
    /// Near the right end: its name is set from its right edge.
    pub at_end: bool,
    /// Events that start so near each other that their names would lie
    /// over each other share one place on the line: their names are one
    /// under the other there. This is which of those lines it is (0 the first).
    pub row: usize,
    /// Percent from the left where its box (bar and name) begins.
    pub box_left: f64,
    /// It shares its place with other events: its name and its time are on one line.
    pub together: bool,
    /// How many more events share the place than there are lines for (said on this line).
    pub more: usize,
    /// Its name shows (it is one of the first few of its place).
    pub named: bool,
}

/// The line of a day: the hours it runs over, its timed events, the names
/// of its all-day events, and where "now" is (percent), if it is on the line.
#[derive(Clone, Debug, PartialEq)]
pub struct Ribbon {
    pub first_hour: u32,
    pub last_hour: u32,
    pub events: Vec<RibbonEvent>,
    /// The most names that are one under the other at one place (one at the least).
    pub rows: usize,
    pub all_day: Vec<(Id, String)>,
    pub now: Option<f64>,
}

/// Lay the events of one day (already those of that day only) out on a line
/// from 8 AM to 6 PM, or longer when an event is outside those hours.
pub fn day_ribbon(events: &[Event], day_start: i64, now: i64) -> Ribbon {
    let minute = |ms: i64| ((ms - day_start) / crate::when::MINUTE).clamp(0, 24 * 60);
    let mut timed: Vec<&Event> = events.iter().filter(|e| !e.all_day).collect();
    timed.sort_by_key(|e| (e.start, e.end));
    let first_hour = timed.iter().map(|e| minute(e.start) / 60).min().map_or(8, |h| h.min(8)) as u32;
    let last_hour = timed.iter().map(|e| (minute(e.end.max(e.start)) + 59) / 60).max().map_or(18, |h| h.max(18)).min(24) as u32;
    let span = ((last_hour - first_hour) * 60).max(60) as f64;
    let pct = |m: i64| (((m - first_hour as i64 * 60) as f64 / span) * 100.0).clamp(0.0, 100.0);
    let mut out: Vec<RibbonEvent> = timed
        .iter()
        .map(|e| {
            let left = pct(minute(e.start));
            let width = (pct(minute(e.end)) - left).max(1.2).min(100.0 - left);
            RibbonEvent {
                id: e.id.clone(),
                title: e.display_title().to_string(),
                when: crate::when::time_label(e.start),
                color: e.color.clone(),
                left,
                width,
                room: 100.0 - left,
                at_end: left > 78.0,
                row: 0,
                box_left: left,
                together: false,
                more: 0,
                named: true,
            }
        })
        .collect();
    // One line for the day. Events that start so near each other that their
    // names would lie over each other (two meetings at 11) share one place:
    // each has its bar on the line, and their names are one under the other.
    const NAME_ROOM: f64 = 15.0;
    const MOST_NAME_ROOM: f64 = 40.0;
    const MOST_NAMES: usize = 3;
    let mut places: Vec<Vec<usize>> = Vec::new();
    let mut place_left = 0.0;
    for (i, e) in out.iter().enumerate() {
        match places.last_mut() {
            Some(place) if e.left < place_left + NAME_ROOM => place.push(i),
            _ => {
                places.push(vec![i]);
                place_left = e.left;
            }
        }
    }
    // Where the names before a place end (for a place near the end of the
    // day, whose names run back from its right edge).
    let mut free_from: f64 = 0.0;
    for (n, place) in places.iter().enumerate() {
        let left = out[place[0]].left;
        let end = place.iter().map(|&i| out[i].left + out[i].width).fold(0.0, f64::max);
        let at_end = left > 78.0;
        let (box_left, room) = if at_end {
            let room = (end - free_from).clamp(NAME_ROOM.min(end), MOST_NAME_ROOM.max(end - left)).min(end);
            (end - room, room)
        } else {
            // Up to the next place (never less than a little).
            let room = match places.get(n + 1).map(|next| out[next[0]].left) {
                Some(next) => (next - left - 1.0).max(6.0).min(100.0 - left),
                None => 100.0 - left,
            };
            (left, room)
        };
        free_from = (left + NAME_ROOM).max(end).min(100.0);
        let hidden = place.len().saturating_sub(MOST_NAMES);
        for (row, &i) in place.iter().enumerate() {
            let e = &mut out[i];
            e.at_end = at_end;
            e.box_left = box_left;
            e.room = room;
            e.row = row;
            e.together = place.len() > 1;
            e.named = row < MOST_NAMES;
            e.more = if row + 1 == MOST_NAMES { hidden } else { 0 };
        }
    }
    let rows = places.iter().map(|p| p.len().min(MOST_NAMES)).max().unwrap_or(1);
    let now_minute = (now - day_start) / crate::when::MINUTE;
    let on_line = now_minute >= first_hour as i64 * 60 && now_minute <= last_hour as i64 * 60;
    Ribbon {
        first_hour,
        last_hour,
        events: out,
        rows,
        all_day: events.iter().filter(|e| e.all_day).map(|e| (e.id.clone(), e.display_title().to_string())).collect(),
        now: on_line.then(|| pct(now_minute)),
    }
}

/// The first words of a page, for a card that shows what the page is about.
pub fn snippet_of(blocks: &[Block], most: usize) -> String {
    let mut out = String::new();
    for b in blocks {
        if matches!(b.kind, BlockKind::Code | BlockKind::Divider | BlockKind::Drawing | BlockKind::Image | BlockKind::Table | BlockKind::Chart | BlockKind::PageLink) {
            continue;
        }
        let line = b.plain_text();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
        if out.chars().count() >= most {
            break;
        }
    }
    if out.chars().count() > most {
        let cut: String = out.chars().take(most).collect();
        let cut = cut.rsplit_once(' ').map(|(head, _)| head.to_string()).filter(|h| h.chars().count() > most / 2).unwrap_or(cut);
        return format!("{}…", cut.trim_end_matches([',', '.', ';', ':', ' ']));
    }
    out
}

/// What the card over a link to a page says (see `page_preview`).
#[derive(Clone, Debug, PartialEq)]
pub struct PagePreview {
    pub title: String,
    pub icon: Option<String>,
    /// Where the page is: the pages it is inside, or its space.
    pub place: String,
    /// Its first lines, as plain words. None for a locked page.
    pub lines: Vec<String>,
    pub locked: bool,
}

/// The lines a card shows of a page, and one more (the card cuts what does
/// not fit, with its sign that there is more).
const PREVIEW_LINES: usize = 5;

/// What the card over a link to a page says. Nothing for a page that is in
/// the Trash or gone. A locked page gives its name only, also while it is
/// open (as search and "Linked from": see `lock::hides`). The words are
/// plain: a table says "Table", a chart "Chart", a line that is a sub-page
/// its name, and a picture, a drawing or code nothing.
pub fn page_preview(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, spaces: &[Space], id: &str) -> Option<PagePreview> {
    let page = pages.get(id).filter(|p| !p.trashed)?;
    let above = ancestors(pages, id);
    let place = if above.is_empty() {
        spaces.iter().find(|s| s.id == page.space_id).map(|s| s.name.clone()).unwrap_or_default()
    } else {
        above.iter().map(|a| a.display_title().to_string()).collect::<Vec<_>>().join(" / ")
    };
    let own = blocks.get(id).map(Vec::as_slice).unwrap_or_default();
    let locked = crate::lock::hides(own, id);
    let mut lines = Vec::new();
    for b in own.iter().filter(|_| !locked) {
        let line = match b.kind {
            BlockKind::Table => "Table".to_string(),
            BlockKind::Chart => "Chart".to_string(),
            BlockKind::PageLink => b.target.as_ref().and_then(|t| pages.get(t)).filter(|p| !p.trashed).map(|p| p.display_title().to_string()).unwrap_or_default(),
            _ => snippet_of(std::slice::from_ref(b), 200),
        };
        if !line.is_empty() {
            lines.push(line);
        }
        if lines.len() == PREVIEW_LINES {
            break;
        }
    }
    Some(PagePreview { title: page.display_title().to_string(), icon: page.icon.clone(), place, lines, locked })
}

/// The tables that a change of a page from `current` to `target` puts back
/// (Undo, Redo, an older version of the page). A table that was cut here
/// and pasted in another page is the same block there (see `paste`): when
/// a table with its id is in another page (`elsewhere`), the one put back
/// is a copy, and gets an id of its own. With one id in two pages, the two
/// tables shared the pages of their rows. Returns the ids of the others,
/// which come back as the blocks they were, and for each copy the id it
/// had and the id it has now.
pub fn tables_put_back(target: &mut [Block], current: &[Block], elsewhere: impl Fn(&str) -> bool) -> (Vec<Id>, Vec<(Id, Id)>) {
    let (mut back, mut copies) = (Vec::new(), Vec::new());
    for b in target.iter_mut().filter(|b| b.kind == BlockKind::Table) {
        if current.iter().any(|c| c.id == b.id) {
            continue;
        }
        if elsewhere(&b.id) {
            let was = std::mem::replace(&mut b.id, new_id());
            copies.push((was, b.id.clone()));
        } else {
            back.push(b.id.clone());
        }
    }
    (back, copies)
}

/// Put text on the Mac's clipboard. A test run keeps the person's clipboard
/// as it is: the text goes where the test can read it instead.
fn set_clipboard(text: String) -> Result<(), String> {
    if std::env::var_os("RUNE_SELFTEST").is_some() {
        document::eval(&format!("document.body.dataset.copied = {};", serde_json::to_string(&text).unwrap_or_default()));
        return Ok(());
    }
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())
}

/// The blocks to pick for a press on the block `id`: that block and what is
/// nested in it. With blocks picked already (`from`, for a press with Shift):
/// every block from the first of those to this one, in the order of the page.
pub fn pick_blocks(blocks: &[Block], from: Option<&[Id]>, id: &str) -> Vec<Id> {
    let Some(at) = index_of(blocks, id) else { return Vec::new() };
    let end = subtree_end(blocks, at);
    let (mut first, mut last) = (at, end);
    if let Some(from) = from {
        let held: Vec<usize> = blocks.iter().enumerate().filter(|(_, b)| from.contains(&b.id)).map(|(i, _)| i).collect();
        if let (Some(&lo), Some(&hi)) = (held.first(), held.last()) {
            first = first.min(lo);
            last = last.max(hi + 1);
        }
    }
    blocks[first..last.min(blocks.len())].iter().map(|b| b.id.clone()).collect()
}

/// The pages to offer for a link, for what is typed after "@" or "[[": those
/// of this space whose name has every typed word (not the page itself, not
/// the Trash). Names that begin with what is typed come first, then the
/// pages changed last. With nothing typed: the pages changed last.
pub fn pages_to_link(pages: &HashMap<Id, Page>, space: &str, here: &str, query: &str) -> Vec<Page> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<Page> = pages
        .values()
        .filter(|p| !p.trashed && p.id != here && p.space_id == space)
        .filter(|p| {
            let title = p.display_title().to_lowercase();
            words.iter().all(|w| title.contains(w))
        })
        .cloned()
        .collect();
    found.sort_by(|a, b| {
        let begins = |p: &Page| !query.is_empty() && p.display_title().to_lowercase().starts_with(&query);
        begins(b).cmp(&begins(a)).then(b.updated_at.cmp(&a.updated_at)).then(a.title.cmp(&b.title))
    });
    found.truncate(8);
    found
}

/// For an export of every page: a locked page that is open (its lines are
/// in plain in the window) goes out as one that is shut does, as the one
/// line that says it is locked. Its file held its lines in plain before.
pub fn shut_open_pages(blocks: &mut HashMap<Id, Vec<Block>>, open: &[Id]) {
    for id in open {
        if let Some(lines) = blocks.get_mut(id) {
            *lines = vec![Block::new(BlockKind::Locked, "")];
        }
    }
}

/// The pages that link to a page: with a link in their text, or with a line
/// that is a link to it. The page it is inside is left out when that page
/// only lists it as one of its pages (its place is shown in the bar already).
pub fn linked_from(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, id: &str) -> Vec<Page> {
    let parent = pages.get(id).and_then(|p| p.parent_id.clone());
    let mut out: Vec<Page> = pages
        .values()
        .filter(|p| !p.trashed && p.id != id)
        .filter(|p| {
            // (What a locked page holds is not told, also while it is open.)
            blocks.get(&p.id).filter(|bs| !crate::lock::hides(bs, &p.id)).is_some_and(|bs| {
                bs.iter().any(|b| match b.kind {
                    BlockKind::PageLink => b.target.as_deref() == Some(id) && parent.as_deref() != Some(p.id.as_str()),
                    BlockKind::Code => false,
                    _ => inline::links_to_page(&b.text, id),
                })
            })
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| a.display_title().to_lowercase().cmp(&b.display_title().to_lowercase()).then(a.created_at.cmp(&b.created_at)));
    out
}

/// The calendars of this Mac (see maccal.rs): whether they are shown, and
/// what was read of them last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MacCal {
    /// The person turned the option on.
    pub on: bool,
    pub access: crate::maccal::Access,
    pub calendars: Vec<crate::maccal::MacCalendar>,
    pub events: Vec<crate::maccal::MacEvent>,
    /// The calendars the person does not want shown (their ids).
    pub hidden: Vec<String>,
    /// The time the events were read for (milliseconds).
    pub from: i64,
    pub to: i64,
    /// The notes page of an event, by the event's id.
    pub pages: HashMap<String, Id>,
    /// A reading is under way.
    pub reading: bool,
    /// The person lets the app change the Mac's calendars (off until they
    /// turn it on): move an event, make a new one.
    pub write: bool,
    /// The calendar of the Mac that new events go to ("" for the app's own).
    pub new_in: String,
}

impl MacCal {
    const ON: &'static str = "mac_calendars";
    const HIDDEN: &'static str = "mac_calendars_hidden";
    const PAGES: &'static str = "mac_event_pages";
    const WRITE: &'static str = "mac_calendars_write";
    const NEW_IN: &'static str = "mac_calendars_new_in";

    fn load() -> MacCal {
        let list = |key: &str| store::setting(key).and_then(|s| serde_json::from_str(&s).ok());
        MacCal {
            on: store::setting(Self::ON).as_deref() == Some("1"),
            hidden: list(Self::HIDDEN).unwrap_or_default(),
            pages: store::setting(Self::PAGES).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            write: store::setting(Self::WRITE).as_deref() == Some("1"),
            new_in: store::setting(Self::NEW_IN).unwrap_or_default(),
            ..MacCal::default()
        }
    }

    /// The app may change the Mac's calendars now: the person said so, and macOS lets it read them.
    pub fn can_write(&self) -> bool {
        self.on && self.write && self.access == crate::maccal::Access::Allowed
    }

    /// The calendar of the Mac that a new event goes to, when there is one
    /// chosen that can be changed.
    fn new_events_calendar(&self) -> Option<String> {
        (self.can_write() && self.calendars.iter().any(|c| c.id == self.new_in && c.writable)).then(|| self.new_in.clone())
    }

    /// The colour an event of this calendar is drawn in.
    fn color_of(&self, calendar: &str) -> String {
        self.calendars.iter().find(|c| c.id == calendar).map(|c| c.color.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| "gray".to_string())
    }

    /// The name of a calendar ("Work"), with its account when it has one.
    pub fn name_of(&self, calendar: &str) -> String {
        match self.calendars.iter().find(|c| c.id == calendar) {
            Some(c) if !c.source.is_empty() && c.source != c.title => format!("{} · {}", c.title, c.source),
            Some(c) => c.title.clone(),
            None => "Calendar".to_string(),
        }
    }
}

/// Where the player is in the window, and how wide: so far from the two
/// edges of the window it is nearest to (so it stays by its corner when the
/// window changes size). The person moves it and gives it its size.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DockPlace {
    /// "left" or "right": the edge `x` is from.
    pub h: String,
    /// "top" or "bottom": the edge `y` is from.
    pub v: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
}

impl Default for DockPlace {
    fn default() -> Self {
        DockPlace { h: "right".into(), v: "bottom".into(), x: 18.0, y: 18.0, w: 440.0 }
    }
}

impl DockPlace {
    pub const LEAST: f64 = 260.0;
    pub const MOST: f64 = 1600.0;

    /// The same place with values the page can use, whatever was sent.
    pub fn tidy(self) -> DockPlace {
        let num = |n: f64, low: f64, high: f64| if n.is_finite() { n.clamp(low, high) } else { low };
        DockPlace {
            h: if self.h == "left" { "left".into() } else { "right".into() },
            v: if self.v == "top" { "top".into() } else { "bottom".into() },
            x: num(self.x, 0.0, 6000.0).round(),
            y: num(self.y, 0.0, 6000.0).round(),
            w: num(self.w, Self::LEAST, Self::MOST).round(),
        }
    }

    /// How high the player is at its width: its bar, its 16:9 frame and the room round the frame.
    pub fn height(&self) -> f64 {
        42.0 + (self.w - 14.0) * 9.0 / 16.0
    }

    /// Its place as styles. Every side is named each time (a side that is
    /// not named would keep its last value), and it never leaves the window.
    pub fn style(&self) -> String {
        let near = |from: f64, size: f64, view: &str| format!("max(8px, min({from:.0}px, calc(100{view} - {size:.0}px - 8px)))");
        let across = near(self.x, self.w, "vw");
        let down = near(self.y, self.height(), "vh");
        let (left, right) = if self.h == "left" { (across.as_str(), "auto") } else { ("auto", across.as_str()) };
        let (top, bottom) = if self.v == "top" { (down.as_str(), "auto") } else { ("auto", down.as_str()) };
        format!("left: {left}; right: {right}; top: {top}; bottom: {bottom}; width: min({:.0}px, calc(100vw - 16px));", self.w)
    }
}

/// What the AI did to a page: it changed the page and made these sub-pages.
#[derive(Clone, Debug, PartialEq)]
pub struct AiDone {
    pub page: Id,
    /// What was done, in a few words.
    pub text: String,
    /// The page was changed as one step of Undo, and these sub-pages were
    /// made with it.
    pub step: bool,
    pub made: Vec<Id>,
    /// What the AI changed with the tools of Rune (see mcp.rs).
    pub changes: Vec<crate::mcp::Change>,
    stamp: u64,
}

/// A video in the player at the corner of the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Playing {
    pub video: crate::video::Video,
    /// The page and the line the video was started from (its notes go
    /// under that line).
    pub from: Option<(Id, Id)>,
    /// Why it cannot play here, when it cannot.
    pub problem: Option<String>,
    /// YouTube's player came up in it.
    pub ready: bool,
}

thread_local! {
    /// Where the player's own words go (see `AppState::play_video`).
    static PLAYER_SAYS: std::cell::RefCell<Option<futures_channel::mpsc::UnboundedSender<Option<&'static str>>>> = const { std::cell::RefCell::new(None) };
}

/// A page in the graph of pages.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: Id,
    pub title: String,
    /// The colour of the page's icon ("" when it has none).
    pub color: String,
}

/// The pages of a space and what joins them. An edge is two places in
/// `nodes` (the smaller first) and whether it is only nesting: a page and a
/// page inside it, with no link in the text of one to the other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<(usize, usize, bool)>,
}

impl Graph {
    /// How many of the edges are links (not nesting only).
    pub fn links(&self) -> usize {
        self.edges.iter().filter(|e| !e.2).count()
    }

    /// How many of the edges are a page inside another (and no link).
    pub fn nested(&self) -> usize {
        self.edges.iter().filter(|e| e.2).count()
    }

    /// The names of the pages joined to the page at `i`.
    pub fn joined_to(&self, i: usize) -> Vec<&str> {
        self.edges
            .iter()
            .filter_map(|&(a, b, _)| if a == i { Some(b) } else if b == i { Some(a) } else { None })
            .map(|n| self.nodes[n].title.as_str())
            .collect()
    }

    /// For the page script that draws it: {"nodes":[{id,t,c}],"edges":[[a,b,nested]]}.
    pub fn to_json(&self) -> String {
        let nodes: Vec<serde_json::Value> =
            self.nodes.iter().map(|n| serde_json::json!({ "id": n.id, "t": n.title, "c": n.color })).collect();
        let edges: Vec<serde_json::Value> = self.edges.iter().map(|&(a, b, nested)| serde_json::json!([a, b, u8::from(nested)])).collect();
        serde_json::json!({ "nodes": nodes, "edges": edges }).to_string()
    }
}

/// The graph of the pages of one space: a link in the text of a page to
/// another page joins the two; so does a page inside another (nesting).
pub fn page_graph(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, space: &str) -> Graph {
    let mut shown: Vec<&Page> = pages.values().filter(|p| !p.trashed && p.space_id == space).collect();
    // An order that does not change as pages are edited, so a dot keeps its place.
    shown.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    let at: HashMap<&str, usize> = shown.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
    // Each pair once; a link wins over nesting.
    let mut pairs: std::collections::BTreeMap<(usize, usize), bool> = std::collections::BTreeMap::new();
    let mut join = |a: usize, b: usize, nested: bool| {
        if a != b {
            let pair = (a.min(b), a.max(b));
            let was = pairs.get(&pair).copied().unwrap_or(true);
            pairs.insert(pair, was && nested);
        }
    };
    for (i, p) in shown.iter().enumerate() {
        // The page of a table's row is an entry of that table, not a page
        // inside the page: it gets no line to it.
        if let Some(parent) = p.parent_id.as_deref().and_then(|id| at.get(id)).filter(|_| p.row_of.is_none()) {
            join(*parent, i, true);
        }
        // (What a locked page links to is not shown, also while it is open:
        // as in "Linked from".)
        for b in blocks.get(&p.id).filter(|bs| !crate::lock::hides(bs, &p.id)).map(|b| b.as_slice()).unwrap_or_default() {
            match b.kind {
                BlockKind::Code => {}
                // The line a sub-page has in its parent is the nesting, not a link.
                BlockKind::PageLink => {
                    if let Some(t) = b.target.as_deref().and_then(|t| at.get(t)) {
                        let nested = shown[*t].parent_id.as_deref() == Some(p.id.as_str());
                        join(i, *t, nested);
                    }
                }
                _ => {
                    for id in inline::page_links(&b.text) {
                        if let Some(t) = at.get(id.as_str()) {
                            join(i, *t, false);
                        }
                    }
                }
            }
        }
    }
    // A page's colour: that of its icon, else of the nearest page it is
    // inside that has one, else one for the page at the top of its tree (in
    // the order of the sidebar's pages). So a page and the pages in it are
    // one group of one colour.
    const GROUP_COLORS: [&str; 8] = ["blue", "purple", "green", "orange", "pink", "yellow", "red", "brown"];
    let icon_color = |p: &Page| p.icon.as_deref().and_then(|i| i.split_once(':')).map(|(_, c)| c.to_string()).filter(|c| !c.is_empty() && c != "gray");
    let mut tops: Vec<&Page> = shown.iter().copied().filter(|p| p.parent_id.as_deref().and_then(|id| at.get(id)).is_none()).collect();
    tops.sort_by(|a, b| a.position.total_cmp(&b.position).then(a.created_at.cmp(&b.created_at)));
    let color_of = |p: &Page| -> String {
        let mut here = p;
        for _ in 0..64 {
            if let Some(c) = icon_color(here) {
                return c;
            }
            match here.parent_id.as_deref().and_then(|id| at.get(id)) {
                Some(&up) => here = shown[up],
                None => break,
            }
        }
        let top = tops.iter().position(|t| t.id == here.id).unwrap_or(0);
        GROUP_COLORS[top % GROUP_COLORS.len()].to_string()
    };
    // The pages of table rows show only when a link joins them to a page.
    let linked: std::collections::HashSet<usize> = pairs.keys().flat_map(|&(a, b)| [a, b]).collect();
    let kept: Vec<usize> = (0..shown.len()).filter(|&i| shown[i].row_of.is_none() || linked.contains(&i)).collect();
    let new_at: HashMap<usize, usize> = kept.iter().enumerate().map(|(n, &i)| (i, n)).collect();
    let nodes = kept
        .iter()
        .map(|&i| GraphNode {
            id: shown[i].id.clone(),
            title: shown[i].display_title().to_string(),
            color: color_of(shown[i]),
        })
        .collect();
    let edges = pairs.into_iter().filter_map(|((a, b), nested)| Some((*new_at.get(&a)?, *new_at.get(&b)?, nested))).collect();
    Graph { nodes, edges }
}

/// The page and every page nested under it.
pub fn descendants(pages: &HashMap<Id, Page>, id: &str) -> Vec<Id> {
    let mut out = vec![id.to_string()];
    // Each page once: a loop of parents (two programs moving pages at the
    // same moment could make one) must not go round for ever.
    let mut seen: std::collections::HashSet<Id> = out.iter().cloned().collect();
    let mut i = 0;
    while i < out.len() {
        let parent = out[i].clone();
        for p in pages.values().filter(|p| p.parent_id.as_deref() == Some(&parent)) {
            if seen.insert(p.id.clone()) {
                out.push(p.id.clone());
            }
        }
        i += 1;
    }
    out
}

/// Ancestors from the top level down to (not including) the page.
pub fn ancestors(pages: &HashMap<Id, Page>, id: &str) -> Vec<Page> {
    let mut out = Vec::new();
    let mut cur = pages.get(id).and_then(|p| p.parent_id.clone());
    while let Some(pid) = cur {
        match pages.get(&pid) {
            Some(p) if out.len() < 32 => {
                cur = p.parent_id.clone();
                out.push(p.clone());
            }
            _ => break,
        }
    }
    out.reverse();
    out
}

thread_local! {
    /// An update is being put in place (see `install_update`).
    static UPDATING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
thread_local! {
    /// The blocks last copied or cut as a whole, with the words that went
    /// to the clipboard for them, whether they were cut, and the pages of
    /// the rows of cut tables that the cut put in the Trash (see
    /// `copy_picked` and `paste`).
    static COPIED_BLOCKS: std::cell::RefCell<Option<(String, Vec<Block>, bool, Vec<Id>)>> = const { std::cell::RefCell::new(None) };
}
thread_local! {
    /// The picture put last: (page, block), for several dropped together.
    static LAST_IMAGE: std::cell::RefCell<Option<(Id, Id)>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// The app's own listener, while it types into a text box (the
    /// microphones beside the AI box, a quick note, an event's notes).
    static BOX_EARS: std::cell::RefCell<Option<crate::listen::Listener>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// The listener that is running, if the app is listening to a meeting.
    static LISTENER: std::cell::RefCell<Option<crate::listen::Listener>> = const { std::cell::RefCell::new(None) };
    /// The window that the listener (`LISTENER`) writes for, and the page
    /// it writes in (see `window_gone`).
    static LISTENER_WINDOW: std::cell::RefCell<Option<(u64, Id)>> = const { std::cell::RefCell::new(None) };
    /// The pages with words of a meeting that no notes were written from
    /// (the app was closed while it listened, or the notes failed), each
    /// with the line of its transcript where those words begin: the next
    /// notes of such a page are written from them too. With no line (the
    /// app was closed, or the window went): the part that was heard last.
    /// (Kept by page alone, a part that had its notes was noted again.)
    static HEARD_NOT_NOTED: std::cell::RefCell<Vec<(Id, Option<usize>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Write down that a page has words with no notes, from this line of its
/// transcript on (see `HEARD_NOT_NOTED`). What is known already stays.
fn heard_not_noted(page: &str, from: Option<usize>) {
    HEARD_NOT_NOTED.with(|h| {
        let mut h = h.borrow_mut();
        if !h.iter().any(|(p, _)| p == page) {
            h.push((page.to_string(), from));
        }
    });
}

/// Whether the app listens, in any window (the listener is one for the app;
/// each window knows only what it started itself).
fn listener_runs() -> bool {
    LISTENER.with(|l| l.borrow().is_some())
}

/// The window the app listens for and the page it listens in (a meeting,
/// the notes of a video), if it listens. One for the app: a window's own
/// `listening` says only what that window started.
pub fn listening_in() -> Option<(u64, Id)> {
    LISTENER_WINDOW.with(|w| w.borrow().clone())
}

#[cfg(test)]
pub fn listen_for_test(at: Option<(u64, Id)>) {
    LISTENER_WINDOW.with(|w| *w.borrow_mut() = at);
}

/// What a window makes of the list of spaces that another window sends: the
/// list to show, and the spaces that are gone. The sender's list can be
/// older than the notes file (a space came later from another device, and
/// only a window opened after that knows it). A space counts as gone only
/// when the notes file does not have it either, and a space that the file
/// has stays in the list. Told by the sender's list alone, the pages of
/// such a space were put in the first space, and the next save wrote that.
/// (`in_file`: none when the file could not be read; nothing is gone then.)
fn spaces_told(mine: &[Space], told: Vec<Space>, in_file: Option<Vec<Space>>) -> (Vec<Space>, Vec<Id>) {
    let has = |list: &[Space], id: &str| list.iter().any(|s| s.id == id);
    let kept: Vec<Space> = in_file.as_deref().unwrap_or(mine).iter().filter(|s| !has(&told, &s.id)).cloned().collect();
    let mut list = told;
    if !kept.is_empty() {
        list.extend(kept);
        list.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    let gone = mine.iter().filter(|s| !has(&list, &s.id)).map(|s| s.id.clone()).collect();
    (list, gone)
}

/// A window goes. When the listener wrote for that window, it ends now: the
/// tasks that read what it hears went with the window, and the microphone
/// stayed on, unread, until the app quit. The words heard since the last
/// write to the page (20 seconds at the most) and the notes are not
/// written; the next notes of that page are written from all its transcript.
/// (The order of use of its tabs goes too: see `walk_tabs`.)
pub fn window_gone(key: u64) {
    TAB_USE.with(|u| u.borrow_mut().remove(&key));
    let Some((_, page)) = LISTENER_WINDOW.with(|w| w.borrow_mut().take_if(|(of, _)| *of == key)) else { return };
    LISTENER.with(|l| l.borrow_mut().take());
    heard_not_noted(&page, None);
}

thread_local! {
    /// The program that is reading a page aloud, if one is.
    static READER: std::cell::RefCell<Option<std::process::Child>> = const { std::cell::RefCell::new(None) };
}

/// The meeting whose notes page was made last (see meeting.rs).
struct MeetingNotes {
    page: Id,
    /// The block that holds its date line.
    stamp: Id,
    started: i64,
    ended: Option<i64>,
    /// The program it was noticed in ("Microsoft Teams"), if it was.
    app: Option<&'static str>,
    /// The meeting watcher saw it start (so it will also see it end).
    noticed: bool,
    /// Its time on the calendar, when an event was on as it started.
    span: Option<(i64, i64)>,
    /// That event (with `span`, it says which meeting this is).
    event: Option<Id>,
    /// Its pad showed its last card (so the pad comes back if the meeting
    /// goes on after all).
    padded: bool,
    /// The end stopped the listening (so it is offered again if the meeting
    /// goes on after all).
    cut: bool,
    /// The app made the page by itself when the call started, and the
    /// person has not said yes yet (Listen, Notes only, or a word typed).
    /// A draft that is still empty goes when the person says no, or when
    /// the meeting ends: no page is kept for a call they did not want notes of.
    draft: bool,
    /// Another program that this meeting is in (it passed there from a
    /// browser, or its notes were made by hand and the call was noticed
    /// then). The notes end when none of their programs is in the call
    /// (see `meeting_left`). Not written down between runs.
    also: Option<&'static str>,
}

/// The folder for the recorder of the sidebar swipe, and its two files: the
/// marker that turns it on, and what it wrote.
fn swipe_trace_folder() -> std::path::PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Rune")
}

/// Is the recorder of the sidebar swipe on? (When the file "swipe-log-on"
/// is in the app's cache folder. It is for one fault that only the person's
/// own trackpad shows; it keeps numbers only, and only on this Mac.)
pub fn swipe_trace_on() -> bool {
    swipe_trace_folder().join("swipe-log-on").exists()
}

/// Keep the steps of one swipe (see `Bridge::SwipeTrace`). The file starts
/// again when it is large.
fn swipe_trace(lines: &str) {
    use std::io::Write;
    if !swipe_trace_on() {
        return;
    }
    let path = swipe_trace_folder().join("swipes.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 400_000) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "--- {}\n{lines}", now_ms());
    }
}

/// A call that is noticed again this soon after it went quiet is the same
/// meeting, with the same page; and so is one that starts soon after its
/// notes were made by hand.
const SAME_MEETING_MS: i64 = 20 * when::MINUTE;
const JUST_MADE_MS: i64 = 15 * when::MINUTE;
/// An event of the calendar that starts this close to now is the meeting
/// that starts now.
const NEXT_MEETING_MS: i64 = 10 * when::MINUTE;

/// Whether a call noticed at `now` is the meeting whose notes were made
/// last: one meeting has one page. `of`: the event those notes are of (its
/// id, start and end), if they are of one. `quiet_since`: when that meeting
/// was last seen. `on_now`: the event that is on now, or about to start.
fn same_meeting(of: Option<&(Id, i64, i64)>, quiet_since: i64, now: i64, on_now: Option<&(Id, i64, i64)>) -> bool {
    // Its own event is still on: the call went quiet, or the person left
    // and came back.
    if of.is_some_and(|(_, _, end)| now < *end) {
        return true;
    }
    // Another event starts about now: this is the next meeting.
    if let Some(next) = on_now {
        let own = of.is_some_and(|e| e.0 == next.0 && e.1 == next.1);
        if !own && (now - next.1).abs() <= NEXT_MEETING_MS {
            return false;
        }
    }
    now - quiet_since < SAME_MEETING_MS
}

/// Whether a call noticed at `now` in the program `app` is the call the
/// person said no to (`declined`: its program, and when it was last seen).
/// The same rule as for one page for one meeting: a call that went quiet and
/// is back within the time of `same_meeting` is the same call, unless an
/// event of the calendar starts about now. (The no was forgotten at each
/// pause of a long call and at each start of the app: the question, and a
/// new page, came again and again.)
fn still_declined(declined: Option<&Declined>, app: &str, now: i64, on_now: Option<&(Id, i64, i64)>) -> bool {
    declined.is_some_and(|d| {
        // A refused call that was seen a moment ago only paused: it is not
        // "the next meeting" when an event of the calendar starts about
        // now. (Each pause of a voice channel that is on all day, near any
        // event, asked again under the event's name.) A call that ended
        // some minutes ago is over: a meeting of the calendar that starts
        // in the same program then is the next one, and asks.
        // A call that went on long after the no (that voice channel) can
        // be silent for some minutes and still be the same call. (Silent
        // for three minutes before an event, it asked again.)
        let went_on = d.seen - d.said >= SAME_MEETING_MS;
        let paused = d.app.as_deref() == Some(app) && now - d.seen < if went_on { LONG_PAUSE_MS } else { PAUSE_MS };
        // The event of the no (the call was refused under its name) keeps
        // the no only for the call that was refused: one that was still
        // seen when the event had started, or that only paused. (A call
        // refused ten minutes before an event, and over before it, kept the
        // meeting of that event itself from asking, for the whole event.)
        let of = d.of.as_ref().filter(|(_, start, _)| d.seen >= *start || paused);
        d.app.as_deref().map_or(true, |of| of == app)
            // (One no does not close a program for the day: calls that follow
            // each other closely each kept it alive, with no end.)
            && now - d.said < DECLINED_MOST_MS
            // Its own event is not "the next meeting": a meeting of the
            // calendar that was refused stays refused while its event is on.
            && same_meeting(of, d.seen, now, on_now.filter(|_| !paused))
    })
}

/// A no lasts this long at the most, however long its call goes on.
const DECLINED_MOST_MS: i64 = 4 * when::HOUR;
/// A refused call that comes back this soon after it was last seen only
/// paused (a bad line, a quiet half minute), whatever the calendar says.
const PAUSE_MS: i64 = 2 * when::MINUTE;
/// The same, for a call that went on for twenty minutes or more after the no.
const LONG_PAUSE_MS: i64 = 5 * when::MINUTE;

/// The no to the notes of a call (see `MEETING_DECLINED`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Declined {
    /// The program of the call (none: notes made by hand).
    app: Option<String>,
    /// When that call was last seen.
    seen: i64,
    /// When the no was said.
    said: i64,
    /// The event of the calendar the notes were of (its id, start and end).
    of: Option<(Id, i64, i64)>,
}

/// The nos as the setting `meeting_declined` holds them: a list, one for
/// each program. (It held one no: a meeting in a second program took the no
/// to the first call away. That form is still read.)
fn read_declined(text: &str) -> Vec<Declined> {
    serde_json::from_str::<Vec<Declined>>(text).or_else(|_| serde_json::from_str::<Declined>(text).map(|d| vec![d])).unwrap_or_default()
}

/// The no to a call in this program, if there is one (a no to notes made
/// by hand is for any program).
fn no_for<'a>(nos: &'a [Declined], app: &str) -> Option<&'a Declined> {
    nos.iter().find(|d| d.app.as_deref().map_or(true, |of| of == app))
}

/// The nos with this one: it takes the place of an earlier no to its
/// program, and the nos to other programs stay (one for each program, so
/// the list stays short).
fn with_no(mut nos: Vec<Declined>, no: Declined) -> Vec<Declined> {
    nos.retain(|d| d.app != no.app);
    nos.push(no);
    nos
}

/// The nos without the one to this program (and without one to notes made
/// by hand): a new call in it asks again.
fn without_no(mut nos: Vec<Declined>, app: &str) -> Vec<Declined> {
    nos.retain(|d| d.app.as_deref().is_some_and(|of| of != app));
    nos
}

/// Whether a call in this program was refused and seen a short time ago:
/// that call can still be on, whether or not its no still holds.
fn refused_lately(nos: &[Declined], app: &str, now: i64) -> bool {
    nos.iter().any(|d| d.app.as_deref() == Some(app) && now - d.seen < SAME_MEETING_MS)
}

/// Whether a call that starts in `app` must leave the notes of the last
/// meeting alone: they are of a meeting in another call program
/// (`notes_app`), and the call in `app` is one the person refused a short
/// time ago. (A refused voice call that came back after a pause took over
/// the page of a meeting that had ended while its event was still on, and
/// that meeting's program could not end those notes any more.)
fn not_its_notes(nos: &[Declined], notes_app: Option<&str>, app: &str, now: i64) -> bool {
    let browser = |a: &str| crate::meeting::call_app_named(a).is_some_and(|c| c.browser);
    notes_app.is_some_and(|of| of != app && !browser(of) && !browser(app)) && refused_lately(nos, app, now)
}

/// The program that the notes of a meeting are bound to (besides the one
/// they were made in) when a call in `app` carries them on: `app`, unless
/// they are bound to another call program already. A browser gives way: its
/// call passes to the call program. (Notes made from an event and bound to
/// the meeting's program were bound anew to a voice call that took the
/// microphone again: the meeting's end did not end them, and the app
/// listened on into the voice call.)
fn bound_to(also: Option<&'static str>, app: &'static str) -> &'static str {
    let browser = |a: &str| crate::meeting::call_app_named(a).is_some_and(|c| c.browser);
    also.filter(|of| !browser(of)).unwrap_or(app)
}

/// Whether the notes that are on are those of a call in another program
/// than `app`: made for one (`notes_app`), or bound to one (`also`).
fn notes_of_another(notes_app: Option<&str>, also: Option<&str>, app: &str) -> bool {
    notes_app.is_some_and(|of| of != app) || also.is_some_and(|of| of != app)
}

fn declined() -> Vec<Declined> {
    MEETING_DECLINED.with(|d| d.borrow().clone())
}

/// Write the nos down, for this run and the next.
fn keep_declined(nos: Vec<Declined>) {
    store::set_setting("meeting_declined", &if nos.is_empty() { String::new() } else { serde_json::to_string(&nos).unwrap_or_default() });
    MEETING_DECLINED.with(|d| *d.borrow_mut() = nos);
}

/// Write down the no to a call.
fn set_declined(no: Declined) {
    keep_declined(with_no(declined(), no));
}

/// The meeting of the last notes, as it is written down between runs of the
/// app. A call can go on while the app is closed and opened again (an update
/// does that): the same meeting is then known again, and gets no second page.
#[derive(Serialize, Deserialize)]
struct KeptMeeting {
    page: Id,
    stamp: Id,
    started: i64,
    ended: Option<i64>,
    app: Option<String>,
    noticed: bool,
    span: Option<(i64, i64)>,
    event: Option<Id>,
    /// Its pad was on the screen, and the app listened to it, when this was written.
    padded: bool,
    listened: bool,
    /// When this was written.
    seen: i64,
    #[serde(default)]
    draft: bool,
}

thread_local! {
    /// The event copied with ⌘C in the calendar (see `paste_event`).
    static COPIED_EVENT: std::cell::RefCell<Option<Event>> = const { std::cell::RefCell::new(None) };
    static MEETING: std::cell::RefCell<Option<MeetingNotes>> = const { std::cell::RefCell::new(None) };
    /// The "Meetings" heading that was put in a page of the person's own
    /// for the line of a draft (the draft's page, the heading's block): it
    /// goes with a draft that is refused.
    static DRAFT_HEADING: std::cell::RefCell<Option<(Id, Id)>> = const { std::cell::RefCell::new(None) };
    /// The last press on empty time in the calendar: when, and the minute of
    /// the day under it, as the page script measured it.
    static CAL_PRESS: std::cell::Cell<Option<(i64, i64)>> = const { std::cell::Cell::new(None) };
    /// (For the self-test: a meeting that starts takes no event of the
    /// calendar, whatever is on it at the hour of the run.)
    static MEETING_ALONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The loop that lets the pad's questions go away by themselves is running.
    static PAD_WATCH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// When the meeting was written down last, as one that goes on (`meeting_seen`).
    static MEETING_KEPT: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    /// The person said no to notes of a call: the program it was in (none:
    /// notes made by hand), and when that call was last seen. It asks no more
    /// while that call is on, and not when it goes quiet for a while and
    /// comes back, or when the app is closed and opened during it (see
    /// `still_declined`). One no for each program: a meeting in a second
    /// program does not end the no to the first call. Written down as the
    /// setting `meeting_declined`.
    static MEETING_DECLINED: std::cell::RefCell<Vec<Declined>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The pages made as meeting notes (the newest few hundred), wherever they
/// are kept: a page stays a meeting's page when notes move to another place.
fn meeting_page_ids() -> Vec<Id> {
    store::setting("meeting_pages").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn remember_meeting_page(id: &str) {
    let mut ids = meeting_page_ids();
    ids.retain(|p| p != id);
    ids.push(id.to_string());
    let from = ids.len().saturating_sub(400);
    store::set_setting("meeting_pages", &serde_json::to_string(&ids[from..]).unwrap_or_default());
}

/// The name of the meeting that each notes page is of (the newest few
/// hundred): the name of the calendar event that was on.
fn meeting_names() -> Vec<(Id, String)> {
    store::setting("meeting_names").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn remember_meeting_name(page: &str, name: &str) {
    let mut names = meeting_names();
    names.retain(|(p, _)| p != page);
    names.push((page.to_string(), name.trim().to_string()));
    let from = names.len().saturating_sub(400);
    store::set_setting("meeting_names", &serde_json::to_string(&names[from..]).unwrap_or_default());
}

/// The setting with the notes page of each repeat of a repeating event (see
/// `repeat::page_of`). The event's own record has one page for all its
/// repeats, and is not changed for this: a Rune from before this setting
/// does not read it, and makes its pages as it always did.
const REPEAT_PAGES: &str = "repeat_event_pages";

fn repeat_pages_kept() -> HashMap<String, Id> {
    store::setting(REPEAT_PAGES).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Change the kept pages of repeats: the list is read again, changed and
/// written, and given back as it is kept now. (Each window has a copy of
/// the list to draw from. A window that wrote its copy took away what
/// another window had put in the list since that copy was read: the page
/// of a repeat was a page of no repeat after a drag in the other window.)
fn change_repeat_pages_kept(change: impl FnOnce(&mut HashMap<String, Id>)) -> HashMap<String, Id> {
    let was = repeat_pages_kept();
    let mut now = was.clone();
    change(&mut now);
    if now != was {
        store::set_setting(REPEAT_PAGES, &serde_json::to_string(&now).unwrap_or_default());
    }
    now
}

#[cfg(test)]
mod repeat_pages_tests {
    use super::*;

    #[test]
    fn a_change_of_the_pages_of_repeats_starts_from_the_list_as_it_is_kept() {
        store::install(store::Store::open_in_memory().unwrap());
        // Window 2 opens: its copy of the list has nothing.
        let copy_of_window_2 = repeat_pages_kept();
        assert!(copy_of_window_2.is_empty());
        // In window 1 a page is made for a repeat of one event.
        change_repeat_pages_kept(|own| {
            own.insert("review@2026-11-10".into(), "page-1".into());
        });
        // In window 2 the repeats of another event move: its page goes to another day.
        // (Window 2 wrote its own copy, changed: the entry of window 1 was gone.)
        change_repeat_pages_kept(|own| {
            own.insert("standup@2026-11-12".into(), "page-2".into());
        });
        let now = change_repeat_pages_kept(|own| {
            if let Some(page) = own.remove("standup@2026-11-12") {
                own.insert("standup@2026-11-13".into(), page);
            }
        });
        assert_eq!(now.get("review@2026-11-10").map(String::as_str), Some("page-1"), "{now:?}");
        assert_eq!(now.get("standup@2026-11-13").map(String::as_str), Some("page-2"));
        assert_eq!(repeat_pages_kept(), now);
    }
}

/// The page that the notes of the meetings of each name are kept in, as the
/// person chose it (the name in small letters).
fn meeting_homes() -> HashMap<String, Id> {
    store::setting("meeting_homes").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// The meetings that come again at one time of the day (slots.rs).
fn meeting_slots() -> Vec<crate::slots::Slot> {
    store::setting("meeting_slots").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_meeting_slots(slots: &[crate::slots::Slot]) {
    store::set_setting("meeting_slots", &serde_json::to_string(slots).unwrap_or_default());
}

/// The notes pages of meetings with no calendar event (the newest few
/// hundred): the day (0 Monday) and the minute each started at, and the
/// name the app gave the page.
fn slot_pages() -> Vec<(Id, u32, i64, String)> {
    store::setting("meeting_slot_pages").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn remember_slot_page(page: &str, day: u32, minute: i64, title: &str) {
    let mut pages = slot_pages();
    pages.retain(|(p, ..)| p != page);
    pages.push((page.to_string(), day, minute, title.to_string()));
    let from = pages.len().saturating_sub(300);
    store::set_setting("meeting_slot_pages", &serde_json::to_string(&pages[from..]).unwrap_or_default());
}

/// The day of the week (0 Monday) and the minute of the day of a moment.
fn day_and_minute(ms: i64) -> (u32, i64) {
    let at = when::local(ms);
    (chrono::Datelike::weekday(&at).num_days_from_monday(), i64::from(chrono::Timelike::hour(&at)) * 60 + i64::from(chrono::Timelike::minute(&at)))
}

/// The name of a meeting from the name of its notes page ("Apollo standup –
/// Nov 10, 2026" gives "Apollo standup").
fn name_before_date(title: &str) -> &str {
    title.rsplit_once(" – ").map(|(name, _)| name).unwrap_or(title).trim()
}

fn set_meeting_home(name: &str, home: Option<&str>) {
    let mut homes = meeting_homes();
    let key = name.trim().to_lowercase();
    match home {
        Some(home) => homes.insert(key, home.to_string()),
        None => homes.remove(&key),
    };
    store::set_setting("meeting_homes", &serde_json::to_string(&homes).unwrap_or_default());
}

/// The date line of a meeting notes page: "Monday, November 9, 2026 · 2:03 PM",
/// then "… 2:03 PM – 2:45 PM (42 min)" once it has ended.
pub fn meeting_stamp(started: i64, ended: Option<i64>, app: Option<&str>) -> String {
    let mut s = format!("{} · {}", when::date_of(started).format("%A, %B %-d, %Y"), when::time_label(started));
    if let Some(end) = ended {
        let minutes = ((end - started) as f64 / when::MINUTE as f64).round().max(1.0) as i64;
        s.push_str(&format!(" – {} ({minutes} min)", when::time_label(end)));
    }
    if let Some(app) = app {
        s.push_str(&format!(" · {app}"));
    }
    s
}

/// While a page is being edited, a copy of it is kept for Page history at
/// most this often.
const VERSION_EVERY_MS: i64 = 10 * 60 * 1000;

thread_local! {
    /// When each page's newest kept version was made (0 = none), so typing
    /// doesn't ask the database on every key.
    static LAST_VERSION: std::cell::RefCell<HashMap<Id, i64>> = std::cell::RefCell::new(HashMap::new());
}

impl AppState {
    /// State for a window. The primary window restores its tabs; others open `route`.
    pub fn load(primary: bool, route: Option<Route>) -> AppState {
        // (Another program can hold the file for a moment: one more try.)
        let read = store::with(|s| s.load()).or_else(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
            store::with(|s| s.load())
        });
        // The notes file is open, but the notes could not be read. Nothing is
        // made in their place: the starter pages were written into the file
        // then, as if it were new, over notes that were only not read.
        let unread = read.is_none() && store::is_open();
        if unread {
            store::tell("Your notes could not be read. Nothing was changed. Quit Rune and open it again; if this stays, the Backups folder has yesterday's notes.");
        }
        let (mut pages, mut blocks) = read.unwrap_or_default();
        // Pages of rows whose table or row went while the app was last open.
        if primary {
            let (keep, gone) = tables::lost_row_pages(&pages, &blocks);
            for id in &keep {
                if let Some(p) = pages.get_mut(id) {
                    p.row_of = None;
                    let p = p.clone();
                    store::with(|s| s.save_page(&p));
                }
            }
            for id in &gone {
                pages.remove(id);
                blocks.remove(id);
            }
            if !gone.is_empty() {
                store::with(|s| s.delete_pages(&gone));
            }
        }
        let mut spaces = store::with(|s| s.load_spaces()).unwrap_or_default();
        if spaces.is_empty() {
            spaces.push(Space::new("My Space", "blue", 1.0));
        }
        let space = store::setting("space").filter(|id| spaces.iter().any(|s| &s.id == id)).unwrap_or_else(|| spaces[0].id.clone());
        let space = match &route {
            Some(Route::Page(id)) => pages.get(id).map(|p| p.space_id.clone()).unwrap_or(space),
            _ => space,
        };
        if pages.is_empty() && !unread {
            seed(&mut pages, &mut blocks, &space);
        }
        // (A few builds saved tab groups as a third item; it's ignored.)
        let saved_tabs: Option<(Vec<Tab>, usize)> = store::setting("tabs").and_then(|s| {
            serde_json::from_str::<(Vec<Tab>, usize)>(&s)
                .or_else(|_| serde_json::from_str::<(Vec<Tab>, usize, serde_json::Value)>(&s).map(|t| (t.0, t.1)))
                .ok()
        });
        let saved_tabs = if primary && route.is_none() { saved_tabs } else { None };
        let saved_sets: Option<HashMap<Id, TabSet>> = store::setting("space_tabs").and_then(|s| serde_json::from_str(&s).ok());
        let mut parked: HashMap<Id, TabSet> = HashMap::new();
        let (tabs, active) = match saved_tabs {
            _ if route.is_some() => (vec![Tab::new(route.unwrap_or(Route::Home))], 0),
            Some((mut tabs, active)) if !tabs.is_empty() => {
                forget_gone(&mut tabs, &pages);
                match saved_sets {
                    Some(sets) => {
                        parked = sets;
                        let active = active.min(tabs.len() - 1);
                        (tabs, active)
                    }
                    None => {
                        let (here, active, others) = sort_tabs_by_space(tabs, active, &space, &pages);
                        parked = others;
                        if here.is_empty() { (vec![Tab::new(Route::Home)], 0) } else { (here, active) }
                    }
                }
            }
            _ => {
                let first = roots_in(&pages, &space).first().map(|p| Route::Page(p.id.clone())).unwrap_or(Route::Home);
                (vec![Tab::new(first)], 0)
            }
        };
        parked.retain(|id, _| *id != space && spaces.iter().any(|s| &s.id == id));
        for set in parked.values_mut() {
            forget_gone(&mut set.0, &pages);
        }
        let expanded: HashSet<Id> = store::setting("expanded")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let ui = Ui {
            tabs,
            active,
            closed: Vec::new(),
            parked,
            sidebar_open: store::setting("sidebar_open").map(|v| v != "0").unwrap_or(true),
            sidebar_width: store::setting("sidebar_width")
                .and_then(|w| w.parse::<f64>().ok())
                .map(|w| w.clamp(SIDEBAR_MIN, SIDEBAR_MAX))
                .unwrap_or(SIDEBAR_DEFAULT),
            expanded,
            popover: None,
            modal: None,
            zoom: store::setting("zoom").and_then(|z| z.parse().ok()).unwrap_or(1.0),
            theme: Theme::parse(&store::setting("theme").unwrap_or_default()),
            recent: store::setting("recent").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            space,
            history: None,
            voice: VoiceFor::NewPage,
            focus_mode: false,
            // (A Rune from before folded headings has no such setting. Pages
            // that are gone take their entries along; with no page read at
            // all, nothing is dropped.)
            folds: store::setting("folds")
                .and_then(|s| serde_json::from_str::<HashMap<Id, HashSet<Id>>>(&s).ok())
                .map(|mut folds| {
                    folds.retain(|page, ids| !ids.is_empty() && (pages.is_empty() || pages.contains_key(page)));
                    folds
                })
                .unwrap_or_default(),
        };
        AppState {
            pages: Signal::new(pages),
            blocks: Signal::new(blocks),
            ui: Signal::new(ui),
            ai: Signal::new(None),
            history: Signal::new(History::default()),
            win: Win { key: sync::join(), primary },
            slash: Signal::new(None),
            focus: Signal::new(None),
            toast_msg: Signal::new(None),
            look: Signal::new(Look::load()),
            reveal: Signal::new(None),
            spaces: Signal::new(spaces),
            events: Signal::new(
                store::with(|s| s.load_events()).unwrap_or_default().into_iter().map(|e| (e.id.clone(), e)).collect(),
            ),
            cal: Signal::new(Cal {
                view: CalView::parse(store::setting("cal_view").as_deref()),
                anchor: when::today(),
                selected: None,
                details: false,
                all_spaces: store::setting("cal_spaces").as_deref() != Some("here"),
            }),
            update: Signal::new(None),
            reading: Signal::new(false),
            listening: Signal::new(None),
            listen_offer: Signal::new(None),
            picked: Signal::new(None),
            edited: Signal::new(HashMap::new()),
            player: Signal::new(None),
            mac_cal: Signal::new(MacCal::load()),
            repeat_pages: Signal::new(repeat_pages_kept()),
            place_asks: Signal::new(store::setting("place_asks").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()),
            place_now: Signal::new(None),
            cal_draft: Signal::new(None),
            space_slide: Signal::new(0),
            settings_first: Signal::new(None),
            ai_done: Signal::new(None),
            ai_on: Signal::new(ai::shown()),
            ask_pictures: Signal::new(Vec::new()),
            ask_given: Signal::new(Vec::new()),
            player_dock: Signal::new(store::setting("player_dock").and_then(|s| serde_json::from_str::<DockPlace>(&s).ok()).unwrap_or_default().tidy()),
            day_page: Signal::new(None),
            pad: Signal::new(None),
            tab_walk: Signal::new(None),
        }
    }

    // ----- reading -------------------------------------------------------

    /// Route of the active tab (subscribes the caller to UI changes).
    pub fn route(&self) -> Route {
        self.ui.read().tab().route().clone()
    }

    /// The page in the focused pane.
    pub fn current_page(&self) -> Option<Id> {
        let ui = self.ui.peek();
        let route = ui.tab().route().clone();
        drop(ui);
        match &route {
            Route::Page(id) => Some(id.clone()),
            // In the calendar's day view, the notes of the event beside the day.
            Route::Calendar => self.day_page.peek().clone().filter(|_| self.cal.peek().view == CalView::Day),
            Route::Home | Route::Graph => None,
        }
    }

    /// The calendar's day view shows this notes page beside the day (or none).
    pub fn set_day_page(mut self, page: Option<Id>) {
        if *self.day_page.peek() != page {
            self.day_page.set(page);
        }
    }

    /// The day view no longer shows this page.
    pub fn clear_day_page(mut self, page: &str) {
        if self.day_page.peek().as_deref() == Some(page) {
            self.day_page.set(None);
        }
    }

    /// The pane of this tab that a page shows in: its own, or the calendar's
    /// when it is the notes beside the day there.
    fn pane_of_page(&self, page_id: &str) -> Option<Id> {
        let ui = self.ui.peek();
        let tab = ui.tab();
        tab.pane_showing(&Route::Page(page_id.to_string()))
            .or_else(|| (self.day_page.peek().as_deref() == Some(page_id)).then(|| tab.pane_showing(&Route::Calendar)).flatten())
    }

    pub fn page_title(&self, id: &str) -> String {
        self.pages.peek().get(id).map(|p| p.display_title().to_string()).unwrap_or_else(|| "Deleted page".into())
    }

    pub fn page_markdown(&self, page_id: &str) -> String {
        let title = self.pages.peek().get(page_id).map(|p| p.title.clone()).unwrap_or_default();
        let blocks = self.blocks.peek().get(page_id).cloned().unwrap_or_default();
        markdown::from_blocks(&title, &blocks, |id| self.page_title(id))
    }

    // ----- persistence ---------------------------------------------------

    fn save_page(&self, id: &str) {
        if let Some(p) = self.pages.peek().get(id).cloned() {
            store::with(|s| s.save_page(&p));
            sync::others(self.win.key, SyncMsg::Page(p));
        }
    }

    /// Note that a page was written to now (for the line under its title).
    fn note_edit(&self, page_id: &str) {
        // Nothing draws from this by itself (it is read with `peek`), so a
        // write redraws nothing.
        let mut edited = self.edited;
        edited.write().insert(page_id.to_string(), now_ms());
    }

    /// When a page was last changed: in this window, or as its record says.
    pub fn edited_at(&self, page: &Page) -> i64 {
        self.edited.peek().get(&page.id).copied().unwrap_or(0).max(page.updated_at)
    }

    fn save_blocks(&self, page_id: &str) {
        // Each line has an id of its own in the whole notes file: a line of
        // this page whose id is also a line's of this page or of another
        // page (a copy that kept its id) gets a new one, or the save fails.
        let clashes = {
            let blocks = self.blocks.peek();
            let mine = blocks.get(page_id).map(|bs| bs.as_slice()).unwrap_or_default();
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            let elsewhere: std::collections::HashSet<&str> =
                blocks.iter().filter(|(p, _)| p.as_str() != page_id).flat_map(|(_, bs)| bs.iter().map(|b| b.id.as_str())).collect();
            mine.iter().enumerate().filter(|(_, b)| !seen.insert(b.id.as_str()) || elsewhere.contains(b.id.as_str())).map(|(i, _)| i).collect::<Vec<usize>>()
        };
        if !clashes.is_empty() {
            let mut sig = self.blocks;
            let mut all = sig.write();
            if let Some(bs) = all.get_mut(page_id) {
                for i in clashes {
                    if let Some(b) = bs.get_mut(i) {
                        eprintln!("[rune] line {} of page {page_id} had the id of another line: a new id", b.id);
                        b.id = new_id();
                    }
                }
            }
            drop(all);
        }
        let Some(bs) = self.blocks.peek().get(page_id).cloned() else { return };
        self.note_edit(page_id);
        let now = now_ms();
        store::with(|s| s.save_blocks_at(page_id, &bs, now));
        if let Some(p) = self.pages.peek().get(page_id) {
            let mut p = p.clone();
            p.updated_at = now;
            sync::others(self.win.key, SyncMsg::Page(p));
        }
        sync::others(self.win.key, SyncMsg::Blocks(page_id.to_string(), bs));
    }

    /// Remember the window's layout, in one write.
    fn save_ui(&self) {
        let doubled = {
            let ui = self.ui.peek();
            let twice = |tabs: &Vec<Tab>| ONE_TAB_VIEWS.iter().any(|v| tabs.iter().filter(|t| view_tab(t) == Some(v)).count() > 1);
            twice(&ui.tabs) || ui.parked.values().any(|set| twice(&set.0))
        };
        if doubled {
            let mut sig = self.ui;
            let mut ui = sig.write();
            let ui = &mut *ui;
            let mut active = ui.active;
            one_home_tab(&mut ui.tabs, &mut active);
            ui.active = active;
            for set in ui.parked.values_mut() {
                one_home_tab(&mut set.0, &mut set.1);
            }
        }
        // Pinned tabs stand first, whatever moved a tab (a drop, a page
        // that changed its space, a tab opened again).
        let loose = {
            let ui = self.ui.peek();
            let loose = |tabs: &Vec<Tab>| tabs.iter().skip_while(|t| t.pinned).any(|t| t.pinned);
            loose(&ui.tabs) || ui.parked.values().any(|set| loose(&set.0))
        };
        if loose {
            let mut sig = self.ui;
            let mut ui = sig.write();
            let ui = &mut *ui;
            pins_first(&mut ui.tabs, &mut ui.active);
            for set in ui.parked.values_mut() {
                pins_first(&mut set.0, &mut set.1);
            }
        }
        store::set_settings(&ui_settings(&self.ui.peek(), self.win.primary));
        self.note_tab_use();
    }

    /// Typing changed only this block's text: save just that.
    /// Save one table after a letter was typed in a cell: its cells and the
    /// words kept beside them, in one row.
    fn save_block_cells(&self, page_id: &str, block_id: &str) {
        // (A locked page is sealed whole at each save.)
        if crate::lock::is_open(page_id) {
            return self.save_blocks(page_id);
        }
        self.note_edit(page_id);
        let for_others = {
            let blocks = self.blocks.peek();
            let Some(bs) = blocks.get(page_id) else { return };
            if let Some(b) = bs.iter().find(|b| b.id == block_id) {
                store::with(|s| s.save_block_cells(page_id, block_id, &b.text, &b.data, now_ms()));
            }
            (sync::window_count() > 1).then(|| bs.clone())
        };
        if let Some(bs) = for_others {
            sync::others(self.win.key, SyncMsg::Blocks(page_id.to_string(), bs));
        }
    }

    fn save_block_text(&self, page_id: &str, block_id: &str) {
        if crate::lock::is_open(page_id) {
            return self.save_blocks(page_id);
        }
        self.note_edit(page_id);
        let for_others = {
            let blocks = self.blocks.peek();
            let Some(bs) = blocks.get(page_id) else { return };
            if let Some(b) = bs.iter().find(|b| b.id == block_id) {
                store::with(|s| s.save_block_text(page_id, block_id, &b.text, now_ms()));
            }
            (sync::window_count() > 1).then(|| bs.clone())
        };
        if let Some(bs) = for_others {
            sync::others(self.win.key, SyncMsg::Blocks(page_id.to_string(), bs));
        }
    }

    // ----- glass -----------------------------------------------------------

    /// Change the glass. While a slider moves only the window updates; `save`
    /// writes it down and tells the other windows.
    pub fn set_look(mut self, look: Look, save: bool) {
        if *self.look.peek() != look {
            self.look.set(look.clone());
        }
        if save {
            store::set_settings(&[
                ("glass", look.glass.to_string()),
                ("glass_radius", look.blur.to_string()),
                ("glass_tint", look.tint.clone()),
                ("accent", look.accent.clone()),
            ]);
            sync::others(self.win.key, SyncMsg::Look { glass: look.glass, blur: look.blur, tint: look.tint, accent: look.accent });
        }
    }

    // ----- small UI helpers ---------------------------------------------

    /// A message that stays long enough to read: for what went wrong and
    /// what to do about it. (`toast` is for a word of confirmation.)
    pub fn notice(self, message: impl Into<String>) {
        self.toast_for(message.into(), 6000);
    }

    pub fn toast(self, message: impl Into<String>) {
        self.toast_for(message.into(), 1000);
    }

    /// A message about something that was taken away, with an "Undo" in it
    /// that puts it back. It stays long enough to be pressed.
    pub fn toast_back(self, message: impl Into<String>, back: Back) {
        self.toast_with(message.into(), 6000, Some(back));
    }

    /// The same for a change of a page that its own Undo takes back: the
    /// message gets the step that was just made. With no new step (`was`
    /// is the step Undo would have taken before the change), it has no
    /// "Undo".
    fn toast_step(self, message: impl Into<String>, page: &str, was: Option<u64>) {
        let step = self.history.peek().next_undo(page).filter(|s| Some(*s) != was);
        match step {
            Some(step) => self.toast_back(message, Back::Step { page: page.to_string(), step }),
            None => self.toast(message),
        }
    }

    /// Whether the "Undo" of a message would still put back the thing the
    /// message is about, and nothing else. (It is asked while the message
    /// is drawn, so the "Undo" goes when this is no longer so, and again
    /// at the press.)
    pub fn back_holds(&self, back: &Back) -> bool {
        match back {
            Back::Page { id, .. } => self.pages.read().get(id).is_some_and(|p| p.trashed),
            // The person changed the page after that step (they typed, say):
            // Undo would now take back the later change. And a page that is
            // not on the screen is not changed behind the person's back.
            Back::Step { page, step } => {
                let _ = self.ui.read();
                self.history.read().next_undo(page) == Some(*step) && self.shows_page(page)
            }
            Back::Event { event, .. } => !self.events.read().contains_key(&event.id),
            Back::Repeat { series, day } => self.events.read().get(series).and_then(|e| e.repeat.as_ref()).is_some_and(|r| r.skip.contains(day)),
            Back::Tab(id) => {
                let pages = self.pages.read();
                self.ui.read().closed.last().is_some_and(|t| {
                    t.id == *id
                        && match t.route() {
                            Route::Page(p) => pages.get(p).is_some_and(|p| !p.trashed),
                            _ => true,
                        }
                })
            }
        }
    }

    /// The "Undo" of the message was pressed: the message goes at once, and
    /// its thing comes back, when that is still the thing to bring back.
    pub fn toast_undo(mut self) {
        let Some(back) = self.toast_msg.peek().as_ref().and_then(|t| t.3.clone()) else { return };
        self.toast_msg.set(None);
        if !self.back_holds(&back) {
            return;
        }
        match back {
            Back::Page { id, tab } => {
                self.take_out_of_trash(&id);
                // (Not `open_page`: that opens a page no tab shows in a new
                // tab, and the tab it left stayed beside it, at Home.)
                let shown = tab.is_some_and(|tab| self.ui.write().put_page_back_in(&tab, &id));
                if shown {
                    self.save_ui();
                }
            }
            Back::Step { page, .. } => self.step_on(&page, true),
            Back::Event { event, pages } => {
                let id = event.id.clone();
                self.events.write().insert(id.clone(), event);
                self.save_event(&id);
                self.change_repeat_pages(|own| own.extend(pages));
            }
            Back::Repeat { series, day } => {
                if let Some(r) = self.events.write().get_mut(&series).and_then(|e| e.repeat.as_mut()) {
                    r.skip.retain(|d| *d != day);
                }
                self.save_event(&series);
            }
            Back::Tab(_) => self.reopen_closed_tab(),
        }
    }

    fn toast_for(self, message: String, show_ms: u64) {
        self.toast_with(message, show_ms, None);
    }

    fn toast_with(mut self, message: String, show_ms: u64, back: Option<Back>) {
        let id = now_ms() as u64;
        self.toast_msg.set(Some((id, message, false, back)));
        // On the window, not the component that asked: that one is often
        // gone a moment later (the event panel after "Delete"), and its
        // tasks with it, which left the message on screen.
        spawn_forever(async move {
            ai::sleep_ms(show_ms).await;
            if let Some(t) = self.toast_msg.write().as_mut().filter(|t| t.0 == id) {
                t.2 = true;
            }
            ai::sleep_ms(180).await;
            let current = self.toast_msg.peek().as_ref().map(|t| t.0);
            if current == Some(id) {
                self.toast_msg.set(None);
            }
        });
    }

    pub fn set_popover(mut self, popover: Option<Popover>) {
        if self.ui.peek().popover != popover {
            self.ui.write().popover = popover;
        }
    }

    pub fn set_modal(mut self, modal: Option<Modal>) {
        self.clear_slash();
        {
            let ui = self.ui.peek();
            if ui.modal == modal && ui.popover.is_none() {
                return;
            }
        }
        let mut ui = self.ui.write();
        ui.modal = modal;
        ui.popover = None;
    }

    /// Close the `/` menu (without redrawing anything if it's already closed).
    pub fn clear_slash(mut self) {
        if self.slash.peek().is_some() {
            self.slash.set(None);
        }
    }

    pub fn toggle_expanded(mut self, id: &str) {
        {
            let mut ui = self.ui.write();
            if !ui.expanded.remove(id) {
                ui.expanded.insert(id.to_string());
            }
        }
        self.save_ui();
    }

    pub fn set_theme(mut self, theme: Theme) {
        self.ui.write().theme = theme;
        self.save_ui();
        sync::others(self.win.key, SyncMsg::Theme(theme.as_str().to_string()));
    }

    // ----- calendar --------------------------------------------------------

    pub fn cal_set_view(mut self, view: CalView) {
        self.cal.write().view = view;
        store::set_setting("cal_view", view.as_str());
    }

    /// Show the events of every space in the calendar (and on Home), or only
    /// those of the space in front.
    pub fn cal_set_all_spaces(mut self, all: bool) {
        self.cal.write().all_spaces = all;
        store::set_setting("cal_spaces", if all { "all" } else { "here" });
    }

    /// Move an event (a repeating one with all its repeats) to another space.
    pub fn set_event_space(self, id: &str, space: &str) {
        let Some(name) = self.spaces.peek().iter().find(|s| s.id == space).map(|s| s.display_name().to_string()) else { return };
        let (series, _) = repeat::split_id(id);
        if self.events.peek().get(series).is_some_and(|e| e.space_id == space) {
            return;
        }
        let space = space.to_string();
        self.update_event(id, move |ev| ev.space_id = space);
        self.toast(format!("Moved to {name}"));
    }

    /// Previous or next month/week.
    pub fn cal_step(mut self, dir: i64) {
        let mut c = self.cal.write();
        c.anchor = match c.view {
            CalView::Month => when::add_months(c.anchor, dir as i32),
            CalView::Week => when::add_days(c.anchor, 7 * dir),
            CalView::Day => when::add_days(c.anchor, dir),
        };
        drop(c);
        self.mac_calendars_in_view();
    }

    pub fn cal_today(mut self) {
        self.cal.write().anchor = when::today();
        self.mac_calendars_in_view();
    }

    // ----- the calendars of this Mac (maccal.rs) ---------------------------

    /// Turn the Mac's calendars on or off. Turning them on is when macOS
    /// asks the person, if it has not before.
    pub fn set_mac_calendars(mut self, on: bool) {
        store::set_setting(MacCal::ON, if on { "1" } else { "" });
        {
            let mut m = self.mac_cal.write();
            m.on = on;
            if !on {
                m.events.clear();
                m.calendars.clear();
                m.access = crate::maccal::Access::Unknown;
                m.from = 0;
                m.to = 0;
            }
        }
        if on {
            self.read_mac_calendars(true);
        }
    }

    /// Read the events again: around today and the day the calendar shows.
    pub fn read_mac_calendars(mut self, ask: bool) {
        if !self.mac_cal.peek().on || self.mac_cal.peek().reading {
            return;
        }
        if !crate::maccal::available() {
            self.mac_cal.write().access = crate::maccal::Access::Unavailable;
            return;
        }
        let today = when::today();
        let anchor = self.cal.peek().anchor;
        let from = when::midnight(when::add_days(today.min(anchor), -45));
        let to = when::midnight(when::add_days(today.max(anchor), 100));
        self.mac_cal.write().reading = true;
        spawn_forever(async move {
            let got = crate::maccal::fetch(from, to, ask).await;
            let mut m = self.mac_cal.write();
            m.reading = false;
            // Turned off while the reading was under way: nothing is kept.
            if !m.on {
                return;
            }
            m.access = got.access;
            m.calendars = got.calendars;
            m.events = got.events;
            m.from = from;
            m.to = to;
        });
    }

    /// The calendar went to a day near the edge of what was read: read more.
    fn mac_calendars_in_view(self) {
        let (on, from, to) = {
            let m = self.mac_cal.peek();
            (m.on && m.access == crate::maccal::Access::Allowed, m.from, m.to)
        };
        if !on {
            return;
        }
        let anchor = self.cal.peek().anchor;
        let near = when::midnight(when::add_days(anchor, -40)) < from || when::midnight(when::add_days(anchor, 45)) > to;
        if near {
            self.read_mac_calendars(false);
        }
    }

    /// Let the app change the Mac's calendars, or not.
    pub fn set_mac_write(mut self, on: bool) {
        store::set_setting(MacCal::WRITE, if on { "1" } else { "" });
        self.mac_cal.write().write = on;
    }

    /// The calendar of the Mac that new events go to ("" for the app's own).
    pub fn set_mac_new_in(mut self, calendar: &str) {
        store::set_setting(MacCal::NEW_IN, calendar);
        self.mac_cal.write().new_in = calendar.to_string();
    }

    /// Make an event that has its name and its time: in the calendar of
    /// the Mac the person chose for new events, or as one of the app's own.
    /// The id of the event when it is the app's own.
    pub fn make_event(mut self, title: &str, start: i64, end: i64, all_day: bool) -> Option<Id> {
        let Some(calendar) = self.mac_cal.peek().new_events_calendar() else {
            if !all_day {
                return Some(self.add_event(title, start, end));
            }
            let space = self.ui.peek().space.clone();
            let mut e = Event::new(space, title.trim(), start, end, true);
            e.remind = crate::notify::default_reminder();
            let id = e.id.clone();
            self.events.write().insert(id.clone(), e);
            self.save_event(&id);
            return Some(id);
        };
        // It shows at once; the Mac's calendar gets it, and is read again.
        let shown = crate::maccal::MacEvent {
            id: format!("{}new-{}", crate::maccal::PREFIX, new_id()),
            calendar: calendar.clone(),
            title: title.trim().to_string(),
            start,
            end,
            all_day,
            location: String::new(),
            join: String::new(),
        };
        let shown_id = shown.id.clone();
        self.mac_cal.write().events.push(shown);
        let title = title.trim().to_string();
        spawn_forever(async move {
            match crate::maccal::add_event(&calendar, &title, start, end, all_day).await {
                Ok(()) if !crate::maccal::stand_in_run() => self.read_mac_calendars(false),
                Ok(()) => {}
                Err(problem) => {
                    self.mac_cal.write().events.retain(|e| e.id != shown_id);
                    self.notice(problem);
                }
            }
        });
        None
    }

    /// An event of the Mac was dragged: it moves at once in the app, the
    /// Mac's calendar is told, and it goes back if the calendar says no.
    fn move_mac_event(mut self, id: &str, days: i64, minutes: i64, grow: i64) {
        if !self.mac_cal.peek().can_write() {
            return;
        }
        let Some(was) = self.mac_cal.peek().events.iter().find(|e| e.id == id).cloned() else { return };
        let (start, end) = if was.all_day {
            let first = when::date_of(was.start);
            let last = when::date_of(was.end - 1);
            (when::midnight(when::add_days(first, days)), when::midnight(when::next_day(when::add_days(last, days))))
        } else {
            let at = when::local(was.start);
            let time = chrono::NaiveTime::from_hms_opt(chrono::Timelike::hour(&at), chrono::Timelike::minute(&at), 0).unwrap_or_default();
            let start = when::at(when::add_days(when::date_of(was.start), days), time) + minutes * when::MINUTE;
            (start, start + ((was.end - was.start) + grow * when::MINUTE).max(15 * when::MINUTE))
        };
        if (start, end) == (was.start, was.end) {
            return;
        }
        if let Some(e) = self.mac_cal.write().events.iter_mut().find(|e| e.id == id) {
            e.start = start;
            e.end = end;
        }
        let id = id.to_string();
        // The Mac ends an all-day event in its last day.
        let told_end = if was.all_day { end - 1000 } else { end };
        spawn_forever(async move {
            let moved = crate::maccal::move_event(&id, start, told_end).await;
            if moved.is_ok() {
                // Its id holds its start: it has a new id now. Its notes
                // page and its open panel go with it (left on the old id,
                // the page was no more its page, and the panel shut).
                let new = crate::maccal::moved_id(&id, start);
                {
                    let mut m = self.mac_cal.write();
                    if let Some(e) = m.events.iter_mut().find(|e| e.id == id) {
                        e.id = new.clone();
                    }
                    if let Some(page) = m.pages.remove(&id) {
                        m.pages.insert(new.clone(), page);
                        store::set_setting(MacCal::PAGES, &serde_json::to_string(&m.pages).unwrap_or_default());
                    }
                }
                if self.cal.peek().selected.as_deref() == Some(id.as_str()) {
                    self.cal.write().selected = Some(new);
                }
            }
            match moved {
                // (The calendar is read again: what the Mac made of the change is what shows.)
                Ok(()) if !crate::maccal::stand_in_run() => self.read_mac_calendars(false),
                Ok(()) => {}
                Err(problem) => {
                    if let Some(e) = self.mac_cal.write().events.iter_mut().find(|e| e.id == id) {
                        e.start = was.start;
                        e.end = was.end;
                    }
                    self.notice(problem);
                }
            }
        });
    }

    /// Show or hide one of the Mac's calendars.
    pub fn show_mac_calendar(mut self, id: &str, show: bool) {
        let mut m = self.mac_cal.write();
        m.hidden.retain(|h| h != id);
        if !show {
            m.hidden.push(id.to_string());
        }
        store::set_setting(MacCal::HIDDEN, &serde_json::to_string(&m.hidden).unwrap_or_default());
    }

    /// The Mac's events between two moments, as the views draw events.
    pub fn mac_events(&self, space: &str, from: i64, to: i64) -> Vec<Event> {
        let m = self.mac_cal.read();
        if !m.on {
            return Vec::new();
        }
        m.events
            .iter()
            .filter(|e| e.start < to && e.end > from && !m.hidden.contains(&e.calendar))
            .map(|e| e.as_event(space, &m.color_of(&e.calendar), m.pages.get(&e.id).filter(|p| self.pages.peek().get(*p).is_some_and(|p| !p.trashed)).cloned()))
            .collect()
    }

    /// One of the Mac's events, by its id.
    pub fn mac_event(&self, id: &str) -> Option<Event> {
        let space = self.ui.peek().space.clone();
        let m = self.mac_cal.read();
        let e = m.events.iter().find(|e| e.id == id)?;
        Some(e.as_event(&space, &m.color_of(&e.calendar), m.pages.get(&e.id).filter(|p| self.pages.peek().get(*p).is_some_and(|p| !p.trashed)).cloned()))
    }

    /// An event by its id: one of the app's own, or one of the Mac's.
    pub fn any_event(&self, id: &str) -> Option<Event> {
        if id.starts_with(crate::maccal::PREFIX) { self.mac_event(id) } else { repeat::find(&self.events.peek(), id).map(|e| self.with_own_page(e)) }
    }

    /// Change the pages of repeats (see `change_repeat_pages_kept`); this
    /// window then draws from the list as it is kept.
    fn change_repeat_pages(mut self, change: impl FnOnce(&mut HashMap<String, Id>)) {
        let kept = change_repeat_pages_kept(change);
        if *self.repeat_pages.peek() != kept {
            self.repeat_pages.set(kept);
        }
    }

    /// A repeat of a repeating event with the notes page that is its own
    /// (see `repeat::page_of`); any other event as it is.
    pub fn with_own_page(&self, mut e: Event) -> Event {
        if let (series, Some(day)) = repeat::split_id(&e.id) {
            if let Some(stored) = self.events.peek().get(series).filter(|s| s.repeat.is_some()) {
                let pages = self.pages.peek();
                e.page_id = repeat::page_of(stored, day, &self.repeat_pages.read(), |p| pages.get(p).filter(|p| !p.trashed).map(|p| when::date_of(p.created_at)));
            }
        }
        e
    }

    pub fn select_event(mut self, id: Option<Id>) {
        if self.cal.peek().selected != id {
            let mut c = self.cal.write();
            c.selected = id;
            c.details = false;
        }
    }

    /// Show one day with its notes (the day view).
    pub fn cal_open_day(mut self, day: chrono::NaiveDate) {
        {
            let mut c = self.cal.write();
            c.anchor = day;
            c.view = CalView::Day;
        }
        store::set_setting("cal_view", CalView::Day.as_str());
    }

    /// The day view: open the panel of the event whose notes show.
    pub fn cal_details(mut self, id: &str) {
        let mut c = self.cal.write();
        c.selected = Some(id.to_string());
        c.details = true;
    }

    /// Close the event's panel. In the day view the event stays chosen: its
    /// notes are still at the right.
    pub fn close_event_panel(mut self) {
        let mut c = self.cal.write();
        if c.view == CalView::Day && c.details {
            c.details = false;
        } else {
            c.selected = None;
            c.details = false;
        }
    }

    /// The page that holds an event's notes; it is made when there is none.
    fn event_page(mut self, id: &str) -> Option<Id> {
        let of_the_mac = id.starts_with(crate::maccal::PREFIX);
        // A repeat has a page of its own, kept by the repeat's id. (Read
        // again first: another window may have made its page a moment ago.)
        let repeat_key = match repeat::split_id(id) {
            (series, Some(day)) if !of_the_mac => self.events.peek().get(series).filter(|e| e.repeat.is_some()).map(|e| repeat::page_key(e, day)),
            _ => None,
        };
        if repeat_key.is_some() {
            self.change_repeat_pages(|_| {});
        }
        let e = self.any_event(id)?;
        if let Some(pid) = e.page_id.as_ref().filter(|pid| self.pages.peek().get(*pid).is_some_and(|p| !p.trashed)) {
            return Some(pid.clone());
        }
        // Where the notes of a meeting of this name were kept the last time, if the person chose a page.
        let home = self.meeting_home(e.display_title());
        let page = self.create_page(home.clone());
        if let Some(p) = self.pages.write().get_mut(&page) {
            p.title = e.display_title().to_string();
            p.icon = Some(format!("calendar:{}", e.color));
            // (Kept in a page: the space of that page, which may be another.)
            if home.is_none() {
                p.space_id = e.space_id.clone();
            }
        }
        self.save_page(&page);
        remember_meeting_name(&page, e.display_title());
        if let Some(home) = &home {
            self.link_meeting_in(home, &page);
        }
        let when_text = if e.all_day {
            when::day_label(when::date_of(e.start))
        } else {
            format!("{} · {}–{}", when::day_label(when::date_of(e.start)), when::time_label(e.start), when::time_label(e.end))
        };
        let first = inline::escape(&format!("Notes for {} ({when_text})", e.display_title()));
        self.blocks.write().insert(page.clone(), vec![Block::text(first), Block::text("")]);
        self.save_blocks(&page);
        // When the event is over, its page asks which page to keep it in.
        self.ask_place(&page, e.end.max(now_ms()));
        if of_the_mac {
            // The event is the Mac's, and is not changed: the app only keeps
            // which page goes with it.
            let mut m = self.mac_cal.write();
            m.pages.insert(id.to_string(), page.clone());
            store::set_setting(MacCal::PAGES, &serde_json::to_string(&m.pages).unwrap_or_default());
        } else if let Some(key) = repeat_key {
            self.change_repeat_pages(|own| {
                own.insert(key, page.clone());
            });
            // (The other windows read the list again when an event comes to them.)
            if let Some(series) = self.events.peek().get(repeat::split_id(id).0).cloned() {
                sync::others(self.win.key, SyncMsg::Event(series));
            }
        } else {
            self.update_event(id, |ev| ev.page_id = Some(page.clone()));
        }
        Some(page)
    }

    /// The day view: start the notes of an event in place (no other tab).
    pub fn event_notes_here(self, id: &str) {
        let Some(page) = self.event_page(id) else { return };
        self.focus(Caret::end(&self.blocks.peek().get(&page).and_then(|b| b.last()).map(|b| b.id.clone()).unwrap_or_default()));
    }

    /// Join the meeting of an event: its address opens (in the browser, or
    /// the meeting's own app when the Mac hands it over), and the event's
    /// notes page comes up, made if there is none. The page asks whether to
    /// listen; the app never listens by itself.
    pub fn join_event(mut self, id: &str) {
        let Some(e) = self.any_event(id) else { return };
        let Some(link) = e.join_url() else { return };
        self.open_url(&link);
        let Some(page) = self.event_page(id) else { return };
        // This page is the notes of the meeting that starts now: when the
        // meeting watcher sees it start, it makes no second page.
        if MEETING.with(|m| m.borrow().as_ref().is_none_or(|n| n.page != page)) {
            let stamp = self.blocks.peek().get(&page).and_then(|b| b.first()).map(|b| b.id.clone()).unwrap_or_default();
            MEETING.with(|m| *m.borrow_mut() = Some(MeetingNotes { page: page.clone(), stamp, started: now_ms(), ended: None, app: None, noticed: false, span: (!e.all_day).then_some((e.start, e.end)), event: (!e.all_day).then(|| e.id.clone()), padded: false, cut: false, draft: false, also: None }));
            remember_meeting_page(&page);
            self.keep_meeting();
        }
        // In the day view the notes are at the right of the day; elsewhere the page opens.
        let in_day_view = self.cal.peek().view == CalView::Day && *self.ui.peek().tab().route() == Route::Calendar;
        if in_day_view {
            let mut c = self.cal.write();
            c.selected = Some(id.to_string());
            c.details = false;
        } else {
            self.open_page(&page);
        }
        if crate::listen::available() && self.listening.peek().is_none() {
            self.listen_offer.set(Some(page));
        }
    }

    /// The reminder of an event was clicked: join its meeting when it has
    /// an address to join by, else show the event.
    pub fn reminder_clicked(self, id: &str) {
        let joins = self.any_event(id).is_some_and(|e| e.join_url().is_some());
        if joins { self.join_event(id) } else { self.show_event(id) }
    }

    /// The day view: listen to the meeting of an event; the notes go in its page.
    pub fn listen_to_event(self, id: &str) {
        if let Some(page) = self.event_page(id) {
            self.start_listening(&page);
        }
    }

    fn save_event(&self, id: &str) {
        if let Some(e) = self.events.peek().get(id).cloned() {
            store::with(|s| s.save_event(&e));
            sync::others(self.win.key, SyncMsg::Event(e));
            crate::notify::mark_dirty();
        }
    }

    /// Make an event in this space and open it in the editor.
    /// Add an event whose name is already typed (a click on an empty time in
    /// the week). It is not selected, so its panel stays shut.
    pub fn add_event(mut self, title: &str, start: i64, end: i64) -> Id {
        let space = self.ui.peek().space.clone();
        let mut e = Event::new(space, title.trim(), start, end, false);
        e.remind = crate::notify::default_reminder();
        let id = e.id.clone();
        self.events.write().insert(id.clone(), e);
        self.save_event(&id);
        id
    }

    pub fn new_event(mut self, start: i64, end: i64, all_day: bool) {
        let space = self.ui.peek().space.clone();
        let mut e = Event::new(space, "", start, end, all_day);
        e.remind = crate::notify::default_reminder();
        let id = e.id.clone();
        self.events.write().insert(id.clone(), e);
        self.save_event(&id);
        let mut c = self.cal.write();
        c.selected = Some(id.clone());
        c.anchor = when::date_of(start);
        drop(c);
        document::eval(&format!(
            "requestAnimationFrame(()=>requestAnimationFrame(()=>{{const t=document.getElementById({});if(t){{t.focus();t.select&&t.select();}}}}))",
            serde_json::to_string(&format!("ev-title-{id}")).unwrap_or_default()
        ));
    }

    /// "New event" button: the next hour today, for an hour.
    pub fn new_event_now(self) {
        let start = when::next_hour(now_ms());
        self.new_event(start, start + when::HOUR, false);
    }

    /// An event typed in words in the calendar ("Lunch with Sam Friday 1pm",
    /// see quickadd.rs). When the words say a day or a time, the event is
    /// made there and the calendar goes to its day; no panel opens. With no
    /// words it is the plain new event; with words that say no day and no
    /// time, a new event of that name, with its panel open to set them.
    pub fn quick_event(mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return self.new_event_now();
        }
        let now = now_ms();
        let Some(read) = crate::quickadd::parse(text, now) else {
            let start = when::next_hour(now);
            let id = self.add_event(text, start, start + when::HOUR);
            let mut c = self.cal.write();
            c.anchor = when::date_of(start);
            c.selected = Some(id);
            c.details = true;
            return;
        };
        self.make_event(&read.title, read.start, read.end, read.all_day);
        let day = when::date_of(read.start);
        self.cal.write().anchor = day;
        self.mac_calendars_in_view();
        let at = if read.all_day { when::day_label(day) } else { format!("{} · {}", when::day_label(day), when::time_label(read.start)) };
        self.notice(format!("Added: {} ({at})", read.title.trim()));
    }

    /// Change an event. `id` may name one repeat of a repeating event
    /// ("<id>@<day>"): the change then goes to every repeat (moved by the same
    /// days and time), and the selection follows the repeat if it moved.
    pub fn update_event(mut self, id: &str, f: impl FnOnce(&mut Event)) {
        fn keep_long_enough(e: &mut Event) {
            let shortest = if e.all_day { 24 * when::HOUR } else { 15 * when::MINUTE };
            if e.end < e.start + shortest {
                e.end = e.start + shortest;
            }
        }
        let (series, day) = repeat::split_id(id);
        let series = series.to_string();
        let mut follow = None;
        // (A repeating event as it was: the notes pages of its repeats go with them.)
        let mut repeated = None;
        {
            let mut events = self.events.write();
            let Some(e) = events.get_mut(&series) else { return };
            if e.repeat.is_some() {
                repeated = Some(e.clone());
            }
            match day {
                Some(d) if e.repeat.is_some() => {
                    let before = repeat::at(e, d);
                    let mut after = before.clone();
                    f(&mut after);
                    keep_long_enough(&mut after);
                    repeat::apply(e, &before, &after);
                    let moved_to = when::date_of(after.start);
                    if e.repeat.is_none() {
                        follow = Some(series.clone());
                    } else if moved_to != d {
                        follow = Some(repeat::instance_id(&series, moved_to));
                    }
                }
                _ => {
                    // (An all-day event that was made in another time zone:
                    // the change is made to its days as they are read.)
                    if e.all_day {
                        *e = repeat::as_read(e);
                    }
                    f(e);
                    keep_long_enough(e);
                }
            }
            e.updated_at = now_ms();
        }
        if let Some(to) = follow {
            if self.cal.peek().selected.as_deref() == Some(id) {
                self.cal.write().selected = Some(to);
            }
        }
        if let Some(old) = repeated {
            self.carry_repeat_pages(&old);
        }
        self.save_event(&series);
    }

    /// A repeating event was changed (`old` is what it was): the notes
    /// pages of its repeats go with their repeats (see `repeat::carry_pages`),
    /// and when it repeats no more, the page of its first repeat is its page.
    fn carry_repeat_pages(mut self, old: &Event) {
        let Some(new) = self.events.peek().get(&old.id).cloned() else { return };
        let mut of_the_first = None;
        self.change_repeat_pages(|own| of_the_first = repeat::carry_pages(own, old, &new));
        if let Some(page) = of_the_first.filter(|_| new.repeat.is_none()) {
            if let Some(e) = self.events.write().get_mut(&old.id) {
                e.page_id = Some(page);
            }
        }
    }

    /// An event was dragged in the calendar: move it by days and minutes,
    /// or make it longer or shorter by `grow` minutes. An event of the Mac's
    /// calendars is not ours to change. (For a repeat, all its repeats move:
    /// see `update_event`.)
    pub fn move_event(self, id: &str, days: i64, minutes: i64, grow: i64) {
        if days == 0 && minutes == 0 && grow == 0 {
            return;
        }
        let (days, minutes, grow) = (days.clamp(-42, 42), minutes.clamp(-24 * 60, 24 * 60), grow.clamp(-24 * 60, 24 * 60));
        // An event of the Mac: only when the person lets the app change those.
        if id.starts_with(crate::maccal::PREFIX) {
            return self.move_mac_event(id, days, minutes, grow);
        }
        self.update_event(id, |ev| {
            if ev.all_day {
                // Whole days: the same number of them, so many days later.
                let first = when::date_of(ev.start);
                let last = when::date_of(ev.end - 1);
                ev.start = when::midnight(when::add_days(first, days));
                ev.end = when::midnight(when::next_day(when::add_days(last, days)));
                return;
            }
            let length = ev.end - ev.start;
            // By the clock of the day it lands on (an hour is not lost or
            // won on the day the clocks change).
            let at = when::local(ev.start);
            let time = chrono::NaiveTime::from_hms_opt(chrono::Timelike::hour(&at), chrono::Timelike::minute(&at), 0).unwrap_or_default();
            let start = when::at(when::add_days(when::date_of(ev.start), days), time) + minutes * when::MINUTE;
            ev.start = start;
            ev.end = start + (length + grow * when::MINUTE).max(15 * when::MINUTE);
        });
    }

    /// A to-do was dragged onto a time: half an hour is put on the
    /// calendar for it there, with its words, and the to-do is due that day.
    /// The event's notes page is the page the to-do is on.
    pub fn plan_todo(self, page: &str, block: &str, day: &str, minutes: i64) {
        let Some(day) = when::parse_date(day) else { return };
        let words = self.blocks.peek().get(page).and_then(|bs| bs.iter().find(|b| b.id == block && b.kind == BlockKind::Todo).map(|b| b.plain_text()));
        let Some(words) = words.filter(|w| !w.trim().is_empty()) else { return };
        let start = when::midnight(day) + minutes.clamp(0, 24 * 60 - 30) * when::MINUTE;
        // (An event that goes to a calendar of the Mac has no notes page of the app's.)
        if let Some(id) = self.make_event(words.trim(), start, start + 30 * when::MINUTE, false) {
            let home = page.to_string();
            self.update_event(&id, |ev| ev.page_id = Some(home));
        }
        self.set_due(page, block, Some(day));
    }

    /// ⌘C on an event: it is kept to paste (⌘V) at another time.
    pub fn copy_event(self, id: &str) {
        let Some(e) = self.any_event(id) else { return };
        let title = e.display_title().to_string();
        COPIED_EVENT.with(|c| *c.borrow_mut() = Some(e));
        self.toast(format!("Copied “{title}”. Point at a time and press ⌘V"));
    }

    /// ⌘V in the calendar: a new event like the copied one (its name, length,
    /// colour, notes, address and reminder; not its repeats or notes page),
    /// on `day` from `minute`, or at its own time with a negative minute.
    pub fn paste_event(self, day: &str, minute: i64) {
        let Some(copied) = COPIED_EVENT.with(|c| c.borrow().clone()) else {
            return self.toast("Copy an event first: ⌘C with the pointer on it");
        };
        let Some(day) = when::parse_date(day) else {
            return self.toast("Point at a time in the calendar, then press ⌘V");
        };
        let length = (copied.end - copied.start).max(15 * when::MINUTE);
        let (start, end) = if copied.all_day {
            repeat::whole_days_from(&copied, day)
        } else if minute >= 0 {
            let start = when::midnight(day) + minute.clamp(0, 24 * 60 - 15) * when::MINUTE;
            (start, start + length)
        } else {
            let at = when::local(copied.start);
            let time = chrono::NaiveTime::from_hms_opt(chrono::Timelike::hour(&at), chrono::Timelike::minute(&at), 0).unwrap_or_default();
            let start = when::at(day, time);
            (start, start + length)
        };
        let Some(id) = self.make_event(copied.display_title(), start, end, copied.all_day) else { return };
        if !id.starts_with(crate::maccal::PREFIX) {
            let c = copied.clone();
            self.update_event(&id, move |ev| {
                ev.color = c.color;
                ev.notes = c.notes;
                ev.join = c.join;
                ev.remind = c.remind;
            });
        }
        self.select_event(Some(id));
        self.toast(format!("Pasted “{}” · {} {}", copied.display_title(), day.format("%a %-d %b"), if copied.all_day { "all day".to_string() } else { when::time_label(start) }));
    }

    /// The minute of the day that was just pressed in the calendar, as the
    /// page script measured it (taken once; none when the press is old).
    pub fn cal_press_minute() -> Option<u32> {
        CAL_PRESS.with(|p| p.take()).filter(|(at, _)| now_ms() - at < 5_000).map(|(_, minute)| minute.clamp(0, 24 * 60 - 15) as u32)
    }

    /// Empty time was dragged over: a slot of that length to type the
    /// name of the new event in.
    pub fn draw_event(mut self, day: &str, from: i64, to: i64) {
        // (The press became a drag: its minute is for no click.)
        CAL_PRESS.with(|p| p.set(None));
        let Some(day) = when::parse_date(day) else { return };
        let (from, to) = (from.clamp(0, 24 * 60 - 15), to.clamp(15, 24 * 60));
        if to <= from {
            return;
        }
        let at = |minutes: i64| when::midnight(day) + minutes * when::MINUTE;
        self.cal_draft.set(Some((at(from), at(to))));
    }

    /// Set how an event repeats (a choice from `repeat::choices`), keeping
    /// its end date and any repeats deleted on their own.
    pub fn set_event_repeat(mut self, id: &str, choice: &str) {
        let Ok(rule) = repeat::parse(choice) else { return };
        let (series, _) = repeat::split_id(id);
        let series = series.to_string();
        let (first, old) = {
            let mut events = self.events.write();
            let Some(e) = events.get_mut(&series) else { return };
            let old = e.repeat.is_some().then(|| e.clone());
            e.repeat = rule.map(|mut r| {
                if let Some(old) = &e.repeat {
                    r.until = old.until.clone();
                    r.skip = old.skip.clone();
                }
                r
            });
            e.updated_at = now_ms();
            (repeat::first_day(e), old)
        };
        if let Some(old) = old {
            self.carry_repeat_pages(&old);
        }
        self.save_event(&series);
        // Select the event's first time, which is now a repeat (or not).
        let repeats = self.events.peek().get(&series).is_some_and(|e| e.repeat.is_some());
        self.cal.write().selected = Some(if repeats { repeat::instance_id(&series, first) } else { series });
    }

    /// The last day a repeating event repeats on (None: no end).
    pub fn set_event_until(self, id: &str, until: Option<chrono::NaiveDate>) {
        self.update_event(id, |e| {
            if let Some(r) = &mut e.repeat {
                r.until = until.map(when::date_value);
            }
        });
    }

    /// Delete an event; for a repeating one, every repeat.
    pub fn delete_event(mut self, id: &str) {
        let (series, _) = repeat::split_id(id);
        let series = series.to_string();
        let was = self.events.write().remove(&series);
        store::with(|s| s.delete_event(&series));
        // (The pages of its repeats stay, as pages; the list has no use for them.)
        let pages: Vec<(String, Id)> = repeat_pages_kept().into_iter().filter(|(key, _)| repeat::split_id(key).0 == series).collect();
        self.change_repeat_pages(|own| own.retain(|key, _| repeat::split_id(key).0 != series));
        sync::others(self.win.key, SyncMsg::EventRemoved(series));
        crate::notify::mark_dirty();
        self.select_event(None);
        match was {
            Some(event) => self.toast_back("Event deleted", Back::Event { event, pages }),
            None => self.toast("Event deleted"),
        }
    }

    /// Delete one repeat of a repeating event, keeping the others.
    pub fn skip_event(mut self, id: &str) {
        let (series, Some(day)) = repeat::split_id(id) else { return self.delete_event(id) };
        let series = series.to_string();
        if let Some(r) = self.events.write().get_mut(&series).and_then(|e| e.repeat.as_mut()) {
            r.skip.push(when::date_value(day));
        }
        self.save_event(&series);
        self.select_event(None);
        self.toast_back("Deleted this repeat", Back::Repeat { series, day: when::date_value(day) });
    }

    /// Make one repeat an event of its own, so it can change without the
    /// others (the repeating event skips that day from now on).
    pub fn detach_event(mut self, id: &str) {
        let (series, Some(day)) = repeat::split_id(id) else { return };
        let series = series.to_string();
        let Some(e) = self.events.peek().get(&series).cloned() else { return };
        // (With the notes page of this repeat, not the one page of an older Rune for all of them.)
        let mut one = self.with_own_page(repeat::at(&e, day));
        let now = now_ms();
        one.id = new_id();
        one.repeat = None;
        one.created_at = now;
        one.updated_at = now;
        let one_id = one.id.clone();
        self.events.write().insert(one_id.clone(), one);
        // (Its page is on its own record now.)
        self.change_repeat_pages(|own| {
            own.remove(&repeat::page_key(&e, day));
        });
        self.save_event(&one_id);
        if let Some(r) = self.events.write().get_mut(&series).and_then(|e| e.repeat.as_mut()) {
            r.skip.push(when::date_value(day));
        }
        self.save_event(&series);
        self.select_event(Some(one_id));
        self.toast("This one now changes on its own");
    }

    /// Open the event's notes page, making it the first time.
    pub fn event_notes(self, id: &str) {
        let had_one = self
            .any_event(id)
            .and_then(|e| e.page_id)
            .is_some_and(|pid| self.pages.peek().get(&pid).is_some_and(|p| !p.trashed));
        let Some(page) = self.event_page(id) else { return };
        self.open_page(&page);
        if !had_one {
            self.focus(Caret::end(&self.blocks.peek().get(&page).and_then(|b| b.last()).map(|b| b.id.clone()).unwrap_or_default()));
        }
    }

    /// The jot box on Home: save a line as a new page, without leaving Home.
    pub fn capture_note(mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let id = self.create_page(None);
        let title: String = text.chars().take(60).collect();
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = title.trim().to_string();
            p.icon = Some("note:yellow".into());
        }
        self.save_page(&id);
        self.blocks.write().insert(id.clone(), vec![Block::text(inline::escape(&text))]);
        self.save_blocks(&id);
        self.toast("Saved as a page");
    }

    /// Open the calendar on an event (from Home).
    pub fn show_event(mut self, id: &str) {
        let Some(start) = self.any_event(id).map(|e| e.start) else { return };
        {
            let mut c = self.cal.write();
            c.anchor = when::date_of(start);
            c.selected = Some(id.to_string());
        }
        self.navigate(Route::Calendar);
    }

    // ----- spaces ----------------------------------------------------------

    /// The space shown in this window.
    pub fn space(&self) -> Option<Space> {
        let id = self.ui.peek().space.clone();
        self.spaces.peek().iter().find(|s| s.id == id).cloned()
    }

    fn save_spaces(&self) {
        let spaces = self.spaces.peek().clone();
        for sp in &spaces {
            store::with(|s| s.save_space(sp));
        }
        sync::others(self.win.key, SyncMsg::Spaces(spaces));
    }

    /// Rename the space shown in this window.
    pub fn rename_space(mut self, name: String) {
        let id = self.ui.peek().space.clone();
        if let Some(sp) = self.spaces.write().iter_mut().find(|s| s.id == id) {
            sp.name = name;
        }
        self.save_spaces();
    }

    /// Put space `id` just before or just after space `at`: the order of
    /// the space menu, and of the swipe from one space to the next.
    pub fn move_space(mut self, id: &str, at: &str, after: bool) {
        if id == at {
            return;
        }
        {
            let mut spaces = self.spaces.write();
            let Some(from) = spaces.iter().position(|s| s.id == id) else { return };
            let moved = spaces.remove(from);
            let Some(to) = spaces.iter().position(|s| s.id == at) else {
                spaces.insert(from, moved);
                return;
            };
            spaces.insert(if after { to + 1 } else { to }, moved);
            for (i, s) in spaces.iter_mut().enumerate() {
                s.position = (i + 1) as f64;
            }
        }
        self.save_spaces();
    }

    pub fn set_space_color(mut self, color: &str) {
        let id = self.ui.peek().space.clone();
        if let Some(sp) = self.spaces.write().iter_mut().find(|s| s.id == id) {
            sp.color = color.to_string();
        }
        self.save_spaces();
    }

    /// Show another space: its sidebar and its own tabs, as they were left.
    /// A space with no tabs yet starts on the page you were last on there,
    /// or its Home.
    pub fn switch_space(mut self, id: &str) {
        self.set_popover(None);
        if self.ui.peek().space == id {
            return;
        }
        self.clear_slash();
        let last = {
            let pages = self.pages.peek();
            self.ui.peek().recent.iter().find(|r| pages.get(*r).is_some_and(|p| p.space_id == id && !p.trashed)).cloned()
        };
        self.enter_space(id, last.map(Route::Page).unwrap_or(Route::Home));
        {
            let pages = self.pages.peek();
            let mut ui = self.ui.write();
            ui.modal = None;
            let route = ui.tab().route().clone();
            remember(&mut ui, &pages, &route);
        }
        self.save_ui();
    }

    /// Bring a space to the front with its tabs. `first` is the tab of a
    /// space with none yet; the answer says whether it was made.
    fn enter_space(mut self, id: &str, first: Route) -> bool {
        // Its list comes in from the side it is on (see the dots at the foot
        // of the sidebar). The mark is taken away when the move is over.
        let slide = {
            let spaces = self.spaces.peek();
            let at = |id: &str| spaces.iter().position(|s| s.id == id);
            match (at(&self.ui.peek().space), at(id)) {
                (Some(from), Some(to)) if to < from => -1,
                _ => 1,
            }
        };
        self.space_slide.set(slide);
        let mut mark = self.space_slide;
        spawn_forever(async move {
            ai::sleep_ms(420).await;
            if *mark.peek() != 0 {
                mark.set(0);
            }
        });
        let fresh = {
            let pages = self.pages.peek();
            self.ui.write().show_tabs_of(id, &pages, first)
        };
        if self.win.primary {
            store::set_setting("space", id);
        }
        fresh
    }

    /// A page of another space is about to open: that space comes to the
    /// front first, so the page gets its tab among the tabs of its own
    /// space. The answer says whether a tab showing it was made on the way.
    fn enter_space_of(self, route: &Route) -> bool {
        let space = match route {
            Route::Page(id) => self.pages.peek().get(id).map(|p| p.space_id.clone()).filter(|s| !s.is_empty()),
            _ => None,
        };
        match space {
            Some(space) if self.ui.peek().space != space => self.enter_space(&space, route.clone()),
            _ => false,
        }
    }

    /// The space after this one (1) or before it (-1), in the order of the
    /// dots at the foot of the sidebar. At the two ends nothing happens.
    pub fn step_space(self, by: isize) {
        let next = {
            let spaces = self.spaces.peek();
            let here = self.ui.peek().space.clone();
            spaces.iter().position(|s| s.id == here).and_then(|at| at.checked_add_signed(by)).and_then(|i| spaces.get(i)).map(|s| s.id.clone())
        };
        if let Some(id) = next {
            self.switch_space(&id);
        }
    }

    /// Make a new space and switch to it.
    pub fn create_space(mut self, name: String) {
        let colors = ["blue", "purple", "green", "orange", "pink", "yellow", "red", "brown", "gray"];
        let n = self.spaces.peek().len();
        let position = self.spaces.peek().last().map(|s| s.position + 1.0).unwrap_or(1.0);
        let name = if name.trim().is_empty() { "New space".to_string() } else { name.trim().to_string() };
        let space = Space::new(name, colors[n % colors.len()], position);
        let id = space.id.clone();
        self.spaces.write().push(space);
        self.save_spaces();
        self.switch_space(&id);
        self.toast("Space created");
    }

    /// Delete the space shown in this window. Only an empty space can go
    /// (pages in its Trash move to another space's Trash).
    pub fn delete_space(mut self) {
        let id = self.ui.peek().space.clone();
        let in_use = self.pages.peek().values().any(|p| p.space_id == id && !p.trashed);
        let others: Vec<Space> = self.spaces.peek().iter().filter(|s| s.id != id).cloned().collect();
        if in_use || others.is_empty() {
            self.toast(if others.is_empty() { "You need at least one space" } else { "Move this space's pages to Trash first" });
            return;
        }
        let into = others[0].id.clone();
        store::with(|s| s.delete_space(&id, &into));
        rehome(&mut self.pages.write(), &mut self.events.write(), std::slice::from_ref(&id), &into);
        self.spaces.set(others.clone());
        sync::others(self.win.key, SyncMsg::Spaces(others));
        self.set_modal(None);
        self.switch_space(&into);
        self.ui.write().parked.remove(&id);
        self.toast("Space deleted");
    }

    /// A tab goes with the page it shows: when the page in front is in
    /// another space (it was moved there, or Back led there), the tab moves
    /// to that space, and the space comes to the front. A tab that shows
    /// several pages stays where it is.
    pub fn follow_route_space(mut self) {
        let space = {
            let ui = self.ui.peek();
            let tab = ui.tab();
            match tab_space(tab, &self.pages.peek()) {
                Some(space) if !tab.is_split() && ui.space != space => space,
                _ => return,
            }
        };
        {
            let pages = self.pages.peek();
            self.ui.write().carry_tab_to(&space, &pages);
        }
        if self.win.primary {
            store::set_setting("space", &space);
        }
        self.save_ui();
    }

    /// Give a page to a chat in Aven: handed over as a file where the
    /// installed Aven takes one, otherwise copied, with Aven brought to the
    /// front for a paste (see aven.rs).
    pub fn send_to_aven(self, page_id: &str) {
        if self.locked_lines(page_id) {
            return self.toast("A locked page is not given to other apps");
        }
        let Some(app) = crate::aven::app() else {
            return self.toast("Aven is not on this Mac");
        };
        let title = self.page_title(page_id);
        let text = crate::aven::note_for_chat(&title, page_id, &self.page_markdown(page_id));
        if std::env::var_os("RUNE_SELFTEST").is_some() {
            // For the self-test: what was given, without the clipboard.
            let first = text.lines().next().unwrap_or_default().to_string();
            document::eval(&format!("document.body.dataset.sentToAven = {}", serde_json::to_string(&first).unwrap_or_default()));
        }
        let copy = |text: String| {
            if std::env::var_os("RUNE_SELFTEST").is_some() {
                return Ok(());
            }
            arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())
        };
        match crate::aven::send(&app, &title, &text, copy) {
            Ok(crate::aven::Sent::Handed) => self.toast("Sent to Aven"),
            Ok(crate::aven::Sent::Copied) => self.notice("Copied. Paste it in the Aven chat (⌘V)"),
            Err(e) => self.notice(format!("Couldn't give it to Aven: {e}")),
        }
    }

    pub fn copy_to_clipboard(self, text: String, message: &str) {
        match set_clipboard(text) {
            Ok(()) => self.toast(message),
            Err(e) => self.toast(format!("Couldn't copy: {e}")),
        }
    }

    fn clear_picked(mut self) {
        if self.picked.peek().is_some() {
            self.picked.set(None);
        }
    }

    /// The picked blocks as Markdown, in the order of the page.
    fn picked_markdown(&self) -> Option<String> {
        let picked = self.picked.peek().clone()?;
        let all = self.blocks.peek();
        let blocks: Vec<Block> = all.get(&picked.page)?.iter().filter(|b| picked.ids.contains(&b.id)).cloned().collect();
        (!blocks.is_empty()).then(|| markdown::from_blocks("", &blocks, |id| self.page_title(id)).trim_end().to_string())
    }

    /// Copy the picked blocks; with `cut`, take them out of the page too.
    pub fn copy_picked(self, cut: bool) {
        let Some(mut text) = self.picked_markdown() else { return };
        let count = self.picked.peek().as_ref().map(|p| p.ids.len()).unwrap_or(0);
        // The blocks themselves are kept beside the words on the clipboard:
        // pasted in the app, they come back as they were (a picture, a
        // drawing, a toggle, a table with its columns), not as the plain
        // words that another program gets. One block that is no plain line
        // gets a line end, so that the paste comes to the app and is not
        // put in as typed words ("- [ ] Call Sam" became a bullet).
        // One plain line gets none (in a title the line end was a Return,
        // in a cell a space, in a terminal the Return key): the page
        // script is told the words that are kept, and gives a paste of
        // just those words in a line to the app (a line with a bold word
        // showed its stars and backslashes).
        let blocks: Vec<Block> = {
            let picked = self.picked.peek().clone();
            let all = self.blocks.peek();
            picked.and_then(|p| all.get(&p.page).map(|bs| bs.iter().filter(|b| p.ids.contains(&b.id)).cloned().collect())).unwrap_or_default()
        };
        if !text.contains('\n') && blocks.iter().any(|b| b.kind != BlockKind::Text) {
            text.push('\n');
        }
        document::eval(&format!("window.__rune && (window.__rune.keptCopy = {});", serde_json::to_string(&text).unwrap_or_default()));
        // (The pages of rows that are not in the Trash now: the cut puts
        // them there, and a paste of the table takes those out again, and
        // not one that the person put there by hand before.)
        let row_pages: Vec<Id> = if cut {
            let pages = self.pages.peek();
            blocks.iter().filter(|b| b.kind == BlockKind::Table).flat_map(|b| self.row_pages(&b.id).into_values()).filter(|id| pages.get(id).is_some_and(|p| !p.trashed)).collect()
        } else {
            Vec::new()
        };
        COPIED_BLOCKS.with(|c| *c.borrow_mut() = Some((text.clone(), blocks, cut, row_pages)));
        if let Err(e) = set_clipboard(text) {
            return self.toast(format!("Couldn't copy: {e}"));
        }
        if cut {
            self.delete_picked();
        } else {
            self.toast(if count == 1 { "Copied 1 block".to_string() } else { format!("Copied {count} blocks") });
        }
    }

    /// Take the picked blocks out of their page, as one step to undo. A page
    /// that one of them links to stays (it is still in the sidebar).
    pub fn delete_picked(mut self) {
        let Some(picked) = self.picked.peek().clone() else { return };
        self.picked.set(None);
        let caret = self.edit_blocks(&picked.page, |bs| {
            let first = bs.iter().position(|b| picked.ids.contains(&b.id))?;
            bs.retain(|b| !picked.ids.contains(&b.id));
            ensure_not_empty(bs);
            // The caret goes to the line before what was taken out, or to the first line.
            let at = (0..first.min(bs.len())).rev().find(|&i| bs[i].kind.is_editable()).or_else(|| bs.iter().position(|b| b.kind.is_editable()))?;
            Some(Caret::end(&bs[at].id))
        });
        if let Some(caret) = caret {
            self.focus(caret);
        }
    }

    // ----- focus and DOM sync -------------------------------------------

    /// Put the caret in a block. Runs after the next render.
    pub fn focus(&self, caret: Caret) {
        // A line under a folded heading is not on the page: the heading
        // opens first. (So a line made or changed there shows: after Enter
        // at the end of the heading, a paste, Undo, the "+" of the heading.)
        let page = {
            let ui = self.ui.peek();
            let blocks = self.blocks.peek();
            ui.folds.keys().find(|p| blocks.get(*p).is_some_and(|bs| bs.iter().any(|b| b.id == caret.id))).cloned()
        };
        if let Some(page) = page {
            self.show_line(&page, &caret.id);
        }
        let pos: i64 = if caret.pos == usize::MAX { -1 } else { caret.pos as i64 };
        let target = serde_json::to_string(&format!("blk-{}", caret.id)).unwrap_or_default();
        document::eval(&format!("window.__rune && window.__rune.focus({target}, {pos})"));
    }

    pub fn focus_title(&self, page_id: &str) {
        let target = serde_json::to_string(&format!("title-{page_id}")).unwrap_or_default();
        document::eval(&format!("window.__rune && window.__rune.focus({target}, -1)"));
    }

    /// Block editors are not re-rendered on every keystroke (so fast typing is
    /// never overwritten); after an edit made by code, push the new text of the
    /// blocks it changed. New blocks get their text when they are created.
    fn sync_blocks(&self, page_id: &str, ids: &[Id]) {
        if ids.is_empty() || !self.shows_page(page_id) {
            return;
        }
        let blocks = self.blocks.peek();
        let Some(bs) = blocks.get(page_id) else { return };
        let map: HashMap<String, String> = bs
            .iter()
            .filter(|b| b.kind.is_editable() && ids.contains(&b.id))
            .map(|b| (format!("blk-{}", b.id), inline::to_html(b.kind, &b.text)))
            .collect();
        if let Ok(json) = serde_json::to_string(&map) {
            document::eval(&format!("window.__rune && window.__rune.setValues({json})"));
        }
    }

    // ----- navigation ----------------------------------------------------

    pub fn navigate(mut self, route: Route) {
        if ONE_TAB_VIEWS.contains(&route) {
            let other = {
                let ui = self.ui.peek();
                ui.tabs.iter().position(|t| view_tab(t) == Some(&route)).filter(|&i| i != ui.active.min(ui.tabs.len() - 1))
            };
            if let Some(i) = other {
                self.select_tab(i);
                return self.set_modal(None);
            }
        }
        if let Route::Page(id) = &route {
            // A page of another space opens among the tabs of its space.
            if self.pages.peek().get(id).is_some_and(|p| !p.space_id.is_empty() && p.space_id != self.ui.peek().space) {
                return self.open_page(id);
            }
        }
        self.clear_slash();
        let mut fresh = None;
        {
            let pages = self.pages.peek();
            let mut ui = self.ui.write();
            ui.popover = None;
            ui.modal = None;
            if let Route::Page(id) = &route {
                ui.recent.retain(|r| r != id);
                ui.recent.insert(0, id.clone());
                ui.recent.truncate(20);
                for a in ancestors(&pages, id) {
                    ui.expanded.insert(a.id);
                }
            }
            // Pages open in the pane that has focus.
            let tab = ui.tab_mut();
            if *tab.route() != route {
                fresh = Some((tab.focused().id.clone(), route.place_key()));
            }
            tab.push(route);
        }
        if let Some((pane, place)) = fresh {
            self.place_fresh(&pane, &place);
        }
        self.save_ui();
    }

    /// A pane goes to a page anew (from the sidebar, search, a link): the
    /// page starts at its top. Back, Forward, another tab and a split come
    /// back to the place the page was left at; the page script keeps it.
    pub(crate) fn place_fresh(&self, pane: &str, place: &str) {
        let (pane, place) = (serde_json::to_string(pane).unwrap_or_default(), serde_json::to_string(place).unwrap_or_default());
        document::eval(&format!("window.__rune && window.__rune.placeFresh && window.__rune.placeFresh({pane}, {place})"));
    }

    // ----- dragging tabs ----------------------------------------------------

    /// A tab was dropped on another: "before"/"after" moves it there; "split"
    /// (the middle of the other tab) brings its pages into that tab, on the right.
    pub fn drop_tab(mut self, from: &str, to: Option<&str>, mode: &str) {
        if to == Some(from) {
            return;
        }
        {
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            let active_id = ui.tab().id.clone();
            let Some(fi) = ui.tabs.iter().position(|t| t.id == from) else { return };
            // A pinned tab keeps to itself: it moves among the pinned ones
            // only (`save_ui` puts it back among them from anywhere else).
            if ui.tabs[fi].pinned && mode == "split" {
                return;
            }
            let tab = ui.tabs.remove(fi);
            match to.and_then(|id| ui.tabs.iter().position(|t| t.id == id)) {
                None => ui.tabs.push(tab),
                Some(ti) if mode == "split" => {
                    // A single page the other tab already shows stays where it was.
                    if !tab.is_split() && ui.tabs[ti].pane_showing(tab.route()).is_some() {
                        ui.tabs.insert(fi.min(ui.tabs.len()), tab);
                        return;
                    }
                    ui.tabs[ti].add_tab(tab, None, Edge::Right);
                    ui.active = ti;
                    ui.popover = None;
                    drop(guard);
                    self.save_ui();
                    self.flash_active_tab();
                    return;
                }
                Some(ti) => ui.tabs.insert(if mode == "after" { ti + 1 } else { ti }, tab),
            }
            ui.active = ui.tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
            ui.popover = None;
        }
        self.save_ui();
        self.flash_tab(from);
    }

    /// A tab dropped on the edge of a pane in the tab in front: all its pages
    /// join this tab there.
    pub fn place_tab(mut self, from: &str, target: &str, edge: Edge) {
        {
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            let active_id = ui.tab().id.clone();
            if from == active_id {
                return;
            }
            let Some(fi) = ui.tabs.iter().position(|t| t.id == from) else { return };
            // (A pinned tab is not pulled into a pane.)
            if ui.tabs[fi].pinned {
                return;
            }
            let tab = ui.tabs.remove(fi);
            let ai = ui.tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
            ui.tabs[ai].add_tab(tab, Some(target), edge);
            ui.active = ai;
            ui.popover = None;
        }
        self.save_ui();
        self.flash_active_tab();
    }

    /// A sidebar page dropped on the edge of a pane: it opens there.
    pub fn place_page(mut self, page: &str, target: &str, edge: Edge) {
        let route = Route::Page(page.to_string());
        {
            let pages = self.pages.peek();
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            remember(ui, &pages, &route);
            ui.tab_mut().add_pane(Pane::new(route), Some(target), edge);
            ui.popover = None;
        }
        self.save_ui();
    }

    /// One page of a tab (dragged by its part of the tab, or by its bar)
    /// dropped on the edge of a pane in the tab in front: it moves there,
    /// from this tab or another one.
    pub fn place_pane(mut self, tab: &str, pane: &str, target: &str, edge: Edge) {
        if pane == target {
            return;
        }
        {
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            let active_id = ui.tab().id.clone();
            if tab == active_id {
                let t = ui.tab_mut();
                if !t.layout.move_pane(pane, target, edge) {
                    return;
                }
                t.focus = pane.to_string();
            } else {
                let Some(si) = ui.tabs.iter().position(|t| t.id == tab) else { return };
                // A tab's last page brings the tab along.
                let moved = if ui.tabs[si].is_split() {
                    ui.tabs[si].take_pane(pane).map(Tab::with_pane)
                } else {
                    Some(ui.tabs.remove(si))
                };
                let Some(moved) = moved else { return };
                let ai = ui.tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
                ui.tabs[ai].add_tab(moved, Some(target), edge);
                ui.active = ai;
            }
            ui.popover = None;
        }
        self.save_ui();
    }

    /// One page of a tab that shows several was pulled out of it (as in Aven).
    /// `mode` says where it was let go: "before"/"after" another tab, "end"
    /// of the strip, "split" (the middle of another tab: it joins that tab),
    /// or "out" (anywhere else: a tab of its own next to where it came from).
    pub fn detach_pane(mut self, tab: &str, pane: &str, to: Option<&str>, mode: &str) {
        {
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            let Some(si) = ui.tabs.iter().position(|t| t.id == tab) else { return };
            let Some(pulled) = ui.tabs[si].take_pane(pane) else { return };
            let target = to.and_then(|id| ui.tabs.iter().position(|t| t.id == id));
            match (mode, target) {
                ("split", Some(ti)) if ti != si => {
                    ui.tabs[ti].add_tab(Tab::with_pane(pulled), None, Edge::Right);
                    ui.active = ti;
                }
                ("before", Some(ti)) => {
                    ui.tabs.insert(ti, Tab::with_pane(pulled));
                    ui.active = ti;
                }
                ("after", Some(ti)) | ("split", Some(ti)) => {
                    ui.tabs.insert(ti + 1, Tab::with_pane(pulled));
                    ui.active = ti + 1;
                }
                ("end", _) => {
                    ui.tabs.push(Tab::with_pane(pulled));
                    ui.active = ui.tabs.len() - 1;
                }
                // Let go away from the tabs: right after where it came from.
                _ => {
                    ui.tabs.insert(si + 1, Tab::with_pane(pulled));
                    ui.active = si + 1;
                }
            }
            ui.popover = None;
        }
        self.save_ui();
        self.flash_active_tab();
    }

    /// A drawing changed in its frame. Saved on its own (no page rewrite) and
    /// shown in other windows; not a page undo step (the canvas has its own).
    pub fn save_drawing(mut self, page_id: &str, block_id: &str, data: String) {
        let blocks = {
            let mut all = self.blocks.write();
            let Some(bs) = all.get_mut(page_id) else { return };
            let Some(b) = bs.iter_mut().find(|b| b.id == block_id && b.kind == BlockKind::Drawing) else { return };
            if b.data == data {
                return;
            }
            b.data = data.clone();
            bs.clone()
        };
        crate::drawing::mark_shown(self.win.key, block_id, &data);
        self.history.write().set_data(block_id, &data);
        if crate::lock::is_open(page_id) {
            store::with(|s| s.save_blocks_at(page_id, &blocks, now_ms()));
        } else {
            store::with(|s| s.save_block_data(page_id, block_id, &data, now_ms()));
        }
        sync::others(self.win.key, SyncMsg::Blocks(page_id.to_string(), blocks));
    }

    /// Hand a drawing's scene to this window's frames.
    pub fn send_drawing(self, block_id: &str) {
        let data = self.blocks.peek().values().flatten().find(|b| b.id == block_id).map(|b| b.data.clone());
        if let Some(data) = data {
            crate::drawing::mark_shown(self.win.key, block_id, &data);
            document::eval(&crate::drawing::load_script(block_id, &data));
        }
    }

    /// A page dragged from the sidebar onto the tab strip: a tab of its own at
    /// that spot, or paired with the tab it was let go on the middle of.
    pub fn drop_page_on_strip(mut self, page: &str, to: Option<&str>, mode: &str) {
        let route = Route::Page(page.to_string());
        // A tab that shows the page already comes to the front, as at a
        // click on the page (the drop made a second tab of it). Not when
        // it is let go on the middle of another tab: that pairs the two.
        let (shown, pairs) = {
            let ui = self.ui.peek();
            let free = |t: &Tab| t.pane_showing(&route).is_none();
            (!ui.tabs.iter().all(free), mode == "split" && to.is_some_and(|id| ui.tabs.iter().any(|t| t.id == id && free(t))))
        };
        if shown && !pairs {
            return self.open_page(page);
        }
        {
            let pages = self.pages.peek();
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            remember(ui, &pages, &route);
            let target = to.and_then(|id| ui.tabs.iter().position(|t| t.id == id));
            match (mode, target) {
                ("split", Some(ti)) if ui.tabs[ti].pane_showing(&route).is_none() => {
                    ui.tabs[ti].add_tab(Tab::new(route), None, Edge::Right);
                    ui.active = ti;
                }
                ("before", Some(ti)) => {
                    ui.tabs.insert(ti, Tab::new(route));
                    ui.active = ti;
                }
                (_, Some(ti)) => {
                    ui.tabs.insert(ti + 1, Tab::new(route));
                    ui.active = ti + 1;
                }
                (_, None) => {
                    ui.tabs.push(Tab::new(route));
                    ui.active = ui.tabs.len() - 1;
                }
            }
            ui.popover = None;
        }
        self.save_ui();
        self.flash_active_tab();
    }

    /// Give each page of a tab a tab of its own.
    pub fn separate_tab(mut self, index: usize) {
        {
            let mut ui = self.ui.write();
            if !ui.tabs.get(index).is_some_and(Tab::is_split) {
                return;
            }
            let active_id = ui.tab().id.clone();
            let parts = ui.tabs.remove(index).split_apart();
            for (k, t) in parts.into_iter().enumerate() {
                ui.tabs.insert(index + k, t);
            }
            ui.active = ui.tabs.iter().position(|t| t.id == active_id).unwrap_or(index);
            ui.popover = None;
        }
        self.save_ui();
    }

    // ----- split view ------------------------------------------------------

    /// Show a page beside the one in focus (⌥-click, "Open in split view").
    pub fn open_split(mut self, route: Route) {
        {
            let pages = self.pages.peek();
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            ui.popover = None;
            remember(ui, &pages, &route);
            let tab = ui.tab_mut();
            match tab.pane_showing(&route) {
                Some(id) => tab.focus = id,
                None => tab.add_pane(Pane::new(route), None, Edge::Right),
            }
        }
        self.save_ui();
    }

    /// Close one page of a tab that shows several (the focused one if none is named).
    pub fn close_pane(mut self, id: Option<&str>) {
        {
            let mut ui = self.ui.write();
            let tab = ui.tab_mut();
            let id = id.map(str::to_string).unwrap_or_else(|| tab.focus.clone());
            if tab.take_pane(&id).is_none() {
                return;
            }
            ui.popover = None;
        }
        self.save_ui();
    }

    /// `close_pane` for any tab, not only the one in front: one part of a
    /// tab of several pages was middle-clicked, or its page in front went
    /// to a window of its own. The tab keeps its other pages.
    pub fn close_pane_of(mut self, index: usize, id: Option<&str>) {
        {
            let mut ui = self.ui.write();
            let Some(tab) = ui.tabs.get_mut(index) else { return };
            let id = id.map(str::to_string).unwrap_or_else(|| tab.focus.clone());
            if tab.take_pane(&id).is_none() {
                return;
            }
            ui.popover = None;
        }
        self.save_ui();
    }

    /// Give a pane of the tab in front the focus.
    pub fn focus_pane(mut self, id: &str) {
        // Working in a folded pane opens it.
        if self.ui.peek().tab().folded.iter().any(|p| p == id) {
            return self.fold_pane(id, false);
        }
        let change = {
            let ui = self.ui.peek();
            ui.tab().focus != id && ui.tab().pane(id).is_some()
        };
        if change {
            self.ui.write().tab_mut().focus = id.to_string();
        }
    }

    /// Fold a pane of the tab in front to a slim rail, or open it again (at
    /// the size it had). The last open part of a row or column stays open.
    pub fn fold_pane(mut self, pane: &str, on: bool) {
        let changed = {
            let ui = self.ui.peek();
            let tab = ui.tab();
            let folded = tab.folded.iter().any(|p| p == pane);
            tab.pane(pane).is_some() && on != folded && (!on || tab.layout.can_fold(pane, &tab.folded))
        };
        if !changed {
            return;
        }
        {
            let mut ui = self.ui.write();
            let tab = ui.tab_mut();
            let ids = tab.layout.panes();
            tab.folded.retain(|p| ids.contains(p) && p != pane);
            if on {
                tab.folded.push(pane.to_string());
                if tab.focus == pane {
                    let rails = tab.rails();
                    if let Some(open) = ids.iter().find(|p| !rails.contains(p)) {
                        tab.focus = open.clone();
                    }
                }
            } else {
                tab.focus = pane.to_string();
            }
            ui.popover = None;
        }
        self.save_ui();
    }

    /// Whether a pane of the tab in front shows this page.
    pub fn shows_page(&self, page_id: &str) -> bool {
        self.pane_of_page(page_id).is_some()
    }

    /// Open in a new tab at the end (always a new one).
    pub fn open_in_new_tab(mut self, route: Route) {
        if route == Route::Home {
            return self.new_tab();
        }
        // The calendar and the graph have one tab at most too: it comes forward.
        let there = self.ui.peek().tabs.iter().position(|t| view_tab(t) == Some(&route));
        if let Some(i) = there {
            self.select_tab(i);
            self.flash_active_tab();
            return;
        }
        self.clear_slash();
        let made = self.enter_space_of(&route);
        {
            let pages = self.pages.peek();
            let mut ui = self.ui.write();
            remember(&mut ui, &pages, &route);
            if !made {
                ui.tabs.push(Tab::new(route));
            }
            ui.active = ui.tabs.len() - 1;
            ui.popover = None;
            ui.modal = None;
        }
        self.save_ui();
        self.flash_active_tab();
    }

    /// Clicking a page: it gets a tab of its own, so tabs can be moved and
    /// paired freely. If a tab already shows it, that tab comes forward.
    pub fn open_page(self, id: &str) {
        let route = Route::Page(id.to_string());
        self.enter_space_of(&route);
        let found = {
            let ui = self.ui.peek();
            // The tab in front first, then the others.
            let a = ui.active.min(ui.tabs.len() - 1);
            ui.tabs[a]
                .pane_showing(&route)
                .map(|p| (a, p))
                .or_else(|| ui.tabs.iter().enumerate().find_map(|(i, t)| t.pane_showing(&route).map(|p| (i, p))))
        };
        match found {
            Some((i, pane)) => {
                self.select_tab(i);
                self.focus_pane(&pane);
                self.set_modal(None);
            }
            None => self.open_in_new_tab(route),
        }
    }

    /// Briefly light up the tab in front, so you can see where a tab landed.
    fn flash_active_tab(&self) {
        let id = self.ui.peek().tab().id.clone();
        self.flash_tab(&id);
    }

    fn flash_tab(&self, id: &str) {
        let id = serde_json::to_string(id).unwrap_or_default();
        document::eval(&format!("window.__rune && window.__rune.landed({id})"));
    }

    pub fn back(mut self) {
        self.clear_slash();
        {
            let mut ui = self.ui.write();
            let tab = ui.tab_mut();
            if tab.can_back() {
                tab.focused_mut().cursor -= 1;
            }
            ui.popover = None;
        }
        self.save_ui();
    }

    pub fn forward(mut self) {
        self.clear_slash();
        {
            let mut ui = self.ui.write();
            let tab = ui.tab_mut();
            if tab.can_forward() {
                tab.focused_mut().cursor += 1;
            }
            ui.popover = None;
        }
        self.save_ui();
    }

    pub fn new_tab(mut self) {
        // There is one Home tab at most: it comes forward. The read ends
        // before select_tab writes (a guard in the `if let` would live on).
        let home = self.ui.peek().tabs.iter().position(is_home_tab);
        if let Some(i) = home {
            self.select_tab(i);
            self.flash_active_tab();
            return;
        }
        {
            let mut ui = self.ui.write();
            ui.tabs.push(Tab::new(Route::Home));
            ui.active = ui.tabs.len() - 1;
            ui.popover = None;
            // (As a press on a tab: the new tab is what the person asked to see.)
            if ui.modal == Some(Modal::Settings) {
                ui.modal = None;
            }
        }
        self.save_ui();
    }

    pub fn select_tab(mut self, index: usize) {
        self.clear_slash();
        {
            let ui = self.ui.peek();
            // (Settings lies over the page while the row of tabs stays in
            // view: a press on a tab leaves Settings, also on the tab in
            // front. It did nothing there, and on another tab it changed
            // the tab under Settings.)
            if index == ui.active && ui.popover.is_none() && ui.modal != Some(Modal::Settings) {
                return;
            }
        }
        {
            let mut ui = self.ui.write();
            if index < ui.tabs.len() {
                ui.active = index;

                ui.popover = None;
                if ui.modal == Some(Modal::Settings) {
                    ui.modal = None;
                }
            }
        }
        self.save_ui();
    }

    pub fn cycle_tab(self, delta: i64) {
        let (len, active) = {
            let ui = self.ui.peek();
            (ui.tabs.len() as i64, ui.active as i64)
        };
        self.select_tab((active + delta).rem_euclid(len) as usize);
    }

    /// Ctrl+Tab (`by` 1) and Ctrl+Shift+Tab (-1): the tabs in the order
    /// they were used, the last first. One press goes to the tab used before
    /// this one; more presses while Control is held go further along that
    /// order (it stays as it was at the first press). When Control is let
    /// go (`walk_tabs_done`) the tab reached is first in the order.
    pub fn walk_tabs(self, by: i64) {
        let to = {
            let ui = self.ui.peek();
            TAB_USE.with(|u| u.borrow_mut().entry(self.win.key).or_default().walk(&ui.tab().id, &ui.tabs, by))
        };
        let index = to.and_then(|id| self.ui.peek().tabs.iter().position(|t| t.id == id));
        if let Some(i) = index {
            self.select_tab(i);
        }
        self.show_walk();
    }

    /// Control was let go after Ctrl+Tab (or a new Ctrl+Tab begins): the
    /// tab in front comes first in the order of use.
    pub fn walk_tabs_done(self) {
        TAB_USE.with(|u| u.borrow_mut().entry(self.win.key).or_default().walk = None);
        self.note_tab_use();
    }

    /// Escape while Control is held after Ctrl+Tab: the tab that was in
    /// front at the first Tab comes back, and the order is as it was.
    pub fn walk_tabs_cancel(self) {
        let first = TAB_USE.with(|u| u.borrow_mut().get_mut(&self.win.key).and_then(|used| used.walk.take()).and_then(|(list, _)| list.into_iter().next()));
        let index = first.and_then(|id| self.ui.peek().tabs.iter().position(|t| t.id == id));
        match index {
            Some(i) => self.select_tab(i),
            None => self.note_tab_use(),
        }
        self.show_walk();
    }

    /// A press on a row of that list: its tab comes forward, as the one used last.
    pub fn walk_tabs_to(self, id: &str) {
        TAB_USE.with(|u| u.borrow_mut().entry(self.win.key).or_default().walk = None);
        let index = self.ui.peek().tabs.iter().position(|t| t.id == id);
        match index {
            Some(i) => self.select_tab(i),
            None => self.note_tab_use(),
        }
        self.show_walk();
    }

    /// After each change of the tabs (`save_ui`): the tab in front was used.
    fn note_tab_use(&self) {
        {
            let ui = self.ui.peek();
            let every: Vec<&Tab> = ui.tabs.iter().chain(ui.parked.values().flat_map(|set| set.0.iter())).collect();
            TAB_USE.with(|u| u.borrow_mut().entry(self.win.key).or_default().used(&ui.tab().id, &every));
        }
        self.show_walk();
    }

    /// The list of the walk (`TabWalk` in chrome.rs) shows what `TAB_USE`
    /// holds: the walk's order and the tab reached, or nothing.
    fn show_walk(&self) {
        let walk = TAB_USE.with(|u| u.borrow().get(&self.win.key).and_then(|used| used.walk.clone()));
        if *self.tab_walk.peek() == walk {
            return;
        }
        let moved = walk.is_some();
        let mut shown = self.tab_walk;
        shown.set(walk);
        if moved {
            // (Beyond the rows in view, the list follows the mark.)
            document::eval("requestAnimationFrame(() => requestAnimationFrame(() => { const el = document.querySelector('.tab-walk-row.on'); if (el) el.scrollIntoView({ block: 'nearest' }); }))");
        }
    }

    /// A mouse's side button (Back or Forward), or a swipe of two fingers
    /// on the trackpad: Back or Forward in the pane under the pointer
    /// (`pane`), else in the pane in front, as ⌘[ and ⌘] do. Only pages go
    /// back: with Settings or another sheet open nothing happens (the step
    /// would change the page under it, out of sight).
    pub fn step_pane(self, pane: Option<&str>, back: bool) {
        if self.ui.peek().modal.is_some() {
            return;
        }
        if let Some(pane) = pane {
            self.focus_pane(pane);
        }
        if back {
            self.back()
        } else {
            self.forward()
        }
    }

    /// Close a tab as the person asked (⌘W, its ×, its menu, a middle
    /// click): the message says so, and can open the tab again. A pinned tab
    /// stays (nothing happens: no sound, no shake); it closes once it is
    /// unpinned.
    pub fn close_tab(self, index: usize) {
        if self.ui.peek().tabs.get(index).is_some_and(|t| t.pinned) {
            return;
        }
        if let Some(id) = self.take_tab(index) {
            self.toast_back("Tab closed", Back::Tab(id));
        }
    }

    /// Take a tab out of the row, also a pinned one (it goes to a window of
    /// its own); it is kept with the closed tabs. Gives the tab's id, unless
    /// the window closed with it, or nothing went that a person saw (the
    /// only tab, at Home: a new Home tab is in its place).
    pub fn take_tab(mut self, index: usize) -> Option<Id> {
        self.clear_slash();
        let id;
        {
            let mut ui = self.ui.write();
            if index >= ui.tabs.len() {
                return None;
            }
            if ui.tabs.len() == 1 && !self.win.primary && ui.parked.values().all(|set| set.0.is_empty()) {
                drop(ui);
                dioxus::desktop::window().close();
                return None;
            }
            let mut closed = ui.tabs.remove(index);
            closed.pinned = false;
            id = (!ui.tabs.is_empty() || closed.route() != &Route::Home).then(|| closed.id.clone());
            ui.closed.push(closed);
            trim_closed(&mut ui.closed);
            if ui.tabs.is_empty() {
                ui.tabs.push(Tab::new(Route::Home));
            }
            if ui.active > index || ui.active >= ui.tabs.len() {
                ui.active = ui.active.saturating_sub(1).min(ui.tabs.len() - 1);
            }

            ui.popover = None;
        }
        self.save_ui();
        id
    }

    pub fn close_other_tabs(mut self, keep: usize) {
        {
            let mut ui = self.ui.write();
            if keep >= ui.tabs.len() {
                return;
            }
            let others = ui.close_others(keep);
            ui.closed.extend(others);
            trim_closed(&mut ui.closed);

            ui.popover = None;
        }
        self.save_ui();
    }

    /// Pin a tab, or unpin it ("Pin Tab" in the tab's menu).
    pub fn pin_tab(mut self, index: usize, on: bool) {
        {
            let mut ui = self.ui.write();
            ui.pin_tab(index, on);
            ui.popover = None;
        }
        self.save_ui();
    }

    /// "Duplicate Tab": a copy of the tab right after it, in front. Each
    /// page of the copy starts at the place it has in the tab it came from
    /// (the page script keeps places by pane: `placeCopy` in bridge.js).
    pub fn duplicate_tab(mut self, index: usize) {
        let names = {
            let mut ui = self.ui.write();
            ui.popover = None;
            ui.duplicate(index)
        };
        let Some(names) = names else { return };
        self.save_ui();
        for (from, to) in names {
            let (from, to) = (serde_json::to_string(&from).unwrap_or_default(), serde_json::to_string(&to).unwrap_or_default());
            document::eval(&format!("window.__rune && window.__rune.placeCopy && window.__rune.placeCopy({from}, {to})"));
        }
        self.flash_active_tab();
    }

    pub fn reopen_closed_tab(mut self) {
        {
            let pages = self.pages.peek();
            let mut ui = self.ui.write();
            while let Some(tab) = ui.closed.pop() {
                let alive = match tab.route() {
                    Route::Home | Route::Calendar | Route::Graph => true,
                    Route::Page(id) => pages.get(id).is_some_and(|p| !p.trashed),
                };
                if alive {
                    ui.tabs.push(tab);
                    ui.active = ui.tabs.len() - 1;
                    break;
                }
            }
        }
        self.save_ui();
    }

    pub fn toggle_sidebar(mut self) {
        let open = !self.ui.peek().sidebar_open;
        self.ui.write().sidebar_open = open;
        self.save_ui();
    }

    pub fn zoom(mut self, delta: f64) {
        {
            let mut ui = self.ui.write();
            ui.zoom = if delta == 0.0 { 1.0 } else { (ui.zoom + delta).clamp(0.5, 2.0) };
            ui.zoom = (ui.zoom * 10.0).round() / 10.0;
        }
        self.save_ui();
        document::eval("window.__rune && window.__rune.sizeAll()");
    }

    // ----- pages ---------------------------------------------------------

    pub fn create_page(mut self, parent: Option<Id>) -> Id {
        let position = {
            let pages = self.pages.peek();
            match parent.as_deref() {
                Some(_) => children_of(&pages, parent.as_deref()),
                None => roots_in(&pages, &self.ui.peek().space),
            }
            .last()
            .map(|p| p.position + 1.0)
            .unwrap_or(1.0)
        };
        let mut page = Page::new(parent.clone(), position);
        page.space_id = match &parent {
            Some(pid) => self.pages.peek().get(pid).map(|p| p.space_id.clone()).unwrap_or_default(),
            None => self.ui.peek().space.clone(),
        };
        let id = page.id.clone();
        self.pages.write().insert(id.clone(), page);
        self.blocks.write().insert(id.clone(), vec![Block::text("")]);
        self.save_page(&id);
        self.save_blocks(&id);
        if let Some(parent) = parent {
            self.ui.write().expanded.insert(parent);
        }
        id
    }

    /// Move a page, with all the pages inside it, to another place of the
    /// sidebar (see `plan_page_move`). The page it goes into opens, so the
    /// moved page is in view.
    pub fn move_page(self, id: &str, target: Option<&str>, mode: &str) {
        let space = self.ui.peek().space.clone();
        self.move_page_in(&space, id, target, mode);
    }

    /// `move_page`, with "root" meaning the top of the list of `space`.
    fn move_page_in(mut self, space: &str, id: &str, target: Option<&str>, mode: &str) {
        let was = self.pages.peek().get(id).map(|p| p.space_id.clone());
        let Some((parent, position, space)) = plan_page_move(&self.pages.peek(), space, id, target, mode) else { return };
        let inside = descendants(&self.pages.peek(), id);
        let mut changed = vec![id.to_string()];
        {
            let mut pages = self.pages.write();
            if let Some(p) = pages.get_mut(id) {
                // (The page of a row that goes to another page is a page like any other there.)
                if p.parent_id != parent {
                    p.row_of = None;
                }
                p.parent_id = parent.clone();
                p.position = position;
            }
            // The pages inside it go to the same space.
            for pid in &inside {
                if let Some(p) = pages.get_mut(pid).filter(|p| p.space_id != space) {
                    p.space_id = space.clone();
                    if pid != id {
                        changed.push(pid.clone());
                    }
                }
            }
        }
        // Pages with the same place (two copies of one page, say) would make
        // the next move land by age and not where it was let go: give the
        // pages beside this one whole numbers again.
        let beside: Vec<Id> = {
            let pages = self.pages.peek();
            match parent.as_deref() {
                Some(p) => children_of(&pages, Some(p)),
                None => roots_in(&pages, &space),
            }
            .into_iter()
            .map(|p| p.id)
            .collect()
        };
        {
            let mut pages = self.pages.write();
            for (n, pid) in beside.iter().enumerate() {
                if let Some(p) = pages.get_mut(pid).filter(|p| p.position != (n + 1) as f64) {
                    p.position = (n + 1) as f64;
                    if !changed.contains(pid) {
                        changed.push(pid.clone());
                    }
                }
            }
        }
        for pid in &changed {
            self.save_page(pid);
        }
        if let Some(parent) = &parent {
            let opened = self.ui.write().expanded.insert(parent.clone());
            if opened {
                self.save_ui();
            }
            let name = self.page_title(parent);
            self.toast(format!("Moved into {name}"));
        } else if was.as_deref() != Some(space.as_str()) {
            if let Some(name) = self.spaces.peek().iter().find(|s| s.id == space).map(|s| s.display_name().to_string()) {
                self.toast(format!("Moved to {name}"));
            }
        }
    }

    /// New page: create, open it and put the caret in its title.
    pub fn new_page(self, parent: Option<Id>) {
        let id = self.create_page(parent);
        self.open_in_new_tab(Route::Page(id.clone()));
        self.focus_title(&id);
    }

    /// A new page by name, from search: "Plan" makes the page Plan; "Work/
    /// Plan" makes Plan inside the page Work (made too if there is none).
    /// The page opens with the caret on its first line.
    pub fn new_page_at_path(self, typed: &str) {
        let parts = path_parts(typed);
        let Some((name, above)) = parts.split_last() else { return };
        let (mut parent, missing) = {
            let pages = self.pages.peek();
            find_path(&pages, &self.ui.peek().space, above)
        };
        for title in missing.iter().chain(std::iter::once(name)) {
            let id = self.create_page(parent.clone());
            self.rename_page(&id, title.clone());
            parent = Some(id);
        }
        let Some(id) = parent else { return };
        self.open_in_new_tab(Route::Page(id.clone()));
        if let Some(first) = self.blocks.peek().get(&id).and_then(|bs| bs.first()).map(|b| b.id.clone()) {
            self.focus(Caret::end(&first));
        }
    }

    /// Focus mode on or off (⌘., Escape leaves it). The page script keeps
    /// the line being written near the middle while it is on.
    pub fn toggle_focus_mode(mut self) {
        let on = {
            let mut ui = self.ui.write();
            ui.focus_mode = !ui.focus_mode;
            ui.focus_mode
        };
        document::eval(&format!("window.__rune && window.__rune.focusMode({on})"));
        if on {
            self.toast("Focus mode. Esc leaves it.");
        }
    }

    pub fn rename_page(mut self, id: &str, title: String) {
        self.keep_version(id, None, false);
        let old = self.page_title(id);
        if let Some(p) = self.pages.write().get_mut(id) {
            p.title = title;
            p.updated_at = now_ms();
        }
        self.save_page(id);
        // Links to the page that carry its old name take the new one.
        let new = self.page_title(id);
        if old != new {
            let urls = [inline::page_url(id), format!("{}{id}", inline::OLD_PAGE_URL)];
            let renamed: Vec<(Id, Vec<Block>)> = self
                .blocks
                .peek()
                .iter()
                // Only the pages that hold such a link are looked at closely.
                .filter(|(_, bs)| bs.iter().any(|b| urls.iter().any(|u| b.text.contains(u.as_str()))))
                .filter_map(|(page, bs)| {
                    let mut out = bs.clone();
                    let mut changed = false;
                    for b in out.iter_mut().filter(|b| b.kind != BlockKind::Code) {
                        for url in &urls {
                            if let Some(text) = inline::retitle_links(&b.text, url, &old, &new) {
                                b.text = text;
                                changed = true;
                            }
                        }
                    }
                    changed.then(|| (page.clone(), out))
                })
                .collect();
            for (page, bs) in renamed {
                self.set_blocks(&page, bs, true);
            }
        }
        // The name of a page that is a row is the first cell of its row.
        self.name_row_cell(id);
    }

    /// Set a page's icon. Leaves the picker open so a colour can follow.
    pub fn set_page_icon(mut self, id: &str, icon: Option<String>) {
        let removed = icon.is_none();
        if let Some(p) = self.pages.write().get_mut(id) {
            p.icon = icon;
        }
        self.save_page(id);
        if removed {
            self.set_popover(None);
        }
    }

    pub fn toggle_favorite(mut self, id: &str) {
        let fav = {
            let mut pages = self.pages.write();
            let Some(p) = pages.get_mut(id) else { return };
            p.favorite = !p.favorite;
            p.favorite
        };
        self.save_page(id);
        self.set_popover(None);
        self.toast(if fav { "Added to Favorites" } else { "Removed from Favorites" });
    }

    pub fn duplicate_page(mut self, id: &str) {
        // (A copy would be plain, or sealed to the page it came from.)
        if self.locked_lines(id) {
            self.set_popover(None);
            return self.toast("A locked page cannot be copied");
        }
        let Some(original) = self.pages.peek().get(id).cloned() else { return };
        let mut copy = Page::new(original.parent_id.clone(), original.position + 0.5);
        copy.space_id = original.space_id.clone();
        copy.title = format!("{} (copy)", original.display_title());
        copy.icon = original.icon;
        // (A copy came out bare: its cover, the place of the cover and the
        // face of its title are part of the page.)
        copy.cover = original.cover;
        copy.cover_y = original.cover_y;
        copy.font = original.font;
        let blocks: Vec<Block> = self
            .blocks
            .peek()
            .get(id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut b| {
                b.id = new_id();
                b
            })
            .collect();
        let new_id = copy.id.clone();
        self.pages.write().insert(new_id.clone(), copy);
        self.blocks.write().insert(new_id.clone(), blocks);
        self.save_page(&new_id);
        self.save_blocks(&new_id);
        self.set_popover(None);
        self.open_page(&new_id);
    }

    pub fn trash_page(self, id: &str) {
        let tab = {
            let ui = self.ui.peek();
            let tab = ui.tab();
            (tab.panes.len() == 1 && tab.route() == &Route::Page(id.to_string())).then(|| tab.id.clone())
        };
        self.set_trashed(id, true);
        self.set_popover(None);
        self.toast_back("Moved to Trash", Back::Page { id: id.to_string(), tab });
    }

    pub fn restore_page(self, id: &str) {
        self.take_out_of_trash(id);
        self.toast("Page restored");
    }

    /// A page comes out of the Trash ("Restore" there, and the "Undo" of
    /// the message that said it went).
    fn take_out_of_trash(self, id: &str) {
        self.set_trashed(id, false);
        self.unrow_if_lost(id);
    }

    /// Move a page and everything under it to the Trash, or back.
    fn set_trashed(mut self, id: &str, trashed: bool) {
        let ids = descendants(&self.pages.peek(), id);
        {
            let mut pages = self.pages.write();
            let parent_gone = pages
                .get(id)
                .and_then(|p| p.parent_id.clone())
                .is_some_and(|parent| pages.get(&parent).is_none_or(|pp| pp.trashed));
            for pid in &ids {
                if let Some(p) = pages.get_mut(pid) {
                    p.trashed = trashed;
                }
            }
            if !trashed && parent_gone {
                if let Some(p) = pages.get_mut(id) {
                    p.parent_id = None;
                    p.row_of = None;
                }
            }
        }
        for pid in &ids {
            self.save_page(pid);
        }
        if trashed {
            self.stop_hearing(&ids);
            self.leave_pages(&ids);
        }
    }

    /// A page that is listened to goes to the Trash, or is deleted: the
    /// listening stops at once. Nothing on the screen would say any more
    /// that the app listens, and there is no page to write in.
    fn stop_hearing(mut self, ids: &[Id]) {
        if !self.listening.peek().as_ref().is_some_and(|l| ids.contains(&l.page)) {
            return;
        }
        if let Some(l) = self.listening.write().as_mut() {
            l.stopping = true;
        }
        LISTENER.with(|l| {
            if let Some(listener) = l.borrow_mut().as_mut() {
                listener.end();
            }
        });
    }

    /// Tabs showing pages that are gone go back to Home.
    fn leave_pages(mut self, ids: &[Id]) {
        self.ui.write().leave_pages(ids);
        self.save_ui();
    }

    pub fn delete_forever(mut self, id: &str) {
        let ids = descendants(&self.pages.peek(), id);
        self.stop_hearing(&ids);
        {
            let mut pages = self.pages.write();
            let mut blocks = self.blocks.write();
            for pid in &ids {
                pages.remove(pid);
                blocks.remove(pid);
            }
        }
        store::with(|s| s.delete_pages(&ids));
        sync::others(self.win.key, SyncMsg::Removed(ids.clone()));
        {
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            for tab in ui.tabs.iter_mut().chain(ui.closed.iter_mut()).chain(ui.parked.values_mut().flat_map(|set| set.0.iter_mut())) {
                for pane in tab.panes.iter_mut() {
                    pane.forget(&ids);
                }
            }
        }
        self.save_ui();
    }

    // ----- blocks --------------------------------------------------------

    /// Change a page's blocks, then save, record an undo step, and push any
    /// text the page doesn't show yet.
    fn edit_blocks<R>(self, page_id: &str, f: impl FnOnce(&mut Vec<Block>) -> R) -> R {
        self.edit_blocks_from(page_id, Origin::Other, f)
    }

    fn edit_blocks_from<R>(mut self, page_id: &str, origin: Origin, f: impl FnOnce(&mut Vec<Block>) -> R) -> R {
        let mut before = self.blocks.peek().get(page_id).cloned().unwrap_or_default();
        // Another program (an AI tool through the connector) wrote to the
        // workspace since this window last looked: take the page as it is on
        // disk first, so this edit is made on top of that and does not write
        // the older page back over it. The step that Undo takes back begins
        // at that page too: begun at the older one, one Undo also took away
        // what the other program had just added.
        let mut on_screen = None;
        if sync::outside_change_pending() {
            if let Some(fresh) = store::with(|s| s.load_page_blocks(page_id)).filter(|fresh| !fresh.is_empty()) {
                self.blocks.write().insert(page_id.to_string(), fresh.clone());
                on_screen = Some(std::mem::replace(&mut before, fresh));
            }
        }
        // A locked page that is not open is one sealed line, and takes no
        // line (that of a meeting whose notes are kept in it, say). The change is
        // made on a copy, for what the caller wants back, and dropped. Made
        // in the window, the store refused it but the window kept it: the
        // page showed a "Locked page" line with the new lines under it and
        // no password box, until the app was started again.
        if crate::lock::sealed(&before).is_some() {
            let mut copy = before.clone();
            let result = f(&mut copy);
            if copy != before {
                self.tell_locked(page_id);
            }
            return result;
        }
        let (result, after) = {
            let mut all = self.blocks.write();
            let blocks = all.entry(page_id.to_string()).or_default();
            let r = f(blocks);
            ensure_not_empty(blocks);
            (r, blocks.clone())
        };
        if before == after {
            return result;
        }
        self.drop_dead_folds(page_id, &after);
        // (What the window showed is the page of before the other program's change.)
        self.show_newly_hidden(page_id, on_screen.as_ref().unwrap_or(&before), &after);
        self.keep_version(page_id, Some(&before), false);
        // Nothing but this one block changed, and only its words (for a
        // table, its cells too): one row of the notes file is written, not
        // the page.
        let only_text = match origin {
            Origin::Typing { block, .. } | Origin::Cell { block, .. } => {
                let cell = matches!(origin, Origin::Cell { .. });
                let bare = |b: &Block| Block { text: String::new(), data: if cell { String::new() } else { b.data.clone() }, ..b.clone() };
                before.len() == after.len()
                    && before.iter().zip(&after).all(|(a, b)| if a.id == block { bare(a) == bare(b) } else { a == b })
            }
            Origin::Other => false,
        };
        match origin {
            Origin::Typing { block, .. } if only_text => self.save_block_text(page_id, block),
            Origin::Cell { block, .. } if only_text => self.save_block_cells(page_id, block),
            _ => self.save_blocks(page_id),
        }
        let typing = match origin {
            Origin::Typing { block, .. } => Some(block),
            Origin::Cell { group, .. } => Some(group),
            Origin::Other => None,
        };
        // Blocks whose text on screen is now out of date (the block being
        // typed in already shows its text).
        let changed: Vec<Id> = if only_text {
            Vec::new()
        } else {
            // (What the window shows is the page of before the other program's change.)
            let shown_before: HashMap<&str, &str> = on_screen.as_ref().unwrap_or(&before).iter().map(|o| (o.id.as_str(), o.text.as_str())).collect();
            after
                .iter()
                .filter(|b| {
                    let shown = match origin {
                        Origin::Typing { block, text } if block == b.id => Some(text),
                        _ => shown_before.get(b.id.as_str()).copied(),
                    };
                    shown.is_some_and(|t| t != b.text)
                })
                .map(|b| b.id.clone())
                .collect()
        };
        // Rows or a table that went with this change take the pages of
        // their rows along. (Typing takes no row out.)
        if origin == Origin::Other {
            self.drop_lost_row_pages(&before, &after);
        }
        {
            let mut h = self.history.write();
            if typing.is_none() {
                h.seal();
            }
            h.record(page_id, before, after, typing, now_ms());
            if typing.is_none() {
                h.seal();
            }
        }
        self.sync_blocks(page_id, &changed);
        result
    }

    // ----- updates (see update.rs) -----------------------------------------

    /// Look for a newer build now. `say` tells the person the answer.
    pub fn check_update(mut self, say: bool) {
        spawn_forever(async move {
            // ("Check now" asks the published releases at once; by itself the app asks once a day.)
            let (found, problem) = crate::update::check(say).await;
            if *self.update.peek() != found {
                self.update.set(found.clone());
            }
            if say {
                match (found, problem) {
                    (Some(u), _) => self.toast(format!("A newer build is ready ({})", crate::update::build_label(u.build))),
                    (None, Some(why)) => self.notice(format!("Rune could not look for a newer version: {why}.")),
                    (None, None) => self.toast("Rune is up to date"),
                }
            }
        });
    }

    /// Install the build that is ready, then start the app again.
    pub fn install_update(self) {
        let Some(update) = self.update.peek().clone() else { return };
        // (The update starts the app again at once: not in the middle of a
        // meeting that is listened to, or of an AI's work on the notes.)
        // (In any window: the listening of another window was not seen,
        // and the restart lost its last words and its notes.)
        if self.listening.peek().is_some() || listener_runs() {
            return self.toast("Rune is listening to a meeting. Stop listening first, then update.");
        }
        if self.ai.peek().as_ref().is_some_and(|s| s.status == AiStatus::Streaming) {
            return self.toast("The AI is at work. Update when it is done.");
        }
        // (One install at a time: a second press while the first copies the
        // app took its files away.)
        if UPDATING.with(|u| u.replace(true)) {
            return;
        }
        self.toast("Updating…");
        spawn_forever(async move {
            match crate::update::install(update).await {
                Ok((app, build)) => crate::update::relaunch(&app, build),
                Err(e) => {
                    UPDATING.with(|u| u.set(false));
                    self.toast(format!("Couldn't update: {e}"))
                }
            }
        });
    }

    // ----- a page from speech (see VoiceModal, dictation.rs) ---------------

    /// Tell the page script whether there is an AI to ask (it hides what
    /// needs one, and words the microphones to match).
    pub fn show_ai_state(self) {
        let on = ai::shown();
        let mut ai_on = self.ai_on;
        if *ai_on.peek() != on {
            ai_on.set(on);
        }
        // Kept for the next time the app opens (only what it knows for certain).
        if ai::local::installed_cached().is_some() && std::env::var_os("RUNE_SELFTEST").is_none() {
            let found = if on { "1" } else { "0" };
            if store::setting(ai::FOUND_SETTING).as_deref() != Some(found) {
                store::set_setting(ai::FOUND_SETTING, found);
            }
        }
        document::eval(&format!("document.body.dataset.ai = '{}'", if on { 1 } else { 0 }));
        if swipe_trace_on() {
            document::eval("document.body.dataset.swipeLog = '1'");
        }
    }

    /// Plain voice: have the Mac's Dictation type what is said into the text
    /// box or line that has the caret. `mark` says where, for the page script.
    fn dictate(self, mark: &str) {
        document::eval(&format!("document.body.dataset.dictating = {}", serde_json::to_string(mark).unwrap_or_default()));
        if std::env::var_os("RUNE_SELFTEST").is_none() {
            crate::dictation::start();
        }
    }

    /// A microphone beside a text box was pressed. The box has the caret and
    /// the page has noted which box it is. With the app's own listener the
    /// words go into the box as they are heard, until the microphone is
    /// pressed again, the speaker pauses, or the box is left; otherwise the
    /// Mac's Dictation types them.
    fn dictate_box(self) {
        if !crate::listen::own_ears() {
            if crate::dictation::allowed() {
                let started = crate::dictation::start();
                if std::env::var_os("RUNE_SELFTEST").is_some() {
                    document::eval(&format!("document.body.dataset.dictationAsked = '{started}'"));
                }
            }
            return;
        }
        // Pressed again while it listens: that is "stop".
        if BOX_EARS.with(|e| e.borrow().is_some()) {
            Self::stop_box_ears();
            return;
        }
        let (listener, mut heard) = match crate::listen::start(false) {
            Ok(started) => started,
            Err(message) => {
                document::eval("window.__rune && window.__rune.hearEnd()");
                return self.notice(message);
            }
        };
        BOX_EARS.with(|e| *e.borrow_mut() = Some(listener));
        document::eval("window.__rune && window.__rune.hearStart()");
        spawn_forever(async move {
            use crate::listen::Heard;
            use futures_util::future::{select, Either};
            let join = |said: &str, more: &str| if said.is_empty() { more.to_string() } else { format!("{said} {more}") };
            let show = |text: &str| {
                document::eval(&format!("window.__rune && window.__rune.hear({})", serde_json::to_string(text).unwrap_or_default()));
            };
            let mut said = String::new();
            let mut any = false;
            let mut last = now_ms();
            loop {
                let event = match select(heard.next(), Box::pin(ai::sleep_ms(300))).await {
                    Either::Left((event, _)) => event,
                    Either::Right(_) => {
                        // It stops by itself: a pause after speech, or a
                        // while with nothing heard at all.
                        let quiet = now_ms() - last;
                        if (any && quiet > 2_500) || (!any && quiet > 10_000) {
                            Self::stop_box_ears();
                            last = now_ms() + 60_000;
                        }
                        continue;
                    }
                };
                match event {
                    Some(Heard::Partial { text, .. }) => {
                        any = true;
                        last = last.max(now_ms());
                        show(&join(&said, &text));
                    }
                    Some(Heard::Final { text, .. }) => {
                        any = true;
                        last = last.max(now_ms());
                        said = join(&said, &text);
                        show(&said);
                    }
                    Some(Heard::Listening { .. }) => last = last.max(now_ms()),
                    Some(Heard::Note(message)) => self.notice(message),
                    Some(Heard::Error(message)) => {
                        self.notice(message);
                        break;
                    }
                    Some(Heard::Done) | None => break,
                }
            }
            BOX_EARS.with(|e| *e.borrow_mut() = None);
            document::eval("window.__rune && window.__rune.hearEnd()");
        });
    }

    /// Ask the listener of a text box to finish (its last words still arrive).
    fn stop_box_ears() {
        BOX_EARS.with(|e| {
            if let Some(listener) = e.borrow_mut().as_mut() {
                listener.stop();
            }
        });
    }

    /// Plain voice for a new page (no AI tool to write it): a new page with
    /// the caret in its first line, and Dictation typing what is said there.
    fn plain_voice_page(mut self) {
        let id = self.create_page(None);
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = format!("Voice note – {}", when::date_of(now_ms()).format("%b %-d, %Y"));
            p.icon = Some("note:purple".into());
        }
        self.save_page(&id);
        let first = self.blocks.peek().get(&id).and_then(|b| b.first()).map(|b| b.id.clone()).unwrap_or_default();
        self.open_in_new_tab(Route::Page(id));
        self.focus(Caret::start(&first));
        self.toast("Speak: your words are typed as you say them");
        spawn_forever(async move {
            // Once the line has the caret.
            ai::sleep_ms(350).await;
            self.dictate("page");
        });
    }

    /// Open the voice box for `what` (the AI does the writing; with no AI
    /// tool this says so instead). Something that no longer exists (a block
    /// that went, say) makes it a box for a new page.
    pub fn open_voice(mut self, what: VoiceFor) {
        if !ai::has_ai() {
            return;
        }
        // The AI does this writing. On a locked page (also an open one) it
        // would be given the picked words, or the words meant for the page.
        if let VoiceFor::Into { page, .. } | VoiceFor::Edit { page, .. } = &what {
            if self.locked_lines(page) {
                return self.toast("The AI does not read locked pages");
            }
        }
        let has_block = |page: &Id, block: &Id| self.blocks.peek().get(page).is_some_and(|bs| bs.iter().any(|b| &b.id == block));
        let ok = match &what {
            VoiceFor::NewPage => true,
            VoiceFor::Into { page, block } => has_block(page, block),
            VoiceFor::Edit { page, block, start, end } => end > start && has_block(page, block),
        };
        self.ui.write().voice = if ok { what } else { VoiceFor::NewPage };
        self.set_modal(Some(Modal::Voice));
    }

    /// The speaker has finished: do what the voice box was opened for.
    pub fn finish_voice(self, spoken: String) {
        let what = self.ui.peek().voice.clone();
        match what {
            VoiceFor::NewPage => self.page_from_speech(spoken),
            VoiceFor::Into { page, block } => self.speech_into_page(&page, &block, spoken),
            VoiceFor::Edit { page, block, start, end } => self.speech_edit(&page, &block, start, end, spoken),
        }
    }

    /// Write what was said into a page, at `block`: in its place when it is
    /// an empty line, else after it. The AI tidies the words; they arrive in
    /// the page as it writes, and the whole thing is one step to undo. A
    /// request for the calendar becomes an event instead.
    pub fn speech_into_page(mut self, page: &str, block: &str, spoken: String) {
        let spoken = spoken.trim().to_string();
        let before = self.blocks.peek().get(page).cloned().unwrap_or_default();
        if spoken.is_empty() || !before.iter().any(|b| b.id == block) {
            return;
        }
        self.set_modal(None);
        let (page, anchor) = (page.to_string(), block.to_string());
        let title = self.page_title(&page);
        let messages = vec![
            ai::ChatMessage::system(format!("{}\n\n{}", ai::SPEECH_INTO_PAGE, voice::calendar_rule(now_ms()))),
            ai::ChatMessage::user(format!("The page is called \"{title}\". The spoken note:\n\n{spoken}")),
        ];
        let mut rx = ai::start(ai::Prefs::load(), messages);
        spawn_forever(async move {
            let mut reply = String::new();
            let mut written: Vec<Id> = Vec::new();
            let mut shown_at = 0;
            let mut error = None;
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(text) => {
                        reply.push_str(&text);
                        if now_ms() - shown_at > 200 && !voice::may_be_directives(&reply) {
                            shown_at = now_ms();
                            self.show_spoken_blocks(&page, &anchor, &mut written, &reply, None);
                        }
                    }
                    AiEvent::Done => break,
                    AiEvent::Error(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            // Asked for the calendar: make the events; tasks go in as to-dos here.
            let mut reply = reply;
            if let Some(asked) = voice::directives(&ai::clean_reply(&reply).0) {
                let tasks: Vec<String> = asked
                    .iter()
                    .filter_map(|d| match d {
                        Directive::Todo(t) => Some(format!("- [ ] {t}")),
                        Directive::Event(_) => None,
                    })
                    .collect();
                self.add_spoken_events(&asked, false);
                if tasks.is_empty() {
                    return;
                }
                reply = tasks.join("\n");
            }
            self.show_spoken_blocks(&page, &anchor, &mut written, &reply, Some(&spoken));
            let after = self.blocks.peek().get(&page).cloned().unwrap_or_default();
            if after != before {
                self.keep_version(&page, Some(&before), false);
                {
                    let mut h = self.history.write();
                    h.seal();
                    h.record(&page, before, after, None, now_ms());
                    h.seal();
                }
                self.save_blocks(&page);
            }
            if let Some(last) = written.last() {
                self.focus(Caret::end(last));
            }
            match error {
                Some(e) if ai::clean_reply(&reply).0.trim().is_empty() => self.toast(format!("Your words are in the page as spoken. {e}")),
                Some(_) => self.toast("The AI stopped early"),
                None => {}
            }
        });
    }

    /// Draw what the AI has written from speech so far into the page:
    /// `written` are the blocks it put there before (replaced each time, and
    /// kept by id so only new text is redrawn). At the end `spoken` is
    /// given, and is what goes in if the AI wrote nothing.
    fn show_spoken_blocks(self, page: &str, anchor: &str, written: &mut Vec<Id>, reply: &str, spoken: Option<&str>) {
        let Some(mut blocks) = self.blocks.peek().get(page).cloned() else { return };
        let (text, _) = ai::clean_reply(reply);
        let text = text.trim_start();
        // A title line is not wanted here; drop one if the AI wrote it.
        let body = match text.strip_prefix("# ") {
            Some(rest) => rest.split_once('\n').map(|(_, body)| body).unwrap_or(""),
            None if text == "#" => "",
            None => text,
        };
        let mut new = markdown::to_blocks(body);
        if new.is_empty() {
            match spoken {
                Some(spoken) => new.push(Block::text(inline::escape(spoken))),
                None => return,
            }
        }
        // Where it goes: where it went before; else in place of the empty
        // line it was asked for on; else after that line.
        let at = match written.first().and_then(|first| index_of(&blocks, first)) {
            Some(i) => i,
            None => match index_of(&blocks, anchor) {
                Some(i) if blocks[i].kind == BlockKind::Text && blocks[i].text.trim().is_empty() => {
                    written.push(anchor.to_string());
                    i
                }
                Some(i) => i + 1,
                None => blocks.len(),
            },
        };
        let old: Vec<Block> = written.iter().filter_map(|id| blocks.iter().find(|b| &b.id == id).cloned()).collect();
        for (n, o) in new.iter_mut().zip(&old) {
            if n.kind != o.kind {
                break;
            }
            n.id = o.id.clone();
        }
        blocks.retain(|b| !written.contains(&b.id));
        let at = at.min(blocks.len());
        *written = new.iter().map(|b| b.id.clone()).collect();
        blocks.splice(at..at, new);
        self.set_blocks(page, blocks, false);
    }

    /// Make a page from what was said: the AI gives it a title and a shape,
    /// and it is written into the new page as it arrives. The words as
    /// spoken are kept at the end, under a toggle. When what was said is for
    /// the calendar, or is only a task or two, no page is made: the events
    /// go on the calendar and the tasks on the To-dos page.
    pub fn page_from_speech(self, spoken: String) {
        let spoken = spoken.trim().to_string();
        if spoken.is_empty() {
            return;
        }
        self.set_modal(None);
        self.toast("Writing…");
        let system = format!("{}\n\n{}\n\n{}", ai::SPEECH_TO_PAGE, voice::calendar_rule(now_ms()), voice::TODO_RULE);
        let messages = vec![ai::ChatMessage::system(system), ai::ChatMessage::user(spoken.clone())];
        let mut rx = ai::start(ai::Prefs::load(), messages);
        spawn_forever(async move {
            let mut reply = String::new();
            let mut page: Option<Id> = None;
            let mut shown_at = 0;
            let mut error = None;
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(text) => {
                        reply.push_str(&text);
                        // The page is made when the first text for it is here.
                        let text_has_begun = !reply.trim().is_empty() && !voice::may_be_directives(&reply);
                        if text_has_begun && now_ms() - shown_at > 200 {
                            shown_at = now_ms();
                            let id = page.get_or_insert_with(|| self.new_voice_page()).clone();
                            self.show_spoken_page(&id, &reply, None);
                        }
                    }
                    AiEvent::Done => break,
                    AiEvent::Error(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            if let Some(asked) = voice::directives(&ai::clean_reply(&reply).0) {
                self.add_spoken_events(&asked, true);
                return;
            }
            let id = page.unwrap_or_else(|| self.new_voice_page());
            let wrote = !ai::clean_reply(&reply).0.trim().is_empty();
            self.show_spoken_page(&id, &reply, Some(&spoken));
            self.save_blocks(&id);
            match error {
                Some(e) if !wrote => self.toast(format!("The page holds your words as spoken. {e}")),
                Some(_) => self.toast("The AI stopped early; your words are at the end of the page"),
                None => self.toast("Your page is ready"),
            }
        });
    }

    /// The page a spoken note is written into, opened in a tab of its own.
    fn new_voice_page(mut self) -> Id {
        let id = self.create_page(None);
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = format!("Voice note – {}", when::date_of(now_ms()).format("%b %-d, %Y"));
            p.icon = Some("note:purple".into());
        }
        self.save_page(&id);
        self.blocks.write().insert(id.clone(), vec![Block::text("*Writing your page…*")]);
        self.open_in_new_tab(Route::Page(id.clone()));
        id
    }

    /// Do what the speaker asked for instead of text: events go on the
    /// calendar. With `todos`, tasks go on the To-dos page (when a page is
    /// open they are written into it instead, by the caller). Says what was
    /// added, and shows it.
    fn add_spoken_events(mut self, asked: &[Directive], todos: bool) {
        let space = self.ui.peek().space.clone();
        let mut events: Vec<(Id, String)> = Vec::new();
        let mut tasks: Vec<String> = Vec::new();
        for d in asked {
            match d {
                Directive::Event(e) => {
                    let mut event = Event::new(space.clone(), e.title.clone(), e.start, e.end, e.all_day);
                    event.notes = e.notes.clone();
                    event.repeat = e.repeat.clone();
                    event.remind = crate::notify::default_reminder();
                    let id = event.id.clone();
                    self.events.write().insert(id.clone(), event);
                    self.save_event(&id);
                    events.push((id, voice::event_summary(e)));
                }
                Directive::Todo(t) if todos => tasks.push(t.clone()),
                Directive::Todo(_) => {}
            }
        }
        let mut todo_page = None;
        if !tasks.is_empty() {
            let page = self.todos_page();
            let new: Vec<Block> = tasks.iter().map(|t| Block::new(BlockKind::Todo, inline::escape(t))).collect();
            self.edit_blocks(&page, move |bs| {
                if bs.len() == 1 && bs[0].kind == BlockKind::Text && bs[0].text.trim().is_empty() {
                    bs.clear();
                }
                bs.extend(new);
            });
            todo_page = Some(page);
        }
        // Show what was added: the calendar on the first event, else To-dos.
        if let Some((first, _)) = events.first() {
            self.show_event(first);
        } else if let Some(page) = &todo_page {
            self.open_page(page);
        }
        let said = match (events.len(), tasks.len()) {
            (1, 0) => format!("On your calendar: {}", events[0].1),
            (n, 0) if n > 1 => format!("{n} events are on your calendar"),
            (0, 1) => "Added to To-dos".to_string(),
            (0, n) if n > 1 => format!("{n} tasks added to To-dos"),
            (0, 0) => return,
            (n, m) => format!("{n} on your calendar, {m} in To-dos"),
        };
        self.toast(said);
    }

    /// The page in this space that spoken tasks go on; made the first time.
    fn todos_page(mut self) -> Id {
        let key = format!("todos_page:{}", self.ui.peek().space);
        if let Some(id) = store::setting(&key).filter(|id| self.pages.peek().get(id).is_some_and(|p| !p.trashed)) {
            return id;
        }
        let id = self.create_page(None);
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = "To-dos".into();
            p.icon = Some("check-circle:green".into());
        }
        self.save_page(&id);
        store::set_setting(&key, &id);
        id
    }

    /// Speak to edit: change the selected text of a block the way the
    /// speaker said. One step to undo.
    pub fn speech_edit(self, page: &str, block: &str, start: usize, end: usize, instruction: String) {
        let instruction = instruction.trim().to_string();
        if self.locked_lines(page) {
            return self.toast("The AI does not read locked pages");
        }
        let Some(target) = self.blocks.peek().get(page).and_then(|bs| bs.iter().find(|b| b.id == block).cloned()) else { return };
        let selected = voice::selected_text(&target, start, end);
        if instruction.is_empty() || selected.trim().is_empty() {
            return;
        }
        self.set_modal(None);
        self.toast("Changing the text…");
        let messages = vec![
            ai::ChatMessage::system(voice::EDIT),
            ai::ChatMessage::user(format!("The text:\n\n{selected}\n\nWhat to do with it: {instruction}")),
        ];
        let mut rx = ai::start(ai::Prefs::load(), messages);
        let (page, block) = (page.to_string(), block.to_string());
        spawn_forever(async move {
            let mut reply = String::new();
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(text) => reply.push_str(&text),
                    AiEvent::Done => break,
                    AiEvent::Error(e) => return self.toast(format!("Couldn't change the text. {e}")),
                }
            }
            let new = markdown::to_blocks(ai::clean_reply(&reply).0.trim());
            // Only if the line is still as it was when the change was asked for.
            let same = self.blocks.peek().get(&page).and_then(|bs| bs.iter().find(|b| b.id == block).map(|b| b.text == target.text));
            if same != Some(true) {
                return self.toast("The text changed in the meantime, so nothing was replaced");
            }
            let target_block = block.clone();
            match self.edit_blocks(&page, move |bs| voice::replace_selection(bs, &target_block, start, end, new)) {
                Some(last) => self.focus(Caret::end(&last)),
                None => self.toast("The AI returned nothing to put in"),
            }
        });
    }

    /// A spoken recap of a meeting: the AI writes it as notes and action
    /// items, which go under those headings of the meeting's page.
    pub fn speech_recap(self, page: &str, spoken: String) {
        let spoken = spoken.trim().to_string();
        if spoken.is_empty() || !self.pages.peek().contains_key(page) {
            return;
        }
        self.set_modal(None);
        self.toast("Writing the recap…");
        let messages = vec![ai::ChatMessage::system(voice::RECAP), ai::ChatMessage::user(format!("{}\n\n{spoken}", when::today_for_ai()))];
        let mut rx = ai::start(ai::Prefs::load(), messages);
        let page = page.to_string();
        spawn_forever(async move {
            let mut reply = String::new();
            let mut error = None;
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(text) => reply.push_str(&text),
                    AiEvent::Done => break,
                    AiEvent::Error(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            let mut recap = markdown::to_blocks(ai::clean_reply(&reply).0.trim());
            if recap.iter().all(|b| b.text.trim().is_empty()) {
                // Nothing from the AI: the words as spoken are the note.
                recap = vec![Block::new(BlockKind::Bulleted, inline::escape(&spoken))];
            }
            let added = self.edit_blocks(&page, move |bs| voice::fill_recap(bs, recap));
            match (error, added) {
                (Some(e), _) => self.toast(format!("Your recap is in the page as spoken. {e}")),
                (None, 0) => self.toast("The AI found nothing to add"),
                (None, _) => self.toast("The recap is in the page"),
            }
        });
    }

    /// Whether a page is one of the meeting notes pages (under "Meeting notes").
    pub fn is_meeting_page(&self, id: &str) -> bool {
        if meeting_page_ids().iter().any(|p| p == id) {
            return true;
        }
        // Ones made before the list was kept: in the "Meeting notes" page.
        let pages = self.pages.peek();
        let Some(page) = pages.get(id) else { return false };
        page.icon.as_deref().is_some_and(|i| i.starts_with("calendar:"))
            && page.parent_id.as_ref().and_then(|p| pages.get(p)).is_some_and(|p| p.title == "Meeting notes")
    }

    // ----- listening to a meeting (see listen.rs) --------------------------

    /// Listen to the meeting whose notes page is `page`: the microphone and
    /// the Mac's sound are turned into a transcript, kept in the page under
    /// a "Transcript" toggle as it grows. When it stops, the AI writes the
    /// notes and action items from it.
    pub fn start_listening(mut self, page: &str) {
        if self.listening.peek().is_some() {
            return self.toast("Already listening");
        }
        // (The transcript is kept in the page, and a locked page that is not
        // open takes no lines.)
        if self.blocks.peek().get(page).is_some_and(|bs| crate::lock::sealed(bs).is_some()) {
            return self.notice("Open the page with its password first: the transcript is kept in it.");
        }
        // (One listener for the app: a second one, started in another
        // window, ended the first, and then both windows stopped.)
        if listener_runs() {
            return self.toast("Rune is listening in another window. Stop it there first.");
        }
        self.confirm_meeting(page);
        self.listen_offer.set(None);
        let (listener, mut heard) = match crate::listen::start(true) {
            Ok(started) => started,
            Err(e) => {
                self.pad_problem(page, &e);
                return self.notice(e);
            }
        };
        LISTENER.with(|l| *l.borrow_mut() = Some(listener));
        LISTENER_WINDOW.with(|w| *w.borrow_mut() = Some((self.win.key, page.to_string())));
        self.pad_listens(page);
        let page = page.to_string();
        let started = now_ms();
        self.listening.set(Some(Listening { page: page.clone(), started, seconds: 0, others: false, words: 0, latest: String::new(), stopping: false, video: false }));
        // The clock on the "Listening" line.
        spawn_forever(async move {
            loop {
                ai::sleep_ms(1000).await;
                let mut guard = self.listening.write();
                match guard.as_mut().filter(|l| l.started == started) {
                    Some(l) => l.seconds = (now_ms() - started) / 1000,
                    None => break,
                }
            }
        });
        spawn_forever(async move {
            use crate::listen::Heard;
            let last_words = |text: &str| {
                let words: Vec<&str> = text.split_whitespace().collect();
                words[words.len().saturating_sub(9)..].join(" ")
            };
            let mut transcript = crate::listen::Transcript::default();
            // Words heard in this page before (Stop and then Listen again, or
            // a meeting that went on after it was said to be over) stay: the
            // new ones go on under them in the same toggle, after a line
            // that says when.
            let (mut toggle, kept) = self.blocks.peek().get(&page).map(|bs| crate::listen::transcript_so_far(bs)).unwrap_or((None, 0));
            let again = (kept > 0).then(|| crate::listen::again_line(&when::time_label(started)));
            // Words of this page that no notes were written from (the app
            // was closed while it listened, or the notes failed): the notes
            // at the end are written from them too. (They were written from
            // the new part alone: the first part of the meeting had none.)
            let earlier = match HEARD_NOT_NOTED.with(|h| h.borrow_mut().iter_mut().find(|(p, _)| *p == page).map(|(_, from)| *from.get_or_insert_with(|| self.blocks.peek().get(&page).map_or(0, |bs| crate::listen::last_part(bs))))) {
                Some(from) => self.blocks.peek().get(&page).map(|bs| crate::listen::heard_before(bs, from)).unwrap_or_default(),
                None => String::new(),
            };
            // (The lines as they were written the last time: see `keep_changes`.)
            let mut shown = Vec::new();
            let mut written_at = now_ms();
            let mut problem = None;
            while let Some(event) = heard.next().await {
                match event {
                    Heard::Listening { others } => {
                        if let Some(l) = self.listening.write().as_mut() {
                            l.others = others;
                        }
                    }
                    Heard::Partial { text, .. } => {
                        if let Some(l) = self.listening.write().as_mut() {
                            l.latest = last_words(&text);
                        }
                    }
                    Heard::Final { who, text, at } => {
                        transcript.add(who, &text, at);
                        if let Some(l) = self.listening.write().as_mut() {
                            l.words = transcript.words();
                            l.latest = last_words(&text);
                        }
                        // Into the page now and then, so nothing is lost if the app closes.
                        if now_ms() - written_at > 20_000 {
                            written_at = now_ms();
                            self.write_transcript(&page, &mut toggle, kept, again.as_ref(), &transcript, &mut shown);
                        }
                    }
                    Heard::Note(message) => {
                        self.pad_problem(&page, &message);
                        self.notice(message)
                    }
                    Heard::Error(message) => {
                        problem = Some(message);
                        break;
                    }
                    Heard::Done => break,
                }
            }
            LISTENER.with(|l| l.borrow_mut().take());
            LISTENER_WINDOW.with(|w| w.borrow_mut().take());
            self.write_transcript(&page, &mut toggle, kept, again.as_ref(), &transcript, &mut shown);
            self.listening.set(None);
            // (A locked page stayed open for its transcript: see `relock_unshown`.)
            self.relock_unshown();
            // The page went to the Trash, or was deleted, while the app
            // listened: there is no page to write the notes in.
            if !self.pages.peek().get(&page).is_some_and(|p| !p.trashed) {
                return self.toast("Stopped listening");
            }
            // (Until notes are written from these words: see `earlier`.)
            if transcript.words() > 0 {
                heard_not_noted(&page, Some(kept));
            }
            if let Some(message) = problem {
                self.pad_problem(&page, &message);
                self.pad_status(&page, "", false);
                return self.notice(message);
            }
            if transcript.words() == 0 {
                self.pad_status(&page, "Nothing was heard, so there are no notes.", false);
                return self.notice("Nothing was heard, so there are no notes. Check the microphone, and that Rune may record audio (System Settings, Privacy & Security).");
            }
            // The AI writes the notes; without one, the transcript is what there is.
            // (What was heard into a locked page is not given to the AI.)
            if !ai::has_ai() || transcript.words() + earlier.split_whitespace().count() < 12 || self.locked_lines(&page) {
                self.pad_status(&page, "The transcript is in the page.", false);
                return self.toast("The transcript is in the page");
            }
            self.pad_status(&page, "Writing the notes…", true);
            self.toast("Writing the notes…");
            let messages = vec![
                ai::ChatMessage::system(crate::listen::NOTES),
                ai::ChatMessage::user(format!("{}\n\n{}", when::today_for_ai(), crate::listen::for_ai(&crate::listen::all_heard(&earlier, &transcript.text()), 120_000))),
            ];
            let mut rx = ai::start(ai::Prefs::load(), messages);
            let mut reply = String::new();
            let mut error = None;
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(text) => reply.push_str(&text),
                    AiEvent::Done => break,
                    AiEvent::Error(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            let notes = markdown::to_blocks(ai::clean_reply(&reply).0.trim());
            let added = if self.pages.peek().contains_key(&page) { self.edit_blocks(&page, move |bs| voice::fill_recap(bs, notes)) } else { 0 };
            if added > 0 {
                HEARD_NOT_NOTED.with(|h| h.borrow_mut().retain(|(p, _)| *p != page));
            }
            match (error, added) {
                (Some(e), 0) => {
                    self.pad_status(&page, "The transcript is in the page. The AI could not write the notes.", false);
                    self.notice(format!("The transcript is in the page, but the AI could not write the notes. {e}"))
                }
                (_, 0) => {
                    self.pad_status(&page, "The transcript is in the page.", false);
                    self.toast("The transcript is in the page")
                }
                _ => {
                    self.pad_status(&page, "The notes are in the page.", false);
                    self.toast("The notes are in the page")
                }
            }
        });
    }

    /// Put the transcript so far into the page (inside its "Transcript"
    /// toggle). `kept`: the lines that were in the toggle before this time of
    /// listening; they stay, and `again` (a line that says when the app
    /// started to listen again) comes between them and the new ones.
    /// `shown`: the lines as this wrote them the last time.
    fn write_transcript(self, page: &str, toggle: &mut Option<Id>, kept: usize, again: Option<&Block>, transcript: &crate::listen::Transcript, shown: &mut Vec<String>) {
        let Some(mut blocks) = self.blocks.peek().get(page).cloned() else { return };
        let mut lines = transcript.blocks();
        if lines.is_empty() {
            return;
        }
        if let Some(again) = again {
            lines.insert(0, again.clone());
        }
        crate::listen::keep_changes(&blocks, toggle.as_ref(), kept, shown, &mut lines);
        crate::listen::put_transcript(&mut blocks, toggle, kept, lines);
        self.set_blocks(page, blocks, true);
    }

    /// Stop listening. The last words are finished first; then the notes are written.
    pub fn stop_listening(mut self) {
        let Some(started) = self.listening.peek().as_ref().map(|l| l.started) else { return };
        // Stopped while its meeting goes on: a pause. The person may not
        // want some part heard, and can press Listen again; the notes of
        // what was heard are written now, in the same page, and the pad
        // stays the notepad. The question where to keep the notes waits for
        // the end of the meeting. With no meeting on (it is over, or these
        // notes were made by hand) the person is done: the page asks.
        if let Some(page) = self.listening.peek().as_ref().map(|l| l.page.clone()).filter(|p| meeting_page_ids().contains(p)) {
            let goes_on = MEETING.with(|m| m.borrow().as_ref().is_some_and(|n| n.page == page && n.noticed && n.ended.is_none()));
            if !goes_on {
                self.ask_place(&page, now_ms());
                self.pad_end(&page);
            }
        }
        if let Some(l) = self.listening.write().as_mut() {
            l.stopping = true;
        }
        LISTENER.with(|l| {
            if let Some(listener) = l.borrow_mut().as_mut() {
                listener.stop();
            }
        });
        // If it does not finish by itself, end it.
        spawn_forever(async move {
            ai::sleep_ms(8000).await;
            if self.listening.peek().as_ref().is_some_and(|l| l.started == started) {
                LISTENER.with(|l| {
                    if let Some(listener) = l.borrow_mut().as_mut() {
                        listener.end();
                    }
                });
            }
        });
    }

    // ----- read aloud ------------------------------------------------------

    /// Read a page aloud with the Mac's voice, or stop if one is being read.
    pub fn toggle_reading(mut self, page_id: &str) {
        if self.stop_reading() {
            return;
        }
        let mut text = format!("{}.\n", self.page_title(page_id));
        for b in self.blocks.peek().get(page_id).map(|b| b.as_slice()).unwrap_or_default() {
            let line = match b.kind {
                BlockKind::Divider | BlockKind::Drawing | BlockKind::Image => continue,
                BlockKind::PageLink => b.target.as_deref().map(|t| self.page_title(t)).unwrap_or_default(),
                _ => b.plain_text(),
            };
            if !line.trim().is_empty() {
                text.push_str(line.trim());
                // A stop after each block, so lists don't run together.
                text.push_str(if line.trim_end().ends_with(['.', '!', '?', ':', ';']) { "\n" } else { ".\n" });
            }
        }
        // In a test run nothing is spoken; a short wait stands in for it.
        let child = if std::env::var_os("RUNE_SELFTEST").is_some() {
            std::process::Command::new("/bin/sleep").arg("30").spawn()
        } else {
            std::process::Command::new("/usr/bin/say").stdin(std::process::Stdio::piped()).spawn().map(|mut child| {
                if let Some(mut stdin) = child.stdin.take() {
                    // On a thread of its own: a long page is more than the pipe holds.
                    std::thread::spawn(move || {
                        use std::io::Write;
                        let _ = stdin.write_all(text.as_bytes());
                    });
                }
                child
            })
        };
        match child {
            Ok(child) => {
                READER.with(|r| *r.borrow_mut() = Some(child));
                self.reading.set(true);
                // Notice when it has read to the end.
                spawn_forever(async move {
                    loop {
                        ai::sleep_ms(300).await;
                        let done = READER.with(|r| match r.borrow_mut().as_mut() {
                            Some(c) => c.try_wait().map(|s| s.is_some()).unwrap_or(true),
                            None => true,
                        });
                        if done {
                            READER.with(|r| *r.borrow_mut() = None);
                            if *self.reading.peek() {
                                self.reading.set(false);
                            }
                            break;
                        }
                    }
                });
            }
            Err(e) => self.toast(format!("Couldn't read aloud: {e}")),
        }
    }

    /// Stop reading aloud. True if something was being read.
    pub fn stop_reading(mut self) -> bool {
        let Some(mut child) = READER.with(|r| r.borrow_mut().take()) else { return false };
        let _ = child.kill();
        let _ = child.wait();
        self.reading.set(false);
        true
    }

    // ----- ask your notes ----------------------------------------------------

    /// The pages of this space most likely to hold the answer to a question.
    pub fn notes_for(&self, question: &str) -> Vec<voice::Note> {
        let space = self.ui.peek().space.clone();
        let pages = self.pages.peek();
        let list: Vec<(&Page, String)> =
            pages.values().filter(|p| p.space_id == space && !p.trashed && !self.locked_lines(&p.id)).map(|p| (p, self.page_markdown(&p.id))).collect();
        voice::relevant_pages(question, &list, 6)
    }

    /// Lines were about to be put into a locked page that is not open.
    /// Nothing was written: the person is told why. (The same words are
    /// told again only after two minutes: a transcript comes every 20 seconds.)
    fn tell_locked(&self, page_id: &str) {
        store::tell(&format!("“{}” is locked: nothing was written in it. Open it with its password first.", self.page_title(page_id)));
    }

    /// The page is locked (open or not): its lines are kept from the AI,
    /// and from everything else that takes the words of a page out of it.
    pub fn locked_lines(&self, page_id: &str) -> bool {
        self.blocks.peek().get(page_id).is_some_and(|bs| crate::lock::hides(bs, page_id))
    }

    /// Keep an answer from Ask your notes as a page: the answer, then links
    /// to the pages it came from.
    pub fn save_answer(mut self, question: &str, answer: &str, sources: &[voice::Note]) {
        let id = self.create_page(None);
        let title: String = question.trim().chars().take(70).collect();
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = title;
            p.icon = Some("lightbulb:yellow".into());
        }
        self.save_page(&id);
        let mut blocks = markdown::to_blocks(answer);
        if !sources.is_empty() {
            blocks.push(Block::new(BlockKind::Heading3, "From these pages"));
            blocks.extend(sources.iter().map(|n| Block::page_link(n.id.clone())));
        }
        if blocks.is_empty() {
            blocks.push(Block::text(""));
        }
        self.blocks.write().insert(id.clone(), blocks);
        self.save_blocks(&id);
        self.open_in_new_tab(Route::Page(id));
        self.toast("Saved as a page");
    }

    /// Draw the page the AI is writing from speech. `spoken` is given at the
    /// end: the words as said then go under a toggle (or are the whole page,
    /// if the AI wrote nothing).
    fn show_spoken_page(self, page: &str, reply: &str, spoken: Option<&str>) {
        if !self.pages.peek().contains_key(page) {
            return;
        }
        let (text, _) = ai::clean_reply(reply);
        let text = text.trim_start();
        // The first line is the title, once it has been written out in full.
        let (title, body) = match text.strip_prefix("# ") {
            Some(rest) => match rest.split_once('\n') {
                Some((title, body)) => (Some(title), body),
                // Still being written; or, at the end, a title and no more.
                None => (spoken.is_some().then_some(rest), ""),
            },
            // "#" has arrived but not the space after it yet.
            None if text == "#" && spoken.is_none() => (None, ""),
            None => (None, text),
        };
        if let Some(title) = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            if self.pages.peek().get(page).is_some_and(|p| p.title != title) {
                self.rename_page(page, title);
            }
        }
        let mut blocks = markdown::to_blocks(body);
        if let Some(spoken) = spoken {
            if blocks.is_empty() {
                blocks.push(Block::text(inline::escape(spoken)));
            } else {
                let mut toggle = Block::new(BlockKind::Toggle, "What you said");
                toggle.collapsed = true;
                blocks.push(Block::new(BlockKind::Divider, ""));
                blocks.push(toggle);
                blocks.push(Block::text(inline::escape(spoken)).with_indent(1));
            }
        }
        if blocks.is_empty() {
            return;
        }
        // Blocks already drawn keep their ids, so only new text is redrawn.
        let old = self.blocks.peek().get(page).cloned().unwrap_or_default();
        for (new, old) in blocks.iter_mut().zip(&old) {
            if new.kind != old.kind {
                break;
            }
            new.id = old.id.clone();
        }
        self.set_blocks(page, blocks, false);
    }

    // ----- meeting notes (see meeting.rs) ---------------------------------

    /// The page in this space that holds meeting notes; made the first time.
    fn meetings_page(mut self) -> Id {
        let key = format!("meetings_page:{}", self.ui.peek().space);
        // The "Meeting notes" page the app made, wherever it is now and
        // whatever its name is now. (A new meeting's page starts there; when
        // the meeting is over, the page asks where to keep it.)
        if let Some(id) = store::setting(&key).filter(|id| self.pages.peek().get(id).is_some_and(|p| !p.trashed && p.icon.as_deref() == Some("folder:blue"))) {
            return id;
        }
        // None yet (or it is gone): the "Meeting notes" page made before, if there is one.
        let space = self.ui.peek().space.clone();
        let made_before = roots_in(&self.pages.peek(), &space)
            .into_iter()
            .find(|p| p.title == "Meeting notes" && p.icon.as_deref() == Some("folder:blue"))
            .map(|p| p.id);
        if let Some(id) = made_before {
            store::set_setting(&key, &id);
            return id;
        }
        let id = self.create_page(None);
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = "Meeting notes".into();
            p.icon = Some("folder:blue".into());
        }
        self.save_page(&id);
        self.blocks.write().insert(id.clone(), vec![Block::text("Notes from your meetings, newest first.")]);
        self.save_blocks(&id);
        store::set_setting(&key, &id);
        id
    }

    /// The timed event of the calendar that is on now, or about to start
    /// (the nearest by its start), if there is one.
    fn meeting_event(&self, now: i64) -> Option<Event> {
        // (A test run can ask for meetings that take no event.)
        if MEETING_ALONE.with(std::cell::Cell::get) {
            return None;
        }
        let space = self.ui.peek().space.clone();
        let events = self.events.peek();
        let mut on = repeat::expand(events.values().filter(|e| !e.all_day), now - 5 * when::MINUTE, now + 10 * when::MINUTE);
        // The calendars of this Mac too, when the person has turned them on.
        on.extend(self.mac_events(&space, now - 5 * when::MINUTE, now + 10 * when::MINUTE).into_iter().filter(|e| !e.all_day));
        on.sort_by_key(|e| (e.start - now).abs());
        on.into_iter().next()
    }

    /// Make a notes page for a meeting that starts now and open it: named
    /// after the calendar event on now (if there is one) and the date, with
    /// the date and time as its first line, under "Meeting notes".
    pub fn make_meeting_notes(mut self, app: Option<&'static str>) -> Id {
        let now = now_ms();
        let event = self.meeting_event(now);
        let plain_name = match (&event, app) {
            (Some(e), _) => e.display_title().to_string(),
            (None, Some(app)) => crate::meeting::call_app_named(app).map(|a| a.title.to_string()).unwrap_or_else(|| format!("{app} meeting")),
            (None, None) => "Meeting".to_string(),
        };
        // With no event on the calendar: the meeting that comes at this time
        // of this day, if the person said before where its notes are kept.
        let (day, minute) = day_and_minute(now);
        let mut slots = meeting_slots();
        let slot = if event.is_none() { self.slot_found(&slots, day, minute) } else { crate::slots::Found::Nothing };
        let likely = match &slot {
            crate::slots::Found::Keep(i) => Some(*i),
            crate::slots::Found::Propose(list) if list.len() == 1 => Some(list[0]),
            _ => None,
        };
        // The name the person gave its notes the last time, if they gave one.
        let name = likely.and_then(|i| self.slot_name(&slots[i])).unwrap_or_else(|| plain_name.clone());
        let title = format!("{name} – {}", when::date_of(now).format("%b %-d, %Y"));
        let plain_title = format!("{plain_name} – {}", when::date_of(now).format("%b %-d, %Y"));
        let color = event.as_ref().map(|e| e.color.clone()).unwrap_or_else(|| "blue".into());

        // Where the notes of this meeting were kept the last time (the
        // person chose it then); for a new meeting, "Meeting notes".
        let kept = match (&event, &slot) {
            (Some(e), _) => self.meeting_home(e.display_title()),
            (None, crate::slots::Found::Keep(i)) => Some(slots[*i].home.clone()),
            _ => None,
        };
        let kept_title = kept.as_ref().map(|home| self.page_title(home));
        let parent = kept.unwrap_or_else(|| self.meetings_page());
        // Newest first there.
        let before_first = children_of(&self.pages.peek(), Some(&parent)).first().map(|p| p.position - 1.0);
        let id = self.create_page(Some(parent.clone()));
        if let Some(p) = self.pages.write().get_mut(&id) {
            p.title = title;
            p.icon = Some(format!("calendar:{color}"));
            if let Some(pos) = before_first {
                p.position = pos;
            }
        }
        self.save_page(&id);
        let stamp = Block::text(inline::escape(&meeting_stamp(now, None, app)));
        let first_note = Block::new(BlockKind::Bulleted, "");
        let (stamp_id, note_id) = (stamp.id.clone(), first_note.id.clone());
        self.blocks.write().insert(
            id.clone(),
            vec![stamp, Block::new(BlockKind::Heading2, "Notes"), first_note, Block::new(BlockKind::Heading2, "Action items"), Block::new(BlockKind::Todo, "")],
        );
        self.save_blocks(&id);
        // Its link in the page that holds meeting notes, above the earlier
        // ones: under that page's "Meetings" heading when it is a page of the
        // person's own (the heading is added the first time), so the links
        // stay together and apart from what else is on the page.
        let heading = self.link_meeting_in(&parent, &id);
        // (A heading made for the line of a draft goes with a draft that is refused.)
        DRAFT_HEADING.with(|h| *h.borrow_mut() = heading.filter(|_| app.is_some()).map(|h| (id.clone(), h)));
        // When the meeting is over, the page asks which page to keep it in:
        // at the end of its event, or when the watcher says it ended, or
        // when listening stops. With none of those, after an hour.
        let over = event.as_ref().map(|e| e.end).filter(|end| *end > now).unwrap_or(now + when::HOUR);
        match &event {
            Some(e) => remember_meeting_name(&id, e.display_title()),
            None => {
                remember_slot_page(&id, day, minute, &plain_title);
                // (Not for a draft, before the person says yes: see `confirm_meeting`.)
                if let (crate::slots::Found::Keep(i), None) = (slot, app) {
                    crate::slots::seen(&mut slots, i, day, minute, &id, now);
                    save_meeting_slots(&slots);
                }
            }
        }
        self.ask_place(&id, over);
        if let Some(home) = kept_title {
            self.toast(format!("These notes are in “{home}”, as before"));
        }
        let span = event.as_ref().map(|e| (e.start, e.end));
        let event_id = event.as_ref().map(|e| e.id.clone());
        // Made because a call started (not by the person): a draft until they say yes.
        let draft = app.is_some();
        // A one-off event of the app with no notes page yet gets this one.
        if let Some(e) = event.filter(|e| e.repeat.is_none() && e.page_id.is_none() && !e.id.starts_with(crate::maccal::PREFIX)) {
            let page = id.clone();
            self.update_event(&e.id, move |ev| ev.page_id = Some(page));
        }
        remember_meeting_page(&id);
        // Ask (on the page) whether to listen and write the notes.
        if crate::listen::available() && self.listening.peek().is_none() {
            self.listen_offer.set(Some(id.clone()));
        }
        MEETING.with(|m| *m.borrow_mut() = Some(MeetingNotes { page: id.clone(), stamp: stamp_id, started: now, ended: None, app, noticed: app.is_some(), span, event: event_id, padded: false, cut: false, draft, also: None }));
        self.keep_meeting();
        // A draft is asked about by the pad: its page opens when the person
        // says yes (with no pad, the page itself asks).
        if !draft || !crate::pad::enabled() {
            self.open_in_new_tab(Route::Page(id.clone()));
            self.focus(Caret::start(&note_id));
        }
        id
    }

    /// Whether `page` is the notes of the meeting on now and still a draft.
    pub fn meeting_draft(&self, page: &str) -> bool {
        MEETING.with(|m| m.borrow().as_ref().is_some_and(|n| n.page == page && n.draft))
    }

    /// The person said yes to the notes of the meeting (Listen, Notes only,
    /// a line typed): its page is kept, and opens if it is not open yet.
    fn confirm_meeting(self, page: &str) {
        let was = MEETING.with(|m| {
            let mut m = m.borrow_mut();
            match m.as_mut() {
                Some(n) if n.page == page && n.draft => {
                    n.draft = false;
                    true
                }
                _ => false,
            }
        });
        if !was {
            return;
        }
        self.keep_meeting();
        DRAFT_HEADING.with(|h| h.borrow_mut().take_if(|(of, _)| of == page));
        // The meeting that comes at this time was seen again, now that the
        // person wants its notes. (A call they refused counted too: its
        // page, which went, was taken as the last notes of that meeting,
        // and the name the person had given them was lost.)
        if let Some((_, day, minute, _)) = slot_pages().into_iter().rev().find(|(p, ..)| p == page) {
            let mut slots = meeting_slots();
            let here = self.pages.peek().get(page).and_then(|p| p.parent_id.clone());
            if let crate::slots::Found::Keep(i) = self.slot_found(&slots, day, minute) {
                if here.as_deref() == Some(slots[i].home.as_str()) {
                    crate::slots::seen(&mut slots, i, day, minute, page, now_ms());
                    save_meeting_slots(&slots);
                }
            }
        }
        let open = self.ui.peek().tabs.iter().any(|t| t.panes.iter().any(|p| *p.route() == Route::Page(page.to_string())));
        if !open && self.pages.peek().contains_key(page) {
            self.open_in_new_tab(Route::Page(page.to_string()));
        }
    }

    /// Nothing is in the notes page of a meeting but what the app put there.
    fn meeting_untouched(&self, page: &str) -> bool {
        if !children_of(&self.pages.peek(), Some(&page.to_string())).is_empty() {
            return false;
        }
        let blocks = self.blocks.peek();
        let Some(bs) = blocks.get(page) else { return true };
        bs.iter().enumerate().all(|(i, b)| {
            let words = b.plain_text();
            let words = words.trim();
            match b.kind {
                // (The first line is the date line the app wrote.)
                BlockKind::Text if i == 0 => true,
                BlockKind::Heading2 => words == "Notes" || words == "Action items",
                BlockKind::Text | BlockKind::Bulleted | BlockKind::Todo => words.is_empty(),
                _ => false,
            }
        })
    }

    /// The person said no to notes of the meeting (the × of the pad, "Not
    /// now" on the page), or it ended with no yes: an empty draft goes for
    /// good, with its line in the page that holds it. A draft with something
    /// in it is kept. Says whether the page went.
    pub fn drop_meeting_draft(mut self, page: &str) -> bool {
        if !self.meeting_draft(page) {
            return false;
        }
        if !self.meeting_untouched(page) || self.listening.peek().as_ref().is_some_and(|l| l.page == page) {
            self.confirm_meeting(page);
            return false;
        }
        let event = MEETING.with(|m| m.borrow_mut().take().and_then(|n| n.event));
        store::set_setting("meeting_last", "");
        if self.listen_offer.peek().as_deref() == Some(page) {
            self.listen_offer.set(None);
        }
        if self.pad.peek().as_ref().is_some_and(|p| p.page == page) {
            self.close_pad();
        }
        self.place_done(page);
        if let Some(e) = event.filter(|e| self.events.peek().get(e).is_some_and(|ev| ev.page_id.as_deref() == Some(page))) {
            self.update_event(&e, |ev| ev.page_id = None);
        }
        let parent = self.pages.peek().get(page).and_then(|p| p.parent_id.clone());
        // (The link of a page is in `target`: looked for in `text`, the line
        // was never found, and each refused call left a dead line.)
        // The "Meetings" heading that was made for this line, when nothing
        // else came under it: it goes too. (Each refused call could leave
        // an empty heading in a page of the person's own.)
        let heading = DRAFT_HEADING.with(|h| h.borrow_mut().take_if(|(of, _)| of == page)).map(|(_, heading)| heading);
        if let Some(parent) = parent.filter(|p| self.blocks.peek().get(p).is_some_and(|bs| bs.iter().any(|b| b.kind == BlockKind::PageLink && b.target.as_deref() == Some(page)))) {
            let gone = page.to_string();
            self.edit_blocks(&parent, move |bs| {
                bs.retain(|b| !(b.kind == BlockKind::PageLink && b.target.as_deref() == Some(gone.as_str())));
                if let Some(h) = heading.and_then(|id| bs.iter().position(|b| b.id == id)) {
                    let untouched = bs[h].kind == BlockKind::Heading2 && bs[h].text == "Meetings";
                    if untouched && bs.get(h + 1).is_none_or(|next| next.kind != BlockKind::PageLink) {
                        bs.remove(h);
                    }
                }
            });
        }
        // A tab that was opened for the draft alone goes with it (not to
        // the closed tabs: there is nothing in it to open again).
        {
            let gone = Route::Page(page.to_string());
            let only = |t: &Tab| t.panes.iter().all(|p| p.history.iter().all(|r| *r == gone));
            let mut ui = self.ui.write();
            if ui.tabs.iter().any(only) {
                let active = ui.tabs.get(ui.active).map(|t| t.id.clone());
                ui.tabs.retain(|t| !only(t));
                if ui.tabs.is_empty() {
                    ui.tabs.push(Tab::new(Route::Home));
                }
                let last = ui.tabs.len() - 1;
                ui.active = active.and_then(|id| ui.tabs.iter().position(|t| t.id == id)).unwrap_or(last);
            }
        }
        let (folder, made) = self.pages.peek().get(page).map(|p| (p.parent_id.clone(), p.created_at)).unwrap_or((None, 0));
        self.delete_forever(page);
        // The "Meeting notes" page that the app made for it, with nothing in
        // it now: it goes too. (Each call that was refused left one more
        // empty "Meeting notes" in the sidebar of the space that was open.)
        if let Some(folder) = folder {
            // Only the page that was made for this call (in the moment before
            // its notes), never one the person has had for longer. And with
            // nothing under it, pages in the Trash too: they would go for good.
            let own = self.pages.peek().get(&folder).is_some_and(|p| {
                p.title == "Meeting notes" && p.icon.as_deref() == Some("folder:blue") && !p.favorite && !p.trashed && p.parent_id.is_none() && (made - p.created_at).abs() < 10_000
            });
            let holds_nothing = descendants(&self.pages.peek(), &folder).len() == 1
                && self.blocks.peek().get(&folder).map_or(true, |bs| bs.iter().all(|b| b.kind == BlockKind::Text && (b.text.trim().is_empty() || b.text == "Notes from your meetings, newest first.")));
            if own && holds_nothing {
                self.delete_forever(&folder);
            }
        }
        true
    }

    /// "Not now" on the question of a meeting's page.
    pub fn decline_listen(mut self, page: &str) {
        if self.listen_offer.peek().as_deref() == Some(page) {
            self.listen_offer.set(None);
        }
        self.decline_meeting(page);
    }

    /// The person said no to the notes of a call (see `drop_meeting_draft`):
    /// when its empty page went, the no is kept for as long as that call
    /// goes on. Says whether the page went.
    fn decline_meeting(self, page: &str) -> bool {
        let (app, of) = MEETING.with(|m| m.borrow().as_ref().filter(|n| n.page == page).map(|n| (n.app, n.event.clone().zip(n.span).map(|(id, (start, end))| (id, start, end))))).unwrap_or((None, None));
        let went = self.drop_meeting_draft(page);
        if went {
            let now = now_ms();
            set_declined(Declined { app: app.map(str::to_string), seen: now, said: now, of });
        }
        went
    }

    /// Write the date line of the last meeting's notes again (when it ends,
    /// or carries on), unless the person has changed that line.
    fn restamp_meeting(self, was_ended: Option<i64>) {
        let Some((page, stamp, old, new)) = MEETING.with(|m| {
            m.borrow().as_ref().map(|n| {
                (n.page.clone(), n.stamp.clone(), meeting_stamp(n.started, was_ended, n.app), meeting_stamp(n.started, n.ended, n.app))
            })
        }) else {
            return;
        };
        let (old, new) = (inline::escape(&old), inline::escape(&new));
        if old == new || !self.pages.peek().contains_key(&page) {
            return;
        }
        self.edit_blocks(&page, move |bs| {
            if let Some(b) = bs.iter_mut().find(|b| b.id == stamp && b.text == old) {
                b.text = new;
            }
        });
    }

    /// Write the meeting of the last notes down for the next run of the app
    /// (see `KeptMeeting`).
    fn keep_meeting(self) {
        let kept = MEETING.with(|m| {
            m.borrow().as_ref().map(|n| KeptMeeting {
                page: n.page.clone(),
                stamp: n.stamp.clone(),
                started: n.started,
                ended: n.ended,
                app: n.app.map(str::to_string),
                noticed: n.noticed,
                span: n.span,
                event: n.event.clone(),
                padded: n.padded || self.pad.peek().as_ref().is_some_and(|p| p.page == n.page),
                listened: n.cut || self.listening.peek().as_ref().is_some_and(|l| l.page == n.page),
                seen: now_ms(),
                draft: n.draft,
            })
        });
        store::set_setting("meeting_last", &kept.and_then(|k| serde_json::to_string(&k).ok()).unwrap_or_default());
    }

    /// The meeting goes on (the watcher still sees it): written down now and
    /// then, so that the next run of the app knows until when it was seen.
    /// `apps`: the programs the call is in now.
    pub fn meeting_seen(self, apps: &[&'static str]) {
        let due = MEETING.with(|m| m.borrow().as_ref().is_some_and(|n| n.ended.is_none()))
            && MEETING_KEPT.with(|at| now_ms() - at.get() >= 30_000);
        if due {
            MEETING_KEPT.with(|at| at.set(now_ms()));
            self.keep_meeting();
        }
        // A call the person said no to goes on: seen now, while its program
        // is in the call. (The no to a long call was not seen during the
        // notes of a meeting in another program: after a meeting of twenty
        // minutes it was forgotten, and the long call asked again.) A no to
        // notes made by hand has no program: seen while there are no notes.
        let now = now_ms();
        let no_notes = MEETING.with(|m| m.borrow().as_ref().map_or(true, |n| n.ended.is_some()));
        let mut nos = declined();
        let mut seen = false;
        for d in nos.iter_mut().filter(|d| now - d.seen >= 30_000 && now - d.seen < SAME_MEETING_MS) {
            if d.app.as_deref().map_or(no_notes, |of| apps.contains(&of)) {
                d.seen = now;
                seen = true;
            }
        }
        if seen {
            keep_declined(nos);
        }
    }

    /// No call is on. A draft that nobody answered, of a call that ended
    /// while the app was closed, goes when that call can no more be the
    /// one that comes back. (It stayed, with its line and its question,
    /// until the next call started.)
    pub fn meeting_quiet(self) {
        let now = now_ms();
        let stale = MEETING.with(|m| {
            m.borrow().as_ref().filter(|n| n.draft && n.ended.is_some_and(|end| now - end >= SAME_MEETING_MS) && !n.span.is_some_and(|(_, end)| now < end)).map(|n| n.page.clone())
        });
        if let Some(page) = stale {
            self.drop_meeting_draft(&page);
        }
    }

    /// As the app opens: take up the meeting that an earlier run wrote down.
    /// One that was on then counts as quiet since the last time it was seen;
    /// a call that is noticed soon after is that meeting (see `same_meeting`).
    /// Gives the programs of the calls that were on then, for the watcher
    /// (`Watch::after`): that of a meeting with notes, less than an hour
    /// ago, first, then those of the calls that were refused.
    pub fn recall_meeting(self) -> Vec<&'static str> {
        let nos = read_declined(&store::setting("meeting_declined").unwrap_or_default());
        MEETING_DECLINED.with(|d| *d.borrow_mut() = nos.clone());
        if MEETING.with(|m| m.borrow().is_some()) {
            return Vec::new();
        }
        // The calls that were refused, if they can still be on: their sound
        // alone says that they go on (the person may be muted).
        let mut were_on: Vec<&'static str> =
            nos.iter().filter(|d| now_ms() - d.seen < SAME_MEETING_MS).filter_map(|d| d.app.as_deref().and_then(crate::meeting::call_app_named)).map(|a| a.name).collect();
        let Some(k) = store::setting("meeting_last").and_then(|s| serde_json::from_str::<KeptMeeting>(&s).ok()) else { return were_on };
        let app = k.app.as_deref().and_then(crate::meeting::call_app_named).map(|a| a.name);
        let was_on = k.ended.is_none() && now_ms() - k.seen < when::HOUR;
        let (k_page, k_draft) = (k.page.clone(), k.draft);
        let was_on_lately = k.ended.is_none() && now_ms() - k.seen < SAME_MEETING_MS;
        let ended = Some(k.ended.unwrap_or(k.seen).max(k.started));
        // The app was closed while it listened: the words it had heard are
        // in the page, and no notes were written from them.
        if k.listened && k.ended.is_none() {
            heard_not_noted(&k.page, None);
        }
        MEETING.with(|m| {
            *m.borrow_mut() = Some(MeetingNotes { page: k.page, stamp: k.stamp, started: k.started, ended, app, noticed: k.noticed, span: k.span, event: k.event, padded: k.padded, cut: k.listened && k.ended.is_none(), draft: k.draft, also: None })
        });
        // A draft that nobody answered, of a call that cannot be on any
        // more: it goes (it stayed for good, with its line and its question).
        if k_draft && !was_on_lately {
            self.drop_meeting_draft(&k_page);
        }
        if let Some(app) = app.filter(|_| was_on) {
            were_on.retain(|a| *a != app);
            were_on.insert(0, app);
        }
        were_on
    }

    /// A call is noticed. When it is the meeting whose notes were made last
    /// (see `same_meeting`), that meeting carries on with the same page, and
    /// what its end did is taken back. Says whether it is.
    fn carry_on_meeting(self, app: Option<&'static str>) -> bool {
        let now = now_ms();
        let on_now = self.meeting_event(now).map(|e| (e.id.clone(), e.start, e.end));
        let carried_on = MEETING.with(|m| {
            let mut m = m.borrow_mut();
            let n = m.as_mut()?;
            let of = n.event.clone().zip(n.span).map(|(id, (start, end))| (id, start, end));
            // A call in another program is another call (a voice channel of
            // Discord opened the page of the Teams meeting that had ended).
            // A browser on one side can be the same call: it passes to the
            // call program. And a meeting whose event is still on is that
            // meeting in any program.
            let browser = |a: &str| crate::meeting::call_app_named(a).is_some_and(|c| c.browser);
            let same_program = match (n.app, app) {
                (Some(was), Some(now)) => was == now || browser(was) || browser(now),
                _ => true,
            };
            let own_event_on = of.as_ref().is_some_and(|(_, _, end)| now < *end);
            let same = match n.ended {
                Some(end) => (same_program || own_event_on) && same_meeting(of.as_ref(), end, now, on_now.as_ref()),
                // Notes made by hand, or by joining from the calendar, just
                // before the call (or for an event that is still on).
                None => now - n.started < JUST_MADE_MS || of.as_ref().is_some_and(|(_, _, end)| now < *end),
            };
            // Notes made by hand, or by joining from the calendar, that the
            // watcher sees for the first time now.
            let first_seen = same && !n.noticed;
            if same {
                n.noticed = true;
                // (The program the meeting is in now, when it is not the
                // one its notes were made in: see `meeting_left`.)
                if let Some(app) = app.filter(|_| app != n.app) {
                    n.also = Some(bound_to(n.also, app));
                }
            }
            same.then(|| (n.ended.take(), first_seen, n.page.clone(), std::mem::take(&mut n.cut), std::mem::take(&mut n.padded)))
        });
        let Some((was_ended, first_seen, page, cut, padded)) = carried_on else { return false };
        self.restamp_meeting(was_ended);
        // The app is behind the meeting now: its pad asks on the screen.
        if first_seen && self.pad.peek().is_none() {
            self.open_pad(&page);
        }
        if was_ended.is_some() {
            self.meeting_goes_on(&page, cut, padded);
        }
        self.keep_meeting();
        true
    }

    /// A meeting that was taken as over goes on (the call went quiet for a
    /// while, passed to another program, or the app was closed and opened
    /// again): what its end did is taken back. Its date line has lost the
    /// end already (`restamp_meeting`), and its page asks for no place while
    /// the meeting is on (`asks_place`). A page that the person put in the
    /// Trash stays there: this meeting gets no other page.
    fn meeting_goes_on(mut self, page: &str, cut: bool, padded: bool) {
        use crate::pad::{Form, Pad};
        if !self.pages.peek().get(page).is_some_and(|p| !p.trashed) {
            return;
        }
        let now = now_ms();
        // The end stopped the listening. It does not start again by itself
        // (the app listens only when the person says so): the pad and the
        // page say that it stopped, and offer it with one press.
        let listens = self.listening.peek().as_ref().is_some_and(|l| l.page == page && !l.stopping);
        let offer = cut && !listens && crate::listen::available();
        let says = if offer { "Listening stopped. Press Listen to go on." } else { "" };
        if self.pad.peek().as_ref().is_some_and(|p| p.page == page) {
            self.change_pad(|p| {
                if matches!(p.form, Form::End | Form::Kept) {
                    p.turn(if offer { Form::Open } else { p.was }, now);
                }
                p.ended = None;
                if offer {
                    p.problem = says.to_string();
                }
            });
        } else if padded && self.pad.peek().is_none() && crate::pad::enabled() {
            // Its pad has gone since the end: it comes back.
            let (started, span) = MEETING.with(|m| m.borrow().as_ref().filter(|n| n.page == page).map(|n| (n.started, n.span))).unwrap_or((now, None));
            let mut pad = Pad::new(page, if offer { Form::Open } else { Form::Small }, now, span);
            pad.started = started;
            pad.used = true;
            pad.heard = cut;
            pad.problem = says.to_string();
            self.pad.set(Some(pad));
            self.watch_pad();
        }
        if offer {
            self.listen_offer.set(Some(page.to_string()));
        }
    }

    /// After an end, the program the meeting was in plays sound again with
    /// no microphone in use (the person is muted). True when that is the
    /// meeting of the last notes carrying on; sound alone never makes the
    /// notes of a new meeting.
    pub fn meeting_back(self, app: &'static str) -> bool {
        // The call the person said no to goes on: the watcher stays in it,
        // and the no stays fresh (`meeting_seen`). (It was taken as not that
        // call; after twenty minutes of mute the no was forgotten.)
        // (Also while there are notes of a meeting in another call program:
        // the sound of the refused call is not that meeting.)
        let now = now_ms();
        let no = no_for(&declined(), app).cloned();
        let browser = |a: &str| crate::meeting::call_app_named(a).is_some_and(|c| c.browser);
        let no_notes = MEETING.with(|m| {
            m.borrow().as_ref().map_or(true, |n| (n.ended.is_some() && now - n.ended.unwrap_or(now) >= SAME_MEETING_MS) || n.app.is_some_and(|of| of != app && !browser(of)))
        });
        if no_notes && still_declined(no.as_ref(), app, now, None) {
            if let Some(d) = no {
                set_declined(Declined { seen: now, ..d });
            }
            return true;
        }
        match MEETING.with(|m| m.borrow().as_ref().map(|n| n.ended.is_some())) {
            Some(true) => self.carry_on_meeting(Some(app)),
            // The app had not taken it as over.
            Some(false) => true,
            None => false,
        }
    }

    /// During a call, another call program took the microphone. When the
    /// call that is on has no notes (the person said no to it, or never
    /// answered: a voice channel of Discord that is on all day), this is a
    /// call of its own and asks; a meeting in Teams was not noticed then.
    /// Notes of a browser's call, and notes made by hand a moment ago, are
    /// the notes of this call: it passed to this program.
    pub fn meeting_joined(self, app: &'static str) {
        let browser = |a: &str| crate::meeting::call_app_named(a).is_some_and(|c| c.browser);
        let now = now_ms();
        let on = MEETING.with(|m| m.borrow().as_ref().filter(|n| n.ended.is_none()).map(|n| (n.page.clone(), n.app, n.draft, n.event.clone().zip(n.span).map(|(id, (start, end))| (id, start, end)))));
        let also = MEETING.with(|m| m.borrow().as_ref().filter(|n| n.ended.is_none()).and_then(|n| n.also));
        // The call the person said no to has its microphone again, during
        // the notes of another meeting (it left the call when it was silent
        // for a while): it is that call, and changes nothing. (Its return
        // must not take away the question of the meeting that is on.)
        // (Not for notes made by hand that have no program yet: they can be
        // the notes of this very call. See below.)
        if on.as_ref().is_some_and(|(_, of, ..)| notes_of_another(*of, also, app)) && refused_lately(&declined(), app, now) {
            if let Some(d) = declined().into_iter().find(|d| d.app.as_deref() == Some(app)) {
                return set_declined(Declined { seen: now, ..d });
            }
        }
        match on {
            None => self.meeting_started(app),
            // A draft of the other program that nobody answered: it goes,
            // and this call gets its own. No answer is a no for that call,
            // as at its end: it asks no more while it goes on.
            Some((page, Some(of), true, event)) if of != app && !browser(of) => {
                if self.drop_meeting_draft(&page) {
                    set_declined(Declined { app: Some(of.to_string()), seen: now, said: now, of: event });
                    self.meeting_started(app);
                }
            }
            // Notes made by hand, or by "Join" on an event, a moment ago:
            // they are the notes of this call (see `carry_on_meeting`).
            Some((_, None, ..)) => {
                self.carry_on_meeting(Some(app));
            }
            // The call passed from the browser to its own program.
            Some((page, Some(of), ..)) if browser(of) && !browser(app) => MEETING.with(|m| {
                if let Some(n) = m.borrow_mut().as_mut().filter(|n| n.page == page) {
                    n.also = Some(app);
                }
            }),
            _ => {}
        }
    }

    /// During a call in several programs, one of them went silent while
    /// another goes on (`rest`: the programs the call is still in). When
    /// the notes that are on are of the call in that program, their meeting
    /// is over, as at `meeting_ended`: the listening stops, the notes are
    /// written and the page asks where to keep them. The other call goes
    /// on, and a no to it holds.
    pub fn meeting_left(self, app: &'static str, rest: &[&'static str], at: i64) {
        let over = MEETING.with(|m| {
            m.borrow().as_ref().is_some_and(|n| {
                let mine = [n.app, n.also];
                n.ended.is_none() && mine.contains(&Some(app)) && !mine.iter().flatten().any(|of| rest.contains(of))
            })
        });
        if over {
            self.meeting_ended(at);
            // A program that joined during those notes was not asked: its
            // call was taken as a part of that meeting. It goes on, so it
            // is a call of its own now. (It got no pad and no page.) A call
            // that was refused a short time ago stays as it is.
            let nos = declined();
            if let Some(other) = rest.iter().find(|of| !refused_lately(&nos, of, now_ms())) {
                self.meeting_started(other);
            }
        }
    }

    /// A call started (the watcher saw its microphone in use): make its
    /// notes page, unless it is the meeting of the last notes that carries
    /// on, and show its pad (the app is usually behind the meeting).
    pub fn meeting_started(self, app: &'static str) {
        let now = now_ms();
        let notes_app = MEETING.with(|m| m.borrow().as_ref().and_then(|n| n.app));
        if !not_its_notes(&declined(), notes_app, app, now) && self.carry_on_meeting(Some(app)) {
            return;
        }
        // The call the person said no to (it went quiet and is back, or the
        // app was opened again during it): it asks no more.
        let on_now = self.meeting_event(now).map(|e| (e.id.clone(), e.start, e.end));
        let no = no_for(&declined(), app).cloned();
        if let Some(d) = no.clone().filter(|d| still_declined(Some(d), app, now, on_now.as_ref())) {
            crate::meeting::log("No notes of this call, as the person said");
            return set_declined(Declined { seen: now, ..d });
        }
        // A new call in this program: the no to its last call is over. (The
        // nos to calls in other programs stay: a meeting joined during a
        // refused call took that call's no away.)
        if no.is_some() {
            keep_declined(without_no(declined(), app));
        }
        // The draft of an earlier call that got no answer and no end (the
        // app was closed during it): it goes now, or it stayed for good.
        if let Some(old) = MEETING.with(|m| m.borrow().as_ref().filter(|n| n.draft).map(|n| n.page.clone())) {
            self.drop_meeting_draft(&old);
        }
        let id = self.make_meeting_notes(Some(app));
        // The app is behind the call: its pad asks on the screen itself,
        // and no notification is needed to say that the page is there.
        let padded = self.open_pad(&id);
        let title = self.page_title(&id);
        spawn_forever(async move {
            // The pad is the message. But if it cannot be seen a moment
            // after it opened (macOS kept it off another program's full
            // screen), the notification says that the page is there.
            if padded {
                ai::sleep_ms(2500).await;
                if crate::pad::in_view() || crate::pad::inline() {
                    return;
                }
            }
            let mut allowed = crate::notify::permission().await;
            if allowed == crate::notify::Permission::NotAsked {
                allowed = crate::notify::request_permission().await;
            }
            if allowed == crate::notify::Permission::Allowed {
                crate::notify::show_now(&format!("page:{id}"), "A call started", &format!("Open “{title}” and press Listen or write a note to keep notes of it.")).await;
            }
        });
    }

    /// Put the line of a meeting's page in the page that holds it, above
    /// the earlier ones (see `make_meeting_notes`). Gives the "Meetings"
    /// heading, when it made one for the line.
    fn link_meeting_in(self, parent: &str, id: &str) -> Option<Id> {
        let link = Block::page_link(id.to_string());
        let own_folder = self.pages.peek().get(parent).is_some_and(|p| p.title == "Meeting notes" && p.icon.as_deref() == Some("folder:blue"));
        self.edit_blocks(parent, move |bs| {
            let heading = bs.iter().position(|b| {
                matches!(b.kind, BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3) && b.plain_text().trim().eq_ignore_ascii_case("Meetings")
            });
            match heading {
                Some(h) => bs.insert(h + 1, link),
                None if own_folder => {
                    let at = bs.iter().position(|b| b.kind == BlockKind::PageLink).unwrap_or(bs.len());
                    bs.insert(at, link);
                }
                None => {
                    if bs.last().is_some_and(|b| b.kind == BlockKind::Text && b.text.trim().is_empty()) {
                        bs.pop();
                    }
                    let heading = Block::new(BlockKind::Heading2, "Meetings");
                    let made = heading.id.clone();
                    bs.push(heading);
                    bs.push(link);
                    return Some(made);
                }
            }
            None
        })
    }

    /// The places a page can be kept in, in every space (see `places_to_keep`).
    pub fn meeting_places(&self, page: &str) -> Vec<crate::pad::Place> {
        places_to_keep(&self.pages.read(), &self.spaces.read(), page, &meeting_page_ids())
    }

    /// Where a page is kept now (see `place_of`).
    pub fn page_place(&self, page: &str) -> (crate::pad::Place, String) {
        place_of(&self.pages.read(), &self.spaces.read(), page)
    }

    /// Whether a page is the notes of a meeting, or of an event of the calendar.
    pub fn is_notes_page(&self, id: &str) -> bool {
        self.is_meeting_page(id)
            || self.events.peek().values().any(|e| e.page_id.as_deref() == Some(id))
            || self.repeat_pages.peek().values().any(|p| p == id)
            || self.mac_cal.peek().pages.values().any(|p| p == id)
    }

    /// Keep a page in `home`: a page of any space, a space (the top of its
    /// list), or "" (the top of the list of the space it is in). Asked on
    /// the page of a meeting when the meeting is over, and by "Move to…".
    /// The line of a meeting's notes goes from the page it was in to that
    /// page, with it. The sidebar goes with a page that is on the screen.
    pub fn keep_page_in(self, page: &str, home: &str) {
        let Some((from, own)) = self.pages.peek().get(page).map(|p| (p.parent_id.clone(), p.space_id.clone())) else { return };
        let notes = self.is_notes_page(page);
        let unlink = |from: &Option<Id>| {
            if let (true, Some(from)) = (notes, from) {
                let page = page.to_string();
                self.edit_blocks(from, move |bs| bs.retain(|b| !(b.kind == BlockKind::PageLink && b.target.as_deref() == Some(page.as_str()))));
            }
        };
        // The notes of the next meeting of this name start in the same place.
        let name = self.meeting_name(page).filter(|_| notes);
        // A meeting with no event on the calendar is known by its day and its time.
        let slot = slot_pages().into_iter().rev().find(|(p, ..)| p == page).filter(|_| notes && name.is_none());
        let top_of = if home.is_empty() { Some(own.clone()) } else { self.spaces.peek().iter().find(|s| s.id == home).map(|s| s.id.clone()) };
        if let Some(space) = top_of {
            if from.is_some() || space != own {
                unlink(&from);
                self.move_page_in(&space, page, None, "root");
                self.follow_route_space();
            }
            if let Some(name) = &name {
                set_meeting_home(name, None);
            }
            if let Some((_, day, minute, _)) = &slot {
                let mut slots = meeting_slots();
                crate::slots::forget(&mut slots, *day, *minute);
                save_meeting_slots(&slots);
            }
            return;
        }
        let known = self.pages.peek().get(home).is_some_and(|p| !p.trashed);
        if !known || home == page || descendants(&self.pages.peek(), page).iter().any(|d| d == home) {
            return;
        }
        if let Some(name) = &name {
            set_meeting_home(name, Some(home));
        }
        if let Some((_, day, minute, _)) = &slot {
            if from.as_deref() != Some(home) {
                let mut slots = meeting_slots();
                crate::slots::learn(&mut slots, *day, *minute, home, page, now_ms());
                save_meeting_slots(&slots);
            }
        }
        if from.as_deref() == Some(home) {
            return;
        }
        unlink(&from);
        if !notes {
            self.move_page(page, Some(home), "into");
            self.follow_route_space();
            return;
        }
        // The newest first, as where it came from.
        let first = children_of(&self.pages.peek(), Some(home)).into_iter().find(|p| p.id != page).map(|p| p.id);
        match first {
            Some(first) => self.move_page(page, Some(&first), "before"),
            None => self.move_page(page, Some(home), "into"),
        }
        self.link_meeting_in(home, page);
        self.follow_route_space();
    }

    /// The name of the meeting that a notes page is of, when it has one
    /// (the calendar event that was on when its notes were made).
    pub fn meeting_name(&self, page: &str) -> Option<String> {
        meeting_names().into_iter().rev().find(|(p, _)| p == page).map(|(_, name)| name).filter(|n| !n.is_empty())
    }

    /// The page that the notes of meetings of this name are kept in: the
    /// person chose it on the notes of one of them, and it is still there
    /// (in any space).
    fn meeting_home(&self, name: &str) -> Option<Id> {
        meeting_homes().get(&name.trim().to_lowercase()).filter(|id| self.pages.peek().get(*id).is_some_and(|p| !p.trashed)).cloned()
    }

    /// The name of this page's meeting, when the notes of the next meeting
    /// of that name start where this page is now (the bar says so).
    pub fn next_notes_go_here(&self, page: &str) -> Option<String> {
        let name = self.meeting_name(page)?;
        let here = self.pages.peek().get(page)?.parent_id.clone()?;
        (self.meeting_home(&name).as_deref() == Some(here.as_str())).then_some(name)
    }

    /// What is known about a meeting at this day and time, with the pages
    /// that are gone left out.
    fn slot_found(&self, slots: &[crate::slots::Slot], day: u32, minute: i64) -> crate::slots::Found {
        let pages = self.pages.peek();
        let live = |i: &usize| pages.get(&slots[*i].home).is_some_and(|p| !p.trashed);
        match crate::slots::find(slots, day, minute) {
            crate::slots::Found::Keep(i) if live(&i) => crate::slots::Found::Keep(i),
            crate::slots::Found::Propose(list) => {
                let list: Vec<usize> = list.into_iter().filter(live).collect();
                if list.is_empty() { crate::slots::Found::Nothing } else { crate::slots::Found::Propose(list) }
            }
            _ => crate::slots::Found::Nothing,
        }
    }

    /// The name the person gave the last notes of a meeting that comes
    /// again (None while that page has the name the app gave it).
    fn slot_name(&self, slot: &crate::slots::Slot) -> Option<String> {
        let (_, _, _, plain) = slot_pages().into_iter().rev().find(|(p, ..)| *p == slot.page)?;
        let title = self.pages.peek().get(&slot.page).filter(|p| !p.trashed)?.title.clone();
        let name = name_before_date(&title);
        (!name.is_empty() && name != name_before_date(&plain)).then(|| name.to_string())
    }

    /// For the notes of a meeting with no calendar event: the pages its
    /// notes were kept in before at this day and time (to offer with one
    /// click), and whether the next notes go where these are by themselves.
    pub fn slot_of_page(&self, page: &str) -> (Vec<(Id, String)>, bool) {
        let Some((_, day, minute, _)) = slot_pages().into_iter().rev().find(|(p, ..)| p == page) else { return (Vec::new(), false) };
        let here = self.pages.peek().get(page).and_then(|p| p.parent_id.clone());
        let slots = meeting_slots();
        match self.slot_found(&slots, day, minute) {
            crate::slots::Found::Keep(i) => {
                let home = slots[i].home.clone();
                if here.as_deref() == Some(home.as_str()) { (Vec::new(), true) } else { (vec![(home.clone(), self.page_title(&home))], false) }
            }
            crate::slots::Found::Propose(list) => (
                list.into_iter().map(|i| slots[i].home.clone()).filter(|home| here.as_deref() != Some(home.as_str())).map(|home| (home.clone(), self.page_title(&home))).collect(),
                false,
            ),
            crate::slots::Found::Nothing => (Vec::new(), false),
        }
    }

    /// From `from` on, this page asks which page to keep it in.
    fn ask_place(mut self, page: &str, from: i64) {
        // Notes that are already in the page the person chose for meetings
        // of this name do not ask again ("Move to…" in the menu of the page
        // still does).
        if self.next_notes_go_here(page).is_some() || self.slot_of_page(page).1 {
            return;
        }
        let mut asks = self.place_asks.peek().clone();
        asks.retain(|(p, _)| p != page);
        asks.push((page.to_string(), from));
        let start = asks.len().saturating_sub(60);
        let asks = asks[start..].to_vec();
        store::set_setting("place_asks", &serde_json::to_string(&asks).unwrap_or_default());
        self.place_asks.set(asks);
        // Draw the question when its time comes, also when nothing else changes then.
        let wait = from - now_ms();
        if wait > 0 && wait < 12 * when::HOUR {
            let mut asks = self.place_asks;
            spawn_forever(async move {
                ai::sleep_ms(wait as u64 + 500).await;
                let same = asks.peek().clone();
                asks.set(same);
            });
        }
    }

    /// Whether this page asks now which page to keep it in.
    pub fn asks_place(&self, page: &str) -> bool {
        if self.place_now.read().as_deref() == Some(page) {
            return true;
        }
        let Some(from) = self.place_asks.read().iter().find(|(p, _)| p == page).map(|(_, from)| *from) else { return false };
        // Not while the meeting goes on (as far as the app can tell), and
        // not while the last words are still being written down.
        let live = MEETING.with(|m| m.borrow().as_ref().is_some_and(|n| n.page == page && n.noticed && n.ended.is_none()));
        let listening = self.listening.read().as_ref().is_some_and(|l| l.page == page);
        !live && !listening && now_ms() >= from
    }

    /// "Move to…" in the menu of a page: the page asks now.
    pub fn ask_place_now(mut self, page: &str) {
        self.place_now.set(Some(page.to_string()));
    }

    /// "Done" on the question: the page asks no more.
    pub fn place_done(mut self, page: &str) {
        if self.place_now.peek().as_deref() == Some(page) {
            self.place_now.set(None);
        }
        if self.place_asks.peek().iter().any(|(p, _)| p == page) {
            let asks: Vec<(Id, i64)> = self.place_asks.peek().iter().filter(|(p, _)| p != page).cloned().collect();
            store::set_setting("place_asks", &serde_json::to_string(&asks).unwrap_or_default());
            self.place_asks.set(asks);
        }
    }

    /// The meeting ended at `at`: its date line gets the end time and length.
    pub fn meeting_ended(mut self, at: i64) {
        // (A no to a call stays for a while after it was last seen, so the
        // call can go quiet and come back: see `meeting_seen`.)
        if let Some((page, app, of)) =
            MEETING.with(|m| m.borrow().as_ref().filter(|n| n.draft && n.ended.is_none()).map(|n| (n.page.clone(), n.app, n.event.clone().zip(n.span).map(|(id, (start, end))| (id, start, end)))))
        {
            if self.drop_meeting_draft(&page) {
                // No answer is a no for this call: it went quiet, and asks
                // no more when it comes back. (A new page and the question
                // came at each pause of a long call.)
                set_declined(Declined { app: app.map(str::to_string), seen: at, said: at, of });
                return;
            }
        }
        let open = MEETING.with(|m| {
            let mut m = m.borrow_mut();
            match m.as_mut() {
                Some(n) if n.ended.is_none() => {
                    n.ended = Some(at.max(n.started));
                    true
                }
                _ => false,
            }
        });
        if open {
            self.restamp_meeting(None);
            // Listening to this meeting: it is over, so write the notes. (No
            // spoken recap is asked for then.)
            // (A page that the person put in the Trash, or deleted, asks nothing.)
            let page = MEETING.with(|m| m.borrow().as_ref().map(|n| n.page.clone())).filter(|p| self.pages.peek().get(p).is_some_and(|p| !p.trashed));
            // The meeting is over: its page asks where to keep it (its pad
            // does, while that is on the screen).
            if let Some(page) = &page {
                self.ask_place(page, now_ms());
                self.pad_end(page);
            }
            if self.listen_offer.peek().is_some() && *self.listen_offer.peek() == page {
                self.listen_offer.set(None);
            }
            if self.listening.peek().as_ref().map(|l| &l.page) == page.as_ref() && page.is_some() {
                // (If the meeting goes on after all, listening is offered again.)
                MEETING.with(|m| {
                    if let Some(n) = m.borrow_mut().as_mut() {
                        n.cut = true;
                    }
                });
                self.keep_meeting();
                self.stop_listening();
                return;
            }
            self.keep_meeting();
            // Offer to take a spoken recap (the notification opens the box).
            // The AI writes it, so only when there is one.
            // (Not while the pad shows that the meeting is over: one message is enough.)
            if let Some(page) = page.filter(|_| ai::has_ai() && self.pad.peek().is_none()) {
                spawn_forever(async move {
                    if crate::notify::permission().await == crate::notify::Permission::Allowed {
                        crate::notify::show_now(&format!("recap:{page}"), "The meeting ended", "Click to speak a short recap. The AI writes the notes and action items.").await;
                    }
                });
            }
        }
    }

    // ----- the meeting pad (see pad.rs) -----------------------------------

    /// Show the pad for the meeting whose notes page is `page`: the question
    /// whether to listen, or, when the app cannot listen now, the small
    /// form. Says whether a pad shows (Settings can turn it off).
    pub fn open_pad(mut self, page: &str) -> bool {
        use crate::pad::{Form, Pad};
        if !crate::pad::enabled() {
            return false;
        }
        let now = now_ms();
        // (A listener that is finishing its last words is free in a moment:
        // a meeting that starts as another ends still asks.)
        let can_listen = crate::listen::available() && self.listening.peek().as_ref().is_none_or(|l| l.stopping);
        let (started, span) = MEETING.with(|m| m.borrow().as_ref().filter(|n| n.page == page).map(|n| (n.started, n.span))).unwrap_or((now, None));
        let mut pad = Pad::new(page, if can_listen { Form::Ask } else { Form::Small }, now, span);
        pad.started = started;
        self.pad.set(Some(pad));
        self.watch_pad();
        true
    }

    /// The pad's two questions go away by themselves (see `pad::by_itself`).
    fn watch_pad(self) {
        use crate::pad::{ByItself, Form};
        if PAD_WATCH.replace(true) {
            return;
        }
        spawn_forever(async move {
            loop {
                ai::sleep_ms(400).await;
                let Some(pad) = self.pad.peek().clone() else { break };
                // Its page was put in the trash: there is nothing to show.
                if !self.pages.peek().get(&pad.page).is_some_and(|p| !p.trashed) {
                    self.close_pad();
                    continue;
                }
                let asks = self.place_asks.peek().iter().any(|(p, _)| *p == pad.page);
                let now = now_ms();
                match crate::pad::by_itself(pad.form, pad.since, now, asks, pad.busy) {
                    Some(ByItself::Fold) => self.change_pad(|p| p.turn(Form::Small, now)),
                    Some(ByItself::Go) => self.close_pad(),
                    None => {}
                }
            }
            PAD_WATCH.set(false);
        });
    }

    /// Change the pad, if there is one (nothing is drawn again when the
    /// change changes nothing).
    fn change_pad(mut self, change: impl FnOnce(&mut crate::pad::Pad)) {
        let Some(mut pad) = self.pad.peek().clone() else { return };
        let before = pad.clone();
        change(&mut pad);
        if pad != before {
            self.pad.set(Some(pad));
        }
    }

    pub fn close_pad(mut self) {
        if self.pad.peek().is_some() {
            self.pad.set(None);
        }
    }

    /// A click in the pad.
    pub fn pad_command(self, command: crate::pad::Command) {
        use crate::pad::{Command, Form};
        let Some(page) = self.pad.peek().as_ref().map(|p| p.page.clone()) else { return };
        let listens = self.listening.peek().as_ref().is_some_and(|l| l.page == page);
        let now = now_ms();
        match command {
            Command::Listen => {
                self.change_pad(|p| {
                    p.turn(Form::Open, now);
                    p.problem.clear();
                });
                if self.listening.peek().is_none() {
                    self.start_listening(&page);
                }
            }
            Command::NotesOnly | Command::Open => {
                self.confirm_meeting(&page);
                self.change_pad(|p| p.turn(Form::Open, now));
            }
            Command::Small => self.change_pad(|p| p.turn(Form::Small, now)),
            // Never while it listens: the pad is the sign that it does.
            Command::NotNow => {
                if !listens {
                    self.decline_meeting(&page);
                    self.close_pad();
                }
            }
            Command::Stop => {
                if listens {
                    self.stop_listening();
                }
            }
            Command::Lines(lines) => self.pad_lines(&page, lines),
            Command::OpenPage => {
                // (A test run keeps its window where it is.)
                if !crate::pad::inline() {
                    let window = dioxus::desktop::window();
                    window.set_visible(true);
                    window.set_focus();
                }
                self.open_page(&page);
            }
            Command::Keep(home) => {
                self.keep_page_in(&page, &home);
                self.pad_kept(&page);
            }
            Command::Done => self.pad_kept(&page),
        }
    }

    /// The question at the end is answered: the pad says where the notes
    /// are, for a moment, and the page asks no more.
    fn pad_kept(self, page: &str) {
        self.place_done(page);
        // (With several spaces, the top of a list is the space.)
        let (place, _) = place_of(&self.pages.peek(), &self.spaces.peek(), page);
        let kept = if place.id.is_empty() { String::new() } else { place.title };
        let now = now_ms();
        self.change_pad(|p| {
            p.kept = kept;
            p.turn(crate::pad::Form::Kept, now);
        });
    }

    /// The lines typed in the pad go under "Notes" of the meeting's page.
    fn pad_lines(self, page: &str, lines: Vec<String>) {
        let Some(mut written) = self.pad.peek().as_ref().map(|p| p.written.clone()) else { return };
        if !self.pages.peek().contains_key(page) {
            return;
        }
        self.edit_blocks(page, |blocks| crate::pad::put_lines(blocks, &mut written, &lines));
        let typed = lines.iter().any(|l| !l.trim().is_empty());
        if typed {
            self.confirm_meeting(page);
        }
        self.change_pad(|p| {
            p.lines = lines;
            p.written = written;
            p.used |= typed;
        });
    }

    /// The meeting is over, or listening was stopped: the pad asks where to
    /// keep the notes. A pad that was not used (nothing heard, nothing
    /// typed) just goes.
    fn pad_end(self, page: &str) {
        use crate::pad::Form;
        let Some(pad) = self.pad.peek().clone().filter(|p| p.page == page) else { return };
        if matches!(pad.form, Form::End | Form::Kept) {
            return;
        }
        let listens = self.listening.peek().as_ref().is_some_and(|l| l.page == page);
        if !pad.used && !listens {
            return self.close_pad();
        }
        let now = now_ms();
        MEETING.with(|m| {
            if let Some(n) = m.borrow_mut().as_mut().filter(|n| n.page == page) {
                n.padded = true;
            }
        });
        self.change_pad(|p| {
            // (The form it goes back to if the meeting goes on after all.)
            p.was = if p.form == Form::Ask { Form::Small } else { p.form };
            p.turn(Form::End, now);
            p.ended = Some(now);
            if listens {
                // The last words are being finished; then the notes are written.
                p.status = "Finishing…".into();
                p.busy = true;
            } else if !p.heard && !p.lines.is_empty() {
                p.status = "Your notes are in the page.".into();
            }
        });
    }

    /// The app listens for this page's pad.
    fn pad_listens(self, page: &str) {
        self.change_pad(|p| {
            if p.page == page {
                p.heard = true;
                p.used = true;
                p.problem.clear();
            }
        });
    }

    /// Why the app does not listen (or does not hear the others), for the pad.
    fn pad_problem(self, page: &str, problem: &str) {
        self.change_pad(|p| {
            if p.page == page {
                p.problem = problem.to_string();
            }
        });
    }

    /// What became of the notes, for the pad's last card. While `busy` the
    /// card waits; when they are written its time starts anew.
    fn pad_status(self, page: &str, status: &str, busy: bool) {
        let now = now_ms();
        self.change_pad(|p| {
            if p.page == page {
                p.status = status.to_string();
                p.busy = busy;
                if p.form == crate::pad::Form::End {
                    p.since = now;
                }
            }
        });
    }

    /// What the pad draws now (None: there is no pad, or its page is gone).
    /// The theme and the glass colours are added by the window that hands it on.
    pub fn pad_view(&self) -> Option<crate::pad::View> {
        use crate::pad::{Form, Hears, View};
        let pad = self.pad.read().clone()?;
        let (title, icon, parent) = {
            let pages = self.pages.read();
            let page = pages.get(&pad.page).filter(|p| !p.trashed)?;
            (name_before_date(page.display_title()).to_string(), page.icon.clone(), page.parent_id.clone())
        };
        let listening = self.listening.read().clone();
        let hears = listening.as_ref().filter(|l| l.page == pad.page).map(|l| Hears {
            clock: crate::pad::clock(l.seconds),
            latest: l.latest.clone(),
            stopping: l.stopping,
        });
        let when = if matches!(pad.form, Form::End | Form::Kept) {
            let minutes = ((pad.ended.unwrap_or_else(now_ms) - pad.started) as f64 / when::MINUTE as f64).round().max(1.0) as i64;
            format!("Over · {minutes} min")
        } else {
            match pad.span {
                Some((start, end)) => format!("{} – {}", when::time_label(start), when::time_label(end)),
                None => format!("Started {}", when::time_label(pad.started)),
            }
        };
        let mut view = View {
            form: pad.form,
            title,
            when,
            icon,
            can_listen: crate::listen::available() && listening.as_ref().is_none_or(|l| l.page == pad.page),
            has_ai: ai::has_ai(),
            sound: crate::listen::where_speech_goes().to_string(),
            hears,
            lines: pad.lines.clone(),
            status: pad.status.clone(),
            kept: pad.kept.clone(),
            problem: pad.problem.clone(),
            ..View::default()
        };
        if pad.form == Form::End {
            view.asks_place = self.place_asks.read().iter().any(|(p, _)| *p == pad.page);
            (view.here, view.here_space) = self.page_place(&pad.page);
            if view.asks_place {
                view.places = self.meeting_places(&pad.page);
                view.before = self.slot_of_page(&pad.page).0;
            } else if parent.is_some() {
                view.told = format!("These notes are in “{}”, as before.", view.here.title);
            }
        }
        Some(view)
    }

    // ----- export and import (see transfer.rs) ----------------------------

    /// Save one page as a Markdown file the person picks.
    pub fn export_page(self, page_id: &str) {
        let markdown = self.page_markdown(page_id) + "\n";
        let name = format!("{}.md", crate::transfer::file_name(&self.page_title(page_id)));
        spawn_forever(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Export Page as Markdown")
                .set_file_name(&name)
                .add_filter("Markdown", &["md"])
                .save_file()
                .await
            else {
                return;
            };
            match std::fs::write(file.path(), markdown) {
                Ok(()) => self.toast("Exported the page"),
                Err(e) => self.toast(format!("Couldn't export: {e}")),
            }
        });
    }

    /// Save every page as Markdown files, in a new folder inside the folder
    /// the person picks, then show it in Finder.
    pub fn export_all(self) {
        let spaces = self.spaces.peek().clone();
        let pages = self.pages.peek().clone();
        let mut blocks = self.blocks.peek().clone();
        shut_open_pages(&mut blocks, &crate::lock::open_pages());
        spawn_forever(async move {
            let Some(parent) = rfd::AsyncFileDialog::new()
                .set_title("Choose where to save the export")
                .set_can_create_directories(true)
                .pick_folder()
                .await
            else {
                return;
            };
            let parent = parent.path().to_path_buf();
            let (tx, rx) = futures_channel::oneshot::channel();
            ai::runtime().spawn_blocking(move || {
                let result = crate::transfer::new_export_dir(&parent).and_then(|dir| {
                    let n = crate::transfer::export_all(&dir, &spaces, &pages, &blocks)?;
                    Ok((dir, n))
                });
                let _ = tx.send(result.map_err(|e| e.to_string()));
            });
            match rx.await {
                Ok(Ok((dir, n))) => {
                    self.toast(format!("Exported {n} {}", if n == 1 { "page" } else { "pages" }));
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                }
                Ok(Err(e)) => self.toast(format!("Couldn't export: {e}")),
                Err(_) => self.toast("The export stopped"),
            }
        });
    }

    /// Pick a Notion export (.zip) or a Markdown file and import it.
    pub fn import(self) {
        spawn_forever(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Import a Notion export or Markdown")
                .add_filter("Notion export or Markdown", &["zip", "md", "markdown"])
                .pick_file()
                .await
            else {
                return;
            };
            self.import_path(file.path().to_path_buf()).await;
        });
    }

    /// Pick a folder of Markdown files (an unzipped Notion export) and import it.
    pub fn import_folder(self) {
        spawn_forever(async move {
            let Some(dir) = rfd::AsyncFileDialog::new().set_title("Import a folder of Markdown files").pick_folder().await
            else {
                return;
            };
            self.import_path(dir.path().to_path_buf()).await;
        });
    }

    /// Read `path` off the UI thread, then add what it holds to the space in
    /// front and open it.
    pub async fn import_path(mut self, path: std::path::PathBuf) {
        let space = self.ui.peek().space.clone();
        let position = roots_in(&self.pages.peek(), &space).last().map(|p| p.position + 1.0).unwrap_or(1.0);
        self.toast("Importing…");
        let (tx, rx) = futures_channel::oneshot::channel();
        ai::runtime().spawn_blocking(move || {
            let _ = tx.send(crate::transfer::import(&path, &space, position));
        });
        let imported = match rx.await {
            Ok(Ok(imported)) => imported,
            Ok(Err(e)) => return self.toast(format!("Couldn't import: {e}")),
            Err(_) => return self.toast("The import stopped"),
        };
        let Some(top) = imported.pages.first().map(|p| p.id.clone()) else { return };
        let count = imported.pages.len() - usize::from(imported.wrapper);
        store::with(|s| s.insert_pages(&imported.pages, &imported.blocks));
        {
            let mut pages = self.pages.write();
            for p in &imported.pages {
                pages.insert(p.id.clone(), p.clone());
            }
        }
        {
            let mut blocks = self.blocks.write();
            for (id, bs) in &imported.blocks {
                blocks.insert(id.clone(), bs.clone());
            }
        }
        for p in imported.pages {
            sync::others(self.win.key, SyncMsg::Page(p));
        }
        for (id, bs) in imported.blocks {
            sync::others(self.win.key, SyncMsg::Blocks(id, bs));
        }
        self.open_in_new_tab(Route::Page(top));
        self.toast(format!("Imported {count} {}", if count == 1 { "page" } else { "pages" }));
    }

    // ----- page history --------------------------------------------------

    /// Keep a copy of a page as it is about to change (its blocks `before`
    /// the change, or as they are now), at most every ten minutes while it is
    /// being edited. `force` keeps one now. Blank pages are not kept.
    fn keep_version(self, page_id: &str, before: Option<&[Block]>, force: bool) {
        let now = now_ms();
        let last = LAST_VERSION.with(|m| m.borrow().get(page_id).copied()).unwrap_or_else(|| {
            store::with(|s| s.latest_version(page_id)).flatten().map_or(0, |v| v.0)
        });
        if !force && now - last < VERSION_EVERY_MS {
            LAST_VERSION.with(|m| m.borrow_mut().insert(page_id.to_string(), last));
            return;
        }
        LAST_VERSION.with(|m| m.borrow_mut().insert(page_id.to_string(), now));
        let title = self.pages.peek().get(page_id).map(|p| p.title.clone()).unwrap_or_default();
        let blocks = self.blocks.peek();
        let blocks: &[Block] = match before {
            Some(b) => b,
            None => blocks.get(page_id).map(|b| b.as_slice()).unwrap_or_default(),
        };
        let blank = title.trim().is_empty() && blocks.iter().all(|b| b.kind == BlockKind::Text && b.text.trim().is_empty());
        if !blank {
            store::with(|s| s.save_version(page_id, now, &title, blocks));
        }
    }

    /// The page is locked and not open: its earlier versions are not shown
    /// or put back. True, with a word to the person, when that is so.
    fn history_is_shut(self, page_id: &str) -> bool {
        let shut = crate::lock::page_lock(&self, page_id) == crate::lock::LockState::Shut;
        if shut {
            self.toast("Open the page with its password first");
        }
        shut
    }

    /// Show the earlier versions of a page.
    pub fn open_history(mut self, page_id: &str) {
        // The earlier versions of a locked page can hold its lines in plain
        // (sync keeps there the lines written here that a locked page took
        // the place of): not shown while the page is shut. Every way to the
        // history comes through here: the menu of the page, search, the menu bar.
        if self.history_is_shut(page_id) {
            return;
        }
        {
            let mut ui = self.ui.write();
            ui.popover = None;
            ui.history = Some(page_id.to_string());
            ui.modal = Some(Modal::History);
        }
        self.clear_slash();
    }

    /// Put a page back as it was in one of its kept versions. The page as it
    /// is now is kept first, so this can be undone (⌘Z or Page history).
    pub fn restore_version(self, page_id: &str, version_id: i64) {
        if self.history_is_shut(page_id) {
            return self.set_modal(None);
        }
        let Some(version) =
            store::with(|s| s.load_versions(page_id)).and_then(|vs| vs.into_iter().find(|v| v.id == version_id))
        else {
            self.toast("That version is no longer kept");
            return;
        };
        self.keep_version(page_id, None, true);
        if self.pages.peek().get(page_id).is_some_and(|p| p.title != version.title) {
            self.rename_page(page_id, version.title.clone());
        }
        let mut blocks = version.blocks.clone();
        // The rows of its tables go by what they go by now, so that the
        // pages of the rows still find them.
        let now = self.blocks.peek().get(page_id).cloned().unwrap_or_default();
        for b in blocks.iter_mut().filter(|b| b.kind == BlockKind::Table) {
            if let Some(current) = now.iter().find(|c| c.id == b.id && c.kind == BlockKind::Table) {
                let mut table = crate::table::Table::parse(&b.data);
                if table.keep_row_names(&crate::table::Table::parse(&current.data)) {
                    b.data = table.to_data();
                }
            }
        }
        let (back, _) = tables_put_back(&mut blocks, &now, |id| self.table_in_another_page(page_id, id));
        self.edit_blocks(page_id, move |b| *b = blocks);
        for id in &back {
            self.bring_row_pages(page_id, id, &[]);
        }
        self.set_modal(None);
        self.toast(format!("Restored the version from {}", when::moment(version.at)));
    }

    /// A table with this id is in a page other than `page`.
    fn table_in_another_page(&self, page: &str, id: &str) -> bool {
        self.blocks.peek().iter().any(|(p, bs)| p != page && bs.iter().any(|b| b.id == id && b.kind == BlockKind::Table))
    }

    /// Replace a page's blocks wholesale (undo, redo, changes from elsewhere).
    fn set_blocks(mut self, page_id: &str, new: Vec<Block>, save: bool) {
        let old = self.blocks.peek().get(page_id).cloned().unwrap_or_default();
        if old == new {
            return;
        }
        // (A locked page that is not open takes no lines, here a transcript:
        // see `edit_blocks_from`. What other windows and the notes file say
        // of the page comes with `save` off, and is taken.)
        if save && crate::lock::sealed(&old).is_some() {
            return self.tell_locked(page_id);
        }
        // (What comes to the page from outside the history, the lines of a
        // transcript or an AI's words, ends the run of typing that is going
        // on: joined into that one step, Undo of the typing also took the
        // new lines away.)
        self.history.write().seal();
        let changed: Vec<Id> = {
            let old_by_id: HashMap<&str, &Block> = old.iter().map(|o| (o.id.as_str(), o)).collect();
            new.iter()
                .filter(|b| old_by_id.get(b.id.as_str()).is_some_and(|o| o.text != b.text || o.kind != b.kind))
                .map(|b| b.id.clone())
                .collect()
        };
        self.blocks.write().insert(page_id.to_string(), new);
        if save {
            self.save_blocks(page_id);
        }
        if self.slash.peek().as_ref().is_some_and(|s| s.page_id == page_id) {
            self.slash.set(None);
        }
        self.sync_blocks(page_id, &changed);
    }

    /// The user typed into a block; `value` is its stored text (inline Markdown).
    /// `pos` is where the caret is (see `Bridge::Input`).
    pub fn on_block_input(mut self, page_id: &str, block_id: &str, value: String, pos: Option<usize>) {
        let mut caret = None;
        let mut slash = None;
        let mut typed_to = 0;
        let mut mid_line = false;
        let origin = Origin::Typing { block: block_id, text: &value };
        self.edit_blocks_from(page_id, origin, |bs| {
            let Some(i) = index_of(bs, block_id) else { return };
            let code = bs[i].kind == BlockKind::Code;
            let plain = inline::plain_for(bs[i].kind, &value);
            if bs[i].kind == BlockKind::Text {
                if let Some(sc) = markdown::detect_shortcut(&plain) {
                    if sc.kind == BlockKind::Divider {
                        bs[i].text.clear();
                        bs[i].turn_into(BlockKind::Divider);
                        let next = Block::text("").with_indent(bs[i].indent);
                        caret = Some(Caret::start(&next.id));
                        bs.insert(i + 1, next);
                    } else {
                        let prefix = utf16_len(&plain[..sc.prefix_len]);
                        bs[i].text = inline::split_at(&value, prefix).1;
                        bs[i].turn_into(sc.kind);
                        bs[i].checked = sc.checked;
                        caret = Some(Caret::start(block_id));
                    }
                    return;
                }
            }
            bs[i].text = value.clone();
            if !code {
                // What is typed for a menu ends at the caret. (It was read
                // to the end of the line: "[[Pr" typed in the middle of a
                // line asked for a page named with the rest of the line,
                // and Return made that page and took those words out.)
                let typed = &plain[..pos.map_or(plain.len(), |p| utf16_to_byte(&plain, p))];
                slash = markdown::slash_query(typed).map(|(at, q)| (utf16_len(&typed[..at]), q, false, false));
                if slash.is_none() {
                    slash = markdown::mention_query(typed).map(|(at, wiki, q)| (utf16_len(&typed[..at]), q, true, wiki));
                }
                typed_to = utf16_len(typed);
                mid_line = typed.len() < plain.len();
            }
        });
        // With words after the caret, "/" opens its list only while a
        // command fits: "see /usr" typed in the middle of a line is words,
        // and the arrows, Return and Tab stay with the line. (Before the
        // caret was known, no list opened there at all. At the end of a
        // line the list says "No results", as it did.)
        if let Some((_, query, false, _)) = &slash {
            if mid_line && filter_slash(query).is_empty() {
                slash = None;
            }
        }
        // "@" opens the list only while a page fits what is typed, so that
        // "@5pm" stays text and Return stays Return.
        if let Some((_, query, true, false)) = &slash {
            if pages_to_link(&self.pages.peek(), &self.ui.peek().space, page_id, query).is_empty() {
                slash = None;
            }
        }

        let current = self.slash.peek().clone();
        match slash {
            Some((slash_at, query, pages, wiki)) => {
                let selected = match &current {
                    Some(s) if s.block_id == block_id && s.query == query => s.selected,
                    _ => 0,
                };
                let next = SlashState {
                    page_id: page_id.to_string(),
                    block_id: block_id.to_string(),
                    slash_at,
                    end: typed_to,
                    added: false,
                    query,
                    selected,
                    pages,
                    wiki,
                };
                if current.as_ref() != Some(&next) {
                    self.slash.set(Some(next));
                    if self.ui.peek().popover.is_some() {
                        self.ui.write().popover = None;
                    }
                }
            }
            None => {
                if current.is_some() {
                    self.slash.set(None);
                }
            }
        }
        if let Some(c) = caret {
            self.focus(c);
        }
    }

    pub fn handle_bridge(mut self, msg: Bridge) {
        match msg {
            Bridge::Input { page, id, md, pos } => {
                self.clear_picked();
                self.on_block_input(&page, &id, md, pos)
            }
            Bridge::Enter { page, id, pos, value } => {
                let caret = self.edit_blocks(&page, |bs| {
                    index_of(bs, &id).map(|i| press_enter(bs, i, pos, &value))
                });
                if let Some(c) = caret {
                    self.focus(c);
                }
            }
            Bridge::Backspace { page, id, value } => {
                // The line above is under a folded heading (a heading
                // nested in a toggle or a list ends with it): the heading
                // opens, and the key joins or takes out nothing yet.
                let above = self.blocks.peek().get(&page).and_then(|bs| index_of(bs, &id).and_then(|i| backspace_reaches(bs, i)).map(|j| bs[j].id.clone()));
                if above.is_some_and(|line| self.show_line(&page, &line)) {
                    return;
                }
                let caret = self.edit_blocks(&page, |bs| {
                    index_of(bs, &id).and_then(|i| press_backspace_at_start(bs, i, &value))
                });
                if let Some(c) = caret {
                    self.focus(c);
                }
            }
            Bridge::Delete { page, id, value } => {
                // At the end of a folded heading the next line is not in
                // view: the heading opens, and the key joins nothing yet.
                if self.is_folded(&page, &id) {
                    return self.set_fold(&page, &id, false);
                }
                let caret = self.edit_blocks(&page, |bs| index_of(bs, &id).and_then(|i| press_delete_at_end(bs, i, &value)));
                if let Some(c) = caret {
                    self.focus(c);
                }
            }
            Bridge::Paste { page, id, pos, value, text } => self.paste(&page, &id, pos, value, &text),
            Bridge::Undo => self.undo(),
            Bridge::Redo => self.redo(),
            Bridge::OpenUrl { url } => self.open_url(&url),
            Bridge::Drag => dioxus::desktop::window().drag(),
            Bridge::ZoomWindow => dioxus::desktop::window().toggle_maximized(),
            Bridge::TabDrop { from, to, mode } => self.drop_tab(&from, to.as_deref(), &mode),
            Bridge::PaneStep { pane, back } => self.step_pane(pane.as_deref(), back),
            Bridge::DrawingSave { page, id, data } => self.save_drawing(&page, &id, data),
            Bridge::ImageData { page, id, data } => self.paste_image(&page, &id, &data),
            Bridge::CellPicture { page, id, r, c, data } => self.paste_cell_picture(&page, &id, r, c, &data),
            Bridge::ImagePlace { page, id, x, w, right, to, after } => {
                let place = Place { free: true, right, x: x.round() as i32, y: 0, w: w.round().max(0.0) as u32, beside: false };
                self.edit_blocks(&page, |bs| place_picture(bs, &id, place, to.as_deref(), after));
            }
            Bridge::ImageSize { page, id, w } => {
                self.edit_blocks(&page, |bs| {
                    let now = index_of(bs, &id).map(|i| bs[i].place).unwrap_or_default();
                    place_picture(bs, &id, Place { w: w.round().max(0.0) as u32, ..now }, None, false)
                });
            }
            Bridge::TableOp { page, id, op, row, column } => self.table_op(&page, &id, &op, row, column),
            Bridge::TableSet { page, id, row, column, text } => self.put_table_cell(&page, &id, row, column, &text),
            Bridge::CoverY { page, y } => self.set_cover_y(&page, y.round().clamp(0.0, 100.0) as u8),
            Bridge::PlayerPlace { x, y, w, h } => self.place_player(crate::player::Place { x, y, w, h }),
            Bridge::PlayerDock { h, v, x, y, w } => self.set_player_dock(DockPlace { h, v, x, y, w }),
            Bridge::SwipeTrace { lines } => swipe_trace(&lines),
            Bridge::AiImage { data } => self.ai_add_picture(&data),
            Bridge::AiFile { name, data } => self.ai_add_file(&name, &data),
            Bridge::EventMove { id, days, minutes, grow } => self.move_event(&id, days, minutes, grow),
            Bridge::EventDraw { day, from, to } => self.draw_event(&day, from, to),
            Bridge::CalPress { minute } => CAL_PRESS.with(|p| p.set(Some((now_ms(), minute)))),
            Bridge::PagesFind { q, n, here } => self.find_pages_for_at(&q, n, &here),
            Bridge::PagePreview { id, n } => self.preview_page(&id, n),
            Bridge::AskGivePage { id } => self.give_page_to_ai(&id),
            Bridge::EventCopy { id } => self.copy_event(&id),
            Bridge::EventPaste { day, minute } => self.paste_event(&day, minute),
            Bridge::TodoPlan { page, block, day, minutes } => self.plan_todo(&page, &block, &day, minutes),
            Bridge::DrawingReady { id } => self.send_drawing(&id),
            Bridge::SegDrop { tab, pane, to, mode } => self.detach_pane(&tab, &pane, to.as_deref(), &mode),
            Bridge::PlaceTab { from, target, edge } => {
                if let Some(edge) = Edge::parse(&edge) {
                    self.place_tab(&from, &target, edge);
                }
            }
            Bridge::PlacePage { page, target, edge } => {
                if let Some(edge) = Edge::parse(&edge) {
                    self.place_page(&page, &target, edge);
                }
            }
            Bridge::PlacePane { tab, pane, target, edge } => {
                if let Some(edge) = Edge::parse(&edge) {
                    self.place_pane(&tab, &pane, &target, edge);
                }
            }
            Bridge::SplitSizes { split, sizes } => {
                let changed = {
                    let mut ui = self.ui.write();
                    let tab = ui.tab_mut();
                    let rails = tab.rails();
                    tab.layout.set_open_sizes(&split, &sizes, &rails)
                };
                if changed {
                    self.save_ui();
                }
            }
            Bridge::FoldPane { pane, on } => self.fold_pane(&pane, on),
            Bridge::SpeakEdit { page, id, start, end } => self.open_voice(VoiceFor::Edit { page, block: id, start, end }),
            Bridge::CopyText { text } => {
                if let Err(e) = set_clipboard(text) {
                    self.toast(format!("Couldn't copy: {e}"));
                }
            }
            Bridge::CopyPicture { name } => self.copy_picture(&name),
            Bridge::PageStripDrop { page, to, mode } => self.drop_page_on_strip(&page, to.as_deref(), &mode),
            Bridge::PageMove { page, target, mode } => self.move_page(&page, target.as_deref(), &mode),
            Bridge::PickAll { page } => {
                let ids: Vec<Id> = self.blocks.peek().get(&page).map(|bs| bs.iter().map(|b| b.id.clone()).collect()).unwrap_or_default();
                self.picked.set((!ids.is_empty()).then_some(Picked { page, ids }));
            }
            Bridge::Pick { page, id, extend } => {
                let now = self.picked.peek().clone().filter(|p| p.page == page);
                let ids = self.blocks.peek().get(&page).map(|bs| pick_blocks(bs, now.as_ref().map(|p| p.ids.as_slice()).filter(|_| extend), &id)).unwrap_or_default();
                self.picked.set((!ids.is_empty()).then_some(Picked { page, ids }));
            }
            Bridge::PickClear => self.clear_picked(),
            Bridge::ImageDelete { page, id } => self.delete_image(&page, &id),
            Bridge::SidebarWidth { w } => {
                self.ui.write().sidebar_width = w.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                self.save_ui();
            }
            Bridge::Block { page, id, action, pos } => self.block_action(&page, &id, &action, pos),
            Bridge::Nav { page, id, dir } => {
                let blocks = self.blocks.peek().get(&page).cloned().unwrap_or_default();
                let Some(i) = index_of(&blocks, &id) else { return };
                // (Over the lines of a folded heading, as over those of a closed toggle.)
                let vis = shown(&blocks, &self.folds_of(&page));
                if dir == "up" {
                    match prev_editable_of(&blocks, i, &vis) {
                        Some(j) => self.focus(Caret::end(&blocks[j].id)),
                        None => self.focus_title(&page),
                    }
                } else if let Some(j) = next_editable_of(&blocks, i, &vis) {
                    self.focus(Caret::start(&blocks[j].id));
                }
            }
            Bridge::Menu { action } => self.slash_key(&action),
            Bridge::Move { page, from, to, after } => {
                // A folded heading goes with the lines it hides. (A line let
                // go under a folded heading opens it: `show_newly_hidden`.)
                let section = self.is_folded(&page, &from);
                self.edit_blocks(&page, |bs| move_lines(bs, &from, &to, after, section));
            }
            Bridge::Shortcut { name } => self.command(&name),
            Bridge::Focus { page, id } => {
                self.clear_picked();
                if self.slash.peek().as_ref().is_some_and(|s| s.block_id != id) {
                    self.slash.set(None);
                }
                // Typing in a pane gives it focus.
                let pane = self.pane_of_page(&page);
                if let Some(pane) = pane {
                    self.focus_pane(&pane);
                }
                self.focus.set(Some((page, id)));
            }
        }
    }

    pub fn block_action(self, page: &str, id: &str, action: &str, pos: usize) {
        // From the page script: open a folded heading (find in page came to
        // words under it), or the headings that hide a line (the outline).
        match action {
            "unfold" => return self.set_fold(page, id, false),
            "show" => {
                self.show_line(page, id);
                return;
            }
            _ => {}
        }
        let mut refocus = Some(Caret { id: id.to_string(), pos });
        let mut opened = false;
        let mut fold = false;
        let folded = self.is_folded(page, id);
        self.edit_blocks(page, |bs| {
            let Some(i) = index_of(bs, id) else { return };
            match action {
                "indent" => {
                    indent(bs, i);
                }
                "outdent" => {
                    outdent(bs, i);
                }
                "move_up" => {
                    move_up(bs, i);
                }
                "move_down" => {
                    move_down(bs, i);
                }
                "duplicate" => {
                    // (A folded heading: with the lines it hides, after them.)
                    let copy = if folded { duplicate_lines(bs, i, section_end(bs, i).max(subtree_end(bs, i))) } else { duplicate(bs, i) };
                    refocus = Some(Caret::end(&copy));
                }
                "toggle" => match bs[i].kind {
                    BlockKind::Todo => bs[i].checked = !bs[i].checked,
                    BlockKind::Toggle => {
                        bs[i].collapsed = !bs[i].collapsed;
                        opened = !bs[i].collapsed;
                    }
                    // (The same key folds a heading and opens it.)
                    kind if heading_level(kind).is_some() => fold = true,
                    _ => {}
                },
                _ => refocus = None,
            }
        });
        if opened {
            self.show_reveal(page, id);
        }
        if fold {
            self.toggle_fold(page, id);
        }
        if let Some(c) = refocus {
            self.focus(c);
        }
        self.set_popover(None);
    }

    /// Every to-do not yet done in this space, from the pages edited last
    /// first (and in page order inside a page). Subscribes the caller to
    /// pages and blocks, so the list follows what is typed and ticked.
    pub fn open_todos(&self) -> Vec<OpenTodo> {
        self.open_todos_of(false)
    }

    /// The open to-dos of the space in front, or of every space (for the
    /// calendar, when it shows every space).
    pub fn open_todos_of(&self, every_space: bool) -> Vec<OpenTodo> {
        let space = self.ui.read().space.clone();
        let pages = self.pages.read();
        let blocks = self.blocks.read();
        let mut list: Vec<&Page> = pages.values().filter(|p| (every_space || p.space_id == space) && !p.trashed).collect();
        list.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        let mut out = Vec::new();
        for p in list {
            for b in blocks.get(&p.id).map(|b| b.as_slice()).unwrap_or_default() {
                if b.kind == BlockKind::Todo && !b.checked && !b.text.trim().is_empty() {
                    out.push(OpenTodo { page: p.id.clone(), page_title: p.display_title().to_string(), block: b.id.clone(), text: b.plain_text(), due: b.due });
                }
            }
        }
        // Those with a day first, the earliest at the top; the others keep
        // their order (the page changed last comes first).
        out.sort_by_key(|t| (t.due.is_none(), t.due));
        out
    }

    /// The open to-dos that are due on `day`. Today also has those that
    /// were due before it and are not done.
    pub fn todos_due(&self, day: chrono::NaiveDate) -> Vec<OpenTodo> {
        let today = when::today();
        let every_space = self.cal.read().all_spaces;
        self.open_todos_of(every_space).into_iter().filter(|t| t.due.is_some_and(|d| d == day || (day == today && d < today))).collect()
    }

    // ----- the video player ---------------------------------------------------

    /// Play a video in the corner of the window. The page draws the frame;
    /// the page script says where the frame is, and the player goes there
    /// (`place_player`).
    pub fn play_video(mut self, video: crate::video::Video) {
        crate::player::hide();
        // What the player says of itself comes back here: a video that
        // cannot play gets its reason in the frame, in place of the player.
        let (tx, mut rx) = futures_channel::mpsc::unbounded();
        PLAYER_SAYS.with(|says| *says.borrow_mut() = Some(tx));
        spawn_forever(async move {
            while let Some(said) = rx.next().await {
                match said {
                    Some(reason) => {
                        crate::player::hide();
                        if let Some(p) = self.player.write().as_mut() {
                            p.problem = Some(reason.to_string());
                        }
                    }
                    None => {
                        if let Some(p) = self.player.write().as_mut() {
                            p.ready = true;
                        }
                    }
                }
            }
        });
        self.player.set(Some(Playing { video, from: None, problem: None, ready: false }));
        document::eval("requestAnimationFrame(()=>requestAnimationFrame(()=>window.__rune_player && window.__rune_player()))");
    }

    /// Play a video from its line in a page (the player remembers the line).
    pub fn play_video_from(mut self, video: crate::video::Video, page: &str, block: &str) {
        // Another video takes the player: the notes of the one before end.
        self.stop_video_notes();
        self.play_video(video);
        if let Some(playing) = self.player.write().as_mut() {
            playing.from = Some((page.to_string(), block.to_string()));
        }
    }

    /// Write down what the video in the player says, and make notes of it.
    /// Only what the Mac plays is heard; the microphone is not used. The
    /// words go under the video's line in its page as they are heard, in a
    /// closed toggle; when it stops, the AI writes the notes over them, in
    /// an open one. With no AI tool the words are what there is.
    pub fn take_video_notes(mut self) {
        if self.listening.peek().is_some() {
            return self.toast("Already listening");
        }
        if listener_runs() {
            return self.toast("Rune is listening in another window. Stop it there first.");
        }
        let Some(playing) = self.player.peek().clone() else { return };
        // The page the video's line is in; a video with no line (or a line
        // that is gone) writes at the end of the page on screen.
        let from = playing.from.filter(|(page, _)| self.pages.peek().get(page).is_some_and(|p| !p.trashed));
        let (page, line) = match from {
            Some((page, line)) => (page, Some(line)),
            None => match self.current_page() {
                Some(page) => (page, None),
                None => return self.toast("Open a page first"),
            },
        };
        let (listener, mut heard) = match crate::listen::start_from(crate::listen::Source::Sound) {
            Ok(started) => started,
            Err(e) => return self.notice(e),
        };
        LISTENER.with(|l| *l.borrow_mut() = Some(listener));
        LISTENER_WINDOW.with(|w| *w.borrow_mut() = Some((self.win.key, page.clone())));
        let started = now_ms();
        self.listening.set(Some(Listening { page: page.clone(), started, seconds: 0, others: true, words: 0, latest: String::new(), stopping: false, video: true }));
        spawn_forever(async move {
            loop {
                ai::sleep_ms(1000).await;
                let mut guard = self.listening.write();
                match guard.as_mut().filter(|l| l.started == started) {
                    Some(l) => l.seconds = (now_ms() - started) / 1000,
                    None => break,
                }
            }
        });
        spawn_forever(async move {
            use crate::listen::Heard;
            let last_words = |text: &str| {
                let words: Vec<&str> = text.split_whitespace().collect();
                words[words.len().saturating_sub(9)..].join(" ")
            };
            // (A copy of the state for the closure: it is used between the writes below.)
            let put = {
                let (state, page, line) = (self, page.clone(), line.clone());
                move |toggle: &mut Option<Id>, title: &str, closed: bool, lines: Vec<Block>| {
                    let Some(mut blocks) = state.blocks.peek().get(&page).cloned() else { return };
                    crate::listen::put_under(&mut blocks, toggle, title, closed, line.as_deref(), lines);
                    state.set_blocks(&page, blocks, true);
                }
            };
            let mut transcript = crate::listen::Transcript::default();
            let (mut words, mut notes) = (None, None);
            let mut written_at = now_ms();
            let mut problem = None;
            while let Some(event) = heard.next().await {
                match event {
                    Heard::Listening { .. } => {}
                    Heard::Partial { text, .. } => {
                        if let Some(l) = self.listening.write().as_mut() {
                            l.latest = last_words(&text);
                        }
                    }
                    Heard::Final { text, at, .. } => {
                        transcript.add(crate::listen::Who::Others, &text, at);
                        if let Some(l) = self.listening.write().as_mut() {
                            l.words = transcript.words();
                            l.latest = last_words(&text);
                        }
                        // Into the page now and then, so nothing is lost if the app closes.
                        if now_ms() - written_at > 20_000 {
                            written_at = now_ms();
                            put(&mut words, "Transcript of the video", true, transcript.plain_blocks());
                        }
                    }
                    Heard::Note(message) => self.notice(message),
                    Heard::Error(message) => {
                        problem = Some(message);
                        break;
                    }
                    Heard::Done => break,
                }
            }
            LISTENER.with(|l| l.borrow_mut().take());
            LISTENER_WINDOW.with(|w| w.borrow_mut().take());
            put(&mut words, "Transcript of the video", true, transcript.plain_blocks());
            self.listening.set(None);
            self.relock_unshown();
            if let Some(message) = problem {
                return self.notice(message);
            }
            if transcript.words() == 0 {
                return self.notice("Nothing was heard of the video. Check that it plays with sound, and that Rune may record the Mac's audio (System Settings, Privacy & Security).");
            }
            // The AI writes the notes; without one, the words are what there is.
            // (What was heard into a locked page is not given to the AI.)
            if !ai::has_ai() || transcript.words() < 12 || self.locked_lines(&page) {
                return self.toast("The words of the video are under it");
            }
            self.toast("Writing the notes…");
            let messages = vec![
                ai::ChatMessage::system(crate::listen::VIDEO_NOTES),
                ai::ChatMessage::user(crate::listen::for_ai(&transcript.plain_text(), 120_000)),
            ];
            let mut rx = ai::start(ai::Prefs::load(), messages);
            let mut reply = String::new();
            let mut error = None;
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(text) => reply.push_str(&text),
                    AiEvent::Done => break,
                    AiEvent::Error(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            // Its points, one step in (they go inside a toggle). A heading it
            // wrote all the same is not a point.
            let points: Vec<Block> = markdown::to_blocks(ai::clean_reply(&reply).0.trim())
                .into_iter()
                .filter(|b| !matches!(b.kind, BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3 | BlockKind::Divider) && !b.text.trim().is_empty())
                .map(|mut b| {
                    b.indent += 1;
                    b
                })
                .collect();
            let wrote = !points.is_empty() && self.pages.peek().contains_key(&page);
            if wrote {
                put(&mut notes, "Notes of the video", false, points);
            }
            match (error, wrote) {
                (Some(e), false) => self.notice(format!("The words of the video are under it, but the AI could not write the notes. {e}")),
                (_, false) => self.toast("The words of the video are under it"),
                _ => self.toast("The notes are under the video"),
            }
        });
    }

    /// The video's notes end (the player closes, or plays another video).
    fn stop_video_notes(self) {
        if self.listening.peek().as_ref().is_some_and(|l| l.video && !l.stopping) {
            self.stop_listening();
        }
    }

    /// The frame of the player is here in the window: put the player on it.
    fn place_player(mut self, place: crate::player::Place) {
        let Some(playing) = self.player.peek().clone().filter(|p| p.problem.is_none()) else { return };
        // A test run has the frame and no player (nothing is fetched): it
        // notes where the player was told to go, so a check can read it.
        if std::env::var_os("RUNE_SELFTEST").is_some() {
            document::eval(&format!("document.body.dataset.playerAt = '{:.0},{:.0},{:.0},{:.0}'", place.x, place.y, place.w, place.h));
        }
        if crate::video::stand_in() || place.w < 40.0 {
            return;
        }
        let says = PLAYER_SAYS.with(|says| says.borrow().clone());
        let heard = move |said: Option<&'static str>| {
            if let Some(tx) = &says {
                let _ = tx.unbounded_send(said);
            }
        };
        let window = dioxus::desktop::window();
        if let Err(problem) = crate::player::show(&*window.window, &playing.video, place, heard) {
            if let Some(p) = self.player.write().as_mut() {
                p.problem = Some(problem);
            }
        }
    }

    /// Put the picture of the video under a line away (the link stays), or
    /// bring it back.
    pub fn show_video_under(self, page: &str, id: &str, show: bool) {
        self.set_popover(None);
        let id = id.to_string();
        self.edit_blocks(page, move |bs| {
            if let Some(b) = bs.iter_mut().find(|b| b.id == id && crate::video::in_block(b.kind, &b.text).is_some()) {
                b.data = if show { String::new() } else { crate::video::HIDDEN.to_string() };
            }
        });
    }

    /// Open Settings at one of its parts ("AI", "Data"…).
    pub fn open_settings_at(mut self, part: &'static str) {
        self.settings_first.set(Some(part));
        self.set_modal(Some(Modal::Settings));
    }

    /// The person moved the player or gave it another size: it is kept,
    /// and the player goes where its frame is now.
    pub fn set_player_dock(mut self, place: DockPlace) {
        let place = place.tidy();
        if *self.player_dock.peek() != place {
            store::set_setting("player_dock", &serde_json::to_string(&place).unwrap_or_default());
            self.player_dock.set(place);
        }
        document::eval("requestAnimationFrame(()=>requestAnimationFrame(()=>window.__rune_player && window.__rune_player()))");
    }

    pub fn close_player(mut self) {
        // The video is over for the person: its notes are written.
        self.stop_video_notes();
        crate::player::hide();
        PLAYER_SAYS.with(|says| says.borrow_mut().take());
        if self.player.peek().is_some() {
            self.player.set(None);
        }
    }

    /// Give a voice of a meeting's transcript a name ("Others" becomes "Sam").
    pub fn rename_voice(self, page: &str, old: &str, new: &str) {
        self.edit_blocks(page, |bs| crate::listen::rename_voice(bs, old, new));
    }

    /// Give a to-do the day it is due, or take the day away.
    pub fn set_due(self, page: &str, id: &str, due: Option<chrono::NaiveDate>) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                bs[i].due = due;
                // (A reminder needs a day.)
                if due.is_none() {
                    bs[i].remind = None;
                }
            }
        });
        crate::notify::mark_dirty();
        self.set_popover(None);
    }

    /// The time of a to-do's reminder (None: no reminder). A to-do with no
    /// day gets today, or tomorrow when the time has passed today; so does
    /// one whose day with that time is gone (see `day_for_reminder`).
    pub fn set_reminder(self, page: &str, id: &str, time: Option<chrono::NaiveTime>) {
        let today = crate::when::today();
        let now = now_ms();
        // The time box of the menu sends each step of a time that is typed
        // ("1", then "16"). The day is worked out from the day the to-do
        // had when the menu opened, at each step: a step that is past (01:00)
        // moved the day to tomorrow, and it stayed there at 16:00.
        let was = match &self.ui.peek().popover {
            Some(Popover::Due { block_id, was }) if block_id == id => Some(*was),
            _ => None,
        };
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                bs[i].remind = time;
                if let Some(t) = time {
                    bs[i].due = Some(day_for_reminder(was.unwrap_or(bs[i].due), t, today, now));
                }
            }
        });
        crate::notify::mark_dirty();
    }

    /// Every open to-do with a reminder still to come, in all spaces:
    /// (its page, its line, the page's name, the words, when).
    pub fn todo_reminders(&self) -> Vec<(Id, Id, String, String, i64)> {
        let now = now_ms();
        let today = crate::when::today();
        let pages = self.pages.peek();
        let blocks = self.blocks.peek();
        let mut out = Vec::new();
        for (page, list) in blocks.iter() {
            let Some(p) = pages.get(page).filter(|p| !p.trashed) else { continue };
            for b in list {
                if let Some(at) = b.remind_at().filter(|&t| t > now) {
                    out.push((page.clone(), b.id.clone(), p.display_title().to_string(), b.plain_text(), at));
                }
                // The days of a table with a reminder: "Rent · Due".
                if b.kind == BlockKind::Table {
                    let table = crate::table::Table::parse(&b.data);
                    for (r, c, at) in table.date_reminders(today) {
                        let at = crate::when::at(at.date(), at.time());
                        if at > now {
                            let row = table.title(r);
                            let words = if row.is_empty() { table.heading(c) } else { format!("{row} · {}", table.heading(c)) };
                            out.push((page.clone(), format!("{}-{r}-{c}", b.id), p.display_title().to_string(), words, at));
                        }
                    }
                }
            }
        }
        out.sort_by_key(|r| r.4);
        out
    }

    pub fn toggle_check(self, page: &str, id: &str) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                bs[i].checked = !bs[i].checked;
            }
        });
        // (A done to-do sends no reminder.)
        crate::notify::mark_dirty();
    }

    pub fn toggle_collapsed(self, page: &str, id: &str) {
        let opened = self.edit_blocks(page, |bs| {
            let i = index_of(bs, id)?;
            bs[i].collapsed = !bs[i].collapsed;
            Some(!bs[i].collapsed)
        });
        if opened == Some(true) {
            self.show_reveal(page, id);
        }
    }

    // ----- folded headings ------------------------------------------------

    /// The headings folded on a page, in this window.
    pub fn folds_of(&self, page: &str) -> HashSet<Id> {
        self.ui.peek().folds.get(page).cloned().unwrap_or_default()
    }

    pub fn is_folded(&self, page: &str, id: &str) -> bool {
        self.ui.peek().folds.get(page).is_some_and(|ids| ids.contains(id))
    }

    /// Fold a heading (its lines are hidden, up to the next heading of its
    /// level or a higher one) or open it again. No line is changed: what is
    /// folded is part of the window's layout. Entries of headings that are
    /// gone from the page, or are headings no more, go at each change.
    pub fn set_fold(mut self, page: &str, id: &str, on: bool) {
        let live: Option<HashSet<Id>> = {
            let all = self.blocks.peek();
            // (The lines of a locked page that is not open cannot be seen.)
            all.get(page).filter(|bs| crate::lock::sealed(bs).is_none()).map(|bs| bs.iter().filter(|b| heading_level(b.kind).is_some()).map(|b| b.id.clone()).collect())
        };
        let Some(live) = live else { return };
        if on == self.is_folded(page, id) || (on && !live.contains(id)) {
            return;
        }
        // (Only a fold can hide the line of an open "/" list. A heading that
        // opens for the new "/" line of its "+" leaves that list open.)
        if on {
            self.clear_slash();
        }
        // The line with the caret goes out of sight: the heading is the
        // line in use from now on, and the caret goes to its end when the
        // keyboard is left with no line (`caretLeft` in bridge.js).
        let lost = self.focus.peek().clone().filter(|(p, line)| {
            on && p == page && self.blocks.peek().get(page).is_some_and(|bs| !folds_hiding(bs, &HashSet::from([id.to_string()]), line).is_empty())
        });
        if let Some((_, line)) = lost {
            self.focus.set(Some((page.to_string(), id.to_string())));
            let (gone, head) = (serde_json::to_string(&format!("blk-{line}")).unwrap_or_default(), serde_json::to_string(&format!("blk-{id}")).unwrap_or_default());
            document::eval(&format!("window.__rune && window.__rune.caretLeft && window.__rune.caretLeft({gone}, {head})"));
        }
        {
            let mut ui = self.ui.write();
            let ids = ui.folds.entry(page.to_string()).or_default();
            if on {
                ids.insert(id.to_string());
            } else {
                ids.remove(id);
            }
            ids.retain(|h| live.contains(h));
            if ids.is_empty() {
                ui.folds.remove(page);
            }
        }
        self.save_ui();
        if !on {
            self.show_reveal(page, id);
        }
    }

    /// A page changed: a folded heading that is gone from it, or is a
    /// heading no more, is folded no more. (Made a heading again later, the
    /// line would fold by itself.)
    fn drop_dead_folds(mut self, page: &str, bs: &[Block]) {
        let live = |id: &Id| bs.iter().any(|b| b.id == *id && heading_level(b.kind).is_some());
        let dead = self.ui.peek().folds.get(page).is_some_and(|ids| !ids.iter().all(live));
        if !dead || crate::lock::sealed(bs).is_some() {
            return;
        }
        {
            let mut ui = self.ui.write();
            if let Some(ids) = ui.folds.get_mut(page) {
                ids.retain(live);
                if ids.is_empty() {
                    ui.folds.remove(page);
                }
            }
        }
        self.save_ui();
    }

    pub fn toggle_fold(self, page: &str, id: &str) {
        self.set_fold(page, id, !self.is_folded(page, id));
    }

    /// Open every folded heading that hides this line. Says whether one did.
    pub fn show_line(self, page: &str, id: &str) -> bool {
        let hiding = {
            let folds = self.folds_of(page);
            self.blocks.peek().get(page).map(|bs| folds_hiding(bs, &folds, id)).unwrap_or_default()
        };
        for heading in &hiding {
            self.set_fold(page, heading, false);
        }
        !hiding.is_empty()
    }

    /// An edit never puts a line out of sight that the person did not
    /// hide: a line that showed before the edit and is under a folded
    /// heading after it (a line moved there, a heading moved or made a
    /// higher one over it) opens that heading.
    fn show_newly_hidden(self, page: &str, before: &[Block], after: &[Block]) {
        let folds = self.folds_of(page);
        for id in newly_hidden(before, after, &folds) {
            self.show_line(page, &id);
        }
    }

    /// Let the children of a toggle that just opened ease in.
    fn show_reveal(mut self, page: &str, id: &str) {
        let key = (page.to_string(), id.to_string());
        self.reveal.set(Some(key.clone()));
        spawn_forever(async move {
            ai::sleep_ms(700).await;
            if self.reveal.peek().as_ref() == Some(&key) {
                self.reveal.set(None);
            }
        });
    }

    /// Give the words of a line a colour ("red"), or its band one
    /// ("red-bg"); `band` says which of the two is set, and "" takes that
    /// one away. The other stays: a line can have both. (The menu stays
    /// open, so that both can be chosen in one go.)
    pub fn set_block_tint(self, page: &str, id: &str, tint: &str, band: bool) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                let (words, behind) = bs[i].tint_parts();
                let both = if band { format!("{words} {tint}") } else { format!("{tint} {behind}") };
                bs[i].tint = Block::tint_or_none(&both);
            }
        });
    }

    pub fn turn_into(self, page: &str, id: &str, kind: BlockKind) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                bs[i].turn_into(kind);
            }
        });
        self.set_popover(None);
        self.focus(Caret::end(id));
    }

    // ----- pictures, tables and covers ----------------------------------------

    /// A picture arrived from the page (pasted or dropped) at a block.
    /// A screenshot (or any picture) pasted into or dropped on a picture
    /// cell: kept with the other pictures, and added to the cell.
    fn paste_cell_picture(self, page: &str, id: &str, row: usize, column: usize, base64: &str) {
        use base64::Engine;
        let bytes = match base64::engine::general_purpose::STANDARD.decode(base64.trim()) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => return self.notice("That picture is too large (the most is 30 MB)."),
            Err(_) => return self.notice("That picture could not be read."),
        };
        match crate::files::save(&bytes) {
            Ok(name) => self.toggle_cell_part(page, id, row, column, &name),
            Err(text) => self.notice(text),
        }
    }

    fn paste_image(self, page: &str, at: &str, base64: &str) {
        use base64::Engine;
        // The first picture of a drop names its line; the others say "last"
        // (see `put_image`). A first one that cannot be kept still says
        // where the drop was: the others went after some older picture of
        // the page, the last one that had been put there.
        if at != "last" {
            LAST_IMAGE.with(|l| *l.borrow_mut() = Some((page.to_string(), at.to_string())));
        }
        let bytes = match base64::engine::general_purpose::STANDARD.decode(base64.trim()) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            // (The page sends no bytes for a file over the limit.)
            Ok(_) => return self.notice("That picture is too large (the most is 30 MB)."),
            Err(_) => return self.notice("That picture could not be read."),
        };
        match crate::files::save(&bytes) {
            Ok(name) => self.put_image(page, at, name),
            Err(text) => self.notice(text),
        }
    }

    /// Show a kept picture at a block: in that block if it is an image
    /// block still empty or an empty line, otherwise in a new block after it.
    fn put_image(self, page: &str, at: &str, name: String) {
        // "last": after the picture put just before (several dropped
        // together), or at the line of the drop when none was put yet.
        let at = if at == "last" { LAST_IMAGE.with(|l| l.borrow().clone()).filter(|(p, _)| p == page).map(|(_, id)| id).unwrap_or_default() } else { at.to_string() };
        let at = at.as_str();
        let mut put: Option<Id> = None;
        let caret = self.edit_blocks(page, |bs| {
            let i = index_of(bs, at).unwrap_or(bs.len().saturating_sub(1));
            if bs.is_empty() {
                bs.push(Block::text(""));
            }
            let empty_image = bs[i].kind == BlockKind::Image && bs[i].data.is_empty();
            let empty_line = bs[i].kind == BlockKind::Text && bs[i].text.trim().is_empty();
            let at = if empty_image || empty_line {
                bs[i].text.clear();
                bs[i].turn_into(BlockKind::Image);
                bs[i].data = name;
                i
            } else {
                // After the line and what is nested in it (the lines of a
                // toggle, sub-bullets), not between them.
                let mut image = Block::new(BlockKind::Image, "").with_indent(bs[i].indent);
                image.data = name;
                let at = subtree_end(bs, i);
                bs.insert(at, image);
                at
            };
            // An empty line right under the picture, to go on typing in: the
            // caret goes there, not into the text that came after.
            if !bs.get(at + 1).is_some_and(|next| next.kind == BlockKind::Text && next.text.trim().is_empty()) {
                bs.insert(at + 1, Block::text("").with_indent(bs[at].indent));
            }
            put = Some(bs[at].id.clone());
            Some(Caret::start(&bs[at + 1].id))
        });
        if let Some(id) = put {
            LAST_IMAGE.with(|l| *l.borrow_mut() = Some((page.to_string(), id)));
        }
        if let Some(caret) = caret {
            self.focus(caret);
        }
    }

    /// Put a free picture back in line, where it is kept in the page.
    pub fn image_in_line(self, page: &str, id: &str) {
        self.edit_blocks(page, |bs| {
            let now = index_of(bs, id).map(|i| bs[i].place).unwrap_or_default();
            place_picture(bs, id, Place { free: false, ..now }, None, false)
        });
    }

    /// Take a picture out of its page (Undo brings it back).
    pub fn delete_image(self, page: &str, id: &str) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id).filter(|&i| bs[i].kind == BlockKind::Image) {
                bs.remove(i);
            }
        });
    }

    /// Choose a picture file for an image block.
    pub fn pick_image(self, page: &str, id: &str) {
        let (page, id) = (page.to_string(), id.to_string());
        spawn_forever(async move {
            let Some(file) = rfd::AsyncFileDialog::new().set_title("Choose a Picture").add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tiff", "bmp"]).pick_file().await else {
                return;
            };
            match crate::files::import(file.path()) {
                Ok(name) => self.put_image(&page, &id, name),
                Err(text) => self.notice(text),
            }
        });
    }

    /// Change a table. Its words are kept in the block's text too, so
    /// search and the AI see them.
    fn edit_table(self, page: &str, id: &str, change: impl FnOnce(&mut crate::table::Table)) {
        let mut after = None;
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id).filter(|&i| bs[i].kind == BlockKind::Table) {
                let mut table = crate::table::Table::parse(&bs[i].data);
                // (Days written before cells held their year get it now.)
                table.keep_years(crate::when::today());
                change(&mut table);
                bs[i].data = table.to_data();
                bs[i].text = table.plain_text();
                crate::ui::tables::changed(id, &table);
                after = Some(table);
            }
        });
        // The first cell of a row is the name of the row's page.
        if let Some(table) = after {
            self.name_row_pages(id, &table);
        }
    }

    /// A letter typed in a cell. This is on the path of every key, so it is
    /// kept light: see [`Origin::Cell`].
    pub fn set_table_cell(self, page: &str, id: &str, row: usize, column: usize, text: &str) {
        let group = format!("{id}#{row},{column}");
        let mut named_row = None;
        self.edit_blocks_from(page, Origin::Cell { block: id, group: &group }, |bs| {
            if let Some(i) = index_of(bs, id).filter(|&i| bs[i].kind == BlockKind::Table) {
                let mut table = crate::table::Table::parse(&bs[i].data);
                // (The headings of a locked table stay as they are.)
                if row == 0 && table.view.locked {
                    return;
                }
                table.set(row, column, text);
                if column == 0 {
                    named_row = table.row_id(row).map(str::to_string);
                }
                bs[i].data = table.to_data();
                bs[i].text = table.plain_text();
            }
        });
        // The first cell of a row is the name of the row's page.
        if let Some(row) = named_row {
            self.name_row_page(id, &row, text);
        }
    }

    /// Set or take away a page's cover.
    pub fn set_cover(mut self, page: &str, cover: Option<String>) {
        if let Some(p) = self.pages.write().get_mut(page) {
            if p.cover != cover {
                p.cover = cover;
                p.cover_y = 50;
                p.updated_at = now_ms();
            }
        }
        self.save_page(page);
    }

    /// Give a page the face of its title and headings ("", "serif", "mono").
    pub fn set_page_font(mut self, page: &str, font: &str) {
        if !PAGE_FONTS.iter().any(|(f, _)| *f == font) {
            return;
        }
        if let Some(p) = self.pages.write().get_mut(page) {
            if p.font != font {
                p.font = font.to_string();
                p.updated_at = now_ms();
            }
        }
        self.save_page(page);
    }

    /// The words under a picture. Typed a letter at a time, so it takes the
    /// light path (one row written, one step for Undo).
    pub fn set_caption(self, page: &str, id: &str, text: &str) {
        let group = format!("{id}#caption");
        self.edit_blocks_from(page, Origin::Cell { block: id, group: &group }, |bs| {
            if let Some(i) = index_of(bs, id).filter(|&i| bs[i].kind == BlockKind::Image) {
                bs[i].text = text.replace('\n', " ");
            }
        });
    }

    pub fn set_cover_y(mut self, page: &str, y: u8) {
        if let Some(p) = self.pages.write().get_mut(page) {
            p.cover_y = y.min(100);
        }
        self.save_page(page);
    }

    /// Choose a picture file as a page's cover.
    pub fn pick_cover(self, page: &str) {
        let page = page.to_string();
        spawn_forever(async move {
            let Some(file) = rfd::AsyncFileDialog::new().set_title("Choose a Cover Picture").add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tiff", "bmp"]).pick_file().await else {
                return;
            };
            match crate::files::import(file.path()) {
                Ok(name) => {
                    self.set_cover(&page, Some(format!("file:{name}")));
                    self.set_popover(None);
                }
                Err(text) => self.notice(text),
            }
        });
    }

    pub fn set_code_language(self, page: &str, id: &str, language: String) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                bs[i].language = (!language.is_empty()).then_some(language);
            }
        });
    }

    pub fn cycle_callout_icon(self, page: &str, id: &str) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id) {
                let cur = bs[i].icon.clone().unwrap_or_default();
                let choices = [
                    "lightbulb:yellow",
                    "warning:orange",
                    "info:blue",
                    "check-circle:green",
                    "pin:red",
                    "flame:orange",
                    "sparkles:purple",
                ];
                let (name, color) = crate::ui::icons::parse_icon(&cur);
                let cur = crate::ui::icons::format_icon(&name, &color);
                let next = choices.iter().position(|c| *c == cur).map(|p| (p + 1) % choices.len()).unwrap_or(0);
                bs[i].icon = Some(choices[next].to_string());
            }
        });
    }

    pub fn delete_block(mut self, page: &str, id: &str) {
        let focus = {
            let blocks = self.blocks.peek();
            blocks.get(page).and_then(|bs| index_of(bs, id).and_then(|i| prev_editable(bs, i).map(|j| bs[j].id.clone())))
        };
        let step_was = self.history.peek().next_undo(page);
        let linked = self.edit_blocks(page, |bs| index_of(bs, id).map(|i| delete(bs, i)).unwrap_or_default());
        let mut trashed = Vec::new();
        for page_id in linked {
            // (Another line of this page that is still the page's own line:
            // the page stays. Only its last line takes it to the Trash.)
            let still_linked = self.blocks.peek().get(page).is_some_and(|bs| bs.iter().any(|b| b.kind == BlockKind::PageLink && b.target.as_deref() == Some(page_id.as_str())));
            if !still_linked && self.pages.peek().get(&page_id).is_some_and(|p| p.parent_id.as_deref() == Some(page) && !p.trashed) {
                self.set_trashed(&page_id, true);
                trashed.push(page_id);
            }
        }
        if !trashed.is_empty() {
            self.history.write().attach(page, &trashed, &[]);
            self.toast_step("Deleted the block and moved its page to Trash", page, step_was);
        }
        self.set_popover(None);
        if let Some(f) = focus {
            self.focus(Caret::end(&f));
        }
    }

    pub fn duplicate_block(self, page: &str, id: &str) {
        self.block_action(page, id, "duplicate", 0);
    }

    /// The "+" button in a block's gutter: add a block below and open the slash menu.
    pub fn add_below_with_menu(mut self, page: &str, id: &str) {
        let new_id = self.edit_blocks(page, |bs| {
            let i = index_of(bs, id)?;
            let at = subtree_end(bs, i);
            let b = Block::text("/").with_indent(bs[i].indent);
            let nid = b.id.clone();
            bs.insert(at, b);
            Some(nid)
        });
        if let Some(nid) = new_id {
            self.set_popover(None);
            {
                self.slash.set(Some(SlashState {
                    page_id: page.to_string(),
                    block_id: nid.clone(),
                    slash_at: 0,
                    end: 1,
                    added: true,
                    query: String::new(),
                    selected: 0,
                    pages: false,
                    wiki: false,
                }));
            }
            self.focus(Caret::end(&nid));
        }
    }

    /// Clicking the empty space below the last block.
    pub fn click_page_tail(self, page: &str) {
        let target = self.edit_blocks(page, |bs| {
            // The last line; free pictures kept after it are not lines.
            match bs.iter().rev().find(|b| !b.floats()) {
                Some(last) if last.kind.is_editable() && last.text.is_empty() && last.indent == 0 => last.id.clone(),
                _ => {
                    let b = Block::text("");
                    let id = b.id.clone();
                    bs.push(b);
                    id
                }
            }
        });
        self.focus(Caret::end(&target));
    }

    /// Enter in the title moves to the first block.
    pub fn title_enter(self, page: &str) {
        let first = self.edit_blocks(page, |bs| {
            if let Some(first) = bs.iter().find(|b| !b.floats()).filter(|b| b.kind.is_editable()) {
                first.id.clone()
            } else {
                let b = Block::text("");
                let id = b.id.clone();
                bs.insert(0, b);
                id
            }
        });
        self.focus(Caret::start(&first));
    }

    /// The down arrow in the title moves to the first line of the page that
    /// can be typed in. It makes none. (As Return does: on a page that
    /// starts with a table, a picture or a divider, an arrow key put an
    /// empty line above it.)
    pub fn title_down(self, page: &str) {
        let first = self.blocks.peek().get(page).and_then(|bs| bs.iter().find(|b| !b.floats() && b.kind.is_editable()).map(|b| b.id.clone()));
        if let Some(first) = first {
            self.focus(Caret::start(&first));
        }
    }

    /// Several lines were pasted into a block: they become blocks.
    fn paste(mut self, page: &str, id: &str, pos: usize, value: String, text: &str) {
        // The link of a page by itself (Copy link): on an empty line, the
        // page's own line (its icon and name, as a page inside shows); in
        // words, a link with the page's name.
        if let Some(target) = inline::page_id_of(text.trim()).map(str::to_string).filter(|t| self.pages.peek().get(t).is_some_and(|p| !p.trashed)) {
            self.history.write().seal();
            let link = inline::link(&self.page_title(&target), &inline::page_url(&target));
            let caret = self.edit_blocks(page, |bs| {
                let i = index_of(bs, id)?;
                let kind = bs[i].kind;
                if value.trim().is_empty() && kind == BlockKind::Text {
                    let indent = bs[i].indent;
                    bs[i] = Block::page_link(target.clone()).with_indent(indent);
                    let next = Block::text("").with_indent(indent);
                    let caret = Caret::start(&next.id);
                    bs.insert(i + 1, next);
                    return Some(caret);
                }
                let (before, after) = inline::split_for(kind, &value, pos);
                let with = inline::concat(&before, &link);
                let at = inline::len_for(kind, &with);
                bs[i].text = inline::concat(&with, &after);
                Some(Caret { id: bs[i].id.clone(), pos: at })
            });
            if let Some(c) = caret {
                self.focus(c);
            }
            return;
        }
        // What was copied or cut in the app as whole blocks, and is still what
        // the clipboard holds: those blocks again, each with a new id.
        let same = |a: &str, b: &str| a.replace("\r\n", "\n").trim() == b.replace("\r\n", "\n").trim();
        let kept = COPIED_BLOCKS.with(|c| c.borrow().as_ref().filter(|(words, bs, ..)| !bs.is_empty() && same(words, text)).map(|(_, bs, cut, row_pages)| (bs.clone(), *cut, row_pages.clone())));
        // A table that was cut and is in no page now (Undo did not put it
        // back, and it was not pasted before) is moved by this paste, not
        // copied: it stays the block it was. The pages of its rows find it
        // by its id: with a new id each row opened an empty page, and what
        // was written in the rows' pages stayed in the Trash.
        let mut moved: Vec<Id> = Vec::new();
        let mut row_pages: Vec<Id> = Vec::new();
        let mut items = match kept {
            Some((blocks, cut, cut_row_pages)) => {
                row_pages = cut_row_pages;
                let least = blocks.iter().map(|b| b.indent).min().unwrap_or(0);
                let all = self.blocks.peek();
                let in_no_page = |id: &str| !all.values().any(|bs| bs.iter().any(|b| b.id == id));
                blocks
                    .into_iter()
                    .map(|mut b| {
                        if cut && b.kind == BlockKind::Table && in_no_page(&b.id) {
                            moved.push(b.id.clone());
                        } else {
                            b.id = new_id();
                        }
                        b.indent -= least;
                        b
                    })
                    .collect()
            }
            None => markdown::to_blocks_by_line(text),
        };
        if items.is_empty() {
            return;
        }
        self.history.write().seal();
        let caret = self.edit_blocks(page, |bs| {
            let i = index_of(bs, id)?;
            let kind = bs[i].kind;
            let base = bs[i].indent;
            let (before, after) = inline::split_for(kind, &value, pos);
            // (Pasted in an empty line, a list, a heading or a table takes
            // the place of the line: no empty line stays over it.)
            let in_place = kind == BlockKind::Text && before.is_empty() && after.is_empty() && items[0].kind != BlockKind::Text;
            bs[i].text = before;
            if items[0].kind == BlockKind::Text {
                let first = items.remove(0);
                bs[i].text = inline::join_for(kind, &bs[i].text, first.kind, &first.text);
            }
            let mut last = bs[i].id.clone();
            for (n, b) in items.into_iter().enumerate() {
                let indent = b.indent;
                let b = b.with_indent(base + indent);
                last = b.id.clone();
                bs.insert(i + 1 + n, b);
            }
            if in_place {
                bs.remove(i);
            }
            let li = index_of(bs, &last)?;
            // (What ends the paste has no words of its own, a divider or a
            // table: what was after the caret goes in a line after it, with
            // the caret. Joined to it, those words were not seen again.)
            if !bs[li].kind.is_editable() {
                let mut tail = Block::text("").with_indent(base);
                tail.text = inline::join_for(BlockKind::Text, "", kind, &after);
                let id = tail.id.clone();
                bs.insert(li + 1, tail);
                return Some(Caret { id, pos: 0 });
            }
            let at = inline::len_for(bs[li].kind, &bs[li].text);
            bs[li].text = inline::join_for(bs[li].kind, &bs[li].text, kind, &after);
            Some(Caret { id: last, pos: at })
        });
        for block in &moved {
            self.bring_row_pages(page, block, &row_pages);
        }
        if let Some(c) = caret {
            self.focus(c);
        }
    }

    // ----- undo and redo ---------------------------------------------------

    pub fn undo(self) {
        self.step(true);
    }

    pub fn redo(self) {
        self.step(false);
    }

    fn step(self, undo: bool) {
        let Some(page) = self.current_page() else { return };
        self.step_on(&page, undo);
    }

    /// One step back or forward on a page: the page in front (⌘Z), or the
    /// page a message's "Undo" is about.
    fn step_on(mut self, page: &str, undo: bool) {
        let page = page.to_string();
        let entry = {
            let mut h = self.history.write();
            if undo { h.take_undo(&page) } else { h.take_redo(&page) }
        };
        let Some(e) = entry else {
            self.toast(if undo { "Nothing to undo" } else { "Nothing to redo" });
            return;
        };
        let current = self.blocks.peek().get(&page).cloned().unwrap_or_default();
        // Only what this step changed: a transcript or an AI's text that came
        // in after it is not part of the step, and stays.
        let mut target = if undo {
            crate::history::rebase(&current, &e.after, &e.before)
        } else {
            crate::history::rebase(&current, &e.before, &e.after)
        };
        let (back, copies) = tables_put_back(&mut target, &current, |id| self.table_in_another_page(&page, id));
        // (The step goes by the new id of a copy from now on: with the old
        // id, Redo did not take the copy away, and the next Undo put one
        // more copy in the page.)
        let mut e = e;
        for (was, now) in &copies {
            e.rename_block(was, now);
        }
        self.set_blocks(&page, target.clone(), true);
        // (Undo and Redo are edits too: see `show_newly_hidden`.)
        self.drop_dead_folds(&page, &target);
        self.show_newly_hidden(&page, &current, &target);
        // (The first cell of a row is the name of the row's page, and a row
        // that this step takes out takes its page along.)
        self.name_row_pages_of(&page);
        self.drop_lost_row_pages(&current, &target);
        // A table that this step puts back takes the pages of its rows
        // back into this page, when the table was pasted in another page
        // and that paste was undone there. They come out of the Trash when
        // their row is opened, as they always did (`row_page`): one that
        // the person put in the Trash by hand must not come out with them.
        for id in &back {
            self.bring_row_pages(&page, id, &[]);
        }
        for id in &e.trashed {
            self.set_trashed(id, !undo);
        }
        for id in &e.created {
            self.set_trashed(id, undo);
        }
        if let Some(c) = caret_between(&current, &target) {
            self.focus(c);
        }
        let mut h = self.history.write();
        if undo { h.push_redo(e) } else { h.push_undo(e) }
    }

    // ----- links -------------------------------------------------------------

    /// A link in a block was clicked.
    pub fn open_url(self, url: &str) {
        let url = url.trim();
        if let Some(id) = inline::page_id_of(url) {
            if self.pages.peek().get(id).is_some_and(|p| !p.trashed) {
                self.open_page(id);
            } else {
                self.toast("That page isn't available");
            }
            return;
        }
        if !inline::safe_url(url) {
            self.toast("Only web, email and page links can be opened");
            return;
        }
        // A test run opens nothing outside the app: it notes the address.
        if std::env::var_os("RUNE_SELFTEST").is_some() {
            document::eval(&format!("document.body.dataset.opened = {}", serde_json::to_string(url).unwrap_or_default()));
            return;
        }
        if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
            self.toast(format!("Couldn't open the link: {e}"));
        }
    }

    // ----- other windows and outside changes -------------------------------

    /// Apply a change made in another window or by another program.
    pub fn apply_sync(mut self, msg: SyncMsg) {
        match msg {
            SyncMsg::Page(p) => {
                let gone = p.trashed;
                let id = p.id.clone();
                self.pages.write().insert(id.clone(), p);
                if gone {
                    let ids = descendants(&self.pages.peek(), &id);
                    self.leave_pages(&ids);
                }
            }
            SyncMsg::Blocks(page, bs) => {
                self.history.write().forget(&page);
                self.set_blocks(&page, bs, false);
            }
            SyncMsg::Removed(ids) => {
                {
                    let mut pages = self.pages.write();
                    let mut blocks = self.blocks.write();
                    for id in &ids {
                        pages.remove(id);
                        blocks.remove(id);
                    }
                }
                self.leave_pages(&ids);
            }
            SyncMsg::Theme(t) => self.ui.write().theme = Theme::parse(&t),
            SyncMsg::Spaces(spaces) => {
                // A space was deleted in another window. This window leaves
                // it, and what it holds of that space goes where the notes
                // file put it (the first space left): a page made here is
                // never saved to a space that is not there.
                // (Only a space that the notes file does not have either:
                // see `spaces_told`.)
                let (spaces, gone) = spaces_told(&self.spaces.peek(), spaces, store::with(|s| s.load_spaces()));
                let into = spaces.first().map(|s| s.id.clone()).filter(|_| !gone.is_empty());
                self.spaces.set(spaces);
                if let Some(into) = into {
                    rehome(&mut self.pages.write(), &mut self.events.write(), &gone, &into);
                    if gone.contains(&self.ui.peek().space) {
                        self.switch_space(&into);
                    }
                    self.ui.write().parked.retain(|id, _| !gone.contains(id));
                }
            }
            SyncMsg::Event(e) => {
                self.events.write().insert(e.id.clone(), e);
                // (Another window may have changed the pages of repeats with it.)
                self.change_repeat_pages(|_| {});
            }
            SyncMsg::EventRemoved(id) => {
                self.events.write().remove(&id);
                self.change_repeat_pages(|_| {});
                if self.cal.peek().selected.as_deref() == Some(id.as_str()) {
                    self.cal.write().selected = None;
                }
            }
            SyncMsg::Events(list) => {
                let map: HashMap<Id, Event> = list.into_iter().map(|e| (e.id.clone(), e)).collect();
                if *self.events.peek() != map {
                    self.events.set(map);
                    crate::notify::mark_dirty();
                }
            }
            SyncMsg::Look { glass, blur, tint, accent } => self.look.set(Look { glass, blur, tint, accent }),
            SyncMsg::Reload(data) => {
                let (pages, blocks) = &*data;
                let changed_pages: Vec<Page> = pages
                    .values()
                    .filter(|p| self.pages.peek().get(&p.id) != Some(p))
                    .cloned()
                    .collect();
                if !changed_pages.is_empty() {
                    let mut mine = self.pages.write();
                    for p in changed_pages {
                        mine.insert(p.id.clone(), p);
                    }
                }
                let changed_blocks: Vec<(Id, Vec<Block>)> = blocks
                    .iter()
                    .filter(|(id, bs)| self.blocks.peek().get(*id) != Some(*bs))
                    .map(|(id, bs)| (id.clone(), bs.clone()))
                    .collect();
                for (page, bs) in changed_blocks {
                    self.history.write().forget(&page);
                    self.set_blocks(&page, bs, false);
                }
                // A page that another program (an AI tool) put in the Trash
                // is left by every pane, also by one that only has it behind
                // the page on show: Back opened it again from there.
                let trashed: Vec<Id> = pages.values().filter(|p| p.trashed).map(|p| p.id.clone()).collect();
                if self.ui.peek().remembers(&trashed) {
                    self.leave_pages(&trashed);
                }
            }
        }
    }

    // ----- slash menu ----------------------------------------------------

    fn slash_key(mut self, action: &str) {
        let Some(slash) = self.slash.peek().clone() else { return };
        let len = if slash.pages {
            let (found, create) = self.mention_items(&slash);
            found.len() + create.is_some() as usize
        } else {
            filter_slash(&slash.query).len()
        };
        match action {
            "down" | "up" if len > 0 => {
                let sel = if action == "down" { (slash.selected + 1) % len } else { (slash.selected + len - 1) % len };
                if let Some(s) = self.slash.write().as_mut() {
                    s.selected = sel;
                }
                document::eval("window.__rune && window.__rune.scrollSelected()");
            }
            "enter" if len > 0 && slash.pages => self.apply_mention(slash.selected),
            "enter" if len > 0 => self.apply_slash(slash.selected),
            "enter" => self.slash.set(None),
            "close" => {
                self.slash.set(None);
                // The "+" beside a line put a "/" in the new line, for this
                // menu. Closed with nothing chosen and nothing typed, the
                // "/" goes too: it stayed in the line, and was saved.
                if slash.added {
                    self.edit_blocks(&slash.page_id, |bs| {
                        if let Some(b) = bs.iter_mut().find(|b| b.id == slash.block_id && b.text == "/") {
                            b.text.clear();
                        }
                    });
                }
            }
            _ => {}
        }
    }

    /// The first words of a page (see `snippet_of`).
    pub fn page_snippet(&self, id: &str) -> String {
        self.blocks.read().get(id).map(|bs| snippet_of(bs, 110)).unwrap_or_default()
    }

    /// The newest meeting notes page of this space that still exists.
    pub fn last_meeting(&self) -> Option<Id> {
        let space = self.ui.read().space.clone();
        let pages = self.pages.read();
        meeting_page_ids().into_iter().rev().find(|id| pages.get(id).is_some_and(|p| !p.trashed && p.space_id == space))
    }

    /// What the list after "@" or "[[" holds: the pages that fit what is
    /// typed, and (after "[[") the name of a page that can be made.
    pub fn mention_items(&self, slash: &SlashState) -> (Vec<Page>, Option<String>) {
        let found = pages_to_link(&self.pages.peek(), &self.ui.peek().space, &slash.page_id, &slash.query);
        let name = slash.query.trim();
        let create = (slash.wiki && !name.is_empty() && !found.iter().any(|p| p.title.trim().eq_ignore_ascii_case(name))).then(|| name.to_string());
        (found, create)
    }

    /// The pages above a page, as "Work › Apollo" (empty at the top level).
    pub fn page_path(&self, id: &str) -> String {
        ancestors(&self.pages.peek(), id).iter().map(|p| p.display_title().to_string()).collect::<Vec<_>>().join(" › ")
    }

    /// Put a link to the chosen page in place of the typed "@name" or
    /// "[[name". The last choice after "[[" makes a page of that name inside
    /// this one and links it.
    pub fn apply_mention(mut self, index: usize) {
        let Some(slash) = self.slash.peek().clone().filter(|s| s.pages) else { return };
        let (found, create) = self.mention_items(&slash);
        self.slash.set(None);
        let mut made = None;
        let target = match (found.get(index), create) {
            (Some(p), _) => p.id.clone(),
            (None, Some(name)) if index == found.len() => {
                let id = self.create_page(Some(slash.page_id.clone()));
                self.rename_page(&id, name);
                made = Some(id.clone());
                id
            }
            _ => return,
        };
        let link = inline::link(&self.page_title(&target), &inline::page_url(&target));
        let (page, id) = (slash.page_id.clone(), slash.block_id.clone());
        let done = self.edit_blocks(&page, |bs| {
            let i = index_of(bs, &id)?;
            let kind = bs[i].kind;
            let before = inline::split_for(kind, &bs[i].text, slash.slash_at).0;
            // (What was after the caret stays in the line.)
            let after = inline::split_for(kind, &bs[i].text, slash.end.max(slash.slash_at)).1;
            // A space after it, to go on typing.
            let with = inline::concat(&inline::concat(&before, &link), " ");
            let at = inline::len_for(kind, &with);
            bs[i].text = inline::concat(&with, &after);
            Some(at)
        });
        if let Some(at) = done {
            // (Undo of the link takes the page it made away too, as for "/page".)
            if let Some(new) = &made {
                self.history.write().attach(&page, &[], std::slice::from_ref(new));
            }
            self.focus(Caret { id, pos: at });
        }
    }

    pub fn apply_slash(mut self, index: usize) {
        let Some(slash) = self.slash.peek().clone() else { return };
        let Some(item) = filter_slash(&slash.query).into_iter().nth(index) else { return };
        self.slash.set(None);
        let page = slash.page_id.clone();
        let id = slash.block_id.clone();

        // Remove the typed "/query" and see what's left in the block.
        // (What was after the caret stays in the line.)
        let remaining = self.edit_blocks(&page, |bs| {
            let i = index_of(bs, &id)?;
            let kind = bs[i].kind;
            let after = inline::split_for(kind, &bs[i].text, slash.end.max(slash.slash_at)).1;
            bs[i].text = inline::concat(&inline::split_for(kind, &bs[i].text, slash.slash_at).0, &after);
            Some(bs[i].plain_text().trim().is_empty())
        });
        let Some(block_is_empty) = remaining else { return };

        match item.action {
            SlashAction::Turn(kind) => {
                let caret = self.edit_blocks(&page, |bs| {
                    let i = index_of(bs, &id)?;
                    if !kind.is_editable() {
                        let indent = bs[i].indent;
                        let insert_at = if block_is_empty {
                            bs[i].text.clear();
                            bs[i].turn_into(kind);
                            i + 1
                        } else {
                            let d = Block::new(kind, "").with_indent(indent);
                            let at = subtree_end(bs, i);
                            bs.insert(at, d);
                            at + 1
                        };
                        let next = Block::text("").with_indent(indent);
                        let nid = next.id.clone();
                        bs.insert(insert_at, next);
                        return Some(Caret::start(&nid));
                    }
                    if block_is_empty {
                        bs[i].text.clear();
                        bs[i].turn_into(kind);
                        Some(Caret::start(&id))
                    } else {
                        let b = Block::new(kind, "").with_indent(bs[i].indent);
                        let nid = b.id.clone();
                        bs.insert(i + 1, b);
                        Some(Caret::start(&nid))
                    }
                });
                if let Some(c) = caret {
                    self.focus(c);
                }
            }
            SlashAction::SubPage => {
                let child = self.create_page(Some(page.clone()));
                let child_id = child.clone();
                self.edit_blocks(&page, move |bs| {
                    let Some(i) = index_of(bs, &id) else { return };
                    if block_is_empty {
                        let indent = bs[i].indent;
                        bs[i] = Block::page_link(child_id).with_indent(indent);
                    } else {
                        let indent = bs[i].indent;
                        bs.insert(i + 1, Block::page_link(child_id).with_indent(indent));
                    }
                });
                self.history.write().attach(&page, &[], std::slice::from_ref(&child));
                self.open_page(&child);
                self.focus_title(&child);
            }
            SlashAction::Ai(action) => self.ai_open(action, &page, &slash.block_id),
        }
    }

    // ----- AI --------------------------------------------------------------

    /// Open the inline AI panel under a block (⌘J, the slash menu or a block menu).
    pub fn ai_open(mut self, action: AiAction, page: &str, anchor: &str) {
        if self.locked_lines(page) {
            return self.toast("The AI does not read locked pages");
        }
        // The AI is at work (most times on another page, where its card is
        // out of view): a press that asks for the AI showed a new box and
        // stopped that job half way, with some of its pages made and no
        // word. The press shows the job; its card has Stop.
        let at_work = self.ai.peek().as_ref().filter(|s| s.status == AiStatus::Streaming).map(|s| s.page_id.clone());
        if let Some(at) = at_work.filter(|at| self.pages.peek().get(at).is_some_and(|p| !p.trashed)) {
            self.set_popover(None);
            if at != page {
                self.open_page(&at);
            }
            return self.toast("The AI is still at work on this page");
        }
        self.ai_stop();
        // A run before this one that changed things: say so as it goes.
        if let Some(old) = self.ai.peek().clone() {
            self.ai_note_done(&old);
        }
        ai::warm_up();
        let prompting = matches!(action, AiAction::Ask | AiAction::Recap);
        *self.ai.write() = Some(AiSession {
            run: 0,
            page_id: page.to_string(),
            anchor: anchor.to_string(),
            action,
            prompt: String::new(),
            output: String::new(),
            status: AiStatus::Prompting,
            task: None,
            steps: Vec::new(),
            journal: None,
            did: Vec::new(),
            plan: None,
            working: None,
            started: 0,
            heard: 0,
            pictures: Vec::new(),
            sent: String::new(),
            given: Vec::new(),
            part: Vec::new(),
            offer: None,
            cut: false,
            line_sent: String::new(),
            did_before: 0,
        });
        // What is picked on the page when the AI is asked: the request is
        // about that. Picked blocks are the part of the page it gets (and
        // may write again); picked rows of a table go with it as a table.
        if self.ai.peek().as_ref().is_some_and(|s| s.action == AiAction::Ask) {
            let part: Vec<Id> = match self.picked.peek().as_ref() {
                Some(picked) if picked.page == page => self.blocks.peek().get(page).map(|bs| bs.iter().filter(|b| picked.ids.contains(&b.id)).map(|b| b.id.clone()).collect()).unwrap_or_default(),
                _ => Vec::new(),
            };
            let rows = self.picked_rows_for_ai(page);
            if let Some(s) = self.ai.write().as_mut() {
                s.part = part;
                s.given.extend(rows);
            }
        }
        self.set_popover(None);
        if !prompting {
            self.ai_run();
        }
    }

    /// ⌘J: ask AI about the block with the caret, or the end of the page.
    pub fn ai_shortcut(self) {
        // No AI tool is connected: no AI box. The person asked for the AI by
        // name (the menu bar or its key), so show where one is connected.
        if !ai::has_ai() {
            return self.open_settings_at("AI");
        }
        // With no page in front (Home, Calendar) the question is for all the
        // notes; so it is on a locked page, whose lines the AI never reads.
        let Some(page) = self.current_page().filter(|p| !self.locked_lines(p)) else {
            self.set_modal(Some(Modal::AskNotes));
            return;
        };
        let focused = self.focus.peek().clone();
        let anchor = match focused {
            Some((p, b)) if p == page => b,
            _ => self.last_shown_block(&page),
        };
        self.ai_open(AiAction::Ask, &page, &anchor);
    }

    /// The last block of a page that shows (not one inside a closed toggle,
    /// such as a meeting's transcript): where the AI box goes when no line
    /// has the caret.
    fn last_shown_block(&self, page: &str) -> Id {
        let all = self.blocks.peek();
        let Some(bs) = all.get(page) else { return Id::default() };
        let shown = shown(bs, &self.folds_of(page));
        (0..bs.len()).rev().find(|&i| shown[i]).map(|i| bs[i].id.clone()).unwrap_or_default()
    }

    /// Open the AI box on a meeting's page to take a recap: what is said or
    /// typed there becomes the page's notes. The one AI box does this too;
    /// there is no second box for it.
    pub fn ai_recap(self, page: &str) {
        if !ai::has_ai() {
            return;
        }
        if !self.pages.peek().contains_key(page) {
            return;
        }
        let anchor = self.last_shown_block(page);
        self.ai_open(AiAction::Recap, page, &anchor);
    }

    pub fn ai_set_prompt(mut self, prompt: String) {
        if let Some(s) = self.ai.write().as_mut() {
            // A web address in the words: the person can ask for its page.
            s.offer = crate::attach::web_addresses(&prompt).into_iter().find(|url| !s.given.iter().any(|g| g.url == *url));
            s.prompt = prompt;
        }
    }

    // ----- what goes with a request besides pictures (see attach.rs) -----

    /// A file dropped or pasted into the AI box: read off the window's
    /// thread (a PDF takes a moment), then given with the request.
    pub fn ai_add_file(self, name: &str, data: &str) {
        use base64::Engine as _;
        if data == "!large" {
            return self.notice(format!("“{name}” is too large for the AI (20 MB at the most)."));
        }
        let data = data.split_once(";base64,").map(|(_, d)| d).unwrap_or(data);
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data.trim()) else {
            return self.toast("That file could not be read");
        };
        let name = name.to_string();
        // Its place shows at once, and the request waits for it: sent before
        // a large PDF was read, the request went without it and said nothing.
        let short = std::path::Path::new(&name).file_name().and_then(|n| n.to_str()).unwrap_or(&name).to_string();
        let waiting = crate::attach::Attached::waiting(&short, crate::attach::Kind::Text, "Reading it…");
        let id = waiting.id.clone();
        if !self.ai_give(waiting) {
            return self.toast("Open the AI box, and then give it the file");
        }
        spawn_forever(async move {
            let read = ai::runtime().spawn_blocking(move || crate::attach::from_file(&name, &bytes)).await.unwrap_or_else(|_| Err("That file could not be read".to_string()));
            self.ai_settle(&id, read);
        });
    }

    /// What was being read or fetched is here, or could not be had: it
    /// takes its place, or its place goes and the person is told why.
    fn ai_settle(mut self, id: &str, got: Result<crate::attach::Attached, String>) {
        let fill = |list: &mut Vec<crate::attach::Attached>| match (list.iter().position(|g| g.id == id), &got) {
            (Some(at), Ok(it)) => list[at] = crate::attach::Attached { id: id.to_string(), ..it.clone() },
            (Some(at), Err(_)) => {
                list.remove(at);
            }
            (None, _) => {}
        };
        if let Some(s) = self.ai.write().as_mut() {
            fill(&mut s.given);
        }
        fill(&mut self.ask_given.write());
        if let Err(why) = got {
            self.notice(why);
        }
    }

    /// Give something with the request that is typed now: in the AI box of
    /// the page, or in the box that asks all the notes.
    /// Returns whether it was taken.
    pub fn ai_give(mut self, it: crate::attach::Attached) -> bool {
        let full = "No more than eight things go with one request";
        let in_bar = self.ai.peek().as_ref().is_some_and(|s| s.status == AiStatus::Prompting);
        // (The box that asks all the notes lies over the page: with both
        // open, what is dropped is for the one in front.)
        if self.ui.peek().modal == Some(Modal::AskNotes) {
            if self.ask_given.peek().len() >= crate::attach::MOST {
                self.toast(full);
                return false;
            }
            self.ask_given.write().push(it);
            true
        } else if in_bar {
            let mut ai = self.ai.write();
            let Some(s) = ai.as_mut() else { return false };
            if s.given.len() >= crate::attach::MOST {
                drop(ai);
                self.toast(full);
                return false;
            }
            s.given.push(it);
            true
        } else {
            false
        }
    }

    /// Take one of them out again ("part": the picked part of the page).
    pub fn ai_remove_given(mut self, id: &str) {
        if let Some(s) = self.ai.write().as_mut() {
            if id == "part" {
                s.part.clear();
            }
            s.given.retain(|g| g.id != id);
            let (prompt, given) = (s.prompt.clone(), &s.given);
            s.offer = crate::attach::web_addresses(&prompt).into_iter().find(|url| !given.iter().any(|g| g.url == *url));
        }
        self.ask_given.write().retain(|g| g.id != id);
        // (The part let go: its blocks are picked no more.)
        if id == "part" {
            self.picked.set(None);
        }
    }

    /// Another page of the notes, chosen after "@" in the AI box: its words
    /// go with the request. `prompt` is the request without the "@name".
    pub fn ai_give_page(mut self, page_id: &str, prompt: String) {
        // (Also while it is open: its lines are in plain in the window then.)
        if self.locked_lines(page_id) {
            return self.toast("A locked page is not given to the AI");
        }
        let link = inline::page_url(page_id);
        let title = self.page_title(page_id);
        let text = self.page_for_ai(page_id);
        if let Some(s) = self.ai.write().as_mut() {
            s.prompt = prompt;
            // (Each page once, by the page and not by its name: of two
            // pages with one name, the second was not given, with no word.)
            if s.given.iter().any(|g| g.kind == crate::attach::Kind::Page && g.url == link) {
                return;
            }
        }
        let mut it = crate::attach::Attached::new(&title, crate::attach::Kind::Page, text, "");
        it.url = link;
        let _ = self.ai_give(it);
    }

    /// Put a kept picture on the clipboard (a test run keeps the person's
    /// clipboard as it is, and says which picture it was).
    fn copy_picture(self, name: &str) {
        if !crate::files::valid_name(name) {
            return;
        }
        if std::env::var_os("RUNE_SELFTEST").is_some() {
            document::eval(&format!("document.body.dataset.copied = {};", serde_json::to_string(&format!("picture:{name}")).unwrap_or_default()));
            return self.toast("Copied the picture");
        }
        let name = name.to_string();
        spawn_forever(async move {
            let copied = ai::runtime().spawn_blocking(move || crate::files::copy_to_clipboard(&name)).await.unwrap_or_else(|_| Err("The picture could not be copied.".to_string()));
            match copied {
                Ok(()) => self.toast("Copied the picture"),
                Err(why) => self.toast(why),
            }
        });
    }

    /// The pages that "@" and some letters in a text box can mean, handed to
    /// the page script (`pagesFound` in bridge.js), which shows the list.
    fn find_pages_for_at(self, query: &str, n: u32, here: &str) {
        let found: Vec<serde_json::Value> = {
            let pages = self.pages.peek();
            pages_to_link(&pages, &self.ui.peek().space, here, query)
                .into_iter()
                .map(|p| {
                    // (An icon that is a sign of its own shows; the app's own
                    // icons are drawn by the app, and the list has none.)
                    let sign = p.icon.clone().filter(|i| !i.contains(':') && i.chars().count() <= 4).unwrap_or_default();
                    let inside = p.parent_id.as_ref().and_then(|id| pages.get(id)).map(|parent| parent.display_title().to_string()).unwrap_or_default();
                    serde_json::json!({ "id": p.id, "title": p.display_title(), "sign": sign, "inside": inside })
                })
                .collect()
        };
        document::eval(&format!("window.__rune && window.__rune.pagesFound && window.__rune.pagesFound({n}, {})", serde_json::Value::Array(found)));
    }

    /// What the card over a link to a page says, handed to the page script
    /// (`pagePreview` in bridge.js), which draws it. `null` for a page that
    /// has no card (see `page_preview`).
    fn preview_page(self, id: &str, n: u32) {
        let card = page_preview(&self.pages.peek(), &self.blocks.peek(), &self.spaces.peek(), id).map(|p| {
            serde_json::json!({
                "id": id,
                "title": p.title,
                "icon": crate::ui::icons::page_icon_html(p.icon.as_deref(), 16),
                "place": p.place,
                "lines": p.lines,
                "locked": p.locked,
            })
        });
        document::eval(&format!("window.__rune && window.__rune.pagePreview && window.__rune.pagePreview({n}, {})", card.unwrap_or(serde_json::Value::Null)));
    }

    /// A page chosen with "@" in the Ask box: it goes with the question, as
    /// a page chosen in the AI box of a page does. Each page once.
    fn give_page_to_ai(self, page_id: &str) {
        if !self.pages.peek().get(page_id).is_some_and(|p| !p.trashed) {
            return;
        }
        if self.locked_lines(page_id) {
            return self.toast("A locked page is not given to the AI");
        }
        let title = self.page_title(page_id);
        // (By the page, not by its name: see `ai_give_page`.)
        let link = inline::page_url(page_id);
        let has = |list: &[crate::attach::Attached]| list.iter().any(|g| g.kind == crate::attach::Kind::Page && g.url == link);
        if has(&self.ask_given.peek()) || self.ai.peek().as_ref().is_some_and(|s| has(&s.given)) {
            return;
        }
        let text = self.page_for_ai(page_id);
        let mut it = crate::attach::Attached::new(&title, crate::attach::Kind::Page, text, "");
        it.url = link;
        let _ = self.ai_give(it);
    }

    /// The person asked for the page of a web address: it is fetched now
    /// (nothing is fetched unasked), and its words go with the request.
    pub fn ai_give_web(mut self, url: &str) {
        let Some(url) = crate::attach::web_address(url) else { return };
        let mut waiting = crate::attach::Attached::waiting(&crate::attach::short_address(&url), crate::attach::Kind::Web, "Getting it…");
        waiting.url = url.clone();
        let id = waiting.id.clone();
        if !self.ai_give(waiting) {
            return;
        }
        // (The next address in the words that is not given yet, if any.)
        if let Some(s) = self.ai.write().as_mut() {
            let (prompt, given) = (s.prompt.clone(), &s.given);
            s.offer = crate::attach::web_addresses(&prompt).into_iter().find(|url| !given.iter().any(|g| g.url == *url));
        }
        spawn_forever(async move {
            let got = ai::runtime().spawn_blocking(move || crate::attach::from_web(&url)).await.unwrap_or_else(|_| Err("The web page could not be read".to_string()));
            self.ai_settle(&id, got);
        });
    }

    /// The rows of a table of this page that are picked, as a table the AI
    /// can read: its headings and those rows.
    fn picked_rows_for_ai(&self, page: &str) -> Vec<crate::attach::Attached> {
        let blocks = self.blocks.peek().get(page).cloned().unwrap_or_default();
        let mut out = Vec::new();
        for b in blocks.iter().filter(|b| b.kind == BlockKind::Table) {
            let table = crate::table::Table::parse(&b.data);
            let rows = crate::ui::tables::picked(&b.id, &table).0;
            if rows.is_empty() {
                continue;
            }
            let line = |r: usize| format!("| {} |", (0..table.columns()).map(|c| if r == 0 { table.heading(c) } else { table.shown(r, c) }.replace('|', "\\|")).collect::<Vec<_>>().join(" | "));
            let mut text = vec![line(0), format!("|{}", " --- |".repeat(table.columns()))];
            text.extend(rows.iter().filter(|&&r| r >= 1 && r < table.rows.len()).map(|&r| format!("{} (row {r})", line(r))));
            let name = if table.name.trim().is_empty() { "the table".to_string() } else { format!("“{}”", table.name.trim()) };
            let label = format!("{} {} of {name}", rows.len(), if rows.len() == 1 { "row" } else { "rows" });
            let mut it = crate::attach::Attached::new(&label, crate::attach::Kind::Part, format!("The person picked these rows of the table {name} on the page. The request is about them.\n\n{}", text.join("\n")), "");
            it.id = format!("rows-{}", b.id);
            out.push(it);
        }
        out
    }

    /// The picked part of a page as the AI is given it (see `page_for_ai`).
    fn part_for_ai(&self, page_id: &str, part: &[Id]) -> String {
        part_as_given(&self.pages.peek(), self.blocks.peek().get(page_id).map(|bs| bs.as_slice()).unwrap_or_default(), part)
    }

    pub fn ai_run(mut self) {
        let Some(session) = self.ai.peek().clone() else { return };
        if matches!(session.action, AiAction::Ask | AiAction::Recap) && session.prompt.trim().is_empty() {
            return;
        }
        // (The page was locked after the box was opened on it.)
        if self.locked_lines(&session.page_id) {
            return self.toast("The AI does not read locked pages");
        }
        if session.action == AiAction::Recap {
            // The recap is written into the page's notes; the box has done its part.
            *self.ai.write() = None;
            return self.speech_recap(&session.page_id, session.prompt);
        }
        let block_text = self
            .blocks
            .peek()
            .get(&session.page_id)
            .and_then(|bs| bs.iter().find(|b| b.id == session.anchor).map(|b| b.text.clone()))
            .unwrap_or_default();
        // For a question or a request to change the page, the AI is given
        // the page with its pictures, drawings and links to pages as lines
        // it can keep in place (see `ai_revise`).
        if session.given.iter().any(|g| g.pending) {
            return self.toast("Rune is still reading what you gave. Ask again in a moment.");
        }
        let no_room = crate::attach::no_room_for(&session.given);
        if !no_room.is_empty() {
            self.notice(format!("There was no room for {} with this request: the AI is told so.", no_room.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ")));
        }
        // (With a part of the page picked: that part is the page it gets.)
        let whole = session.part.is_empty();
        let page_text = match session.action {
            AiAction::Ask if !whole => self.part_for_ai(&session.page_id, &session.part),
            AiAction::Ask => self.page_for_ai(&session.page_id),
            _ => self.page_markdown(&session.page_id),
        };
        let sent = if whole { self.page_for_ai(&session.page_id) } else { page_text.clone() };
        if let Some(s) = self.ai.write().as_mut() {
            s.sent = sent;
            s.offer = None;
            s.cut = !session.action.rewrites_block() && ai::is_cut(&page_text);
            s.line_sent = block_text.clone();
        }
        // The request with what was given with it, as words.
        let mut asked = session.prompt.clone();
        if !whole {
            asked.push_str(&format!(
                "\n\n(The person picked a part of the page, and only that part is given as the page. The request is about that part. \
                 A reply that starts with `{}` is that part as it must be after the change, not the whole page.)",
                ai::REVISE_MARK
            ));
        }
        asked.push_str(&crate::attach::for_request(&session.given));
        // A request in the person's own words is run with the tools of Rune
        // (where the AI tool can use them): the AI can then do anything in
        // the app, and what it changes is written down for "Undo".
        // ("Try again" after a run that changed things and then failed: this
        // run writes into the same record. With a new one, what the first
        // run changed was forgotten, and no Undo took it back.)
        let earlier = session.journal.clone().filter(|_| !session.did.is_empty());
        let tools = (session.action == AiAction::Ask && ai::takes_tools())
            .then(|| ai::Tools { journal: earlier.unwrap_or_else(|| std::env::temp_dir().join(format!("rune-ai-{}-{}.jsonl", std::process::id(), now_ms()))) });
        let messages = match &tools {
            Some(_) => {
                let title = self.pages.peek().get(&session.page_id).map(|p| p.title.clone()).unwrap_or_default();
                let now = when::local(now_ms()).format("%Y-%m-%d %H:%M, %A").to_string();
                // After the person's yes to a plan, the request says so.
                let request = match &session.plan {
                    Some(plan) => ai::approved(&asked, plan),
                    None => asked.clone(),
                };
                ai::build_agent_messages(&session.page_id, &title, &page_text, &request, &now)
            }
            None => ai::build_messages(&session.action, &page_text, &block_text, &asked),
        };
        let journal = tools.as_ref().map(|t| t.journal.clone());
        let run = session.run + 1;
        // Whichever AI Settings picks (an installed tool by default); a missing
        // tool or key arrives as an Error event.
        let mut rx = ai::start_with(ai::Prefs::load(), messages, tools, session.pictures.clone());
        // (A task of the window, not of the card: the card goes when its
        // pane shows another page, and a task of the card went with it. The
        // job then stopped half way with no word, and its card said that
        // it was still at work for as long as it was open.)
        let job_page = session.page_id.clone();
        let task = spawn_forever(async move {
            while let Some(event) = rx.next().await {
                // After a step and at the end: show what the tools changed in the notes.
                let mut look = false;
                let mut ended = false;
                {
                    let mut guard = self.ai.write();
                    let Some(s) = guard.as_mut().filter(|s| s.run == run) else { break };
                    s.heard = now_ms();
                    match event {
                        AiEvent::Delta(text) => {
                            s.working = None;
                            s.output.push_str(&text);
                        }
                        AiEvent::Working { tool, title, chars } => {
                            s.working = Some(ai::working_label(&tool, title.as_deref(), chars));
                        }
                        AiEvent::Step(tool, input) => {
                            s.working = None;
                            let label = {
                                let pages = self.pages.peek();
                                ai::step_label(&tool, &input, |id| pages.get(id).map(|p| p.title.clone()))
                            };
                            s.steps.push(label);
                            // What it wrote before a step was on its way to the step, not the answer.
                            s.output.clear();
                            look = true;
                        }
                        AiEvent::Done => {
                            s.did = s.journal.as_deref().map(crate::mcp::read_journal).unwrap_or_default();
                            if s.output.trim().is_empty() && !s.did.is_empty() {
                                s.output = "Done.".into();
                            }
                            s.status = if s.output.trim().is_empty() {
                                AiStatus::Error("The AI returned an empty reply.".into())
                            } else {
                                AiStatus::Done
                            };
                            s.task = None;
                            if s.status == AiStatus::Done && !dioxus::desktop::window().window.is_focused() {
                                let first = ai::clean_reply(&s.output).0.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").trim_start_matches('#').trim().chars().take(120).collect::<String>();
                                spawn(async move { crate::notify::show_now("ai", "AI reply ready", &first).await });
                            }
                            look = true;
                            ended = true;
                        }
                        AiEvent::Error(e) => {
                            s.did = s.journal.as_deref().map(crate::mcp::read_journal).unwrap_or_default();
                            s.status = AiStatus::Error(e);
                            s.task = None;
                            look = true;
                            ended = true;
                        }
                    }
                }
                if look {
                    sync::check_outside_changes();
                }
                if ended {
                    // The job came to its end while its page is not in
                    // view: a short line says so. (Its card is on its page,
                    // and nothing told the person on another page.)
                    if self.ui.peek().tab().pane_showing(&Route::Page(job_page.clone())).is_none() {
                        let failed = self.ai.peek().as_ref().is_some_and(|s| matches!(s.status, AiStatus::Error(_)));
                        let title = self.page_title(&job_page);
                        self.toast(if failed { format!("The AI stopped on “{title}”") } else { format!("The AI is done on “{title}”") });
                    }
                    break;
                }
            }
        });
        if let Some(s) = self.ai.write().as_mut() {
            s.run = run;
            s.output.clear();
            s.steps.clear();
            s.did_before = if s.journal.is_some() && s.journal == journal { s.did.len() } else { 0 };
            s.did.clear();
            s.journal = journal;
            s.working = None;
            s.started = now_ms();
            s.heard = s.started;
            s.status = AiStatus::Streaming;
            s.task = Some(task);
        }
    }

    /// A run that changed things with the tools of Rune goes from the
    /// screen: a line at the foot of its page says what it did, with Undo.
    fn ai_note_done(self, session: &AiSession) {
        if session.did.is_empty() {
            return;
        }
        self.show_ai_done(AiDone {
            page: session.page_id.clone(),
            text: crate::mcp::count_line(&session.did),
            step: false,
            made: Vec::new(),
            changes: session.did.clone(),
            stamp: 0,
        });
    }

    /// Show the line that says what the AI did, for some seconds.
    fn show_ai_done(mut self, mut done: AiDone) {
        let stamp = now_ms() as u64;
        done.stamp = stamp;
        // It stays longer when something was removed: an event that the AI
        // removed can come back only by this Undo.
        let removed = done.changes.iter().any(|c| match c {
            crate::mcp::Change::EventWas { gone, .. } => *gone,
            crate::mcp::Change::PageWas { what, .. } => what == "trashed",
            _ => false,
        });
        let shown_ms = if removed { 30_000 } else { 9_000 };
        self.ai_done.set(Some(done));
        let mut slot = self.ai_done;
        spawn_forever(async move {
            ai::sleep_ms(shown_ms).await;
            if slot.peek().as_ref().is_some_and(|d| d.stamp == stamp) {
                slot.set(None);
            }
        });
    }

    /// A picture was pasted or dropped into the AI box: it goes with the
    /// request. (Into the box that is open: the bar of a page while it
    /// waits for words, or the box that asks all the notes.)
    pub fn ai_add_picture(mut self, data_url: &str) {
        let Some(picture) = ai::Picture::from_data_url(data_url) else {
            return self.toast("That picture is too large, or not a picture");
        };
        let in_bar = self.ai.peek().as_ref().is_some_and(|s| s.status == AiStatus::Prompting);
        let full = "No more than four pictures go with one request";
        // (The box that asks all the notes lies over the page: with both
        // open, the picture is for the one in front, as a file is in
        // `ai_give`. It went to the box of the page under it, out of
        // sight, and the question went without it.)
        if self.ui.peek().modal == Some(Modal::AskNotes) {
            if self.ask_pictures.peek().len() >= ai::MOST_PICTURES {
                return self.toast(full);
            }
            self.ask_pictures.write().push(picture);
        } else if in_bar {
            let mut ai = self.ai.write();
            let Some(s) = ai.as_mut() else { return };
            if s.pictures.len() >= ai::MOST_PICTURES {
                drop(ai);
                return self.toast(full);
            }
            s.pictures.push(picture);
        }
    }

    /// Take a picture out again, before the request is sent.
    pub fn ai_remove_picture(mut self, id: &str) {
        if let Some(s) = self.ai.write().as_mut() {
            s.pictures.retain(|p| p.id != id);
        }
        self.ask_pictures.write().retain(|p| p.id != id);
    }

    /// "Do it" under a plan for a large job: the AI does the work now.
    pub fn ai_do_plan(mut self) {
        let plan = self.ai.peek().as_ref().and_then(|s| ai::split_plan(&ai::clean_reply(&s.output).0)).map(|(_, steps)| steps.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n"));
        let Some(plan) = plan else { return };
        if let Some(s) = self.ai.write().as_mut() {
            s.plan = Some(plan);
        }
        self.ai_run();
    }

    /// "Change the plan": back to the request, to say it in another way.
    pub fn ai_change_plan(mut self) {
        self.ai_stop();
        if let Some(s) = self.ai.write().as_mut() {
            s.plan = None;
            s.output.clear();
            s.steps.clear();
            s.status = AiStatus::Prompting;
        }
    }

    /// "Undo" in the AI box, after a run that changed things with the tools:
    /// all of it is taken back, and the box goes.
    pub fn ai_undo_run(mut self) {
        let Some(session) = self.ai.peek().clone() else { return };
        self.ai_stop();
        let did = self.ai.peek().as_ref().map(|s| s.did.clone()).unwrap_or(session.did);
        *self.ai.write() = None;
        if !did.is_empty() {
            self.take_back(&did);
            self.toast("Taken back.");
        }
    }

    /// Take back what the tools of Rune changed (see mcp.rs), and show the
    /// notes as they are now in every window.
    pub fn take_back(self, changes: &[crate::mcp::Change]) {
        store::with(|s| crate::mcp::undo_changes(s, changes));
        sync::reload_now();
    }

    pub fn ai_stop(mut self) {
        let task = self.ai.peek().as_ref().and_then(|s| s.task);
        if let Some(t) = task {
            t.cancel();
        }
        if let Some(s) = self.ai.write().as_mut() {
            s.task = None;
            if s.status == AiStatus::Streaming {
                // Stopped part way: what the tools changed up to now is known.
                s.did = s.journal.as_deref().map(crate::mcp::read_journal).unwrap_or_default();
                if s.output.trim().is_empty() && !s.did.is_empty() {
                    s.output = "Stopped.".into();
                }
                s.status = if s.output.is_empty() { AiStatus::Prompting } else { AiStatus::Done };
            }
        }
        if self.ai.peek().as_ref().is_some_and(|s| !s.did.is_empty()) {
            sync::check_outside_changes();
        }
    }

    pub fn ai_discard(mut self) {
        self.ai_stop();
        let session = self.ai.peek().clone();
        *self.ai.write() = None;
        if let Some(s) = session {
            self.ai_note_done(&s);
            self.focus(Caret::end(&s.anchor));
        }
    }

    /// A page as the AI is given it to answer about or to change: its title,
    /// and its blocks as Markdown in which a picture, a drawing and a link
    /// to a page are lines that can be kept where they are.
    fn page_for_ai(&self, page_id: &str) -> String {
        // A locked page that is open has its lines in plain here: the AI
        // gets of it what it gets of one that is shut, the line that says
        // it is locked. (Every way to give a page comes through here.)
        if crate::lock::is_open(page_id) {
            return page_as_given(&self.pages.peek(), page_id, &[Block::new(BlockKind::Locked, "")]);
        }
        page_as_given(&self.pages.peek(), page_id, self.blocks.peek().get(page_id).map(|bs| bs.as_slice()).unwrap_or_default())
    }

    /// The AI's reply is the page as it must be after a change: put it in
    /// place of the page. Lines that did not change stay the blocks they
    /// were, pictures and drawings are never lost, and it is one step to undo.
    pub fn ai_revise(mut self) {
        let Some(session) = self.ai.peek().clone() else { return };
        let Some(body) = ai::split_revision(&ai::clean_reply(&session.output).0) else { return };
        // The page itself, and the sub-pages the reply asks for.
        let (body, wanted) = ai::split_pages(&body);
        let title = self.pages.peek().get(&session.page_id).map(|p| p.title.clone()).unwrap_or_default();
        let empty = |blocks: &[Block]| blocks.iter().all(|b| b.text.trim().is_empty() && b.kind.is_editable());
        // (A part has no line with the page's name before it: a heading of
        // that name in it is a heading of the person's.)
        let title = if session.part.is_empty() { title } else { String::new() };
        if empty(&crate::mcp::parse_body(&self.pages.peek(), &crate::mcp::strip_title_heading(&body, &title))) {
            return self.toast("The AI sent an empty page, so nothing was changed");
        }
        // A long page is given to the AI cut short (see `ai::MAX_CONTEXT_CHARS`):
        // its reply is then the page without its end, and to put it in place
        // of the page would take the end away (the lines, the tables and
        // their rows, a transcript in a closed toggle).
        let whole = session.part.is_empty();
        let given_now = if whole { self.page_for_ai(&session.page_id) } else { self.part_for_ai(&session.page_id, &session.part) };
        if given_now.chars().count() > ai::MAX_CONTEXT_CHARS {
            return self.toast(if whole {
                "This page is too long for the AI to write again as a whole, so nothing was changed. Pick a part of it (drag a box over its lines) and ask about that part."
            } else {
                "This part is too long for the AI to write again, so nothing was changed. Pick a smaller part."
            });
        }
        // The reply is the page as it was when the AI was asked, changed.
        // Lines written on the page since then are not in it: put in place
        // of the page, it would take them away. (What the AI's own tools
        // changed on the page in this run is the AI's, and no reason to
        // refuse: that is taken out before the page is compared. With no
        // check at all after a run that used a tool, a line typed while the
        // AI made an event went when its page was put in place.)
        // (A line put between the blocks of the part since then is not in
        // the reply either: the part must still be one run of blocks.)
        let apart = !whole && {
            let blocks = self.blocks.peek();
            let at: Vec<usize> = blocks.get(&session.page_id).map(|bs| bs.iter().enumerate().filter(|(_, b)| session.part.contains(&b.id)).map(|(i, _)| i).collect()).unwrap_or_default();
            at.is_empty() || at[at.len() - 1] - at[0] + 1 != at.len()
        };
        // (Of this run: after "Try again" the page was given as the runs before it left it.)
        let did = session.did.get(session.did_before..).unwrap_or_default();
        let given_less_tools = if did.is_empty() {
            Some(given_now)
        } else {
            let mut pages = self.pages.peek().clone();
            let now = self.blocks.peek().get(&session.page_id).cloned().unwrap_or_default();
            crate::mcp::before_run(did, &session.page_id, &mut pages, now)
                .map(|was| if whole { page_as_given(&pages, &session.page_id, &was) } else { part_as_given(&pages, &was, &session.part) })
        };
        if apart || (!session.sent.is_empty() && given_less_tools.as_deref() != Some(session.sent.as_str())) {
            return self.toast("The page was changed after you asked, so the AI's page was not put in its place. Ask again to get it with your changes.");
        }
        // The sub-pages first, so that the page can link to them.
        let mut made: Vec<(String, Id)> = Vec::new();
        for (name, text) in wanted {
            let id = self.create_page(Some(session.page_id.clone()));
            if let Some(p) = self.pages.write().get_mut(&id) {
                p.title = name.clone();
            }
            self.save_page(&id);
            let blocks = crate::mcp::parse_body(&self.pages.peek(), &crate::mcp::strip_title_heading(&text, &name));
            self.blocks.write().insert(id.clone(), if empty(&blocks) { vec![Block::text("")] } else { blocks });
            self.save_blocks(&id);
            made.push((name, id));
        }
        let mut body = ai::link_pages(&body, &made);
        // A sub-page that the page does not link to gets its link at the end.
        for (name, id) in &made {
            let link = inline::page_url(id);
            if !body.contains(&link) {
                body.push_str(&format!("\n\n[{name}]({link})\n"));
            }
        }
        let new = crate::mcp::parse_body(&self.pages.peek(), &crate::mcp::strip_title_heading(&body, &title));
        let part = session.part.clone();
        let caret = self.edit_blocks(&session.page_id, |bs| {
            if part.is_empty() {
                let merged = keep_unchanged(bs, crate::mcp::keep_media(bs, new));
                *bs = merged;
                return bs.iter().rev().find(|b| b.kind.is_editable()).map(|b| Caret::end(&b.id));
            }
            // The picked part only: from its first block to its last, the
            // reply takes its place. The rest of the page is not touched.
            let at: Vec<usize> = bs.iter().enumerate().filter(|(_, b)| part.contains(&b.id)).map(|(i, _)| i).collect();
            let (first, last) = (*at.first()?, *at.last()?);
            if last - first + 1 != at.len() {
                return None;
            }
            let mut old: Vec<Block> = bs[first..=last].to_vec();
            let least = old.iter().map(|b| b.indent).min().unwrap_or(0);
            // The part went to the AI from the edge (see `part_for_ai`): the
            // blocks are matched from the edge too, and then all get the
            // step of the part again. (Added only to list items, a toggle
            // inside a toggle came back at the edge and left its parent.)
            for b in old.iter_mut() {
                b.indent -= least;
            }
            let mut merged = keep_unchanged(&old, crate::mcp::keep_media(&old, new));
            for b in merged.iter_mut() {
                b.indent = b.indent.saturating_add(least).min(MAX_INDENT);
            }
            let caret = merged.iter().rev().find(|b| b.kind.is_editable()).map(|b| Caret::end(&b.id));
            bs.splice(first..=last, merged);
            caret
        });
        if !part.is_empty() {
            self.picked.set(None);
        }
        // (A row that the AI gave another name: its page takes the name.)
        self.name_row_pages_of(&session.page_id);
        *self.ai.write() = None;
        let made: Vec<Id> = made.into_iter().map(|(_, id)| id).collect();
        // One step: Undo takes back the change and the sub-pages together.
        self.history.write().attach(&session.page_id, &[], &made);
        if made.is_empty() {
            self.toast("The page was changed. Undo brings it back.");
            self.ai_note_done(&session);
        } else {
            // Its sub-pages show under it in the sidebar; the foot of the
            // page says what was done, with a way to take it all back.
            self.ui.write().expanded.insert(session.page_id.clone());
            self.save_ui();
            let text = format!("Page changed · {} {} made and linked", made.len(), if made.len() == 1 { "sub-page" } else { "sub-pages" });
            self.show_ai_done(AiDone { page: session.page_id.clone(), text, step: true, made, changes: session.did.clone(), stamp: 0 });
        }
        if let Some(caret) = caret {
            self.focus(caret);
        }
    }

    /// Take back what the AI did last: its sub-pages go to the Trash and
    /// the page is as it was before.
    pub fn ai_undo_done(mut self) {
        let Some(done) = self.ai_done.peek().clone() else { return };
        self.ai_done.set(None);
        if !done.step {
            // What the AI did with the tools of Rune.
            self.take_back(&done.changes);
            return self.toast("Taken back.");
        }
        // While nothing was changed after it, this is the step that Undo takes back.
        let latest = self.current_page().as_deref() == Some(done.page.as_str()) && self.history.peek().last_created(&done.page) == done.made.as_slice();
        if latest {
            self.undo();
            self.toast("Taken back. The sub-pages are in the Trash.");
        } else {
            for id in &done.made {
                self.set_trashed(id, true);
            }
            self.toast("The sub-pages are in the Trash. The page was changed after that, so it stays as it is.");
        }
        if !done.changes.is_empty() {
            self.take_back(&done.changes);
        }
    }

    /// Put the AI reply into the page: replace the anchor block or insert below it.
    pub fn ai_accept(mut self, replace: bool) {
        let Some(session) = self.ai.peek().clone() else { return };
        // Drop any pretend tool calls the model wrote (see ai::clean_reply),
        // and the mark of a reply that is a whole page.
        let text = ai::clean_reply(&session.output).0;
        let text = ai::split_revision(&text).unwrap_or(text);
        let new_blocks = markdown::to_blocks(&text);
        if new_blocks.is_empty() {
            return;
        }
        // A rewrite of a line takes the line's place only while the line is
        // as it was sent. Words typed in it since then are not in the
        // reply, and went with the line: the reply goes under it.
        let line_now = self.blocks.peek().get(&session.page_id).and_then(|bs| bs.iter().find(|b| b.id == session.anchor).map(|b| b.text.clone()));
        let typed_since = replace && session.action.rewrites_block() && line_now.is_some_and(|now| now != session.line_sent);
        let replace = replace && !typed_since;
        if typed_since {
            self.toast("The line was changed after you asked, so the AI's text was put under it.");
        }
        let last_id = new_blocks.last().map(|b| b.id.clone());
        self.edit_blocks(&session.page_id, |bs| {
            let at = match index_of(bs, &session.anchor) {
                Some(i) => {
                    let base_indent = bs[i].indent;
                    let replace_anchor = replace
                        || (bs[i].kind == BlockKind::Text && bs[i].text.trim().is_empty());
                    let insert_at = if replace_anchor {
                        bs.remove(i);
                        i
                    } else {
                        subtree_end(bs, i)
                    };
                    let shifted: Vec<Block> =
                        new_blocks.into_iter().map(|b| { let ind = b.indent; b.with_indent(base_indent + ind) }).collect();
                    (insert_at, shifted)
                }
                None => (bs.len(), new_blocks),
            };
            let (insert_at, items) = at;
            for (offset, b) in items.into_iter().enumerate() {
                bs.insert(insert_at + offset, b);
            }
        });
        *self.ai.write() = None;
        self.ai_note_done(&session);
        if let Some(id) = last_id {
            self.focus(Caret::end(&id));
        }
    }

    // ----- commands (menu bar and keyboard shortcuts) ----------------------

    pub fn command(self, name: &str) {
        // (For the self-test: a script for the page of the meeting pad's own window.)
        if let Some(script) = name.strip_prefix("pad_run:") {
            return crate::pad::run_in_window(script);
        }
        if let Some(selector) = name.strip_prefix("pad_press:") {
            return crate::pad::press_in_window(selector);
        }
        match name {
            "settings" => self.set_modal(Some(Modal::Settings)),
            "new_page" => self.new_page(None),
            "new_tab" => self.new_tab(),
            "close_tab" => {
                let i = self.ui.peek().active;
                self.close_tab(i)
            }
            "reopen_tab" => self.reopen_closed_tab(),
            "new_window" => {
                let route = self.current_page().map(Route::Page).unwrap_or(Route::Home);
                crate::ui::open_window(route);
            }
            "undo" => {
                document::eval("window.__rune && window.__rune.undo()");
            }
            "redo" => {
                document::eval("window.__rune && window.__rune.redo()");
            }
            "bold" | "italic" | "strike" | "code" | "link" => {
                let kind = serde_json::to_string(name).unwrap_or_default();
                document::eval(&format!("window.__rune && window.__rune.format({kind})"));
            }
            "send_to_aven" => match self.current_page() {
                Some(id) => self.send_to_aven(&id),
                None => self.toast("Open a page to send it"),
            },
            "copy_link" => match self.current_page() {
                Some(id) => self.copy_to_clipboard(inline::page_url(&id), "Copied link to page"),
                None => self.toast("Open a page to copy its link"),
            },
            "copy_name" => match self.current_page() {
                Some(id) => self.copy_to_clipboard(self.page_title(&id), "Copied page name"),
                None => self.toast("Open a page to copy its name"),
            },
            "copy_markdown" => match self.current_page() {
                Some(id) => self.copy_to_clipboard(self.page_markdown(&id), "Copied page as Markdown"),
                None => self.toast("Open a page first"),
            },
            "meeting_notes" => {
                self.make_meeting_notes(None);
            }
            // The microphones. With an AI tool they open the voice box and the
            // AI writes; without one they type what is said, as it is said.
            "voice" => {
                if ai::has_ai() {
                    self.open_voice(VoiceFor::NewPage);
                } else {
                    self.plain_voice_page();
                }
            }
            // The microphone on an empty line: write where the caret is.
            "voice_here" => {
                let at = self.focus.peek().clone();
                match at {
                    // (On a locked page the words are typed as they are said, with no AI.)
                    Some((page, block)) if ai::has_ai() && !self.locked_lines(&page) => self.open_voice(VoiceFor::Into { page, block }),
                    Some(_) => self.dictate("line"),
                    None => self.command("voice"),
                }
            }
            // For the self-test: as if no AI tool were installed, and back.
            "ai_off_for_test" | "ai_on_for_test" => {
                if std::env::var_os("RUNE_SELFTEST").is_some() {
                    ai::pretend_no_ai(name == "ai_off_for_test");
                    self.show_ai_state();
                }
            }
            // A microphone beside a text box: what is said is typed into that
            // box, by the app's own listener or by the Mac's Dictation.
            "dictate" => self.dictate_box(),
            "dictate_stop" => Self::stop_box_ears(),
            "ask_notes" => {
                if ai::has_ai() {
                    self.set_modal(Some(Modal::AskNotes));
                } else {
                    self.open_settings_at("AI");
                }
            }
            // Paste from the short text menu: the page script puts it where the caret is.
            "paste_clipboard" => {
                if let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    let text = serde_json::to_string(&text).unwrap_or_default();
                    document::eval(&format!("window.__rune && window.__rune.pasteText({text})"));
                }
            }
            "read_aloud" => {
                if let Some(id) = self.current_page() {
                    self.toggle_reading(&id);
                }
            }
            // The self-test stands in for the person's clicks on a real meeting.
            "listen_stop" => self.stop_listening(),
            "picked_copy" => self.copy_picked(false),
            "picked_cut" => self.copy_picked(true),
            "picked_delete" => {
                let Some(picked) = self.picked.peek().clone() else { return };
                let step_was = self.history.peek().next_undo(&picked.page);
                self.delete_picked();
                // (Said only when they went: a locked page keeps its lines.)
                if self.history.peek().next_undo(&picked.page) != step_was {
                    let n = picked.ids.len();
                    self.toast_step(if n == 1 { "Deleted 1 block".to_string() } else { format!("Deleted {n} blocks") }, &picked.page, step_was);
                }
            }
            "meeting_recap" => {
                if let Some(page) = self.current_page() {
                    self.ai_recap(&page);
                }
            }
            // A press outside the AI box while nothing is typed in it.
            "ai_close_idle" => {
                let mut s = self;
                let idle = s.ai.peek().as_ref().is_some_and(|a| a.status == AiStatus::Prompting && a.prompt.trim().is_empty());
                if idle {
                    *s.ai.write() = None;
                }
            }
            // What the meeting watcher calls; also reachable by name so the
            // self-test can stand in for a real meeting.
            "meeting_started" => self.meeting_started(crate::meeting::TEAMS.name),
            "meeting_ended" => self.meeting_ended(now_ms()),
            // (For the self-test: during that call, a second call program
            // takes the microphone; then it goes silent while the first call
            // goes on; and the watcher sees that no call is on.)
            "meeting_joined" => self.meeting_joined("Zoom"),
            "meeting_left" => self.meeting_left("Zoom", &[crate::meeting::TEAMS.name], now_ms()),
            "meeting_left_first" => self.meeting_left(crate::meeting::TEAMS.name, &["Zoom"], now_ms()),
            "meeting_quiet" => self.meeting_quiet(),
            // The self-test draws the meeting pad in this window (see
            // `pad::inline`), and makes the pad's questions old at once.
            "pad_inline" => crate::pad::set_inline(true),
            "pad_inline_off" => {
                self.close_pad();
                crate::pad::set_inline(false);
            }
            "pad_old" => self.change_pad(|p| p.since -= 10 * when::MINUTE),
            // Only when the test asks for the pad's real window (RUNE_REAL_PAD).
            // (For the try of the pad by hand: this window goes behind the
            // others and gives the keyboard back.)
            "step_back" => crate::glass::step_back(&dioxus::desktop::window().window),
            "pad_window" => crate::pad::set_real_in_test(true),
            // (For a picture of the pad in a test run. A pad that is kept out
            // of screen captures cannot be photographed: `screencapture -l`
            // on its window says "could not create image from window", which
            // also shows that the keeping-out works.)
            "pad_in_captures" => store::set_setting("pad_shared", "on"),
            "pad_window_off" => {
                self.close_pad();
                crate::pad::set_real_in_test(false);
            }
            "pad_probe" => {
                let facts = serde_json::to_string(&crate::ui::pad::probe().to_string()).unwrap_or_default();
                document::eval(&format!("document.body.dataset.padProbe = {facts};"));
            }
            "export_page" => match self.current_page() {
                Some(id) => self.export_page(&id),
                None => self.toast("Open a page first"),
            },
            "export_all" => self.export_all(),
            "import" => self.import(),
            "quick_find" => self.set_modal(Some(Modal::QuickFind)),
            // Find in the page in front (drawn and run by bridge.js).
            "find" => {
                document::eval("window.__rune && window.__rune.find()");
            }
            "find_next" => {
                document::eval("window.__rune && window.__rune.findStep(1)");
            }
            "find_prev" => {
                document::eval("window.__rune && window.__rune.findStep(-1)");
            }
            "calendar" => self.navigate(Route::Calendar),
            "graph" => self.navigate(Route::Graph),
            "close_split" => self.close_pane(None),
            // (For the self-test: what was learned about meetings by their time is forgotten.)
            "forget_meeting_slots" => {
                save_meeting_slots(&[]);
                store::set_setting("meeting_homes", "");
            }
            // (For the self-test: the next meeting that starts is a new one.)
            "forget_meeting" => {
                MEETING.with(|m| *m.borrow_mut() = None);
                self.keep_meeting();
                keep_declined(Vec::new());
            }
            // (For the self-test: the watcher hears the sound of the meeting's
            // program again, with no microphone; the answer is left for the test.)
            "meeting_back" => {
                let back = self.meeting_back(crate::meeting::TEAMS.name);
                document::eval(&format!("document.body.dataset.meetingBack = '{}';", if back { "yes" } else { "no" }));
            }
            // (For the self-test: the meeting went quiet half an hour ago.)
            "meeting_old" => MEETING.with(|m| {
                if let Some(end) = m.borrow_mut().as_mut().and_then(|n| n.ended.as_mut()) {
                    *end -= 30 * when::MINUTE;
                }
            }),
            // (For the self-test: the app is closed and opened again. What it
            // knows of the meeting is what it wrote down; its pad is gone.)
            "meeting_restart" => {
                self.keep_meeting();
                MEETING.with(|m| *m.borrow_mut() = None);
                self.close_pad();
                let apps = self.recall_meeting();
                document::eval(&format!("document.body.dataset.meetingApp = {:?};", apps.join(", ")));
            }
            // (For the self-test: the meetings that start take no event of the
            // calendar, so that a check of a meeting with no event does not
            // hang on what the other checks left there at this hour.)
            "meetings_alone" | "meetings_alone_off" => {
                if std::env::var_os("RUNE_SELFTEST").is_some() {
                    MEETING_ALONE.with(|m| m.set(name == "meetings_alone"));
                }
            }
            "space_next" => self.step_space(1),
            "space_prev" => self.step_space(-1),
            "trash" => self.set_modal(Some(Modal::Trash)),
            "page_history" => {
                if let Some(id) = self.current_page() {
                    self.open_history(&id);
                }
            }
            "ai" => self.ai_shortcut(),
            // ⌃⌥Space in any app: the AI, open and ready to type in. A second
            // press leaves it as it is (it does not start again or shut).
            "ai_anywhere" => {
                let open = self.ai.peek().is_some() || self.ui.peek().modal == Some(Modal::AskNotes);
                if !open {
                    self.ai_shortcut();
                }
                document::eval("requestAnimationFrame(()=>requestAnimationFrame(()=>{const t=document.getElementById('ai-input')||document.getElementById('ask-notes-input');if(t)t.focus();}))");
            }
            "toggle_sidebar" => self.toggle_sidebar(),
            "focus_mode" => self.toggle_focus_mode(),
            "lock_page" => match self.current_page() {
                Some(id) if self.listens_in(&id) => {}
                Some(id) if !crate::lock::page_lock(&self, &id).is_locked() => self.set_modal(Some(Modal::LockPage)),
                Some(_) => {}
                None => self.toast("Open a page first"),
            },
            "lock_now" => {
                if let Some(id) = self.current_page() {
                    self.relock(&id);
                }
            }
            "unlock_page" => {
                if let Some(id) = self.current_page() {
                    self.remove_password(&id);
                }
            }
            "sync_now" => {
                let sync = crate::devsync::status();
                if sync.icloud || sync.wifi {
                    crate::devsync::nudge();
                    self.toast("Syncing");
                } else {
                    self.open_settings_at("Sync");
                }
            }
            "dark_mode" => {
                let dark = match self.ui.peek().theme {
                    Theme::Dark => false,
                    Theme::Light => true,
                    Theme::System => !crate::ui::window_is_dark(),
                };
                self.set_theme(if dark { Theme::Dark } else { Theme::Light });
            }
            "zoom_in" => self.zoom(0.1),
            "zoom_out" => self.zoom(-0.1),
            "zoom_reset" => self.zoom(0.0),
            "back" => self.back(),
            "forward" => self.forward(),
            "next_tab" => self.cycle_tab(1),
            "prev_tab" => self.cycle_tab(-1),
            // Ctrl+Tab and Ctrl+Shift+Tab: the order of use (bridge.js
            // sends "tab_walk_done" when Control is let go).
            "tab_walk" => self.walk_tabs(1),
            "tab_walk_back" => self.walk_tabs(-1),
            "tab_walk_done" => self.walk_tabs_done(),
            "tab_walk_cancel" => self.walk_tabs_cancel(),
            "favorite" => {
                if let Some(id) = self.current_page() {
                    self.toggle_favorite(&id);
                }
            }
            "escape" => {
                let mut s = self;
                s.clear_slash();
                let open = {
                    let ui = s.ui.peek();
                    ui.popover.is_some() || ui.modal.is_some()
                };
                if open {
                    let mut ui = s.ui.write();
                    ui.popover = None;
                    ui.modal = None;
                }
            }
            // The thing an open menu hangs on went out of sight (bridge.js,
            // "an open menu stays at its thing"): the menu closes, and
            // nothing else does.
            "close_menu" => {
                self.clear_slash();
                self.set_popover(None);
            }
            other => {
                if let Some(n) = other.strip_prefix("tab_").and_then(|n| n.parse::<usize>().ok()) {
                    let len = self.ui.peek().tabs.len();
                    self.select_tab(if n == 9 { len - 1 } else { n - 1 });
                }
            }
        }
    }
}


// ---------------------------------------------------------------------------
// First-run content
// ---------------------------------------------------------------------------

fn seed(pages: &mut HashMap<Id, Page>, blocks: &mut HashMap<Id, Vec<Block>>, space: &str) {
    let mut welcome = Page::new(None, 1.0);
    welcome.title = "Getting Started".into();
    welcome.icon = Some("sparkles:purple".into());
    welcome.favorite = true;

    let mut ideas = Page::new(Some(welcome.id.clone()), 1.0);
    ideas.title = "Ideas".into();
    ideas.icon = Some("lightbulb:yellow".into());

    let mut reading = Page::new(None, 2.0);
    reading.title = "Reading List".into();
    reading.icon = Some("book:blue".into());

    let mut done = Block::new(BlockKind::Todo, "Open this page");
    done.checked = true;
    let mut code = Block::new(BlockKind::Code, "fn main() {\n    println!(\"Hello from Rust\");\n}");
    code.language = Some("rust".into());
    let mut tips = Block::new(
        BlockKind::Callout,
        "AI is optional. It uses a tool that is already on this Mac: **Claude Code** or **Codex** (Settings, ⌘, then AI shows how to set one up). Then press ⌘J or type `/ai` to ask, summarize, rewrite or translate.",
    );
    tips.icon = Some("sparkles:purple".into());
    let mut agents = Block::new(
        BlockKind::Callout,
        "Claude, Codex and other AI tools can read and write these pages too: Settings (⌘,), AI, **Connect AI tools** gives each tool its lines.",
    );
    agents.icon = Some("bot:blue".into());

    let welcome_blocks = vec![
        Block::text("Welcome to your **local-first** workspace, written in Rust. Everything is saved on this Mac as you type."),
        Block::new(BlockKind::Heading2, "The basics"),
        done,
        Block::new(BlockKind::Todo, "Click anywhere and start typing"),
        Block::new(BlockKind::Todo, "Type `/` to add headings, lists, to-dos, code, sub-pages and more"),
        Block::new(
            BlockKind::Todo,
            "Select text to make it **bold**, _italic_, ~~struck through~~, `code` or a [link](https://www.rust-lang.org)",
        ),
        Block::new(BlockKind::Todo, "Try Markdown shortcuts: `#` heading, `-` bullet, `1.` number, `[]` to-do, `>` toggle"),
        Block::new(BlockKind::Todo, "Drag the handle to move a block, or press ⌘⇧↑ / ⌘⇧↓"),
        Block::new(BlockKind::Toggle, "Keyboard shortcuts"),
        Block::new(BlockKind::Bulleted, "⌘K search, ⌘N new page, ⌘⇧N new window, ⌘\\ hide the sidebar").with_indent(1),
        Block::new(BlockKind::Bulleted, "⌘T new tab, ⌘W close tab, ⌘⇧T reopen it, ⌃Tab next tab").with_indent(1),
        Block::new(BlockKind::Bulleted, "⌘B bold, ⌘I italic, ⌘E code, ⌘⇧S strikethrough, ⌘K on selected text adds a link").with_indent(1),
        Block::new(BlockKind::Bulleted, "⌘Z undo and ⌘⇧Z redo, across the whole page").with_indent(1),
        Block::new(BlockKind::Bulleted, "⌘\\[ and ⌘\\] go back and forward").with_indent(1),
        Block::new(BlockKind::Bulleted, "Tab / ⇧Tab nest and un-nest blocks, ⌘D duplicates").with_indent(1),
        Block::new(BlockKind::Bulleted, "⌘L copies a link to the page, ⌘⇧L switches dark mode").with_indent(1),
        Block::new(BlockKind::Heading2, "AI"),
        tips,
        agents,
        Block::new(BlockKind::Heading2, "Blocks"),
        Block::new(BlockKind::Quote, "Simplicity is prerequisite for reliability."),
        code,
        Block::new(BlockKind::Divider, ""),
        Block::page_link(ideas.id.clone()),
    ];
    let idea_blocks = vec![
        Block::text("A sub-page. Pages can nest as deep as you like."),
        Block::new(BlockKind::Bulleted, "Weekend project: a habit tracker"),
        Block::new(BlockKind::Bulleted, "Learn more about _async_ Rust"),
        Block::new(BlockKind::Bulleted, "Write up notes from the reading list"),
    ];
    let reading_blocks = vec![
        Block::new(BlockKind::Numbered, "_The Rust Programming Language_"),
        Block::new(BlockKind::Numbered, "_Designing Data-Intensive Applications_"),
        Block::new(BlockKind::Numbered, "_A Philosophy of Software Design_"),
    ];

    for (mut page, bs) in [(welcome, welcome_blocks), (ideas, idea_blocks), (reading, reading_blocks)] {
        page.space_id = space.to_string();
        store::with(|s| s.save_page(&page));
        store::with(|s| s.save_blocks(&page.id, &bs));
        blocks.insert(page.id.clone(), bs);
        pages.insert(page.id.clone(), page);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_table_put_back_while_it_is_in_another_page_is_a_copy() {
        let table = |id: &str| {
            let mut b = Block::new(BlockKind::Table, "");
            b.id = id.to_string();
            b
        };
        let line = Block::text("a line");
        let current = vec![line.clone(), table("stays")];
        // Undo puts two tables back: one is in no page (it comes back as
        // the block it was), one was pasted in another page (a copy, with
        // an id of its own). The table that never left, and lines, are as they are.
        let mut target = vec![line.clone(), table("stays"), table("nowhere"), table("moved")];
        let (back, copies) = tables_put_back(&mut target, &current, |id| id == "moved" || id == "stays");
        assert_eq!(back, ["nowhere"]);
        assert_eq!((target[0].id.as_str(), target[1].id.as_str(), target[2].id.as_str()), (line.id.as_str(), "stays", "nowhere"));
        assert!(target[3].id != "moved" && target[3].kind == BlockKind::Table);
        assert_eq!(copies, [("moved".to_string(), target[3].id.clone())]);
    }

    #[test]
    fn undo_and_redo_of_a_cut_whose_table_is_in_another_page_keep_one_copy() {
        let mut table = Block::new(BlockKind::Table, "");
        table.id = "moved".into();
        let line = Block::text("a line");
        // The cut of the table on this page; the table is in another page now.
        let mut h = crate::history::History::default();
        h.record("p", vec![line.clone(), table], vec![line.clone()], None, 0);
        let mut page = vec![line];
        let mut step = |page: &mut Vec<Block>, undo: bool| {
            let mut e = if undo { h.take_undo("p") } else { h.take_redo("p") }.unwrap();
            let mut target = if undo { crate::history::rebase(page, &e.after, &e.before) } else { crate::history::rebase(page, &e.before, &e.after) };
            let (_, copies) = tables_put_back(&mut target, page, |id| id == "moved");
            for (was, now) in &copies {
                e.rename_block(was, now);
            }
            *page = target;
            if undo { h.push_redo(e) } else { h.push_undo(e) }
            page.iter().filter(|b| b.kind == BlockKind::Table).count()
        };
        // Undo: one copy. Redo takes that copy away. Undo again: one copy, not two.
        assert_eq!(step(&mut page, true), 1);
        assert_eq!(step(&mut page, false), 0);
        assert_eq!(step(&mut page, true), 1);
        assert_eq!(step(&mut page, false), 0);
    }

    #[test]
    fn a_path_typed_in_search_finds_the_pages_that_are_there() {
        let mut pages: HashMap<Id, Page> = HashMap::new();
        let mut add = |id: &str, title: &str, parent: Option<&str>, space: &str| {
            let mut p = Page::new(parent.map(String::from), 1.0);
            p.id = id.into();
            p.title = title.into();
            p.space_id = space.into();
            pages.insert(id.into(), p);
        };
        add("w", "Work", None, "s1");
        add("plan", "Plan", Some("w"), "s1");
        add("deep", "Notes", Some("plan"), "s1");
        add("other", "Work", None, "s2");
        assert_eq!(path_parts(" Work / Plan //Ideas "), vec!["Work", "Plan", "Ideas"]);
        assert!(path_parts(" / ").is_empty());
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // All there: the deepest page, nothing to make.
        assert_eq!(find_path(&pages, "s1", &names(&["work", "PLAN"])), (Some("plan".into()), vec![]));
        // Part there: the rest is made inside the last page found.
        assert_eq!(find_path(&pages, "s1", &names(&["Work", "Plan", "Ideas", "Later"])), (Some("plan".into()), names(&["Ideas", "Later"])));
        // None there: all of it is made at the top.
        assert_eq!(find_path(&pages, "s1", &names(&["Trips", "Rome"])), (None, names(&["Trips", "Rome"])));
        // The first name may be a page further down; other spaces do not count.
        assert_eq!(find_path(&pages, "s1", &names(&["Notes"])), (Some("deep".into()), vec![]));
        assert_eq!(find_path(&pages, "s2", &names(&["Work", "Plan"])), (Some("other".into()), names(&["Plan"])));
    }

    fn page_in(id: &str, space: &str) -> (Id, Page) {
        let mut p = Page::new(None, 1.0);
        p.id = id.into();
        p.space_id = space.into();
        (id.to_string(), p)
    }

    fn ui_with(tabs: Vec<Tab>, space: &str) -> Ui {
        Ui {
            tabs,
            active: 0,
            closed: Vec::new(),
            parked: HashMap::new(),
            sidebar_open: true,
            sidebar_width: 240.0,
            expanded: HashSet::new(),
            popover: None,
            modal: None,
            zoom: 1.0,
            theme: Theme::parse(""),
            recent: Vec::new(),
            space: space.into(),
            history: None,
            voice: VoiceFor::NewPage,
            focus_mode: false,
            folds: HashMap::new(),
        }
    }

    fn titles(tabs: &[Tab]) -> Vec<String> {
        tabs.iter()
            .map(|t| match t.route() {
                Route::Page(id) => id.clone(),
                _ => "home".to_string(),
            })
            .collect()
    }

    #[test]
    fn each_space_keeps_its_own_tabs() {
        let pages: HashMap<Id, Page> = [page_in("a", "one"), page_in("b", "one"), page_in("t", "two")].into_iter().collect();
        let mut ui = ui_with(vec![Tab::new(Route::Page("a".into())), Tab::new(Route::Page("b".into()))], "one");
        ui.active = 1;
        // A space with no tabs yet starts with one.
        assert!(ui.show_tabs_of("two", &pages, Route::Page("t".into())));
        assert_eq!((titles(&ui.tabs), ui.space.as_str()), (vec!["t".to_string()], "two"));
        // Back again: the same tabs, the same one in front, and none is made.
        assert!(!ui.show_tabs_of("one", &pages, Route::Home));
        assert_eq!((titles(&ui.tabs), ui.active), (vec!["a".to_string(), "b".to_string()], 1));
        assert_eq!(titles(&ui.parked["two"].0), vec!["t".to_string()]);
    }

    #[test]
    fn pinned_tabs_stand_first_and_stay_when_the_others_close() {
        let tab = |id: &str| Tab::new(Route::Page(id.into()));
        let mut ui = ui_with(vec![tab("a"), tab("b"), tab("c"), tab("d")], "one");
        let pins = |ui: &Ui| ui.tabs.iter().filter(|t| t.pinned).count();
        let names = |tabs: &[Tab]| titles(tabs).join(" ");
        // A pinned tab goes to the left, after the ones pinned before it; the tab in front stays in front.
        ui.active = 3;
        ui.pin_tab(2, true);
        assert_eq!((names(&ui.tabs).as_str(), pins(&ui), ui.active), ("c a b d", 1, 3));
        ui.pin_tab(3, true);
        assert_eq!((names(&ui.tabs).as_str(), pins(&ui), ui.active), ("c d a b", 2, 1));
        // Unpinned, it is the first of the tabs that are not pinned.
        ui.pin_tab(0, false);
        assert_eq!((names(&ui.tabs).as_str(), pins(&ui), ui.active), ("d c a b", 1, 0));
        ui.pin_tab(9, true);
        assert_eq!(pins(&ui), 1, "no such tab: nothing changes");
        // A tab put before a pinned one (a drop, a page that changed its space) goes back behind them.
        ui.tabs.swap(0, 3);
        ui.active = 0;
        assert!(pins_first(&mut ui.tabs, &mut ui.active));
        assert_eq!((names(&ui.tabs).as_str(), ui.active), ("d b c a", 1));
        assert!(!pins_first(&mut ui.tabs, &mut ui.active), "in order: nothing moves");
        // "Close Other Tabs" keeps the tab it was asked on and every pinned one.
        let gone = ui.close_others(3);
        assert_eq!((names(&ui.tabs).as_str(), ui.active, names(&gone).as_str()), ("d a", 1, "b c"));
        // The pages of a pinned tab, each in a tab of its own: the first one keeps the pin.
        let mut two = tab("x");
        two.pinned = true;
        two.add_pane(Pane::new(Route::Page("y".into())), None, Edge::Right);
        assert_eq!(two.split_apart().iter().map(|t| t.pinned).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn a_pinned_tab_whose_page_goes_to_the_trash_is_pinned_no_more() {
        let pin = |id: &str| {
            let mut t = Tab::new(Route::Page(id.into()));
            t.pinned = true;
            t
        };
        let mut two = pin("x");
        two.add_pane(Pane::new(Route::Page("y".into())), None, Edge::Right);
        let mut ui = ui_with(vec![pin("a"), pin("b"), two, Tab::new(Route::Page("c".into()))], "one");
        ui.active = 1;
        ui.leave_pages(&["a".to_string(), "y".to_string()]);
        // The tab of "a" shows Home and is a plain tab, behind the pinned
        // ones. A pinned tab that keeps a page keeps its pin. The tab in
        // front is still in front.
        let row: Vec<String> = ui.tabs.iter().zip(titles(&ui.tabs)).map(|(t, name)| format!("{}{name}", if t.pinned { "*" } else { "" })).collect();
        assert_eq!(row.join(" "), "*b *x home c");
        assert_eq!(titles(&ui.tabs)[ui.active], "b");
        // In a space that is not in front too.
        ui.parked.insert("two".into(), (vec![pin("p")], 0));
        ui.leave_pages(&["p".to_string()]);
        assert!(!ui.parked["two"].0[0].pinned);
    }

    #[test]
    fn duplicate_tab_copies_its_pages_layout_and_history_beside_it() {
        // A tab of two pages, the left one with a page to go Back to, the
        // right one folded and in focus; it is pinned.
        let mut two = Tab::new(Route::Page("a".into()));
        two.push(Route::Page("b".into()));
        let right = Pane::new(Route::Page("c".into()));
        let right_id = right.id.clone();
        two.add_pane(right, None, Edge::Right);
        two.folded.push(right_id.clone());
        two.pinned = true;
        let mut ui = ui_with(vec![two, Tab::new(Route::Page("x".into())), Tab::new(Route::Home)], "one");
        let names = ui.duplicate(0).expect("a tab of pages is copied");
        // The copy is not pinned: it stands after the pinned tabs, in front.
        assert_eq!((titles(&ui.tabs), ui.active), (vec!["c".into(), "c".into(), "x".into(), "home".into()], 1));
        let (from, copy) = (&ui.tabs[0], &ui.tabs[1]);
        assert!(from.pinned && !copy.pinned);
        assert_ne!(from.id, copy.id);
        // Each pane is a copy with its own id, its history and its place in it.
        assert_eq!(names.len(), 2);
        for (old, new) in &names {
            assert_ne!(old, new);
            let (o, n) = (from.pane(old).unwrap(), copy.pane(new).unwrap());
            assert_eq!((&o.history, o.cursor), (&n.history, n.cursor));
        }
        let renamed = |id: &str| names.iter().find(|(o, _)| o == id).unwrap().1.clone();
        assert_eq!(copy.layout.panes(), from.layout.panes().iter().map(|p| renamed(p)).collect::<Vec<_>>());
        assert!(copy.layout.fits(&copy.panes.iter().map(|p| p.id.clone()).collect::<Vec<_>>()));
        assert_ne!(copy.layout.key(), from.layout.key(), "the row of the copy is a row of its own");
        assert_eq!((copy.focus.clone(), copy.folded.clone()), (renamed(&right_id), vec![renamed(&right_id)]));
        assert!(copy.ordered()[0].can_back());
        // A tab of one page: right after it.
        assert!(ui.duplicate(2).is_some());
        assert_eq!((titles(&ui.tabs), ui.active), (vec!["c".into(), "c".into(), "x".into(), "x".into(), "home".into()], 3));
        // Home (and the calendar, the graph) has one tab: no copy.
        assert!(ui.duplicate(4).is_none() && ui.duplicate(9).is_none());
        assert_eq!(ui.tabs.len(), 5);
    }

    #[test]
    fn ctrl_tab_goes_in_the_order_of_use_and_holding_control_walks_further() {
        let tabs: Vec<Tab> = ["a", "b", "c", "d"].iter().map(|p| Tab::new(Route::Page(p.to_string()))).collect();
        let id = |name: &str| tabs[["a", "b", "c", "d"].iter().position(|n| *n == name).unwrap()].id.clone();
        let name = |id: &Option<Id>| id.as_ref().map(|id| ["a", "b", "c", "d"][tabs.iter().position(|t| &t.id == id).unwrap()]);
        let every: Vec<&Tab> = tabs.iter().collect();
        let mut u = TabUse::default();
        // Used: a, then c, then d (b never).
        for n in ["a", "c", "d"] {
            u.used(&id(n), &every);
        }
        // Ctrl+Tab: the tab used before this one. Control let go, it is
        // first; a second Ctrl+Tab goes back again.
        assert_eq!(name(&u.walk(&id("d"), &tabs, 1)), Some("c"));
        u.walk = None;
        u.used(&id("c"), &every);
        assert_eq!(name(&u.walk(&id("c"), &tabs, 1)), Some("d"));
        u.walk = None;
        u.used(&id("d"), &every);
        // Control held, Tab again and again: further back, then the tabs
        // never used (in the order of the row), then round. The order does
        // not change meanwhile (each step brings a tab forward: `used`).
        let mut front = id("d");
        let mut walked = Vec::new();
        for _ in 0..4 {
            let to = u.walk(&front, &tabs, 1);
            front = to.clone().unwrap();
            u.used(&front, &every);
            walked.push(name(&to).unwrap());
        }
        assert_eq!(walked, ["c", "a", "b", "d"]);
        // Shift goes the other way along the same order; let go at b, b is first.
        let to = u.walk(&front, &tabs, -1);
        assert_eq!(name(&to), Some("b"));
        u.used(&id("b"), &every);
        u.walk = None;
        u.used(&id("b"), &every);
        assert_eq!(u.order.iter().map(|i| name(&Some(i.clone())).unwrap()).collect::<Vec<_>>(), ["b", "d", "c", "a"]);
        // Ctrl+Shift+Tab at once: the tab used longest ago.
        assert_eq!(name(&u.walk(&id("b"), &tabs, -1)), Some("a"));
        // A press on another tab in the middle of a walk ends it: that tab is first.
        u.used(&id("c"), &every);
        assert!(u.walk.is_none());
        assert_eq!(u.order[0], id("c"));
        // A closed tab leaves the order, and the walk passes it by.
        let left: Vec<Tab> = tabs.iter().filter(|t| t.id != id("b")).cloned().collect();
        u.used(&id("c"), &left.iter().collect::<Vec<_>>());
        assert!(!u.order.contains(&id("b")));
        assert_eq!(name(&u.walk(&id("c"), &left, 1)), Some("d"));
        u.walk = None;
        // Only the tabs of the row in front (the space in front) are walked;
        // one tab alone has nowhere to go.
        assert_eq!(name(&u.walk(&id("c"), &tabs[2..], 1)), Some("d"));
        u.walk = None;
        assert_eq!(u.walk(&id("c"), &tabs[2..3], 1), None);
    }

    #[test]
    fn a_pin_is_saved_with_its_tab_and_an_older_rune_reads_the_tabs_all_the_same() {
        // The tab as every Rune before pins reads it (`SavedTab` of 0.1.20).
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct TabBeforePins {
            id: Id,
            #[serde(default)]
            panes: Vec<Pane>,
            #[serde(default)]
            layout: Option<Layout>,
            #[serde(default)]
            focus: Option<Id>,
            #[serde(default)]
            history: Vec<Route>,
            #[serde(default)]
            cursor: usize,
            #[serde(default)]
            split: Option<Route>,
            #[serde(default)]
            folded: Vec<Id>,
        }
        let plain = Tab::new(Route::Page("a".into()));
        let mut pinned = Tab::new(Route::Page("b".into()));
        pinned.pinned = true;
        // A tab that is not pinned is written as before, to the letter: no new word in it.
        let old_words = serde_json::to_string(&plain).unwrap();
        assert!(!old_words.contains("pinned"), "{old_words}");
        assert!(old_words.starts_with("{\"id\":") && old_words.ends_with("\"folded\":[]}"), "{old_words}");
        // What an older Rune wrote reads as tabs with no pin.
        let read: Tab = serde_json::from_str(&old_words).unwrap();
        assert_eq!(read, plain);
        let oldest: Tab = serde_json::from_str(&format!("{{\"id\":\"t\",\"history\":[{}],\"cursor\":0}}", serde_json::to_string(&Route::Page("z".into())).unwrap())).unwrap();
        assert!(!oldest.pinned && oldest.route() == &Route::Page("z".into()));
        // The row of tabs and the tabs of the other spaces, with a pin: it comes back…
        let row = serde_json::to_string(&(vec![pinned.clone(), plain.clone()], 1usize)).unwrap();
        let (tabs, active): (Vec<Tab>, usize) = serde_json::from_str(&row).unwrap();
        assert_eq!((tabs, active), (vec![pinned.clone(), plain.clone()], 1));
        let sets: HashMap<Id, TabSet> = [("two".to_string(), (vec![pinned.clone()], 0))].into_iter().collect();
        let back: HashMap<Id, TabSet> = serde_json::from_str(&serde_json::to_string(&sets).unwrap()).unwrap();
        assert!(back["two"].0[0].pinned);
        // …and an older Rune reads the same words as plain tabs, each with its page.
        let (old, front): (Vec<TabBeforePins>, usize) = serde_json::from_str(&row).unwrap();
        assert_eq!((old.len(), front, old[0].id.as_str(), old[0].panes.len()), (2, 1, pinned.id.as_str(), 1));
        assert_eq!(old[0].panes[0].route(), &Route::Page("b".into()));
        assert!(serde_json::from_str::<HashMap<Id, (Vec<TabBeforePins>, usize)>>(&serde_json::to_string(&sets).unwrap()).is_ok());
    }

    #[test]
    fn only_the_first_window_keeps_its_folds_recent_pages_and_zoom() {
        let mut ui = ui_with(vec![Tab::new(Route::Home)], "one");
        ui.zoom = 1.5;
        ui.expanded.insert("a".into());
        let keys = |primary: bool| ui_settings(&ui, primary).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(keys(false), ["theme"], "a second window would write its old copy over the first window's");
        assert_eq!(keys(true), ["theme", "expanded", "recent", "sidebar_open", "sidebar_width", "zoom", "tabs", "space_tabs", "folds"]);
        assert!(ui_settings(&ui, true).contains(&("zoom", "1.5".to_string())));
        // The folded headings: by page, then by heading; read back as they were written.
        ui.folds.insert("p".into(), ["h1".to_string(), "h2".to_string()].into_iter().collect());
        let kept = ui_settings(&ui, true).into_iter().find(|(k, _)| *k == "folds").map(|(_, v)| v).unwrap_or_default();
        assert_eq!(serde_json::from_str::<HashMap<Id, HashSet<Id>>>(&kept).ok(), Some(ui.folds.clone()));
    }

    #[test]
    fn a_page_trashed_by_another_program_is_found_behind_the_page_on_show() {
        let page = |id: &str| Route::Page(id.to_string());
        // A tab went from a to b; another program then put a in the Trash.
        let mut tab = Tab::new(page("a"));
        tab.push(page("b"));
        let mut ui = ui_with(vec![tab], "one");
        let trashed = ["a".to_string(), "old".to_string()];
        assert!(ui.remembers(&trashed), "a is not on show, but Back leads to it");
        ui.leave_pages(&trashed);
        assert_eq!((titles(&ui.tabs), ui.tabs[0].can_back()), (vec!["b".to_string()], false));
        // Nothing is held now: a reload with the same Trash changes nothing.
        assert!(!ui.remembers(&trashed));
        // A tab of a space that is not in front counts too.
        ui.parked.insert("two".into(), (vec![Tab::new(page("old"))], 0));
        assert!(ui.remembers(&trashed));
    }

    #[test]
    fn back_does_not_open_a_page_that_went_to_the_trash() {
        let page = |id: &str| Route::Page(id.to_string());
        // One tab went from a to b; a second shows a beside c.
        let mut first = Tab::new(page("a"));
        first.push(page("b"));
        let mut second = Tab::new(page("c"));
        second.add_pane(Pane::new(page("a")), None, Edge::Right);
        let mut ui = ui_with(vec![first, second], "one");
        ui.recent = vec!["b".into(), "a".into()];
        // b goes to the Trash: its tab shows Home, and Back leads to a, not to b.
        ui.leave_pages(&["b".to_string()]);
        assert_eq!(titles(&ui.tabs), ["home", "a"]);
        assert_eq!(ui.tabs[0].panes[0].history, [page("a"), Route::Home]);
        assert!(ui.tabs[0].can_back() && !ui.tabs[0].can_forward());
        // a goes too: the pane beside c closes, and the first tab has only Home left.
        ui.leave_pages(&["a".to_string()]);
        assert_eq!((ui.tabs[1].panes.len(), titles(&ui.tabs)), (1, vec!["home".to_string(), "c".to_string()]));
        assert_eq!(ui.tabs[0].panes[0].history, [Route::Home]);
        assert!(!ui.tabs[0].can_back());
        assert!(ui.recent.is_empty());
        // A page behind the one on show is forgotten as well, and the page on show stays.
        let mut tab = Tab::new(page("x"));
        tab.push(page("y"));
        let mut ui = ui_with(vec![tab], "one");
        ui.leave_pages(&["x".to_string()]);
        assert_eq!((titles(&ui.tabs), ui.tabs[0].can_back()), (vec!["y".to_string()], false));
    }

    #[test]
    fn undo_of_the_trash_shows_the_page_in_the_tab_it_left_and_makes_no_tab() {
        let page = |id: &str| Route::Page(id.to_string());
        // One tab, with the page "x" after "w": it goes to the Trash.
        let mut one = Tab::new(page("w"));
        one.focused_mut().push(page("x"));
        let left = one.id.clone();
        let mut ui = ui_with(vec![one], "s1");
        ui.leave_pages(&["x".to_string()]);
        assert_eq!(ui.tabs.len(), 1);
        assert_eq!(ui.tabs[0].route(), &Route::Home, "the Trash leaves Home in its place");
        // Undo: the same tab shows it again. (It was opened in a new tab,
        // and this one stayed beside it at Home: two tabs for one.)
        assert!(ui.put_page_back_in(&left, "x"));
        assert_eq!(ui.tabs.len(), 1, "no tab is made");
        assert_eq!(ui.tabs[0].route(), &page("x"));
        assert!(!ui.tabs.iter().any(|t| t.route() == &Route::Home), "no Home tab is left over");
        // …and Back goes where it went before the Trash: to "w", not to Home.
        let pane = ui.tabs[0].focused_mut();
        assert_eq!(pane.history[pane.cursor - 1], page("w"));
        // From another tab, the tab it left comes to the front.
        ui.leave_pages(&["x".to_string()]);
        ui.tabs.push(Tab::new(page("y")));
        ui.active = 1;
        assert!(ui.put_page_back_in(&left, "x"));
        assert_eq!((ui.active, ui.tabs.len()), (0, 2));
        // The tab shows something else by now, or is gone: nothing changes.
        assert!(!ui.put_page_back_in(&left, "x"), "it shows the page, not Home");
        ui.leave_pages(&["x".to_string()]);
        ui.tabs[0].focused_mut().push(page("z"));
        let before = ui.clone();
        assert!(!ui.put_page_back_in(&left, "x"));
        assert!(!ui.put_page_back_in("no such tab", "x"));
        assert!(ui == before);
        // A page in one pane of a split tab closed its pane: no Home stands
        // for it, and a Home the person opened there is not taken.
        let mut home = Tab::new(Route::Home);
        home.panes.push(Pane::new(page("y")));
        let split = home.id.clone();
        let mut ui = ui_with(vec![home], "s1");
        assert!(!ui.put_page_back_in(&split, "x"));
    }

    #[test]
    fn what_a_deleted_space_held_goes_to_the_space_that_takes_it() {
        let mut pages: HashMap<Id, Page> = [page_in("a", "one"), page_in("old", "two")].into_iter().collect();
        let mut events: HashMap<Id, Event> =
            [Event::new("two", "Call", 0, 1, false), Event::new("one", "Lunch", 0, 1, false)].into_iter().map(|e| (e.title.clone(), e)).collect();
        rehome(&mut pages, &mut events, &["two".to_string()], "one");
        assert_eq!((pages["a"].space_id.as_str(), pages["old"].space_id.as_str()), ("one", "one"));
        assert_eq!((events["Call"].space_id.as_str(), events["Lunch"].space_id.as_str()), ("one", "one"));
        // Nothing of a space that stays is touched.
        pages.insert("w".into(), page_in("w", "work").1);
        rehome(&mut pages, &mut events, &["two".to_string()], "one");
        assert_eq!(pages["w"].space_id, "work");
    }

    #[test]
    fn a_tab_goes_with_a_page_moved_to_another_space() {
        let mut pages: HashMap<Id, Page> = [page_in("a", "one"), page_in("b", "one"), page_in("t", "two")].into_iter().collect();
        let mut ui = ui_with(vec![Tab::new(Route::Page("a".into())), Tab::new(Route::Page("b".into()))], "one");
        ui.parked.insert("two".into(), (vec![Tab::new(Route::Page("t".into()))], 0));
        // The tab in front is carried over and is in front there.
        pages.get_mut("a").unwrap().space_id = "two".into();
        ui.carry_tab_to("two", &pages);
        assert_eq!((titles(&ui.tabs), ui.active, ui.space.as_str()), (vec!["t".to_string(), "a".to_string()], 1, "two"));
        assert_eq!(titles(&ui.parked["one"].0), vec!["b".to_string()]);
        // A tab put away finds its page gone to the space you left: it moves.
        pages.get_mut("b").unwrap().space_id = "two".into();
        assert!(ui.show_tabs_of("one", &pages, Route::Home), "the space is left with no tab, so it gets one");
        assert_eq!(titles(&ui.tabs), vec!["home".to_string()]);
        assert_eq!(titles(&ui.parked["two"].0), vec!["t".to_string(), "a".to_string(), "b".to_string()]);
    }

    #[test]
    fn the_tabs_of_every_space_survive_a_restart() {
        let mut parked: HashMap<Id, TabSet> = HashMap::new();
        parked.insert("two".into(), (vec![Tab::new(Route::Home), Tab::new(Route::Page("t".into()))], 1));
        let saved = serde_json::to_string(&parked).unwrap();
        let back: HashMap<Id, TabSet> = serde_json::from_str(&saved).unwrap();
        assert_eq!(back, parked);
    }

    #[test]
    fn tabs_saved_in_one_row_are_sorted_by_space() {
        let pages: HashMap<Id, Page> = [page_in("a", "one"), page_in("t", "two"), page_in("u", "two")].into_iter().collect();
        let mut mixed = Tab::new(Route::Page("t".into()));
        mixed.push(Route::Page("a".into()));
        mixed.push(Route::Page("u".into()));
        let tabs = vec![Tab::new(Route::Home), mixed, Tab::new(Route::Page("a".into()))];
        let (here, active, others) = sort_tabs_by_space(tabs, 2, "one", &pages);
        assert_eq!((titles(&here), active), (vec!["home".to_string(), "a".to_string()], 1));
        assert_eq!(titles(&others["two"].0), vec!["u".to_string()]);
        // Back in that tab stays in its space.
        let pane = &others["two"].0[0].panes[0];
        assert_eq!(pane.history, vec![Route::Page("t".into()), Route::Page("u".into())]);
        assert_eq!(pane.cursor, 1);
    }

    #[test]
    fn a_saved_tab_loses_a_page_that_is_gone_and_keeps_the_one_on_show() {
        let (mut pages, mut pane): (HashMap<Id, Page>, Pane) = ([page_in("a", "one"), page_in("b", "one"), page_in("c", "one")].into_iter().collect(), Pane::new(Route::Page("a".into())));
        pane.push(Route::Page("b".into()));
        pane.push(Route::Page("c".into()));
        pane.cursor = 1;
        pages.get_mut("a").unwrap().trashed = true;
        let mut tabs = vec![Tab::with_pane(pane)];
        forget_gone(&mut tabs, &pages);
        assert_eq!(tabs[0].route(), &Route::Page("b".into()));
    }

    #[test]
    fn the_place_of_the_video_player() {
        let usual = DockPlace::default();
        assert_eq!(usual.style(), "left: auto; right: max(8px, min(18px, calc(100vw - 440px - 8px))); top: auto; bottom: max(8px, min(18px, calc(100vh - 282px - 8px))); width: min(440px, calc(100vw - 16px));");
        // Every side is named, also at the other corner.
        let moved = DockPlace { h: "left".into(), v: "top".into(), x: 30.4, y: 52.0, w: 700.0 }.tidy();
        assert!(moved.style().starts_with("left: max(8px, min(30px, calc(100vw - 700px - 8px))); right: auto; top: max(8px, min(52px, calc(100vh - 428px - 8px))); bottom: auto;"), "{}", moved.style());
        // Whatever is sent, the page gets values it can use.
        let odd = DockPlace { h: "up".into(), v: "".into(), x: f64::NAN, y: -40.0, w: 12.0 }.tidy();
        assert_eq!(odd, DockPlace { h: "right".into(), v: "bottom".into(), x: 0.0, y: 0.0, w: DockPlace::LEAST });
        assert_eq!(DockPlace { w: 99999.0, ..DockPlace::default() }.tidy().w, DockPlace::MOST);
    }

    use super::*;

    /// A page for the tests of the list of places.
    fn place_page(id: &str, title: &str, space: &str, parent: Option<&str>, position: f64) -> Page {
        Page { id: id.into(), title: title.into(), space_id: space.into(), ..Page::new(parent.map(str::to_string), position) }
    }

    fn two_spaces() -> (Vec<Space>, HashMap<Id, Page>) {
        let mut work = Space::new("Work", "blue", 1.0);
        work.id = "work".into();
        let mut trading = Space::new("Trading", "green", 2.0);
        trading.id = "trading".into();
        let pages = [
            place_page("acme", "Acme", "work", None, 1.0),
            place_page("deep1", "One", "work", Some("acme"), 1.0),
            place_page("deep2", "Two", "work", Some("deep1"), 1.0),
            place_page("deep3", "Three", "work", Some("deep2"), 1.0),
            place_page("deep4", "Four", "work", Some("deep3"), 1.0),
            place_page("notes", "Meeting notes", "trading", None, 1.0),
            place_page("call", "Call", "trading", Some("notes"), 1.0),
            place_page("inside", "Inside the call", "trading", Some("call"), 1.0),
            place_page("old", "Old", "trading", None, 2.0),
        ];
        let mut pages: HashMap<Id, Page> = pages.into_iter().map(|p| (p.id.clone(), p)).collect();
        pages.get_mut("old").unwrap().trashed = true;
        (vec![work, trading], pages)
    }

    #[test]
    fn every_space_and_its_pages_can_be_chosen_to_keep_a_page_in() {
        let (spaces, pages) = two_spaces();
        let rows = places_to_keep(&pages, &spaces, "call", &[]);
        let seen: Vec<(&str, usize)> = rows.iter().map(|r| (r.id.as_str(), r.depth)).collect();
        // Grouped by space, as deep as the pages go; not the page itself, what
        // is inside it, or the Trash.
        assert_eq!(seen, [("work", 0), ("acme", 1), ("deep1", 2), ("deep2", 3), ("deep3", 4), ("deep4", 5), ("trading", 0), ("notes", 1)]);
        assert_eq!(rows[0].badge, Some(("W".to_string(), "blue".to_string())));
        assert_eq!(rows[1].badge, None);
        // The notes of a meeting are not a place, but pages inside one are.
        let rows = places_to_keep(&pages, &spaces, "acme", &["call".to_string()]);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["work", "trading", "notes", "inside"]);
        assert_eq!(rows[3].depth, 2);
    }

    #[test]
    fn with_one_space_the_list_of_places_has_no_groups() {
        let (mut spaces, pages) = two_spaces();
        spaces.remove(0);
        let rows = places_to_keep(&pages, &spaces, "call", &[]);
        let seen: Vec<(&str, usize)> = rows.iter().map(|r| (r.id.as_str(), r.depth)).collect();
        assert_eq!(seen, [("", 0), ("notes", 0)]);
        assert_eq!(rows[0].title, NO_PAGE);
    }

    #[test]
    fn where_a_page_is_kept_is_where_it_really_is() {
        let (spaces, pages) = two_spaces();
        let (place, space) = place_of(&pages, &spaces, "call");
        assert_eq!((place.id.as_str(), place.title.as_str(), space.as_str()), ("notes", "Meeting notes", "Trading"));
        let (place, space) = place_of(&pages, &spaces, "acme");
        assert_eq!((place.id.as_str(), place.title.as_str(), space.as_str()), ("work", "Work", ""));
        let one = &spaces[..1];
        let (place, space) = place_of(&pages, one, "acme");
        assert_eq!((place.id.as_str(), place.title.as_str(), space.as_str()), ("", NO_PAGE, ""));
    }

    #[test]
    fn a_page_can_move_to_another_space() {
        let (_, pages) = two_spaces();
        // Into a page of another space, and to the top of another space's list.
        assert_eq!(plan_page_move(&pages, "work", "call", Some("deep2"), "into"), Some((Some("deep2".to_string()), 2.0, "work".to_string())));
        assert_eq!(plan_page_move(&pages, "work", "call", None, "root"), Some((None, 2.0, "work".to_string())));
    }

    #[test]
    fn a_call_that_is_noticed_again_is_the_same_meeting_or_the_next_one() {
        const MIN: i64 = when::MINUTE;
        let t0 = 1_800_000_000_000;
        let standup = ("a".to_string(), t0, t0 + 30 * MIN);
        let review = ("b".to_string(), t0 + 30 * MIN, t0 + 60 * MIN);
        let focus = ("c".to_string(), t0 - 60 * MIN, t0 + 120 * MIN);
        // Its own event is still on: the same meeting, however long it was quiet.
        assert!(same_meeting(Some(&standup), t0 + 2 * MIN, t0 + 29 * MIN, Some(&standup)));
        assert!(same_meeting(Some(&standup), t0 + 2 * MIN, t0 + 29 * MIN, None));
        // It runs over its time: the same meeting for 20 minutes after it went quiet…
        assert!(same_meeting(Some(&standup), t0 + 31 * MIN, t0 + 45 * MIN, None));
        assert!(!same_meeting(Some(&standup), t0 + 31 * MIN, t0 + 52 * MIN, None));
        // …and an event that has been on for a long time (a block of the day) does not make it another one.
        assert!(same_meeting(Some(&standup), t0 + 31 * MIN, t0 + 45 * MIN, Some(&focus)));
        // Another event starts about now: the next meeting, though the last one went quiet a minute ago.
        assert!(!same_meeting(Some(&standup), t0 + 30 * MIN, t0 + 31 * MIN, Some(&review)));
        assert!(!same_meeting(None, t0 + 28 * MIN, t0 + 29 * MIN, Some(&review)));
        // A meeting with no event: the same one for 20 minutes, then a new one.
        assert!(same_meeting(None, t0, t0 + 19 * MIN, None));
        // A no to a call holds while that call can still be the one that is
        // back: the same program, within the same time, and no event of the
        // calendar that starts about now.
        let said = |app: Option<&str>, at: i64| Declined { app: app.map(str::to_string), seen: at, said: at, of: None };
        let no = said(Some("Discord"), t0);
        assert!(still_declined(Some(&no), "Discord", t0 + 7 * MIN, None), "the call went quiet and is back");
        assert!(still_declined(Some(&no), "Discord", t0 + 19 * MIN, Some(&focus)), "an event that is on, but did not start now");
        assert!(!still_declined(Some(&no), "Discord", t0 + 21 * MIN, None), "a later call asks again");
        assert!(!still_declined(Some(&no), "Microsoft Teams", t0 + MIN, None), "a call in another program asks");
        let late = said(Some("Discord"), t0 + 25 * MIN);
        assert!(still_declined(Some(&late), "Discord", t0 + 29 * MIN, None));
        assert!(!still_declined(Some(&late), "Discord", t0 + 29 * MIN, Some(&review)), "a meeting of the calendar starts now");
        assert!(!still_declined(None, "Discord", t0, None));
        // (Notes made by hand and then refused: any program.)
        assert!(still_declined(Some(&said(None, t0)), "Zoom", t0 + MIN, None));
        // A meeting of the calendar that was refused: its own event is not
        // the next meeting, and the no holds while that event is on.
        let of_standup = Declined { of: Some(standup.clone()), ..said(Some("Microsoft Teams"), t0 + MIN) };
        assert!(still_declined(Some(&of_standup), "Microsoft Teams", t0 + 7 * MIN, Some(&standup)), "its own event started just now");
        assert!(still_declined(Some(&of_standup), "Microsoft Teams", t0 + 28 * MIN, Some(&standup)), "quiet for long, but its event is on");
        assert!(!still_declined(Some(&of_standup), "Microsoft Teams", t0 + 31 * MIN, Some(&review)), "the next meeting starts");
        // A no does not last for ever: a call seen all day asks again.
        let all_day = Declined { seen: t0 + 5 * 60 * MIN, ..said(Some("Discord"), t0) };
        assert!(!still_declined(Some(&all_day), "Discord", t0 + 5 * 60 * MIN + MIN, None));
        assert!(!same_meeting(None, t0, t0 + 20 * MIN, None));
        // …and an event that started long before does not cut it in two.
        assert!(same_meeting(None, t0, t0 + 5 * MIN, Some(&focus)));
        // The same event of a repeat on another day is another meeting.
        let tomorrow = ("a".to_string(), t0 + 24 * 60 * MIN, t0 + 24 * 60 * MIN + 30 * MIN);
        assert!(!same_meeting(Some(&standup), t0 + 29 * MIN, t0 + 24 * 60 * MIN + MIN, Some(&tomorrow)));
    }

    #[test]
    fn a_no_to_a_call_holds_at_a_pause_near_an_event() {
        const MIN: i64 = when::MINUTE;
        let now = 1_800_000_000_000i64;
        // Refused an hour ago, on since, quiet for forty seconds; an event
        // of the calendar starts in five minutes.
        let no = Declined { app: Some("Discord".into()), seen: now - 40_000, said: now - 60 * MIN, of: None };
        let event = ("e1".to_string(), now + 5 * MIN, now + 35 * MIN);
        assert!(still_declined(Some(&no), "Discord", now, None));
        assert!(still_declined(Some(&no), "Discord", now, Some(&event)), "a pause of the long call is not the meeting of the calendar");
        // The same for a no that was said under the name of an event that is over.
        let named = Declined { of: Some(("e0".to_string(), now - 60 * MIN, now - 30 * MIN)), ..no.clone() };
        assert!(still_declined(Some(&named), "Discord", now, Some(&event)));
        // A call in another program at that moment is that meeting, and asks.
        assert!(!still_declined(Some(&no), "Microsoft Teams", now, Some(&event)));
        // A call that was refused and has ended, however long it was: the
        // meeting of the calendar that starts in the same program some
        // minutes later is the next one, and asks.
        let event_now = ("e2".to_string(), now, now + 30 * MIN);
        let ended = Declined { app: Some("Microsoft Teams".into()), seen: now - 5 * MIN, said: now - 35 * MIN, of: None };
        assert!(!still_declined(Some(&ended), "Microsoft Teams", now, Some(&event_now)));
        let short = Declined { app: Some("Microsoft Teams".into()), seen: now - 4 * MIN, said: now - 9 * MIN, of: None };
        assert!(!still_declined(Some(&short), "Microsoft Teams", now, Some(&event)));
        // With no event near, a call that is back within twenty minutes is still that call.
        assert!(still_declined(Some(&ended), "Microsoft Teams", now, None));
        // The long call asks again after its twenty quiet minutes.
        let quiet = Declined { seen: now - 21 * MIN, ..no.clone() };
        assert!(!still_declined(Some(&quiet), "Discord", now, Some(&event)));
        // The long call, silent for three minutes: still a pause.
        let three = Declined { seen: now - 190_000, said: now - 3 * 60 * MIN, ..no.clone() };
        assert!(still_declined(Some(&three), "Discord", now, Some(&event)));
        // A call refused a moment ago that is silent for three minutes has ended.
        let brief = Declined { seen: now - 190_000, said: now - 6 * MIN, ..no.clone() };
        assert!(!still_declined(Some(&brief), "Discord", now, Some(&event)));
    }

    #[test]
    fn the_event_of_a_no_keeps_it_only_for_the_call_that_was_refused() {
        const MIN: i64 = when::MINUTE;
        let start = 1_800_000_000_000i64;
        let event = ("e1".to_string(), start, start + 30 * MIN);
        // Refused ten minutes before the event, under its name, and over
        // five minutes before it: the call at the start of the event is
        // the meeting of the event, and asks.
        let over = Declined { app: Some("Microsoft Teams".into()), seen: start - 5 * MIN, said: start - 10 * MIN, of: Some(event.clone()) };
        assert!(!still_declined(Some(&over), "Microsoft Teams", start, Some(&event)));
        assert!(!still_declined(Some(&over), "Microsoft Teams", start + 16 * MIN, Some(&event)));
        // Joined ten minutes early, refused, and still in that call when the event starts: refused.
        let still_on = Declined { seen: start + 2 * MIN, ..over.clone() };
        assert!(still_declined(Some(&still_on), "Microsoft Teams", start + 12 * MIN, Some(&event)));
        assert!(still_declined(Some(&still_on), "Microsoft Teams", start + 28 * MIN, Some(&event)), "quiet for long, but its event is on");
        // That call paused for a minute just before the event: refused.
        let paused = Declined { seen: start - MIN, ..over.clone() };
        assert!(still_declined(Some(&paused), "Microsoft Teams", start, Some(&event)));
    }

    #[test]
    fn notes_bound_to_a_meetings_program_stay_bound_to_it() {
        // Notes made from an event, bound to the meeting's program; the
        // voice call takes the microphone again: they stay the meeting's.
        assert_eq!(bound_to(None, "Microsoft Teams"), "Microsoft Teams");
        assert_eq!(bound_to(Some("Microsoft Teams"), "Discord"), "Microsoft Teams");
        assert_eq!(bound_to(Some("Microsoft Teams"), "Microsoft Teams"), "Microsoft Teams");
        // A call that passes from the browser to its own program is bound to that.
        assert_eq!(bound_to(Some("Google Chrome"), "Zoom"), "Zoom");
        // The notes that are on are another program's: made for it, or bound to it.
        assert!(notes_of_another(Some("Microsoft Teams"), None, "Discord"));
        assert!(notes_of_another(None, Some("Microsoft Teams"), "Discord"));
        assert!(!notes_of_another(None, None, "Discord"), "notes made by hand, with no program yet");
        assert!(!notes_of_another(None, Some("Discord"), "Discord"));
    }

    #[test]
    fn a_refused_call_that_starts_leaves_the_notes_of_another_program_alone() {
        const MIN: i64 = when::MINUTE;
        let now = 1_800_000_000_000i64;
        let nos = vec![Declined { app: Some("Discord".into()), seen: now - MIN, said: now - 5 * 60 * MIN, of: None }];
        // The notes of a meeting in another call program: not its notes,
        // though its no is too old to hold (it was seen a minute ago).
        assert!(!still_declined(no_for(&nos, "Discord"), "Discord", now, None));
        assert!(refused_lately(&nos, "Discord", now));
        assert!(not_its_notes(&nos, Some("Microsoft Teams"), "Discord", now));
        // Notes made by hand, its own notes, and a call that passes from or to a browser can carry on.
        assert!(!not_its_notes(&nos, None, "Discord", now));
        assert!(!not_its_notes(&nos, Some("Discord"), "Discord", now));
        assert!(!not_its_notes(&nos, Some("Google Chrome"), "Discord", now));
        // A program that was not refused, or was refused long ago, is not held back.
        assert!(!not_its_notes(&nos, Some("Microsoft Teams"), "Zoom", now));
        assert!(!refused_lately(&nos, "Discord", now + 20 * MIN));
        assert!(!not_its_notes(&nos, Some("Microsoft Teams"), "Discord", now + 20 * MIN));
    }

    #[test]
    fn the_meeting_is_written_down_in_a_form_that_reads_back() {
        let kept = KeptMeeting { page: "p".into(), stamp: "s".into(), started: 5, ended: None, app: Some("Zoom".into()), noticed: true, span: Some((1, 9)), event: Some("e".into()), padded: true, listened: false, seen: 7, draft: true };
        let back: KeptMeeting = serde_json::from_str(&serde_json::to_string(&kept).unwrap()).unwrap();
        assert_eq!((back.page.as_str(), back.started, back.ended, back.app.as_deref(), back.span, back.event.as_deref(), back.padded, back.seen), ("p", 5, None, Some("Zoom"), Some((1, 9)), Some("e"), true, 7));
        // What an older run wrote (or nothing) is not a meeting.
        assert!(serde_json::from_str::<KeptMeeting>("").is_err());
    }

    #[test]
    fn there_is_one_no_for_each_program() {
        let t0 = 1_800_000_000_000i64;
        let said = |app: &str, at: i64| Declined { app: Some(app.to_string()), seen: at, said: at, of: None };
        // A long call is refused; then a meeting in a second program is refused too.
        let nos = with_no(Vec::new(), said("Discord", t0));
        let nos = with_no(nos, said("Microsoft Teams", t0 + 60_000));
        assert_eq!(nos.len(), 2);
        assert!(still_declined(no_for(&nos, "Discord"), "Discord", t0 + 120_000, None), "the no to the first call holds");
        assert!(still_declined(no_for(&nos, "Microsoft Teams"), "Microsoft Teams", t0 + 120_000, None));
        assert_eq!(no_for(&nos, "Zoom"), None);
        // A later no to the same program takes the place of the earlier one.
        let nos = with_no(nos, said("Discord", t0 + 180_000));
        assert_eq!(nos.len(), 2);
        assert_eq!(no_for(&nos, "Discord").map(|d| d.said), Some(t0 + 180_000));
        // A new meeting in the second program asks again: only its no goes.
        let nos = without_no(nos, "Microsoft Teams");
        assert_eq!(nos.iter().map(|d| d.app.as_deref()).collect::<Vec<_>>(), vec![Some("Discord")]);
        // The list is written down and read back; what an older version
        // wrote (one no, not in a list) is read as a list of one.
        let text = serde_json::to_string(&nos).unwrap();
        assert_eq!(read_declined(&text), nos);
        let old = serde_json::to_string(&said("Discord", t0)).unwrap();
        assert_eq!(read_declined(&old), vec![said("Discord", t0)]);
        assert_eq!(read_declined(""), Vec::new());
    }

    #[test]
    fn blocks_are_picked_alone_with_their_nested_lines_or_as_a_range() {
        let blocks = vec![
            Block::text("a"),
            Block::new(BlockKind::Toggle, "b"),
            Block::text("b1").with_indent(1),
            Block::text("b2").with_indent(1),
            Block::new(BlockKind::Divider, ""),
            Block::text("c"),
        ];
        let id = |i: usize| blocks[i].id.clone();
        let texts = |ids: Vec<Id>| ids.iter().map(|i| blocks.iter().find(|b| &b.id == i).map(|b| b.text.clone()).unwrap_or_default()).collect::<Vec<_>>();
        // One block, with what is nested in it.
        assert_eq!(texts(pick_blocks(&blocks, None, &id(0))), vec!["a"]);
        assert_eq!(texts(pick_blocks(&blocks, None, &id(1))), vec!["b", "b1", "b2"]);
        assert_eq!(pick_blocks(&blocks, None, &id(4)), vec![id(4)]);
        // With Shift: from what is picked to the pressed block, either way.
        let held = vec![id(0)];
        assert_eq!(texts(pick_blocks(&blocks, Some(&held), &id(4))), vec!["a", "b", "b1", "b2", ""]);
        let held = vec![id(5)];
        assert_eq!(texts(pick_blocks(&blocks, Some(&held), &id(1))), vec!["b", "b1", "b2", "", "c"]);
        assert!(pick_blocks(&blocks, None, "gone").is_empty());
        // As Markdown, a picked part of a page has no title line.
        let md = markdown::from_blocks("", &blocks[..2], |_| String::new());
        assert!(md.starts_with("a\n") && !md.contains("# "), "{md}");
    }

    #[test]
    fn the_line_of_the_day_places_its_events() {
        let day = 1_000_000_000_000 - 1_000_000_000_000 % (24 * crate::when::HOUR);
        let at = |h: i64, m: i64| day + h * crate::when::HOUR + m * crate::when::MINUTE;
        let event = |title: &str, start: i64, end: i64, all_day: bool| Event {
            id: new_id(),
            space_id: "home".into(),
            title: title.into(),
            start,
            end,
            all_day,
            color: "blue".into(),
            notes: String::new(),
            join: String::new(),
            page_id: None,
            remind: None,
            repeat: None,
            created_at: 0,
            updated_at: 0,
        };
        // Nothing today: 8 AM to 6 PM, and "now" on the line.
        let empty = day_ribbon(&[], day, at(13, 0));
        assert_eq!((empty.first_hour, empty.last_hour, empty.events.len(), empty.now), (8, 18, 0, Some(50.0)));
        assert_eq!(empty.rows, 1);
        assert_eq!(day_ribbon(&[], day, at(7, 0)).now, None);
        assert_eq!(day_ribbon(&[], day, at(19, 30)).now, None);
        // Events in order, each a part of the line; all-day ones apart.
        let events = vec![
            event("Review", at(11, 0), at(12, 0), false),
            event("Sync", at(9, 0), at(9, 30), false),
            event("Call", at(17, 0), at(17, 30), false),
            event("Holiday", day, day + 24 * crate::when::HOUR, true),
        ];
        let r = day_ribbon(&events, day, at(10, 20));
        assert_eq!(r.events.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), vec!["Sync", "Review", "Call"]);
        assert_eq!(r.all_day.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), vec!["Holiday"]);
        assert_eq!((r.events[0].left, r.events[0].width), (10.0, 5.0));
        assert_eq!((r.events[1].left, r.events[1].width), (30.0, 10.0));
        // A name has the room up to the next event; the last one, near the end, is set from its right.
        assert_eq!(r.events[0].room, 19.0);
        // Its box runs back from its right edge (95), not over the whole line.
        assert!(!r.events[0].at_end && r.events[2].at_end && (r.events[2].room - 40.0).abs() < 0.001 && (r.events[2].box_left - 55.0).abs() < 0.001);
        // Far enough apart: each has a place of its own, with its name and its time on two lines.
        assert_eq!((r.rows, r.events.iter().map(|e| (e.row, e.together)).collect::<Vec<_>>()), (1, vec![(0, false), (0, false), (0, false)]));
        assert!((r.now.unwrap() - 23.333).abs() < 0.01);
        // Two meetings at 11, and one that begins before the first is over:
        // one place on the line, their names one under the other.
        let clash = vec![
            event("Design review", at(11, 0), at(12, 0), false),
            event("Budget call", at(11, 0), at(11, 30), false),
            event("Stand-up", at(11, 45), at(12, 0), false),
            event("Lunch", at(13, 0), at(14, 0), false),
        ];
        let r = day_ribbon(&clash, day, at(10, 0));
        let on = |title: &str| r.events.iter().find(|e| e.title == title).unwrap();
        assert_eq!(r.rows, 3);
        assert_eq!((on("Budget call").row, on("Design review").row, on("Stand-up").row), (0, 1, 2), "in the order they start, the shorter first");
        assert!(on("Budget call").together && on("Design review").together && on("Stand-up").together);
        // They share the place: the same start for their names and the same room, up to "Lunch".
        for name in ["Budget call", "Design review", "Stand-up"] {
            assert!((on(name).box_left - 30.0).abs() < 0.001 && (on(name).room - 19.0).abs() < 0.001, "{name}");
        }
        // Each keeps its own bar on the line, at its own time.
        assert!((on("Stand-up").left - 37.5).abs() < 0.001 && (on("Design review").width - 10.0).abs() < 0.001);
        assert_eq!((on("Lunch").row, on("Lunch").together), (0, false), "far enough: a place of its own");
        // More at one time than there are lines for: three names, and how many more.
        let many: Vec<Event> = (0..5).map(|i| event(&format!("M{i}"), at(15, 0), at(16, 0), false)).collect();
        let r = day_ribbon(&many, day, at(10, 0));
        assert_eq!(r.rows, 3);
        assert_eq!(r.events.iter().map(|e| (e.named, e.more)).collect::<Vec<_>>(), vec![(true, 0), (true, 0), (true, 2), (false, 0), (false, 0)]);
        // An early and a late event make the line longer; a very short one stays visible.
        let long = vec![event("Run", at(6, 30), at(6, 35), false), event("Dinner", at(20, 0), at(21, 15), false)];
        let r = day_ribbon(&long, day, at(12, 0));
        assert_eq!((r.first_hour, r.last_hour), (6, 22));
        assert!(r.events[0].width >= 1.2 && r.events[1].left + r.events[1].width <= 100.0);
        // An event that began yesterday or ends tomorrow stays on the line.
        let over = vec![event("Night", day - crate::when::HOUR, at(1, 0), false), event("Late", at(23, 0), at(25, 0), false)];
        let r = day_ribbon(&over, day, at(12, 0));
        assert_eq!((r.first_hour, r.last_hour, r.events[0].left), (0, 24, 0.0));
        assert!(r.events.iter().all(|e| e.left >= 0.0 && e.left + e.width <= 100.0001));
    }

    #[test]
    fn the_card_of_a_link_says_what_the_page_is_and_keeps_what_is_locked() {
        let space = Space::new("Work", "blue", 1.0);
        let mut pages: HashMap<Id, Page> = HashMap::new();
        let mut add = |id: &str, title: &str, parent: Option<&str>| {
            let mut p = Page::new(parent.map(str::to_string), 1.0);
            p.id = id.into();
            p.title = title.into();
            p.space_id = space.id.clone();
            pages.insert(id.to_string(), p);
        };
        add("plan", "Plan", None);
        add("trip", "Trip <to> Rome & back", Some("plan"));
        add("days", "Day by day", Some("trip"));
        add("bin", "Old", None);
        add("safe", "Diary", None);
        add("bare", "", None);
        pages.get_mut("bin").unwrap().trashed = true;
        pages.get_mut("trip").unwrap().icon = Some("plane:blue".into());
        let mut picture = Block::new(BlockKind::Image, "");
        picture.data = "a.png".into();
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        blocks.insert(
            "trip".into(),
            vec![
                Block::text(""),
                Block::new(BlockKind::Heading1, "What to **pack**"),
                picture,
                Block::new(BlockKind::Table, "a b c"),
                Block::new(BlockKind::Divider, ""),
                Block::page_link("days".into()),
                Block::page_link("bin".into()),
                Block::new(BlockKind::Code, "let x = 1;"),
                Block::new(BlockKind::Todo, "Passport"),
                Block::text("Charger"),
                Block::text("The sixth line is not sent"),
            ],
        );
        blocks.insert("safe".into(), vec![Block::new(BlockKind::Locked, "")]);
        blocks.insert("bin".into(), vec![Block::text("gone words")]);
        let spaces = [space];
        let card = |id: &str| page_preview(&pages, &blocks, &spaces, id);
        // The name, the icon, the pages it is inside, and its first lines as plain words.
        let trip = card("trip").unwrap();
        assert_eq!((trip.title.as_str(), trip.icon.as_deref(), trip.place.as_str(), trip.locked), ("Trip <to> Rome & back", Some("plane:blue"), "Plan", false));
        assert_eq!(trip.lines, ["What to pack", "Table", "Day by day", "Passport", "Charger"]);
        assert_eq!(card("days").unwrap().place, "Plan / Trip <to> Rome & back");
        // A page at the top is in its space; one with no name and no lines still has a card.
        assert_eq!(card("plan").unwrap().place, "Work");
        let bare = card("bare").unwrap();
        assert_eq!((bare.title.as_str(), bare.lines.len()), ("Untitled", 0));
        // In the Trash, or gone: no card.
        assert_eq!(card("bin"), None);
        assert_eq!(card("never"), None);
        // Locked: the name, and none of its words.
        let safe = card("safe").unwrap();
        assert!(safe.locked && safe.lines.is_empty() && safe.title == "Diary");
        // A long line is cut, with the sign that there is more.
        blocks.insert("plan".into(), vec![Block::text("word ".repeat(80).trim())]);
        let long = page_preview(&pages, &blocks, &spaces, "plan").unwrap();
        assert!(long.lines.len() == 1 && long.lines[0].ends_with('…') && long.lines[0].chars().count() <= 201, "{:?}", long.lines);
    }

    #[test]
    fn a_page_s_first_words_make_its_snippet() {
        let mut code = Block::new(BlockKind::Code, "let x = 1;");
        code.language = Some("rust".into());
        let blocks = vec![Block::text("  "), Block::new(BlockKind::Heading1, "Plan"), code, Block::text("The **home** page needs a shorter first line."), Block::new(BlockKind::Todo, "Ask Sam")];
        assert_eq!(snippet_of(&blocks, 110), "Plan The home page needs a shorter first line. Ask Sam");
        assert_eq!(snippet_of(&blocks, 30), "Plan The home page needs a…");
        assert_eq!(snippet_of(&[Block::text("déjà vu, encore et toujours, à chaque fois")], 12), "déjà vu…");
        assert_eq!(snippet_of(&[], 50), "");
        assert_eq!(snippet_of(&[Block::text("Supercalifragilisticexpialidocious")], 10), "Supercalif…");
    }

    #[test]
    fn the_graph_joins_pages_by_their_links_and_their_nesting() {
        let named = |title: &str, parent: Option<&Page>, at: i64| {
            let mut p = Page::new(parent.map(|p| p.id.clone()), 1.0);
            p.title = title.into();
            p.space_id = "home".into();
            p.created_at = at;
            p
        };
        let mut plan = named("Plan", None, 1);
        plan.icon = Some("rocket:blue".into());
        let notes = named("Notes", Some(&plan), 2);
        let ideas = named("Ideas", None, 3);
        let alone = named("Alone", None, 4);
        let mut gone = named("Gone", None, 5);
        gone.trashed = true;
        let mut other = named("Other space", None, 6);
        other.space_id = "work".into();
        let link = |p: &Page| inline::link(&p.title, &format!("notion2://page/{}", p.id));
        let mut sub = Block::new(BlockKind::PageLink, "");
        sub.target = Some(notes.id.clone());
        let mut code = Block::new(BlockKind::Code, "");
        code.text = link(&alone);
        let blocks: HashMap<Id, Vec<Block>> = [
            // Plan has its sub-page's line, and code that only looks like a link.
            (plan.id.clone(), vec![sub, code]),
            // Notes links to Ideas two times, and to pages that are not shown.
            (notes.id.clone(), vec![Block::text(format!("See {} and {}", link(&ideas), link(&gone))), Block::text(link(&ideas)), Block::text(link(&other))]),
            // Ideas links back to Notes, and up to Plan.
            (ideas.id.clone(), vec![Block::text(format!("{} {}", link(&notes), link(&plan)))]),
        ]
        .into_iter()
        .collect();
        let pages: HashMap<Id, Page> = [&plan, &notes, &ideas, &alone, &gone, &other].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        let g = page_graph(&pages, &blocks, "home");
        assert_eq!(g.nodes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Plan", "Notes", "Ideas", "Alone"]);
        // The pages of a table's rows are no pages inside their page: a row
        // page shows only when a page links to it, with that link.
        let mut row = named("AAPL", Some(&plan), 7);
        row.row_of = Some("table/r1".into());
        let mut linked_row = named("TSLA", Some(&plan), 8);
        linked_row.row_of = Some("table/r2".into());
        let mut with_rows = pages.clone();
        with_rows.insert(row.id.clone(), row.clone());
        with_rows.insert(linked_row.id.clone(), linked_row.clone());
        let mut blocks_rows = blocks.clone();
        blocks_rows.insert(alone.id.clone(), vec![Block::text(link(&linked_row))]);
        let r = page_graph(&with_rows, &blocks_rows, "home");
        assert_eq!(r.nodes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Plan", "Notes", "Ideas", "Alone", "TSLA"]);
        assert!(r.edges.contains(&(3, 4, false)), "{:?}", r.edges);
        assert!(!r.edges.iter().any(|&(a, b, nested)| nested && (a == 4 || b == 4)), "no nesting line to a row: {:?}", r.edges);
        // Its icon's colour; a page in it takes that colour; a page at the top
        // with none takes one for its group (Plan, Ideas, Alone: the 1st, 2nd, 3rd).
        assert_eq!(g.nodes.iter().map(|n| n.color.as_str()).collect::<Vec<_>>(), ["blue", "blue", "purple", "green"]);
        // Plan–Notes is nesting only; Plan–Ideas and Notes–Ideas are links, each one time.
        assert_eq!(g.edges, vec![(0, 1, true), (0, 2, false), (1, 2, false)]);
        assert_eq!(g.links(), 2);
        assert_eq!(g.joined_to(2), ["Plan", "Notes"]);
        assert!(g.joined_to(3).is_empty());
        let json: serde_json::Value = serde_json::from_str(&g.to_json()).unwrap();
        assert_eq!(json["nodes"][0]["t"], "Plan");
        assert_eq!(json["edges"][0], serde_json::json!([0, 1, 1]));
    }

    /// A locked page that is open has its lines in plain in the window:
    /// the graph shows none of its links, and an export of every page
    /// writes it as a shut one.
    #[test]
    fn a_space_that_the_sender_does_not_know_is_not_gone() {
        let space = |id: &str, position: f64| {
            let mut s = Space::new(id, "blue", position);
            s.id = id.into();
            s
        };
        let (home, work, synced) = (space("home", 1.0), space("work", 2.0), space("synced", 3.0));
        let ids = |list: &[Space]| list.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        // The sender's list is older: it has no "synced", the notes file has it.
        let file = vec![home.clone(), work.clone(), synced.clone()];
        let (list, gone) = spaces_told(&file, vec![home.clone(), work.clone()], Some(file.clone()));
        assert_eq!((ids(&list), gone), (ids(&file), vec![]), "the space stays, and its pages stay in it");
        // A space deleted in the other window is not in the file either: gone.
        let (list, gone) = spaces_told(&file, vec![home.clone(), synced.clone()], Some(vec![home.clone(), synced.clone()]));
        assert_eq!((ids(&list), gone), (vec!["home".to_string(), "synced".to_string()], vec!["work".to_string()]));
        // A new space of the sender comes; a file that cannot be read takes nothing away.
        let (list, gone) = spaces_told(&[home.clone()], vec![home.clone(), work.clone()], Some(vec![home.clone(), work.clone()]));
        assert_eq!((ids(&list), gone), (vec!["home".to_string(), "work".to_string()], vec![]));
        let (list, gone) = spaces_told(&file, vec![home.clone()], None);
        assert_eq!((ids(&list), gone), (ids(&file), vec![]));
    }

    #[test]
    fn an_open_locked_page_shows_no_links_and_is_exported_shut() {
        let named = |title: &str, at: i64| {
            let mut p = Page::new(None, 1.0);
            p.title = title.into();
            p.space_id = "home".into();
            p.created_at = at;
            p
        };
        let (diary, plan) = (named("Diary", 1), named("Plan", 2));
        let link = |p: &Page| inline::link(&p.title, &format!("notion2://page/{}", p.id));
        let mut blocks: HashMap<Id, Vec<Block>> = [(diary.id.clone(), vec![Block::text(format!("The code is 4711, see {}", link(&plan)))]), (plan.id.clone(), vec![Block::text("Plain")])].into_iter().collect();
        let pages: HashMap<Id, Page> = [&diary, &plan].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        assert_eq!(page_graph(&pages, &blocks, "home").edges, vec![(0, 1, false)]);
        crate::lock::hold(&diary.id, crate::lock::test_key("pw-pw-pw"), 0);
        assert!(page_graph(&pages, &blocks, "home").edges.is_empty(), "open and locked: no link of it shows");
        shut_open_pages(&mut blocks, &crate::lock::open_pages());
        crate::lock::forget(&diary.id);
        let out = markdown::from_blocks(&diary.title, &blocks[&diary.id], |_| String::new());
        assert!(out.contains("Locked page") && !out.contains("4711"), "{out}");
        assert_eq!(blocks[&plan.id][0].text, "Plain", "a page with no lock is as it was");
    }

    #[test]
    fn pages_are_found_for_a_link_and_know_what_links_to_them() {
        let named = |title: &str, parent: Option<&Page>, at: i64| {
            let mut p = Page::new(parent.map(|p| p.id.clone()), 1.0);
            p.title = title.into();
            p.space_id = "home".into();
            p.updated_at = at;
            p
        };
        let apollo = named("Apollo", None, 10);
        let notes = named("Meeting notes", Some(&apollo), 30);
        let standup = named("Apollo Standup", Some(&notes), 20);
        let mut gone = named("Apollo old", None, 99);
        gone.trashed = true;
        let mut elsewhere = named("Apollo elsewhere", None, 99);
        elsewhere.space_id = "other".into();
        let untitled = named("", None, 5);
        let pages: HashMap<Id, Page> = [&apollo, &notes, &standup, &gone, &elsewhere, &untitled].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        let titles = |query: &str, here: &Page| pages_to_link(&pages, "home", &here.id, query).into_iter().map(|p| p.display_title().to_string()).collect::<Vec<_>>();
        // Nothing typed: the pages changed last, without this page, the Trash and other spaces.
        assert_eq!(titles("", &untitled), vec!["Meeting notes", "Apollo Standup", "Apollo"]);
        // Every typed word must be in the name; names that begin with it come first.
        assert_eq!(titles("apollo", &untitled), vec!["Apollo Standup", "Apollo"]);
        assert_eq!(titles("apollo", &standup), vec!["Apollo"]);
        assert_eq!(titles("stand apollo", &untitled), vec!["Apollo Standup"]);
        assert_eq!(titles("meet", &untitled), vec!["Meeting notes"]);
        assert_eq!(titles("unt", &apollo), vec!["Untitled"]);
        assert!(titles("nothing like it", &untitled).is_empty());

        // What links to a page.
        let link = |p: &Page| inline::link(p.display_title(), &format!("notion2://page/{}", p.id));
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        blocks.insert(untitled.id.clone(), vec![Block::text(format!("See {} today", link(&standup))), Block::new(BlockKind::Code, format!("notion2://page/{}", apollo.id))]);
        blocks.insert(notes.id.clone(), vec![Block::page_link(standup.id.clone()), Block::text(format!("for {}", link(&apollo)))]);
        blocks.insert(apollo.id.clone(), vec![Block::page_link(standup.id.clone()), Block::text(format!("plain notion2://page/{}", standup.id))]);
        blocks.insert(gone.id.clone(), vec![Block::text(link(&standup))]);
        let from = |p: &Page| linked_from(&pages, &blocks, &p.id).into_iter().map(|p| p.display_title().to_string()).collect::<Vec<_>>();
        // In text, and as a line that is a link (but not the page it is inside,
        // which only lists it); not plain text, not code, not the Trash.
        assert_eq!(from(&standup), vec!["Apollo", "Untitled"]);
        assert_eq!(from(&apollo), vec!["Meeting notes"]);
        assert!(from(&notes).is_empty());

        // A link takes the page's new name, where it still had the old one.
        let url = format!("notion2://page/{}", standup.id);
        let text = format!("See {} and [my own words]({url})", link(&standup));
        let renamed = inline::retitle_links(&text, &url, "Apollo Standup", "Daily standup").unwrap();
        assert_eq!(renamed, format!("See [Daily standup]({url}) and [my own words]({url})"));
        assert_eq!(inline::retitle_links(&renamed, &url, "Apollo Standup", "Again"), None);
        assert_eq!(inline::retitle_links("no link here", &url, "a", "b"), None);
        assert!(inline::links_to(&renamed, &url) && !inline::links_to(&format!("plain {url}"), &url));
    }

    #[test]
    fn a_page_moves_into_beside_and_out_of_other_pages() {
        let named = |title: &str, parent: Option<&Page>, position: f64| {
            let mut p = Page::new(parent.map(|p| p.id.clone()), position);
            p.title = title.into();
            p.space_id = "home".into();
            p
        };
        let apollo = named("Apollo", None, 1.0);
        let notes = named("Meeting notes", None, 2.0);
        let standup = named("Standup", Some(&notes), 1.0);
        let other = named("Other", None, 3.0);
        let inside = named("Inside Apollo", Some(&apollo), 1.0);
        let mut pages: HashMap<Id, Page> = [&apollo, &notes, &standup, &other, &inside].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        let apply = |pages: &mut HashMap<Id, Page>, id: &str, plan: Option<(Option<Id>, f64, String)>| {
            let (parent, position, space) = plan.expect("the move makes sense");
            let p = pages.get_mut(id).unwrap();
            p.parent_id = parent;
            p.position = position;
            p.space_id = space;
        };
        let names = |pages: &HashMap<Id, Page>, parent: Option<&str>| -> Vec<String> {
            match parent {
                Some(p) => children_of(pages, Some(p)),
                None => roots_in(pages, "home"),
            }
            .into_iter()
            .map(|p| p.title)
            .collect()
        };
        // Onto a page: it goes inside, after the pages there, and takes what is inside it along.
        let plan = plan_page_move(&pages, "home", &notes.id, Some(&apollo.id), "into");
        apply(&mut pages, &notes.id, plan);
        assert_eq!(names(&pages, None), vec!["Apollo", "Other"]);
        assert_eq!(names(&pages, Some(&apollo.id)), vec!["Inside Apollo", "Meeting notes"]);
        assert_eq!(names(&pages, Some(&notes.id)), vec!["Standup"]);
        // Not onto itself, nor into or beside a page that is inside it.
        assert_eq!(plan_page_move(&pages, "home", &apollo.id, Some(&apollo.id), "into"), None);
        assert_eq!(plan_page_move(&pages, "home", &apollo.id, Some(&standup.id), "into"), None);
        assert_eq!(plan_page_move(&pages, "home", &apollo.id, Some(&notes.id), "before"), None);
        // Where it already is: nothing to do.
        assert_eq!(plan_page_move(&pages, "home", &notes.id, Some(&apollo.id), "into"), None);
        assert_eq!(plan_page_move(&pages, "home", &notes.id, Some(&inside.id), "after"), None);
        assert_eq!(plan_page_move(&pages, "home", &other.id, None, "root"), None);
        // Between pages: before and after.
        let plan = plan_page_move(&pages, "home", &notes.id, Some(&inside.id), "before");
        apply(&mut pages, &notes.id, plan);
        assert_eq!(names(&pages, Some(&apollo.id)), vec!["Meeting notes", "Inside Apollo"]);
        let plan = plan_page_move(&pages, "home", &other.id, Some(&apollo.id), "before");
        apply(&mut pages, &other.id, plan);
        assert_eq!(names(&pages, None), vec!["Other", "Apollo"]);
        let plan = plan_page_move(&pages, "home", &other.id, Some(&apollo.id), "after");
        apply(&mut pages, &other.id, plan);
        assert_eq!(names(&pages, None), vec!["Apollo", "Other"]);
        // Out again, to the end of the top level.
        let plan = plan_page_move(&pages, "home", &notes.id, None, "root");
        apply(&mut pages, &notes.id, plan);
        assert_eq!(names(&pages, None), vec!["Apollo", "Other", "Meeting notes"]);
        // Beside a page of another level it takes that page's parent.
        let plan = plan_page_move(&pages, "home", &other.id, Some(&inside.id), "after");
        apply(&mut pages, &other.id, plan);
        assert_eq!(names(&pages, Some(&apollo.id)), vec!["Inside Apollo", "Other"]);
        assert_eq!(plan_page_move(&pages, "home", &notes.id, Some("gone"), "into"), None);
        assert_eq!(plan_page_move(&pages, "home", &notes.id, Some(&apollo.id), "sideways"), None);
    }

    #[test]
    fn there_is_one_home_tab_at_most() {
        let page = |id: &str| Tab::new(Route::Page(id.into()));
        let mut on_home_later = page("c");
        on_home_later.panes[0].push(Route::Home);
        let mut tabs = vec![page("a"), Tab::new(Route::Home), page("b"), Tab::new(Route::Home), on_home_later, Tab::new(Route::Home)];
        let ids: Vec<Id> = tabs.iter().map(|t| t.id.clone()).collect();
        // The Home tab in front stays; the others go, and the front stays in front.
        let mut active = 3;
        assert!(one_home_tab(&mut tabs, &mut active));
        assert_eq!(tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), vec![ids[0].clone(), ids[2].clone(), ids[3].clone()]);
        assert_eq!(active, 2);
        assert!(!one_home_tab(&mut tabs, &mut active), "once is enough");
        // With a page in front: the first Home tab stays, and the page stays in front.
        let mut tabs = vec![Tab::new(Route::Home), page("a"), Tab::new(Route::Home)];
        let front = tabs[1].id.clone();
        let mut active = 1;
        assert!(one_home_tab(&mut tabs, &mut active));
        assert_eq!((tabs.len(), tabs[active].id.clone()), (2, front));
        assert!(is_home_tab(&tabs[0]));
        // Two graph tabs and two calendar tabs: one of each stays.
        let mut tabs = vec![Tab::new(Route::Graph), page("a"), Tab::new(Route::Graph), Tab::new(Route::Calendar), Tab::new(Route::Calendar)];
        let mut active = 1;
        assert!(one_home_tab(&mut tabs, &mut active));
        assert_eq!(tabs.iter().map(|t| t.panes[0].route().clone()).collect::<Vec<_>>(), vec![Route::Graph, Route::Page("a".into()), Route::Calendar]);
        assert_eq!(active, 1);
        // A tab with two panes, one of them on Home, is no Home tab.
        let mut two = Tab::new(Route::Home);
        two.panes.push(Pane::new(Route::Page("x".into())));
        assert!(!is_home_tab(&two));
    }

    #[test]
    fn tab_history_moves_back_and_forward() {
        let mut tab = Tab::new(Route::Home);
        tab.push(Route::Page("a".into()));
        tab.push(Route::Page("b".into()));
        tab.push(Route::Page("b".into()));
        assert_eq!(tab.focused().history.len(), 3);
        tab.focused_mut().cursor = 1;
        tab.push(Route::Page("c".into()));
        assert_eq!(tab.focused().history, vec![Route::Home, Route::Page("a".into()), Route::Page("c".into())]);
        assert!(tab.can_back() && !tab.can_forward());
    }

    #[test]
    fn tabs_hold_any_number_of_pages() {
        let mut tab = Tab::new(Route::Home);
        let first = tab.focus.clone();
        for i in 0..5 {
            let edge = if i % 2 == 0 { Edge::Right } else { Edge::Bottom };
            tab.add_pane(Pane::new(Route::Page(format!("p{i}"))), None, edge);
        }
        assert_eq!(tab.panes.len(), 6);
        assert_eq!(tab.route(), &Route::Page("p4".into()), "the newest pane has focus");
        assert!(tab.layout.fits(&tab.panes.iter().map(|p| p.id.clone()).collect::<Vec<_>>()));
        let p2 = tab.pane_showing(&Route::Page("p2".into())).unwrap();
        assert!(tab.take_pane(&p2).is_some());
        assert_eq!(tab.panes.len(), 5);
        let parts = tab.clone().split_apart();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].id, tab.id, "the first part keeps the tab");
        assert_eq!(parts[0].route(), tab.pane(&first).unwrap().route());
        // The last page can't be taken out.
        let mut one = Tab::new(Route::Home);
        let id = one.focus.clone();
        assert!(one.take_pane(&id).is_none());
    }

    #[test]
    fn old_saved_tabs_still_load() {
        let old = r#"[{"id":"t1","history":["Home",{"Page":"a"}],"cursor":1,"split":{"Page":"b"}}]"#;
        let tabs: Vec<Tab> = serde_json::from_str(old).unwrap();
        let t = &tabs[0];
        assert_eq!(t.panes.len(), 2);
        assert_eq!(t.route(), &Route::Page("a".into()));
        assert!(t.can_back());
        assert_eq!(t.ordered()[1].route(), &Route::Page("b".into()));
        // And the new form round-trips.
        let json = serde_json::to_string(&tabs).unwrap();
        let back: Vec<Tab> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tabs);
    }

    #[test]
    fn slash_filter_matches_labels_and_keywords() {
        assert_eq!(filter_slash_for("", true).len(), slash_items().len());
        assert_eq!(filter_slash_for("h2", true)[0].action, SlashAction::Turn(BlockKind::Heading2));
        assert!(filter_slash_for("todo", true).iter().any(|i| i.action == SlashAction::Turn(BlockKind::Todo)));
        let ai_rows = filter_slash_for("ai", true);
        assert!(!ai_rows.is_empty() && ai_rows.iter().all(|i| matches!(i.action, SlashAction::Ai(_))));
        assert!(filter_slash_for("zzz", true).is_empty());
        // With no AI tool connected, the menu has no AI rows, and the other rows are all there.
        let plain = filter_slash_for("", false);
        assert!(plain.iter().all(|i| !matches!(i.action, SlashAction::Ai(_))) && plain.len() == slash_items().len() - ai_rows.len().max(7));
        assert!(filter_slash_for("ai", false).is_empty());
    }

    #[test]
    fn page_tree_helpers() {
        let mut pages = HashMap::new();
        let a = Page::new(None, 2.0);
        let mut b = Page::new(None, 1.0);
        b.title = "B".into();
        let c = Page::new(Some(a.id.clone()), 1.0);
        let d = Page::new(Some(c.id.clone()), 1.0);
        for p in [&a, &b, &c, &d] {
            pages.insert(p.id.clone(), p.clone());
        }
        let roots: Vec<Id> = children_of(&pages, None).into_iter().map(|p| p.id).collect();
        assert_eq!(roots, vec![b.id, a.id.clone()]);
        let mut desc = descendants(&pages, &a.id);
        desc.sort();
        let mut expected = vec![a.id.clone(), c.id.clone(), d.id.clone()];
        expected.sort();
        assert_eq!(desc, expected);
        let anc: Vec<Id> = ancestors(&pages, &d.id).into_iter().map(|p| p.id).collect();
        assert_eq!(anc, vec![a.id, c.id]);
    }
}
