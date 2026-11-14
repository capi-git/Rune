//! How the pages in one tab are arranged (as in Aven): any number of panes,
//! side by side and on top of each other, as a tree of rows and columns.
//!
//! Dropping something on a pane's edge puts it there: beside its neighbours
//! when the pane's group already runs that way (the pane gives up half its
//! space), otherwise in a new row or column nested where the pane was.
//! Panes in a row or column share their space by `sizes` (fractions adding up
//! to 1); dragging the line between two of them moves that boundary.

use crate::model::{new_id, Id};
use serde::{Deserialize, Serialize};

/// Which way a group runs: side by side, or one above the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    Row,
    Column,
}

/// The edge of a pane something is dropped on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub fn parse(s: &str) -> Option<Edge> {
        match s {
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            "top" => Some(Edge::Top),
            "bottom" => Some(Edge::Bottom),
            _ => None,
        }
    }

    fn dir(self) -> Dir {
        match self {
            Edge::Left | Edge::Right => Dir::Row,
            Edge::Top | Edge::Bottom => Dir::Column,
        }
    }

    /// Whether the newcomer goes after (right of / below) the pane.
    fn after(self) -> bool {
        matches!(self, Edge::Right | Edge::Bottom)
    }
}

/// No pane is squeezed below this share of its group.
pub const MIN_SIZE: f64 = 0.08;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Layout {
    Pane { id: Id },
    Split { id: Id, dir: Dir, children: Vec<Layout>, sizes: Vec<f64> },
}

impl Layout {
    pub fn pane(id: &str) -> Layout {
        Layout::Pane { id: id.to_string() }
    }

    /// Panes in a row, sharing the space equally (one pane needs no row).
    pub fn row(ids: &[Id]) -> Layout {
        match ids {
            [one] => Layout::pane(one),
            _ => Layout::Split {
                id: new_id(),
                dir: Dir::Row,
                children: ids.iter().map(|id| Layout::pane(id)).collect(),
                sizes: vec![1.0 / ids.len() as f64; ids.len()],
            },
        }
    }

    /// A key for drawing this node (stays the same while it exists).
    pub fn key(&self) -> &str {
        match self {
            Layout::Pane { id } | Layout::Split { id, .. } => id,
        }
    }

    fn is_pane(&self, id: &str) -> bool {
        matches!(self, Layout::Pane { id: p } if p == id)
    }

    /// Pane ids in reading order (left to right, top to bottom).
    pub fn panes(&self) -> Vec<Id> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<Id>) {
        match self {
            Layout::Pane { id } => out.push(id.clone()),
            Layout::Split { children, .. } => children.iter().for_each(|c| c.collect(out)),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        match self {
            Layout::Pane { id: p } => p == id,
            Layout::Split { children, .. } => children.iter().any(|c| c.contains(id)),
        }
    }

    /// Put `incoming` (a pane, or a whole arrangement) on `edge` of pane `target`.
    pub fn place(&mut self, target: &str, incoming: Layout, edge: Edge) -> bool {
        if !self.contains(target) {
            return false;
        }
        let mut incoming = Some(incoming);
        let old = std::mem::replace(self, Layout::pane(""));
        *self = insert(old, target, &mut incoming, edge.dir(), edge.after());
        self.tidy();
        incoming.is_none()
    }

    /// Put `incoming` along one side of everything (e.g. a tab dropped on the
    /// middle of another tab joins it on the right).
    pub fn append(&mut self, incoming: Layout, edge: Edge) {
        let old = std::mem::replace(self, Layout::pane(""));
        let children = if edge.after() { vec![old, incoming] } else { vec![incoming, old] };
        *self = Layout::Split { id: new_id(), dir: edge.dir(), children, sizes: vec![0.5, 0.5] };
        self.tidy();
    }

    /// Take pane `id` out; a group left with one member is replaced by it.
    /// Fails for the last pane (a tab always shows something).
    pub fn remove(&mut self, id: &str) -> bool {
        if !self.contains(id) || self.is_pane(id) {
            return false;
        }
        let old = std::mem::replace(self, Layout::pane(""));
        *self = removed(old, id).expect("other panes remain");
        self.tidy();
        true
    }

    /// Move pane `from` onto `edge` of pane `to`. Panes in the same group
    /// along that direction just change places.
    pub fn move_pane(&mut self, from: &str, to: &str, edge: Edge) -> bool {
        if from == to || !self.contains(from) || !self.contains(to) {
            return false;
        }
        if reorder(self, from, to, edge) {
            return true;
        }
        if !self.remove(from) {
            return false;
        }
        self.place(to, Layout::pane(from), edge)
    }

    /// New shares for the members of group `split`, none of them folded.
    #[cfg(test)]
    pub fn set_sizes(&mut self, split: &str, sizes: &[f64]) -> bool {
        self.set_open_sizes(split, sizes, &[])
    }

    /// The panes that show as slim rails: those in `folded`, except that
    /// every row or column keeps a part open (when all its parts are folded
    /// panes, the largest one shows open).
    pub fn rails(&self, folded: &[Id]) -> Vec<Id> {
        let mut out = Vec::new();
        self.collect_rails(folded, &mut out);
        out
    }

    fn collect_rails(&self, folded: &[Id], out: &mut Vec<Id>) {
        let Layout::Split { children, sizes, .. } = self else { return };
        let mut here: Vec<&Id> = children
            .iter()
            .filter_map(|c| match c {
                Layout::Pane { id } if folded.contains(id) => Some(id),
                _ => None,
            })
            .collect();
        if here.len() == children.len() {
            let largest = children.iter().zip(sizes).max_by(|a, b| a.1.total_cmp(b.1)).map(|(c, _)| c.key());
            here.retain(|id| Some(id.as_str()) != largest);
        }
        out.extend(here.into_iter().cloned());
        children.iter().for_each(|c| c.collect_rails(folded, out));
    }

    /// Whether `pane` can fold to a rail: its row or column has another part
    /// that stays open. (A tab's only pane cannot.)
    pub fn can_fold(&self, pane: &str, folded: &[Id]) -> bool {
        match self {
            Layout::Pane { .. } => false,
            Layout::Split { children, .. } if children.iter().any(|c| c.is_pane(pane)) => {
                children.iter().any(|c| !c.is_pane(pane) && !matches!(c, Layout::Pane { id } if folded.contains(id)))
            }
            Layout::Split { children, .. } => children.iter().any(|c| c.can_fold(pane, folded)),
        }
    }

    /// Set a split's shares from what is on screen, where `rails` (folded
    /// panes) take no share: the open parts divide what they had between
    /// them in the new proportions, and each rail keeps the share it had, to
    /// open to later. `sizes` has one entry per part; a rail's is ignored.
    pub fn set_open_sizes(&mut self, split: &str, sizes: &[f64], rails: &[Id]) -> bool {
        match self {
            Layout::Pane { .. } => false,
            Layout::Split { id, sizes: own, children, .. } if id == split => {
                if sizes.len() != own.len() {
                    return false;
                }
                let open: Vec<bool> = children.iter().map(|c| !matches!(c, Layout::Pane { id } if rails.contains(id))).collect();
                if sizes.iter().zip(&open).any(|(s, o)| *o && (!s.is_finite() || *s <= 0.0)) {
                    return false;
                }
                let had: f64 = own.iter().zip(&open).filter(|(_, o)| **o).map(|(s, _)| *s).sum();
                let shown: f64 = sizes.iter().zip(&open).filter(|(_, o)| **o).map(|(s, _)| *s).sum();
                if !(shown > 0.0 && had > 0.0) {
                    return false;
                }
                let next: Vec<f64> =
                    own.iter().zip(sizes).zip(&open).map(|((old, new), o)| if *o { new / shown * had } else { *old }).collect();
                *own = clamp_sizes(&next);
                true
            }
            Layout::Split { children, .. } => children.iter_mut().any(|c| c.set_open_sizes(split, sizes, rails)),
        }
    }

    /// The pane that takes focus when `id` closes: its neighbour.
    pub fn neighbour(&self, id: &str) -> Option<Id> {
        let order = self.panes();
        let i = order.iter().position(|p| p == id)?;
        order.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| order.get(j))).cloned()
    }

    /// Keep the tree simple: groups of one are lifted, a group inside a group
    /// running the same way is merged into it, and sizes add up to 1.
    fn tidy(&mut self) {
        if let Layout::Split { dir, children, sizes, .. } = self {
            for c in children.iter_mut() {
                c.tidy();
            }
            let mut flat_children = Vec::new();
            let mut flat_sizes = Vec::new();
            for (child, size) in std::mem::take(children).into_iter().zip(std::mem::take(sizes)) {
                match child {
                    Layout::Split { dir: d, children: inner, sizes: inner_sizes, .. } if d == *dir => {
                        for (c, s) in inner.into_iter().zip(inner_sizes) {
                            flat_children.push(c);
                            flat_sizes.push(size * s);
                        }
                    }
                    other => {
                        flat_children.push(other);
                        flat_sizes.push(size);
                    }
                }
            }
            *children = flat_children;
            *sizes = clamp_sizes(&flat_sizes);
            if children.len() == 1 {
                let only = children.pop().expect("one child");
                *self = only;
            }
        }
    }

    /// A saved layout is used only if it holds exactly these panes.
    pub fn fits(&self, ids: &[Id]) -> bool {
        let mut mine = self.panes();
        let mut want = ids.to_vec();
        mine.sort();
        want.sort();
        mine == want && self.well_formed()
    }

    /// The same arrangement, its panes named anew by `name`, each row and
    /// column with a new id: the layout of a copy of a tab.
    pub fn renamed(&self, name: &dyn Fn(&str) -> Id) -> Layout {
        match self {
            Layout::Pane { id } => Layout::Pane { id: name(id) },
            Layout::Split { dir, children, sizes, .. } => {
                Layout::Split { id: new_id(), dir: *dir, children: children.iter().map(|c| c.renamed(name)).collect(), sizes: sizes.clone() }
            }
        }
    }

    fn well_formed(&self) -> bool {
        match self {
            Layout::Pane { .. } => true,
            Layout::Split { children, sizes, .. } => {
                children.len() >= 2 && children.len() == sizes.len() && children.iter().all(Layout::well_formed)
            }
        }
    }
}

fn insert(node: Layout, target: &str, incoming: &mut Option<Layout>, dir: Dir, after: bool) -> Layout {
    if node.is_pane(target) {
        let Some(new) = incoming.take() else { return node };
        let children = if after { vec![node, new] } else { vec![new, node] };
        return Layout::Split { id: new_id(), dir, children, sizes: vec![0.5, 0.5] };
    }
    match node {
        Layout::Pane { .. } => node,
        Layout::Split { id, dir: d, mut children, mut sizes } => {
            if d == dir {
                if let Some(i) = children.iter().position(|c| c.is_pane(target)) {
                    if let Some(new) = incoming.take() {
                        // The pane gives up half of its space.
                        let half = sizes[i] / 2.0;
                        sizes[i] = half;
                        let at = if after { i + 1 } else { i };
                        children.insert(at, new);
                        sizes.insert(at, half);
                        return Layout::Split { id, dir: d, children, sizes };
                    }
                }
            }
            let children = children.into_iter().map(|c| insert(c, target, incoming, dir, after)).collect();
            Layout::Split { id, dir: d, children, sizes }
        }
    }
}

fn removed(node: Layout, target: &str) -> Option<Layout> {
    match node {
        Layout::Pane { ref id } if id == target => None,
        Layout::Pane { .. } => Some(node),
        Layout::Split { id, dir, children, sizes } => {
            let (kept, kept_sizes): (Vec<Layout>, Vec<f64>) =
                children.into_iter().zip(sizes).filter_map(|(c, s)| removed(c, target).map(|c| (c, s))).unzip();
            match kept.len() {
                0 => None,
                1 => kept.into_iter().next(),
                _ => Some(Layout::Split { id, dir, children: kept, sizes: kept_sizes }),
            }
        }
    }
}

/// Swap places inside one group when both panes are in it and it runs the
/// way of the edge. Returns false when this doesn't apply.
fn reorder(node: &mut Layout, from: &str, to: &str, edge: Edge) -> bool {
    let Layout::Split { dir, children, sizes, .. } = node else { return false };
    let fi = children.iter().position(|c| c.is_pane(from));
    let ti = children.iter().position(|c| c.is_pane(to));
    if let (Some(fi), Some(ti)) = (fi, ti) {
        if *dir != edge.dir() {
            return false;
        }
        let mut at = if edge.after() { ti + 1 } else { ti };
        if fi < at {
            at -= 1;
        }
        let child = children.remove(fi);
        let size = sizes.remove(fi);
        children.insert(at, child);
        sizes.insert(at, size);
        return true;
    }
    children.iter_mut().any(|c| reorder(c, from, to, edge))
}

/// Shares that add up to 1, none below the minimum.
fn clamp_sizes(sizes: &[f64]) -> Vec<f64> {
    let n = sizes.len().max(1) as f64;
    let total: f64 = sizes.iter().filter(|s| s.is_finite() && **s > 0.0).sum();
    let mut out: Vec<f64> =
        sizes.iter().map(|s| if total > 0.0 && s.is_finite() && *s > 0.0 { s / total } else { 1.0 / n }).collect();
    // Raise any share below the minimum, taking the difference from the
    // others in proportion, so the total stays 1.
    let min = MIN_SIZE.min(1.0 / n);
    let short: f64 = out.iter().filter(|s| **s < min).map(|s| min - s).sum();
    if short > 0.0 {
        let rest: f64 = out.iter().filter(|s| **s >= min).sum();
        for s in out.iter_mut() {
            *s = if *s < min { min } else { *s - short * (*s / rest) };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_fold_to_rails_but_one_part_stays_open() {
        let ids: Vec<Id> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut row = Layout::row(&ids);
        assert!(!Layout::pane("a").can_fold("a", &[]), "a tab's only pane cannot fold");
        assert!(row.can_fold("a", &[]));
        assert!(row.can_fold("a", &["b".to_string()]));
        assert!(!row.can_fold("a", &["b".to_string(), "c".to_string()]), "the last open pane stays");
        assert_eq!(row.rails(&["b".to_string(), "x".to_string()]), vec!["b".to_string()]);
        // All folded (after another pane closed, say): the largest shows open.
        let split = row.key().to_string();
        assert!(row.set_sizes(&split, &[0.2, 0.5, 0.3]));
        let all: Vec<Id> = ids.clone();
        assert_eq!(row.rails(&all), vec!["a".to_string(), "c".to_string()]);

        // Sizes set while "a" is a rail: it keeps its share, the others
        // divide theirs anew.
        let rails = vec!["a".to_string()];
        assert!(row.set_open_sizes(&split, &[0.0, 0.25, 0.75], &rails));
        let Layout::Split { sizes, .. } = &row else { panic!() };
        assert!((sizes[0] - 0.2).abs() < 1e-9 && (sizes[1] - 0.2).abs() < 1e-9 && (sizes[2] - 0.6).abs() < 1e-9, "{sizes:?}");
        // With no rails it is the same as setting the sizes.
        assert!(row.set_open_sizes(&split, &[1.0, 1.0, 2.0], &[]));
        let Layout::Split { sizes, .. } = &row else { panic!() };
        assert!((sizes[2] - 0.5).abs() < 1e-9, "{sizes:?}");
        // An open part with no size is refused.
        assert!(!row.set_open_sizes(&split, &[0.0, 0.5, 0.5], &[]));
    }

    fn ids(l: &Layout) -> String {
        l.panes().join(",")
    }

    /// The shape, for checking: a(…) rows, v(…) columns.
    fn shape(l: &Layout) -> String {
        match l {
            Layout::Pane { id } => id.clone(),
            Layout::Split { dir, children, .. } => {
                let inner: Vec<String> = children.iter().map(shape).collect();
                format!("{}({})", if *dir == Dir::Row { "r" } else { "c" }, inner.join(" "))
            }
        }
    }

    fn sizes_of(l: &Layout) -> Vec<f64> {
        match l {
            Layout::Split { sizes, .. } => sizes.clone(),
            Layout::Pane { .. } => vec![1.0],
        }
    }

    #[test]
    fn edges_put_panes_on_every_side() {
        let mut l = Layout::pane("a");
        assert!(l.place("a", Layout::pane("b"), Edge::Right));
        assert_eq!(shape(&l), "r(a b)");
        assert!(l.place("b", Layout::pane("c"), Edge::Bottom));
        assert_eq!(shape(&l), "r(a c(b c))");
        assert!(l.place("a", Layout::pane("d"), Edge::Top));
        assert_eq!(shape(&l), "r(c(d a) c(b c))");
        assert!(l.place("a", Layout::pane("e"), Edge::Left));
        assert_eq!(shape(&l), "r(c(d r(e a)) c(b c))");
        assert_eq!(ids(&l), "d,e,a,b,c");
    }

    #[test]
    fn as_many_panes_as_you_like() {
        let mut l = Layout::pane("p0");
        for i in 1..12 {
            let last = format!("p{}", i - 1);
            let edge = if i % 2 == 0 { Edge::Right } else { Edge::Bottom };
            assert!(l.place(&last, Layout::pane(&format!("p{i}")), edge));
        }
        assert_eq!(l.panes().len(), 12);
        assert!(l.well_formed());
    }

    #[test]
    fn same_direction_joins_the_group_and_halves_the_pane() {
        let mut l = Layout::row(&["a".into(), "b".into()]);
        l.place("b", Layout::pane("c"), Edge::Right);
        assert_eq!(shape(&l), "r(a b c)");
        let s = sizes_of(&l);
        assert!((s[0] - 0.5).abs() < 1e-9 && (s[1] - 0.25).abs() < 1e-9 && (s[2] - 0.25).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn removing_lifts_and_merges() {
        let mut l = Layout::pane("a");
        l.place("a", Layout::pane("b"), Edge::Right);
        l.place("b", Layout::pane("c"), Edge::Bottom);
        assert!(l.remove("c"));
        assert_eq!(shape(&l), "r(a b)");
        assert!(l.remove("a"));
        assert_eq!(shape(&l), "b");
        assert!(!l.remove("b"), "the last pane stays");
        // A column left inside a row merges into the row when it runs the same way.
        let mut l = Layout::pane("a");
        l.place("a", Layout::pane("b"), Edge::Right);
        l.place("b", Layout::pane("c"), Edge::Bottom);
        l.place("c", Layout::pane("d"), Edge::Right);
        assert_eq!(shape(&l), "r(a c(b r(c d)))");
        l.remove("b");
        assert_eq!(shape(&l), "r(a c d)");
        let total: f64 = sizes_of(&l).iter().sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn moving_reorders_or_nests() {
        let mut l = Layout::row(&["a".into(), "b".into(), "c".into()]);
        assert!(l.move_pane("a", "c", Edge::Right));
        assert_eq!(shape(&l), "r(b c a)");
        assert!(l.move_pane("a", "b", Edge::Left));
        assert_eq!(shape(&l), "r(a b c)");
        assert!(l.move_pane("c", "a", Edge::Bottom));
        assert_eq!(shape(&l), "r(c(a c) b)");
        assert!(!l.move_pane("a", "a", Edge::Top));
        assert!(!l.move_pane("x", "a", Edge::Top));
    }

    #[test]
    fn whole_arrangements_can_be_placed_and_appended() {
        let mut l = Layout::pane("a");
        let other = Layout::row(&["b".into(), "c".into()]);
        l.place("a", other, Edge::Bottom);
        assert_eq!(shape(&l), "c(a r(b c))");
        l.append(Layout::pane("d"), Edge::Right);
        assert_eq!(shape(&l), "r(c(a r(b c)) d)");
    }

    #[test]
    fn sizes_are_kept_sensible() {
        let mut l = Layout::row(&["a".into(), "b".into()]);
        let Layout::Split { id, .. } = &l else { panic!() };
        let id = id.clone();
        assert!(l.set_sizes(&id, &[0.99, 0.01]));
        let s = sizes_of(&l);
        assert!(s[1] >= MIN_SIZE - 1e-9 && ((s[0] + s[1]) - 1.0).abs() < 1e-9, "{s:?}");
        assert!(!l.set_sizes(&id, &[1.0]));
        assert!(!l.set_sizes("nope", &[0.5, 0.5]));
    }

    #[test]
    fn saved_layouts_must_match_their_panes() {
        let l = Layout::row(&["a".into(), "b".into()]);
        assert!(l.fits(&["b".into(), "a".into()]));
        assert!(!l.fits(&["a".into()]));
        let json = serde_json::to_string(&l).unwrap();
        let back: Layout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn closing_focuses_a_neighbour() {
        let l = Layout::row(&["a".into(), "b".into(), "c".into()]);
        assert_eq!(l.neighbour("b").as_deref(), Some("c"));
        assert_eq!(l.neighbour("c").as_deref(), Some("b"));
    }
}
