//! Moving pages in and out of the app as Markdown files.
//!
//! Export writes the workspace as a folder tree: one `.md` file per page, a
//! folder beside it for the pages inside it, links between pages as relative
//! links, and drawings as `.excalidraw` files (which excalidraw.com opens).
//!
//! Import reads a Notion export (the .zip Notion makes, or a folder of it) or
//! Markdown files, and makes pages from them. Notion's file names end in a
//! 32-character id, which is removed; links between the exported files become
//! links between the new pages, and each page's sub-pages appear in it as page
//! links, the same as pages made in the app.

use crate::markdown;
use crate::model::{now_ms, Block, BlockKind, Id, Page, Space};
use crate::state::{children_of, roots_in, written_row_pages};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::inline::{OLD_PAGE_URL, PAGE_URL};

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// A file or folder name made from a page title.
pub fn file_name(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let s = s.trim().trim_start_matches('.').trim();
    // A Mac file name holds 255 bytes, not 255 letters, and one letter can be
    // four bytes. 200 leaves room for " 12" and ".md" after it.
    let mut bytes = 0;
    let s: String = s
        .chars()
        .take(80)
        .take_while(|c| {
            bytes += c.len_utf8();
            bytes <= 200
        })
        .collect();
    let s = s.trim_end();
    if s.is_empty() { "Untitled".into() } else { s.to_string() }
}

/// `name`, made different from every name in `taken` ("Notes", "Notes 2", …).
/// Mac file names ignore case, so the names are compared without it.
fn unique(name: String, taken: &mut HashSet<String>) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("{name} {n}");
        n += 1;
    }
    taken.insert(candidate.to_lowercase());
    candidate
}

/// Where each page goes, as path parts without the ".md" ending:
/// ["Projects", "Website"] is Projects/Website.md, and the pages inside it go
/// in Projects/Website/. Pages in the Trash are left out. With more than one
/// space, each space is a folder of its own. The page of a row of a table
/// goes with the pages inside the page of the table, if something is
/// written in it.
fn plan(spaces: &[Space], pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>) -> Vec<(Id, Vec<String>)> {
    fn place(list: &[Page], dir: &[String], pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, out: &mut Vec<(Id, Vec<String>)>) {
        let mut taken = HashSet::new();
        for p in list {
            let mut path = dir.to_vec();
            path.push(unique(file_name(p.display_title()), &mut taken));
            out.push((p.id.clone(), path.clone()));
            let mut inside = children_of(pages, Some(&p.id));
            inside.extend(written_row_pages(pages, blocks, &p.id));
            place(&inside, &path, pages, blocks, out);
        }
    }
    let mut out = Vec::new();
    let mut space_names = HashSet::new();
    for space in spaces {
        let base: Vec<String> =
            if spaces.len() > 1 { vec![unique(file_name(&space.name), &mut space_names)] } else { Vec::new() };
        place(&roots_in(pages, &space.id), &base, pages, blocks, &mut out);
    }
    out
}

/// The link from a file in folder `from` to `to` (both relative to the export).
fn relative(from: &[String], to: &[String]) -> String {
    // Only `to`'s folder is shared; its last part is the file itself.
    let (to_dir, file) = to.split_at(to.len().saturating_sub(1));
    let common = from.iter().zip(to_dir).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(to_dir[common..].iter().cloned());
    parts.extend(file.iter().cloned());
    parts.join("/")
}

/// A link target safe to put inside Markdown's `( )`.
fn encode(href: &str) -> String {
    let mut out = String::with_capacity(href.len());
    for c in href.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            c => out.push(c),
        }
    }
    out
}

/// Replace every `rune://page/<id>` (or the older `notion2://page/<id>`) in
/// `text` whose page `href` knows.
fn rewrite_page_urls(text: &str, href: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // The next link of either form.
    let next = |s: &str| [PAGE_URL, OLD_PAGE_URL].iter().filter_map(|p| s.find(p).map(|i| (i, p.len()))).min();
    while let Some((i, len)) = next(rest) {
        out.push_str(&rest[..i]);
        let after = &rest[i + len..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(after.len());
        match href(&after[..end]) {
            Some(h) => out.push_str(&encode(&h)),
            None => out.push_str(&rest[i..i + len + end]),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// An empty Excalidraw scene, for a drawing nobody has drawn in yet.
const EMPTY_SCENE: &str = r#"{"type":"excalidraw","version":2,"source":"rune","elements":[],"appState":{},"files":{}}"#;

/// The kept pictures a page shows outside its lines: its cover, when it is
/// the person's own picture, and the pictures in the cells of its tables.
/// They go out beside the page with the pictures of its lines (they were
/// left out: the names in the cells of an exported table named no file).
fn other_pictures(page: &Page, body: &[Block]) -> Vec<String> {
    let mut names: Vec<String> = page.cover_file().map(String::from).into_iter().collect();
    for b in body.iter().filter(|b| b.kind == BlockKind::Table) {
        let table = crate::table::Table::parse(&b.data);
        for (c, _) in table.columns.iter().enumerate().filter(|(_, column)| column.kind == crate::table::Kind::File) {
            for cell in table.rows.iter().skip(1).filter_map(|row| row.get(c)) {
                names.extend(crate::table::parts(cell).into_iter().filter(|name| crate::files::valid_name(name)));
            }
        }
    }
    names.dedup();
    names
}

/// Write every page (except those in the Trash) into `dir`, which must exist.
/// Returns how many pages were written.
pub fn export_all(
    dir: &Path,
    spaces: &[Space],
    pages: &HashMap<Id, Page>,
    blocks: &HashMap<Id, Vec<Block>>,
) -> std::io::Result<usize> {
    let plan = plan(spaces, pages, blocks);
    let paths: HashMap<&str, &[String]> = plan.iter().map(|(id, p)| (id.as_str(), p.as_slice())).collect();
    for (id, path) in &plan {
        let page = &pages[id];
        let folder = &path[..path.len() - 1];
        let link = |target: &str| paths.get(target).map(|to| format!("{}.md", relative(folder, to)));
        let mut drawings: Vec<(String, String)> = Vec::new();
        let mut pictures: Vec<String> = Vec::new();
        let body = blocks.get(id).map(Vec::as_slice).unwrap_or_default();
        let md = markdown::render(&page.title, body, |b| match b.kind {
            BlockKind::PageLink => {
                let target = b.target.as_deref().unwrap_or("");
                let title = pages.get(target).map(|p| p.display_title()).unwrap_or("Untitled");
                Some(match link(target) {
                    Some(href) => format!("[{title}]({})", encode(&href)),
                    None => title.to_string(),
                })
            }
            BlockKind::Drawing => {
                let name = if drawings.is_empty() { "Drawing".to_string() } else { format!("Drawing {}", drawings.len() + 1) };
                let file = format!("{}/{name}.excalidraw", path[path.len() - 1]);
                drawings.push((name.clone(), if b.data.trim().is_empty() { EMPTY_SCENE.to_string() } else { b.data.clone() }));
                Some(format!("{}[{name}]({})", "  ".repeat(b.indent as usize), encode(&file)))
            }
            // A picture goes out as a file beside the page, and the page shows it.
            BlockKind::Image if crate::files::valid_name(&b.data) => {
                let file = format!("{}/{}", path[path.len() - 1], b.data);
                pictures.push(b.data.clone());
                // Its caption is its name in the file (no square brackets: they end the name).
                let caption = b.text.trim().replace(['[', ']'], "");
                let name = if caption.is_empty() { "Image" } else { caption.as_str() };
                Some(format!("{}![{name}]({})", "  ".repeat(b.indent as usize), encode(&file)))
            }
            _ => None,
        });
        let md = rewrite_page_urls(&md, link);
        pictures.extend(other_pictures(page, body));

        let mut file = dir.to_path_buf();
        for part in folder {
            file.push(part);
        }
        std::fs::create_dir_all(&file)?;
        std::fs::write(file.join(format!("{}.md", path[path.len() - 1])), md + "\n")?;
        if !drawings.is_empty() || !pictures.is_empty() {
            let own = file.join(&path[path.len() - 1]);
            std::fs::create_dir_all(&own)?;
            for (name, scene) in drawings {
                std::fs::write(own.join(format!("{name}.excalidraw")), scene)?;
            }
            for name in pictures {
                if let Some(kept) = crate::files::path(&name) {
                    std::fs::copy(kept, own.join(&name))?;
                }
            }
        }
    }
    Ok(plan.len())
}

/// A new folder in `parent` for an export ("Rune export 2026-11-09",
/// then "… 2", …).
pub fn new_export_dir(parent: &Path) -> std::io::Result<PathBuf> {
    let base = format!("Rune export {}", crate::when::local(now_ms()).format("%Y-%m-%d"));
    let mut dir = parent.join(&base);
    let mut n = 2;
    while dir.exists() {
        dir = parent.join(format!("{base} {n}"));
        n += 1;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// What an import made: the pages (the first is the top one) and their blocks.
pub struct Imported {
    pub pages: Vec<Page>,
    pub blocks: Vec<(Id, Vec<Block>)>,
    /// The top page was made to hold everything (not read from a file).
    pub wrapper: bool,
}

/// A page to make: from a Markdown file, a folder, or both.
struct Node {
    title: String,
    md: Option<PathBuf>,
    children: Vec<Node>,
}

/// A Notion file or folder name without its id: "Plan 0a1b…(32 hex)" → "Plan".
fn clean_name(name: &str) -> String {
    let name = name.trim();
    let bytes = name.as_bytes();
    if bytes.len() >= 32 {
        let (head, tail) = name.split_at(name.len() - 32);
        if tail.bytes().all(|b| b.is_ascii_hexdigit()) && (head.is_empty() || head.ends_with(' ')) {
            let head = head.trim_end();
            if !head.is_empty() {
                return head.to_string();
            }
        }
    }
    name.to_string()
}

fn is_markdown(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

fn hidden(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_none_or(|n| n.starts_with('.') || n == "__MACOSX")
}

/// The pages a folder holds: each Markdown file is a page, and a folder with
/// the same name beside it holds that page's sub-pages. A folder with no file
/// of its own (a Notion database) becomes a page holding what is inside it.
/// What a path really is: a folder (`Some(true)`), a file (`Some(false)`),
/// or neither. A link to somewhere else is neither: an import follows no
/// links, so a crafted archive or folder cannot make it read (or unpack
/// into) places outside itself.
fn real_kind(p: &Path) -> Option<bool> {
    let kind = std::fs::symlink_metadata(p).ok()?.file_type();
    if kind.is_symlink() {
        None
    } else if kind.is_dir() {
        Some(true)
    } else if kind.is_file() {
        Some(false)
    } else {
        None
    }
}

/// The most of one Markdown file that an import reads.
const LARGEST_PAGE: u64 = 20 * 1024 * 1024;

/// The text of a Markdown file: only of a real file (no link, no device) of
/// a sane size.
fn read_page_file(p: &Path) -> String {
    let fits = std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file() && m.len() <= LARGEST_PAGE);
    if !fits {
        return String::new();
    }
    std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

fn read_folder(dir: &Path, depth: usize) -> Vec<Node> {
    if depth > 32 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files = Vec::new();
    let mut folders = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if hidden(&p) {
            continue;
        }
        match real_kind(&p) {
            Some(true) => folders.push(p),
            Some(false) if is_markdown(&p) => files.push(p),
            _ => {}
        }
    }
    files.sort();
    folders.sort();
    let mut used: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();
    for f in files {
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let own = dir.join(&stem);
        let children = if real_kind(&own) == Some(true) {
            used.insert(own.clone());
            read_folder(&own, depth + 1)
        } else {
            Vec::new()
        };
        out.push(Node { title: clean_name(&stem), md: Some(f), children });
    }
    for d in folders {
        if used.contains(&d) {
            continue;
        }
        let children = read_folder(&d, depth + 1);
        if !children.is_empty() {
            let name = d.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            out.push(Node { title: clean_name(&name), md: None, children });
        }
    }
    out
}

/// Unpack a .zip into `into` with the Mac's own `ditto`, then any .zip files
/// that were inside it (Notion splits large exports into parts).
fn unzip(zip: &Path, into: &Path, depth: usize) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(into)
        .status()
        .map_err(|e| format!("couldn't open the zip file ({e})"))?;
    if !status.success() {
        return Err("couldn't open the zip file".into());
    }
    if depth < 2 {
        let inner: Vec<PathBuf> = walk_files(into).into_iter().filter(|p| p.extension().is_some_and(|e| e == "zip")).collect();
        for z in inner {
            let dest = z.with_extension("");
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            unzip(&z, &dest, depth + 1)?;
            let _ = std::fs::remove_file(&z);
        }
    }
    Ok(())
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            match real_kind(&p) {
                Some(true) => stack.push(p),
                Some(false) => out.push(p),
                None => {}
            }
        }
    }
    out
}

/// `%20` and the like, decoded.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `dir` joined with a relative link, with "." and ".." worked out.
fn resolve(dir: &Path, link: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for c in dir.join(link).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Call `f(is_image, label, target)` for each `[label](target)` and
/// `![label](target)` in `md`, putting back what it returns.
fn map_links(md: &str, mut f: impl FnMut(bool, &str, &str) -> String) -> String {
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(i) = rest.find('[') {
        let image = rest[..i].ends_with('!');
        let after = &rest[i + 1..];
        let parsed = after.find(']').and_then(|close| {
            let label = &after[..close];
            let tail = after[close + 1..].strip_prefix('(')?;
            let end = tail.find(')')?;
            let target = &tail[..end];
            (!label.contains('\n') && !target.contains('\n')).then(|| (label, target, &tail[end + 1..]))
        });
        match parsed {
            Some((label, target, next)) => {
                out.push_str(&rest[..if image { i - 1 } else { i }]);
                out.push_str(&f(image, label, target));
                rest = next;
            }
            None => {
                out.push_str(&rest[..i + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_external(target: &str) -> bool {
    target.contains("://") || target.starts_with("mailto:") || target.starts_with('#')
}

/// Read a Notion export (.zip or folder) or a Markdown file into new pages in
/// `space`. The top page gets `position` among the space's top pages.
pub fn import(path: &Path, space: &str, position: f64) -> Result<Imported, String> {
    let is_zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let temp = std::env::temp_dir().join(format!("rune-import-{}", now_ms()));
    let result = (|| {
        let (nodes, wrapper_title) = if is_zip {
            std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
            unzip(path, &temp, 0)?;
            (read_folder(&temp, 0), "Imported from Notion".to_string())
        } else if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Imported");
            (read_folder(path, 0), clean_name(name))
        } else if is_markdown(path) {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let own = path.with_file_name(&stem);
            let children = if own.is_dir() { read_folder(&own, 1) } else { Vec::new() };
            (vec![Node { title: clean_name(&stem), md: Some(path.to_path_buf()), children }], String::new())
        } else {
            return Err("choose a Notion export (.zip), a folder, or a Markdown file".to_string());
        };
        // A lone top folder (as some zips have) is looked inside.
        let mut nodes = nodes;
        while nodes.len() == 1 && nodes[0].md.is_none() && is_zip {
            nodes = std::mem::take(&mut nodes[0].children);
        }
        if nodes.is_empty() {
            return Err("there are no Markdown pages in it".to_string());
        }
        let (top, wrapper) = if nodes.len() == 1 && !is_zip && !path.is_dir() {
            (nodes.pop().unwrap(), false)
        } else {
            (Node { title: wrapper_title, md: None, children: nodes }, true)
        };
        Ok(build(top, wrapper, space, position))
    })();
    if is_zip {
        let _ = std::fs::remove_dir_all(&temp);
    }
    result
}

/// Make pages from the tree under `top`.
fn build(top: Node, wrapper: bool, space: &str, position: f64) -> Imported {
    // First give every page an id, so links between files can be made.
    struct Made {
        page: Page,
        md: Option<PathBuf>,
        children: Vec<usize>,
    }
    fn make(node: Node, parent: Option<Id>, position: f64, space: &str, all: &mut Vec<Made>, by_file: &mut HashMap<PathBuf, Id>) -> usize {
        let mut page = Page::new(parent, position);
        page.title = node.title;
        page.space_id = space.to_string();
        if let Some(md) = &node.md {
            by_file.insert(md.clone(), page.id.clone());
        }
        let id = page.id.clone();
        let index = all.len();
        all.push(Made { page, md: node.md, children: Vec::new() });
        let kids: Vec<usize> = node
            .children
            .into_iter()
            .enumerate()
            .map(|(i, child)| make(child, Some(id.clone()), (i + 1) as f64, space, all, by_file))
            .collect();
        all[index].children = kids;
        index
    }
    let mut all: Vec<Made> = Vec::new();
    let mut by_file: HashMap<PathBuf, Id> = HashMap::new();
    make(top, None, position, space, &mut all, &mut by_file);

    let mut blocks_of: Vec<Vec<Block>> = Vec::with_capacity(all.len());
    for made in &mut all {
        let mut blocks = Vec::new();
        if let Some(md) = &made.md {
            let text = read_page_file(md);
            let text = text.trim_start_matches('\u{feff}');
            // Notion starts each file with the page title as a heading.
            let body = match text.lines().next().and_then(|l| l.strip_prefix("# ")) {
                Some(title) => {
                    let title = title.trim();
                    if !title.is_empty() {
                        made.page.title = title.to_string();
                    }
                    text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
                }
                None => text,
            };
            let dir = md.parent().unwrap_or(Path::new(""));
            let body = map_links(body, |image, label, target| {
                let target = target.trim().trim_start_matches('<').trim_end_matches('>');
                if image {
                    let name = if label.trim().is_empty() { decode(target.rsplit('/').next().unwrap_or("")) } else { label.to_string() };
                    return format!("(Image: {})", name.trim());
                }
                if is_external(target) {
                    return format!("[{label}]({target})");
                }
                match by_file.get(&resolve(dir, &decode(target))) {
                    Some(id) => format!("[{label}]({PAGE_URL}{id})"),
                    None => label.to_string(),
                }
            });
            blocks = markdown::to_blocks(&body);
        }
        blocks_of.push(blocks);
    }

    // Sub-pages: a line that is only a link to one becomes a page link (as in
    // pages made here), in the order the page lists them; the rest are added
    // at the end. Their sidebar order follows.
    let mut out_pages = Vec::with_capacity(all.len());
    let mut out_blocks = Vec::with_capacity(all.len());
    let ids: Vec<Id> = all.iter().map(|m| m.page.id.clone()).collect();
    let mut positions: HashMap<Id, f64> = HashMap::new();
    for (i, made) in all.iter().enumerate() {
        let child_ids: HashSet<&str> = made.children.iter().map(|&c| ids[c].as_str()).collect();
        let mut seen: Vec<Id> = Vec::new();
        let mut blocks: Vec<Block> = std::mem::take(&mut blocks_of[i])
            .into_iter()
            .map(|b| {
                if b.kind == BlockKind::Text && b.indent == 0 {
                    if let Some(id) = only_page_link(&b.text) {
                        if child_ids.contains(id.as_str()) && !seen.contains(&id) {
                            seen.push(id.clone());
                            return Block::page_link(id);
                        }
                    }
                }
                b
            })
            .collect();
        for &c in &made.children {
            if !seen.contains(&ids[c]) {
                seen.push(ids[c].clone());
                blocks.push(Block::page_link(ids[c].clone()));
            }
        }
        for (n, id) in seen.iter().enumerate() {
            positions.insert(id.clone(), (n + 1) as f64);
        }
        if blocks.is_empty() {
            blocks.push(Block::text(""));
        }
        out_blocks.push((made.page.id.clone(), blocks));
    }
    for made in all {
        let mut page = made.page;
        if let Some(&p) = positions.get(&page.id) {
            page.position = p;
        }
        out_pages.push(page);
    }
    Imported { pages: out_pages, blocks: out_blocks, wrapper }
}

/// The page id when `text` is nothing but a link to a page.
fn only_page_link(text: &str) -> Option<Id> {
    let t = text.trim();
    let inner = t.strip_prefix('[')?.strip_suffix(')')?;
    let (_, target) = inner.split_once("](")?;
    let id = target.strip_prefix(PAGE_URL)?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rune-transfer-{name}-{}", now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_are_safe_and_unique() {
        assert_eq!(file_name("a/b: c?"), "a-b- c-");
        assert_eq!(file_name("  ...hidden "), "hidden");
        assert_eq!(file_name(""), "Untitled");
        let mut taken = HashSet::new();
        assert_eq!(unique("Notes".into(), &mut taken), "Notes");
        assert_eq!(unique("notes".into(), &mut taken), "notes 2");
        assert_eq!(relative(&["A".into()], &["A".into(), "B".into()]), "B");
        assert_eq!(relative(&["A".into(), "B".into()], &["C".into()]), "../../C");
        assert_eq!(clean_name("Plan 0123456789abcdef0123456789abcdef"), "Plan");
        assert_eq!(clean_name("0123456789abcdef0123456789abcdef"), "0123456789abcdef0123456789abcdef");
        assert_eq!(decode("My%20Page%28x%29.md"), "My Page(x).md");
    }

    #[test]
    fn workspace_exports_as_linked_files() {
        let space = Space { id: "s".into(), name: "Home".into(), color: String::new(), position: 1.0, created_at: 0 };
        let mut parent = Page::new(None, 1.0);
        parent.title = "Projects".into();
        parent.space_id = "s".into();
        let mut child = Page::new(Some(parent.id.clone()), 1.0);
        child.title = "Web site".into();
        child.space_id = "s".into();
        let mut gone = Page::new(None, 2.0);
        gone.title = "Old".into();
        gone.space_id = "s".into();
        gone.trashed = true;
        let mut drawing = Block::new(BlockKind::Drawing, "");
        drawing.data = r#"{"type":"excalidraw"}"#.into();
        let blocks: HashMap<Id, Vec<Block>> = [
            (parent.id.clone(), vec![Block::text("Intro"), Block::page_link(child.id.clone()), drawing]),
            (child.id.clone(), vec![Block::text(format!("Back to [the list]({PAGE_URL}{})", parent.id))]),
        ]
        .into_iter()
        .collect();
        let pages: HashMap<Id, Page> =
            [parent.clone(), child.clone(), gone].into_iter().map(|p| (p.id.clone(), p)).collect();
        let dir = temp("export");
        assert_eq!(export_all(&dir, &[space], &pages, &blocks).unwrap(), 2);
        let top = std::fs::read_to_string(dir.join("Projects.md")).unwrap();
        assert!(top.starts_with("# Projects\n\nIntro"), "{top}");
        assert!(top.contains("[Web site](Projects/Web%20site.md)"), "{top}");
        assert!(top.contains("[Drawing](Projects/Drawing.excalidraw)"), "{top}");
        assert!(dir.join("Projects/Drawing.excalidraw").exists());
        let inner = std::fs::read_to_string(dir.join("Projects/Web site.md")).unwrap();
        assert!(inner.contains("[the list](../Projects.md)"), "{inner}");
        assert!(!dir.join("Old.md").exists(), "pages in the Trash are left out");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cover_and_the_pictures_of_table_cells_go_out_with_a_page() {
        let mut page = Page::new(None, 1.0);
        page.cover = Some("file:cover1.jpg".into());
        let mut table = crate::table::Table::parse("");
        table.rows = vec![vec!["Name".into(), "Shots".into()], vec!["Monday".into(), "a1.png, b2.png".into()], vec!["Tuesday".into(), "not a picture".into()]];
        table.columns = vec![crate::table::Column::default(), crate::table::Column { kind: crate::table::Kind::File, ..Default::default() }];
        let mut block = Block::new(BlockKind::Table, "");
        block.data = table.to_data();
        assert_eq!(other_pictures(&page, &[Block::text("a1.png"), block.clone()]), ["cover1.jpg", "a1.png", "b2.png"]);
        // One of the app's own covers is no file, and a plain table names none.
        page.cover = Some("sea".into());
        table.columns.clear();
        block.data = table.to_data();
        assert!(other_pictures(&page, &[block]).is_empty());
    }

    #[test]
    fn notion_export_imports_with_links_and_sub_pages() {
        let dir = temp("import");
        let id = "0123456789abcdef0123456789abcdef";
        let kid = "fedcba9876543210fedcba9876543210";
        std::fs::write(
            dir.join(format!("Trip {id}.md")),
            format!("# Trip to Rome\n\nPlan below.\n\n[Packing](Trip%20{id}/Packing%20{kid}.md)\n\n![](Trip%20{id}/map.png)\n\nSee [docs](https://example.com)."),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join(format!("Trip {id}"))).unwrap();
        std::fs::write(dir.join(format!("Trip {id}/Packing {kid}.md")), format!("# Packing\n\n- [ ] socks\n\nUp: [Trip](../Trip%20{id}.md)")).unwrap();
        std::fs::write(dir.join(format!("Trip {id}/Extra.md")), "Loose page").unwrap();

        let got = import(&dir, "s", 5.0).unwrap();
        assert!(got.wrapper);
        assert_eq!(got.pages.len(), 4);
        let by_title: HashMap<&str, &Page> = got.pages.iter().map(|p| (p.title.as_str(), p)).collect();
        let trip = by_title["Trip to Rome"];
        let packing = by_title["Packing"];
        let extra = by_title["Extra"];
        assert_eq!(got.pages[0].position, 5.0);
        assert_eq!(trip.parent_id.as_deref(), Some(got.pages[0].id.as_str()));
        assert_eq!(packing.parent_id.as_deref(), Some(trip.id.as_str()));
        assert!(got.pages.iter().all(|p| p.space_id == "s"));

        let blocks: HashMap<&str, &Vec<Block>> = got.blocks.iter().map(|(id, b)| (id.as_str(), b)).collect();
        let trip_blocks = blocks[trip.id.as_str()];
        let kinds: Vec<BlockKind> = trip_blocks.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![BlockKind::Text, BlockKind::PageLink, BlockKind::Text, BlockKind::Text, BlockKind::PageLink]);
        assert_eq!(trip_blocks[1].target.as_deref(), Some(packing.id.as_str()));
        assert_eq!(trip_blocks[2].text, "(Image: map.png)");
        assert!(trip_blocks[3].text.contains("(https://example.com)"));
        assert_eq!(trip_blocks[4].target.as_deref(), Some(extra.id.as_str()), "unlisted sub-pages are added at the end");
        assert_eq!(packing.position, 1.0);
        assert_eq!(extra.position, 2.0);
        let back = &blocks[packing.id.as_str()][1].text;
        assert_eq!(back, &format!("Up: [Trip]({PAGE_URL}{})", trip.id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_markdown_file_imports_as_one_page() {
        let dir = temp("one");
        let file = dir.join("Notes.md");
        std::fs::write(&file, "Just text").unwrap();
        let got = import(&file, "s", 1.0).unwrap();
        assert!(!got.wrapper);
        assert_eq!(got.pages.len(), 1);
        assert_eq!(got.pages[0].title, "Notes");
        assert_eq!(got.blocks[0].1[0].text, "Just text");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
