//! Lines that stand side by side: two or three columns on a page.
//!
//! A page's lines stay one list. A line in a column has the number of that
//! column in its place (`Place::col`, 1 to `MOST`; 0 for a line as wide as
//! the page). The lines of one set of columns follow each other in the list:
//! those of column 1, then those of column 2, then those of column 3. So a
//! set is a run of lines whose column numbers never go down (a smaller
//! number starts the next set), and a run with one column only is no set.
//!
//! Kept that way, a reader that does not know columns (an older Rune, the
//! iPhone app, Markdown for an AI tool) has every line in a sensible order:
//! the first column, then the next.

use crate::model::{index_of, section_end, subtree_end, Block};
use std::ops::Range;

/// The most columns a set can have.
pub const MOST: u8 = 3;

fn col(b: &Block) -> u8 {
    b.place.col
}

/// The runs of lines with column numbers that never go down.
fn runs(blocks: &[Block]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < blocks.len() {
        if col(&blocks[i]) == 0 {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < blocks.len() && col(&blocks[i]) >= col(&blocks[i - 1]).max(1) {
            i += 1;
        }
        out.push(start..i);
    }
    out
}

/// The column numbers of a run, in order, each once.
fn numbers(blocks: &[Block], run: &Range<usize>) -> Vec<u8> {
    let mut out: Vec<u8> = blocks[run.clone()].iter().map(col).collect();
    out.dedup();
    out
}

/// The sets of columns on a page, as ranges of its lines.
pub fn sets(blocks: &[Block]) -> Vec<Range<usize>> {
    runs(blocks).into_iter().filter(|r| numbers(blocks, r).len() > 1).collect()
}

/// For each line: the set it stands in (its number among the page's sets)
/// and its column there (from 0), or none for a line as wide as the page.
/// A set with more than `MOST` columns (from another program) shows the
/// rest in its last one.
pub fn layout(blocks: &[Block]) -> Vec<Option<(usize, usize)>> {
    let mut out = vec![None; blocks.len()];
    for (n, set) in sets(blocks).into_iter().enumerate() {
        let cols = numbers(blocks, &set);
        for i in set {
            let c = cols.iter().position(|&c| c == col(&blocks[i])).unwrap_or(0);
            out[i] = Some((n, c.min(MOST as usize - 1)));
        }
    }
    out
}

/// The set that the line at `i` stands in.
pub fn set_of(blocks: &[Block], i: usize) -> Option<Range<usize>> {
    sets(blocks).into_iter().find(|s| s.contains(&i))
}

/// The arrow keys: from the line at `from`, the caret would go to the line
/// at `to`. When that is another column of the same set, it goes out of
/// the set instead (down: under it; up: above it): the set is passed over.
pub fn across(blocks: &[Block], from: usize, to: usize) -> Option<Range<usize>> {
    let set = set_of(blocks, from)?;
    (set.contains(&to) && col(&blocks[to]) != col(&blocks[from])).then_some(set)
}

/// After an edit: the columns as they must be. A plain line that came in
/// among the lines of a set (from an AI tool, a paste) joins the column
/// before it; the columns are numbered 1, 2, 3 again; a set left with one
/// column is plain lines again.
pub fn tidy(blocks: &mut [Block]) {
    // A run of plain lines inside a set.
    let mut i = 1;
    while i < blocks.len() {
        if col(&blocks[i]) == 0 && col(&blocks[i - 1]) > 0 {
            let start = i;
            while i < blocks.len() && col(&blocks[i]) == 0 {
                i += 1;
            }
            let before = col(&blocks[start - 1]);
            if i < blocks.len() && col(&blocks[i]) >= before {
                for b in &mut blocks[start..i] {
                    b.place.col = before;
                }
            }
        }
        i += 1;
    }
    for run in runs(blocks) {
        let cols = numbers(blocks, &run);
        for b in &mut blocks[run] {
            b.place.col = match cols.len() {
                1 => 0,
                _ => (cols.iter().position(|&c| c == b.place.col).unwrap_or(0) as u8 + 1).min(MOST),
            };
        }
    }
}

/// A line (with what is nested in it, or with the lines a folded heading
/// hides: `section`) dragged to the left or the right side of the line
/// `to`. Beside a line of the top level, the two make a set of two
/// columns; beside a line in a set, the dragged one makes a new column
/// there, at that side of that line's column. `to_section`: the line `to`
/// is a folded heading, whose lines go into its column with it. Returns
/// whether anything changed.
pub fn drop_beside(blocks: &mut Vec<Block>, from_id: &str, to_id: &str, right: bool, section: bool, to_section: bool) -> bool {
    let mut work = blocks.clone();
    let Some(from) = index_of(&work, from_id) else { return false };
    let from_end = if section { section_end(&work, from).max(subtree_end(&work, from)) } else { subtree_end(&work, from) };
    if index_of(&work, to_id).is_some_and(|to| (from..from_end).contains(&to)) {
        return false;
    }
    // A heading that holds a set takes it nowhere into another.
    if work[from..from_end].iter().any(|b| col(b) != col(&work[from])) {
        return false;
    }
    let mut group: Vec<Block> = work.drain(from..from_end).collect();
    let base = group[0].indent;
    for b in &mut group {
        b.indent -= base;
    }
    // (What the drag left behind is put right first: a set it took the
    // last line of a column from has one column less, or none.)
    tidy(&mut work);
    let Some(to) = index_of(&work, to_id) else { return false };
    let at = match set_of(&work, to) {
        Some(set) => {
            let cols = numbers(&work, &set);
            if cols.len() >= MOST as usize {
                return false;
            }
            let c = col(&work[to]);
            let new = if right { c + 1 } else { c };
            for b in &mut work[set.clone()] {
                if b.place.col >= new {
                    b.place.col += 1;
                }
            }
            for b in &mut group {
                b.place.col = new;
            }
            // Before the first line of the column it goes before (or after
            // the last line of the set).
            set.clone().find(|&i| col(&work[i]) > new).unwrap_or(set.end)
        }
        None => {
            if work[to].indent > 0 || work[to].floats() {
                return false;
            }
            let end = if to_section { section_end(&work, to).max(subtree_end(&work, to)) } else { subtree_end(&work, to) };
            if work[to..end].iter().any(|b| col(b) > 0) {
                return false;
            }
            let (theirs, mine) = if right { (1, 2) } else { (2, 1) };
            for b in &mut work[to..end] {
                b.place.col = theirs;
            }
            for b in &mut group {
                b.place.col = mine;
            }
            if right {
                end
            } else {
                to
            }
        }
    };
    for (n, b) in group.into_iter().enumerate() {
        work.insert(at + n, b);
    }
    tidy(&mut work);
    if work == *blocks {
        return false;
    }
    *blocks = work;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{move_block, Block, BlockKind};

    fn line(text: &str, col: u8) -> Block {
        let mut b = Block::text(text);
        b.id = text.to_string();
        b.place.col = col;
        b
    }

    fn cols(blocks: &[Block]) -> Vec<(&str, u8)> {
        blocks.iter().map(|b| (b.text.as_str(), b.place.col)).collect()
    }

    #[test]
    fn a_line_dropped_at_the_side_of_another_makes_two_columns() {
        let mut page = vec![line("a", 0), line("b", 0), line("c", 0)];
        assert!(drop_beside(&mut page, "c", "a", true, false, false));
        assert_eq!(cols(&page), vec![("a", 1), ("c", 2), ("b", 0)]);
        assert_eq!(layout(&page), vec![Some((0, 0)), Some((0, 1)), None]);
        // At the left side, the dragged line is the first column.
        let mut page = vec![line("a", 0), line("b", 0), line("c", 0)];
        assert!(drop_beside(&mut page, "c", "a", false, false, false));
        assert_eq!(cols(&page), vec![("c", 1), ("a", 2), ("b", 0)]);
    }

    #[test]
    fn a_third_column_and_no_fourth() {
        let mut page = vec![line("a", 1), line("b", 2), line("x", 0), line("y", 0)];
        // Between the two columns.
        assert!(drop_beside(&mut page, "x", "a", true, false, false));
        assert_eq!(cols(&page), vec![("a", 1), ("x", 2), ("b", 3), ("y", 0)]);
        assert!(!drop_beside(&mut page, "y", "b", true, false, false), "three is the most");
        assert_eq!(cols(&page), vec![("a", 1), ("x", 2), ("b", 3), ("y", 0)]);
        // A line of the set itself can change its column with no new one.
        assert!(drop_beside(&mut page, "a", "b", true, false, false));
        assert_eq!(cols(&page), vec![("x", 1), ("b", 2), ("a", 3), ("y", 0)]);
    }

    #[test]
    fn the_lines_nested_in_a_line_go_into_its_column() {
        let mut page = vec![line("a", 0), line("a1", 0), line("b", 0), line("b1", 0)];
        page[1].indent = 1;
        page[3].indent = 1;
        assert!(drop_beside(&mut page, "b", "a", true, false, false));
        assert_eq!(cols(&page), vec![("a", 1), ("a1", 1), ("b", 2), ("b1", 2)]);
        // Folded headings take their lines along, on both sides.
        let mut page = vec![line("h", 0), line("under", 0), line("g", 0), line("hidden", 0), line("end", 0)];
        page[0].kind = BlockKind::Heading2;
        page[2].kind = BlockKind::Heading2;
        page[4].kind = BlockKind::Heading2;
        assert!(drop_beside(&mut page, "g", "h", true, true, true));
        assert_eq!(cols(&page), vec![("h", 1), ("under", 1), ("g", 2), ("hidden", 2), ("end", 0)]);
    }

    #[test]
    fn not_beside_a_nested_line_nor_beside_itself() {
        let mut page = vec![line("a", 0), line("a1", 0), line("b", 0)];
        page[1].indent = 1;
        let was = page.clone();
        assert!(!drop_beside(&mut page, "b", "a1", true, false, false));
        assert!(!drop_beside(&mut page, "a", "a1", true, false, false), "into its own nested line");
        assert!(!drop_beside(&mut page, "a", "a", false, false, false));
        assert_eq!(page, was);
    }

    #[test]
    fn the_last_line_dragged_out_of_a_column_ends_the_columns() {
        let mut page = vec![line("a", 1), line("a2", 1), line("b", 2), line("z", 0)];
        assert!(move_block(&mut page, "b", "z", true));
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 0), ("a2", 0), ("z", 0), ("b", 0)]);
        // Dragged above or below a line of a column, it is in that column.
        let mut page = vec![line("a", 1), line("b", 2), line("z", 0)];
        assert!(move_block(&mut page, "z", "b", false));
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 1), ("z", 2), ("b", 2)]);
        // Out of a set of three: two are left.
        let mut page = vec![line("a", 1), line("b", 2), line("c", 3), line("z", 0)];
        assert!(move_block(&mut page, "b", "z", true));
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 1), ("c", 2), ("z", 0), ("b", 0)]);
    }

    #[test]
    fn tidy_puts_lines_from_elsewhere_in_a_column_and_numbers_them_again() {
        // A plain line that came in between the lines of a set.
        let mut page = vec![line("a", 1), line("new", 0), line("b", 2)];
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 1), ("new", 1), ("b", 2)]);
        // Numbers with gaps, and two sets one after the other.
        let mut page = vec![line("a", 2), line("b", 3), line("c", 1), line("d", 3), line("e", 0)];
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 1), ("b", 2), ("c", 1), ("d", 2), ("e", 0)]);
        assert_eq!(sets(&page), vec![0..2, 2..4]);
        // One column alone is no set.
        let mut page = vec![line("a", 2), line("b", 2), line("c", 0)];
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 0), ("b", 0), ("c", 0)]);
        // A plain line after a set stays plain.
        let mut page = vec![line("a", 1), line("b", 2), line("c", 0)];
        tidy(&mut page);
        assert_eq!(cols(&page), vec![("a", 1), ("b", 2), ("c", 0)]);
    }

    #[test]
    fn typing_in_a_column_stays_in_it() {
        use crate::model::{indent, list_numbers, move_up, press_backspace_at_start, press_delete_at_end, press_enter, section_end, visibility};
        // Return at the end of the last line of the last column, at the foot
        // of the page: the new line is in that column.
        let mut page = vec![line("a", 1), line("b", 2)];
        press_enter(&mut page, 1, 1, "b");
        assert_eq!(page[2].place.col, 2);
        // At the start of a column's first line: the empty line above it too.
        press_enter(&mut page, 1, 0, "b");
        assert_eq!(cols(&page)[1..], [("", 2), ("b", 2), ("", 2)]);
        // Backspace at the start of a column's first line joins nothing.
        let mut page = vec![line("a", 1), line("b", 2)];
        assert_eq!(press_backspace_at_start(&mut page, 1, "b"), None);
        assert_eq!(cols(&page), vec![("a", 1), ("b", 2)]);
        // Delete at the end of a column joins nothing either.
        assert_eq!(press_delete_at_end(&mut page, 0, "a"), None);
        assert_eq!(page.len(), 2);
        // An empty first line stays: no line of its column is above it.
        let mut page = vec![line("a", 1), line("", 2), line("b", 2)];
        assert!(press_backspace_at_start(&mut page, 1, "").is_none());
        assert_eq!(cols(&page), vec![("a", 1), ("", 2), ("b", 2)]);
        // Tab does not nest a column's first line under the column before;
        // ⌘⇧↑ does not take it there.
        let mut page = vec![line("a", 1), line("b", 2)];
        assert!(!indent(&mut page, 1));
        assert!(!move_up(&mut page, 1));
        // A closed toggle, a folded heading and a numbered list keep to their column.
        let mut page = vec![line("t", 1), line("b", 2), line("c", 2)];
        page[0].kind = BlockKind::Toggle;
        page[0].collapsed = true;
        page[1].indent = 1;
        page[1].kind = BlockKind::Numbered;
        page[0].kind = BlockKind::Toggle;
        assert_eq!(visibility(&page), vec![true, true, true]);
        page[0].kind = BlockKind::Heading2;
        page[2].kind = BlockKind::Numbered;
        page[2].indent = 1;
        assert_eq!(section_end(&page, 0), 1, "a heading in a column holds lines of its column only");
        assert_eq!(list_numbers(&page)[1..], [Some(1), Some(2)]);
        let mut page = vec![line("n", 0), line("x", 1), line("y", 2)];
        page[0].kind = BlockKind::Numbered;
        page[1].kind = BlockKind::Numbered;
        page[2].kind = BlockKind::Numbered;
        assert_eq!(list_numbers(&page), vec![Some(1), Some(1), Some(1)], "each column counts from 1");
        // A heading as wide as the page holds the columns under it.
        let mut page = vec![line("h", 0), line("a", 1), line("b", 2), line("h2", 0)];
        page[0].kind = BlockKind::Heading2;
        page[3].kind = BlockKind::Heading2;
        assert_eq!(section_end(&page, 0), 3);
    }

    /// A page with two columns, a plain line above and below, a picture and
    /// a table in the second column.
    fn two_columns() -> Vec<Block> {
        let mut picture = Block::new(BlockKind::Image, "");
        picture.data = "a1b2c3.png".into();
        picture.place.col = 2;
        let mut table = crate::markdown::to_blocks("| Name | Qty |\n| --- | --- |\n| Tea | 2 |").remove(0);
        assert_eq!(table.kind, BlockKind::Table);
        table.place.col = 2;
        let mut heading = Block::new(BlockKind::Heading2, "Left");
        heading.place.col = 1;
        let mut item = Block::new(BlockKind::Bulleted, "first point");
        item.place.col = 1;
        let mut right = Block::new(BlockKind::Heading2, "Right");
        right.place.col = 2;
        vec![Block::text("Above"), heading, item, right, picture, table, Block::text("Below")]
    }

    #[test]
    fn markdown_has_the_lines_in_order_and_a_rewrite_keeps_the_columns() {
        // As the connector gives a page to an AI tool, and reads it back.
        let pages = std::collections::HashMap::new();
        let page = two_columns();
        let md = crate::mcp::render_body(&pages, &page);
        let at = |words: &str| md.find(words).unwrap_or_else(|| panic!("{words} in {md}"));
        assert!(at("Above") < at("Left") && at("Left") < at("first point") && at("first point") < at("Right") && at("Right") < at("[Image]") && at("[Image]") < at("| Tea") && at("| Tea") < at("Below"), "{md}");
        let back = |md: &str| crate::model::keep_unchanged(&page, crate::mcp::keep_media(&page, crate::mcp::parse_body(&pages, md)));
        // Given back with no change: every line is there, and the columns
        // are what they were.
        let shape = |bs: &[Block]| bs.iter().map(|b| (b.kind, b.plain_text(), b.place.col)).collect::<Vec<_>>();
        assert_eq!(shape(&back(&md)), shape(&page));
        // A line changed by the AI, and a new one, stay in their column.
        let read = back(&md.replace("- first point", "- first point, and more\n- a second point"));
        assert_eq!(
            read.iter().map(|b| (b.plain_text(), b.place.col)).collect::<Vec<_>>(),
            [("Above", 0), ("Left", 1), ("first point, and more", 1), ("a second point", 1), ("Right", 2), ("", 2), ("Name Qty\nTea 2", 2), ("Below", 0)].map(|(t, c)| (t.to_string(), c))
        );
        // Lines the AI left out are gone as it asked, and a column left
        // with none is no column: no line is lost that it gave back.
        let read = back(&md.replace("## Left\n", "").replace("- first point\n", ""));
        assert_eq!(read.iter().map(|b| b.place.col).collect::<Vec<_>>(), vec![0; read.len()]);
        assert_eq!(read.iter().filter(|b| matches!(b.kind, BlockKind::Image | BlockKind::Table)).count(), 2);
    }

    /// The place of a line as Rune 0.1.23 knew it: it reads the JSON into
    /// this and writes it back from it when it saves the whole page.
    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct OlderPlace {
        #[serde(default)]
        free: bool,
        #[serde(default)]
        right: bool,
        #[serde(default)]
        x: i32,
        #[serde(default)]
        y: i32,
        #[serde(default)]
        w: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        beside: bool,
    }

    #[test]
    fn an_older_rune_shows_and_writes_back_every_line_in_order() {
        let page = two_columns();
        // What it reads: every place is one it knows (no error, so no
        // picture loses its place), and the lines are in the order of the list.
        let older: Vec<OlderPlace> = page.iter().map(|b| serde_json::from_str(&b.place.to_column()).unwrap_or_default()).collect();
        assert_eq!(older.len(), page.len());
        // What it writes back when it saves the page: the same lines, with no columns.
        let back: Vec<Block> = page
            .iter()
            .zip(&older)
            .map(|(b, o)| {
                let column = if b.place.to_column().is_empty() { String::new() } else { serde_json::to_string(o).unwrap() };
                Block { place: crate::model::Place::from_column(&column), ..b.clone() }
            })
            .collect();
        assert!(back.iter().all(|b| b.place.col == 0));
        assert_eq!(back.iter().map(|b| (b.id.clone(), b.kind, b.text.clone(), b.data.clone())).collect::<Vec<_>>(), page.iter().map(|b| (b.id.clone(), b.kind, b.text.clone(), b.data.clone())).collect::<Vec<_>>());
        assert!(sets(&back).is_empty());
    }

    #[test]
    fn a_line_the_ai_changed_stays_in_its_place_in_its_column() {
        let pages = std::collections::HashMap::new();
        let mut page = two_columns();
        let mut last = Block::text("last of the right");
        last.place.col = 2;
        page.insert(6, last);
        let md = crate::mcp::render_body(&pages, &page);
        let back = |md: &str| crate::model::keep_unchanged(&page, crate::mcp::keep_media(&page, crate::mcp::parse_body(&pages, md)));
        let shape = |bs: &[Block]| bs.iter().map(|b| (b.plain_text(), b.place.col)).collect::<Vec<_>>();
        // The first line of column 2, the last line of the last column, and
        // a cell of the table in column 2 are changed.
        let changed = md.replace("## Right", "## Right side").replace("last of the right", "the last, changed").replace("| Tea | 2 |", "| Tea | 3 |");
        let read = back(&changed);
        assert_eq!(
            shape(&read),
            [("Above", 0), ("Left", 1), ("first point", 1), ("Right side", 2), ("", 2), ("Name Qty\nTea 3", 2), ("the last, changed", 2), ("Below", 0)].map(|(t, c)| (t.to_string(), c))
        );
        // A new line takes the column of the line before it.
        let read = back(&md.replace("- first point", "- first point\n- added"));
        assert_eq!(shape(&read)[3], ("added".to_string(), 1));
        assert_eq!(shape(&read)[4], ("Right".to_string(), 2));
    }

    #[test]
    fn backspace_at_the_top_of_a_column_takes_nothing_of_the_column_before() {
        use crate::model::press_backspace_at_start;
        let mut divider = line("", 1);
        divider.kind = BlockKind::Divider;
        let mut page = vec![line("a", 1), divider, line("", 2), line("b", 2)];
        let was = page.clone();
        assert_eq!(press_backspace_at_start(&mut page, 2, ""), None, "the empty line stays: no line of its column is above it");
        assert_eq!(page, was, "the divider of column 1 stays");
        // An empty line under a line of its own column goes, with the caret there.
        let mut page = vec![line("a", 1), line("b", 2), line("", 2)];
        assert_eq!(press_backspace_at_start(&mut page, 2, ""), Some(crate::model::Caret { id: "b".into(), pos: 1 }));
        assert_eq!(cols(&page), vec![("a", 1), ("b", 2)]);
    }

    #[test]
    fn a_total_adds_up_its_own_column() {
        let page = vec![line("Rent 1200", 1), line("Food 300", 1), line("Total", 2), line("Coffee 4", 2), line("Tea 6", 2), line("Total", 2)];
        let answers = crate::calc::answers(&page);
        assert_eq!(answers[2], None, "nothing above it in its column");
        assert_eq!(answers[5].as_deref(), Some("= 10"));
    }

    #[test]
    fn a_transcript_in_a_column_stays_in_its_toggle() {
        let mut toggle = line("Transcript", 2);
        toggle.kind = BlockKind::Toggle;
        let mut page = vec![line("a", 1), toggle.clone(), line("b", 0)];
        let mut at = Some(toggle.id.clone());
        let heard = |w: &str| Block::text(w).with_indent(1);
        crate::listen::put_transcript(&mut page, &mut at, 0, vec![heard("one"), heard("two")]);
        assert_eq!(cols(&page), vec![("a", 1), ("Transcript", 2), ("one", 2), ("two", 2), ("b", 0)]);
        assert_eq!(crate::listen::transcript_so_far(&page).1, 2);
        let mut notes = None;
        crate::listen::put_under(&mut page, &mut notes, "Notes", false, Some("a"), vec![heard("n")]);
        assert_eq!(cols(&page)[..3], [("a", 1), ("Notes", 1), ("n", 1)]);
    }

    #[test]
    fn arrows_pass_over_the_other_columns() {
        let page = vec![line("above", 0), line("a", 1), line("a2", 1), line("b", 2), line("below", 0)];
        // Down from the end of column 1 would go to column 2: the set is passed over.
        assert_eq!(across(&page, 2, 3), Some(1..4));
        // Within a column, and out of the set, the arrows go on as before.
        assert_eq!(across(&page, 1, 2), None);
        assert_eq!(across(&page, 3, 4), None);
        assert_eq!(across(&page, 0, 1), None);
    }
}
