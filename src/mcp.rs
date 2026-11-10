//! MCP server: `notion2 --mcp` lets AI agents (Aven, Claude Code, Claude Desktop,
//! Codex) read and edit the workspace.
//!
//! Transport is stdio: one JSON-RPC 2.0 message per line in, one per line out.
//! Nothing but protocol messages is ever written to stdout; logs go to stderr.
//! The server opens the same SQLite file as the app (WAL mode, so both can be
//! open at once) and reads it fresh on every call.

use crate::markdown;
use crate::model::*;
use crate::state::{ancestors, children_of, descendants};
use crate::store::{Loaded, Store};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

/// Protocol versions this server speaks, newest first.
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const LINK_PREFIX: &str = "notion2://page/";

const INSTRUCTIONS: &str = "Rune is the user's local notes workspace: pages and a calendar. \
Pages nest inside other pages and each has an id. Find ids with search_pages or list_pages before reading or editing. \
Page content is Markdown: # / ## / ### headings, paragraphs, - bullets, 1. numbered items, - [ ] / - [x] to-dos, > quotes, \
``` code fences and --- dividers; indent list items by two spaces to nest them. \
A line that is exactly [Title](notion2://page/<id>) is a link to another page; sub-pages appear in their parent this way. \
A to-do can end with the day it is due, as 📅 2026-11-12 (the sign, then the date): it then shows in that day of the calendar. Keep that ending when you rewrite a to-do; leave it out to take the day away. \
A picture or a drawing shows in read_page as a line [Image](notion2://block/<id>) or [Drawing](notion2://block/<id>): keep that line as it is to keep the picture or drawing in its place when you replace a page (one that you leave out is kept at the end of the page, never lost). \
Changes are saved immediately. trash_page is reversible with restore_page; no page is ever deleted permanently (delete_event does remove an event for good).";

/// What a change gets as its answer when this connector is older than the
/// app that keeps the notes.
const OLDER_THAN_THE_NOTES: &str = "This connector is older than the Rune app that keeps these notes, so it must not change them: the change would lose what the newer app added. Nothing was changed. Start the connector again (start the program that runs it again), then repeat the request. Reading still works.";

/// Serve MCP over stdin/stdout until stdin closes. Returns the process exit code.
pub fn run_stdio() -> i32 {
    let path = Store::default_path();
    let store = match Store::open(path.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[notion2 mcp] couldn't open the workspace at {}: {e}", path.display());
            return 1;
        }
    };
    eprintln!("[notion2 mcp] serving {}", path.display());
    let mut server = Server::new(store);
    match server.serve(io::stdin().lock(), io::stdout().lock()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[notion2 mcp] stopped: {e}");
            1
        }
    }
}

pub struct Server {
    store: Store,
}

type ToolResult = Result<String, String>;

impl Server {
    pub fn new(store: Store) -> Server {
        Server { store }
    }

    /// Read newline-delimited messages until EOF, answering each request.
    pub fn serve<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle_line(&line) {
                writer.write_all(reply.as_bytes())?;
                writer.write_all(b"\n")?;
                writer.flush()?;
            }
        }
        Ok(())
    }

    /// Handle one line of input; `None` when nothing should be sent back.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error_response(Value::Null, -32700, &format!("Parse error: {e}")).to_string()),
        };
        match value {
            Value::Array(items) if items.is_empty() => {
                Some(error_response(Value::Null, -32600, "Invalid Request: empty batch").to_string())
            }
            Value::Array(items) => {
                let replies: Vec<Value> = items.into_iter().filter_map(|m| self.handle_message(m)).collect();
                (!replies.is_empty()).then(|| Value::Array(replies).to_string())
            }
            other => self.handle_message(other).map(|v| v.to_string()),
        }
    }

    fn handle_message(&mut self, msg: Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(error_response(Value::Null, -32600, "Invalid Request"));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            // A response to a request we never sent, or not a message at all.
            if obj.contains_key("result") || obj.contains_key("error") {
                return None;
            }
            return Some(error_response(id.unwrap_or(Value::Null), -32600, "Invalid Request"));
        };
        // Notifications (initialized, cancelled, ...) are never answered.
        let id = id?;
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.call_tool(&params),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "Invalid params: missing tool name".to_string()))?;
        let empty = json!({});
        let args = params.get("arguments").filter(|a| !a.is_null()).unwrap_or(&empty);
        if !args.is_object() {
            return Err((-32602, "Invalid params: arguments must be an object".into()));
        }
        // An old copy of this connector, still open after the app took an
        // update, would write the notes back without what the update added.
        let reads = matches!(name, "search_pages" | "list_pages" | "read_page" | "list_events");
        if !reads && self.store.from_newer_program() {
            return Ok(json!({ "content": [{ "type": "text", "text": OLDER_THAN_THE_NOTES }], "isError": true }));
        }
        let outcome = match name {
            "search_pages" => self.search_pages(args),
            "list_pages" => self.list_pages(args),
            "read_page" => self.read_page(args),
            "create_page" => self.create_page(args),
            "append_to_page" => self.append_to_page(args),
            "replace_page_content" => self.replace_page_content(args),
            "update_page" => self.update_page(args),
            "trash_page" => self.trash_page(args),
            "restore_page" => self.restore_page(args),
            "list_events" => self.list_events(args),
            "create_event" => self.create_event(args),
            "update_event" => self.update_event(args),
            "delete_event" => self.delete_event(args),
            _ => return Err((-32602, format!("Unknown tool: {name}"))),
        };
        let (text, is_error) = match outcome {
            Ok(text) => (text, false),
            Err(message) => (message, true),
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
    }

    // ----- data access ----------------------------------------------------

    fn load(&self) -> Result<Loaded, String> {
        self.store.load().map_err(db_error)
    }

    fn save_page(&self, page: &Page) -> Result<(), String> {
        self.store.save_page(page).map_err(db_error)
    }

    /// Save a page's blocks and mark the page as edited now.
    fn save_blocks(&self, page_id: &str, blocks: &[Block]) -> Result<(), String> {
        self.store.save_blocks_at(page_id, blocks, now_ms()).map_err(db_error)
    }

    // ----- tools ------------------------------------------------------------

    fn search_pages(&self, args: &Value) -> ToolResult {
        let query = arg_str(args, "query").unwrap_or("").trim().to_lowercase();
        let limit = arg_u64(args, "limit")?.unwrap_or(10).clamp(1, 50) as usize;
        let (pages, blocks) = self.load()?;
        let terms: Vec<&str> = query.split_whitespace().collect();

        let mut hits: Vec<(i64, i64, Value)> = Vec::new();
        for page in pages.values().filter(|p| !p.trashed) {
            let title = page.display_title().to_lowercase();
            let page_blocks = blocks.get(&page.id).map(Vec::as_slice).unwrap_or(&[]);
            let mut score = 0i64;
            let mut snippet = String::new();
            if !terms.is_empty() {
                let found_all = terms.iter().all(|t| {
                    title.contains(t) || page_blocks.iter().any(|b| b.text.to_lowercase().contains(t))
                });
                if !found_all {
                    continue;
                }
                if title == query {
                    score += 100;
                } else if title.contains(&query) {
                    score += 50;
                }
                score += 10 * terms.iter().filter(|t| title.contains(*t)).count() as i64;
                let mut content_hits = 0;
                for b in page_blocks {
                    let lower = b.text.to_lowercase();
                    let at = lower.find(&query).or_else(|| terms.iter().find_map(|t| lower.find(t)));
                    if let Some(at) = at {
                        content_hits += 1;
                        if snippet.is_empty() {
                            snippet = snippet_around(&b.text, &lower, at);
                        }
                    }
                }
                score += 2 * content_hits.min(10);
            }
            if snippet.is_empty() {
                snippet = page_blocks
                    .iter()
                    .find(|b| b.kind.is_editable() && !b.text.trim().is_empty())
                    .map(|b| truncate_chars(b.text.trim(), 140))
                    .unwrap_or_default();
            }
            let entry = json!({
                "id": page.id,
                "title": page.display_title(),
                "icon": page.icon,
                "path": path_of(&pages, &page.id),
                "snippet": snippet,
                "link": format!("{LINK_PREFIX}{}", page.id),
            });
            hits.push((score, page.updated_at, entry));
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let results: Vec<Value> = hits.into_iter().take(limit).map(|h| h.2).collect();
        Ok(pretty(&json!({ "count": results.len(), "results": results })))
    }

    fn list_pages(&self, args: &Value) -> ToolResult {
        let (pages, _) = self.load()?;
        let parent = optional_page_id(args, "parent_id")?;
        if let Some(pid) = &parent {
            live_page(&pages, pid)?;
        }
        let depth = arg_u64(args, "depth")?.unwrap_or(3).clamp(1, 10) as usize;

        fn node(pages: &HashMap<Id, Page>, page: &Page, depth: usize) -> Value {
            let kids = children_of(pages, Some(&page.id));
            let children: Vec<Value> =
                if depth > 1 { kids.iter().map(|k| node(pages, k, depth - 1)).collect() } else { Vec::new() };
            json!({
                "id": page.id,
                "title": page.display_title(),
                "icon": page.icon,
                "favorite": page.favorite,
                "child_count": kids.len(),
                "children": children,
            })
        }

        let roots: Vec<Value> =
            children_of(&pages, parent.as_deref()).iter().map(|p| node(&pages, p, depth)).collect();
        Ok(pretty(&json!({ "parent_id": parent, "depth": depth, "pages": roots })))
    }

    fn read_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, blocks) = self.load()?;
        let page = live_page(&pages, &id)?;
        let body = render_body(&pages, blocks.get(&id).map(Vec::as_slice).unwrap_or(&[]));
        let header = format!(
            "---\nid: {}\ntitle: {}\nicon: {}\nparent_id: {}\npath: {}\nfavorite: {}\nupdated: {}\nlink: {LINK_PREFIX}{}\n---\n\n",
            page.id,
            quoted(page.display_title()),
            page.icon.as_deref().unwrap_or("none"),
            page.parent_id.as_deref().unwrap_or("none (top level)"),
            quoted(&path_of(&pages, &page.id)),
            page.favorite,
            iso8601(page.updated_at),
            page.id,
        );
        Ok(header + &body)
    }

    fn create_page(&self, args: &Value) -> ToolResult {
        let title = required_str(args, "title")?.trim().to_string();
        let parent = optional_page_id(args, "parent_id")?;
        let icon = arg_str(args, "icon").map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let (pages, blocks) = self.load()?;
        if let Some(pid) = &parent {
            live_page(&pages, pid)?;
        }
        let markdown = strip_title_heading(arg_str(args, "markdown").unwrap_or(""), &title);

        let mut page = Page::new(parent.clone(), next_position(&pages, parent.as_deref()));
        // A sub-page joins its parent's space; a top-level page joins the space
        // the app last showed (empty falls back to the first space on load).
        page.space_id = match &parent {
            Some(pid) => pages.get(pid).map(|p| p.space_id.clone()).unwrap_or_default(),
            None => self.store.get_setting("space").unwrap_or_default(),
        };
        page.title = title;
        page.icon = icon;
        let body = non_empty(parse_body(&pages, &markdown));
        self.save_page(&page)?;
        self.save_blocks(&page.id, &body)?;

        // Like a sub-page made in the app, the new page appears in its parent's body.
        if let Some(pid) = &parent {
            let mut parent_blocks = blocks.get(pid).cloned().unwrap_or_default();
            if is_blank(&parent_blocks) {
                parent_blocks.clear();
            }
            parent_blocks.push(Block::page_link(page.id.clone()));
            self.save_blocks(pid, &parent_blocks)?;
        }
        Ok(pretty(&json!({
            "id": page.id,
            "title": page.display_title(),
            "parent_id": parent,
            "blocks": body.len(),
            "link": format!("{LINK_PREFIX}{}", page.id),
        })))
    }

    fn append_to_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let markdown = required_str(args, "markdown")?;
        let (pages, blocks) = self.load()?;
        live_page(&pages, &id)?;
        let added = parse_body(&pages, markdown);
        if added.is_empty() {
            return Err("Nothing to append: the markdown is empty.".into());
        }
        // Only the new blocks are written, after the ones there: the page
        // is not written again as a whole, so nothing typed into it in the
        // app at the same moment is lost.
        let _ = blocks;
        let count = added.len();
        let total = self.store.append_blocks(&id, &added, now_ms()).map_err(db_error)?;
        Ok(pretty(&json!({ "page_id": id, "appended_blocks": count, "total_blocks": total })))
    }

    fn replace_page_content(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let markdown = required_str(args, "markdown")?;
        let (pages, blocks) = self.load()?;
        let mut page = live_page(&pages, &id)?.clone();
        // Keep the page as it was, so the change can be undone in Page history.
        let old = blocks.get(&id).map(|b| b.as_slice()).unwrap_or_default();
        self.store.save_version(&id, now_ms(), &page.title, old).map_err(db_error)?;
        if let Some(title) = arg_str(args, "title") {
            page.title = title.trim().to_string();
            page.updated_at = now_ms();
            self.save_page(&page)?;
        }
        let body = non_empty(keep_media(old, parse_body(&pages, &strip_title_heading(markdown, &page.title))));
        self.save_blocks(&id, &body)?;
        Ok(pretty(&json!({ "page_id": id, "title": page.display_title(), "blocks": body.len() })))
    }

    fn update_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, _) = self.load()?;
        let mut page = live_page(&pages, &id)?.clone();
        let mut changed = Vec::new();

        if let Some(title) = arg_str(args, "title") {
            page.title = title.trim().to_string();
            changed.push("title");
        }
        if let Some(icon) = args.get("icon") {
            page.icon = match icon {
                Value::Null => None,
                Value::String(s) if s.trim().is_empty() => None,
                Value::String(s) => Some(s.trim().to_string()),
                _ => return Err("`icon` must be an emoji string, or empty to remove it.".into()),
            };
            changed.push("icon");
        }
        if let Some(fav) = args.get("favorite") {
            page.favorite = fav.as_bool().ok_or("`favorite` must be true or false.")?;
            changed.push("favorite");
        }
        if let Some(parent) = args.get("parent_id") {
            let new_parent = match parent {
                Value::Null => None,
                Value::String(s) if s.trim().is_empty() => None,
                Value::String(s) => Some(normalize_id(s)),
                _ => return Err("`parent_id` must be a page id, or empty to move the page to the top level.".into()),
            };
            if let Some(np) = &new_parent {
                live_page(&pages, np)?;
                if descendants(&pages, &id).contains(np) {
                    return Err("A page can't be moved inside itself or one of its sub-pages.".into());
                }
            }
            if new_parent != page.parent_id {
                page.position = next_position(&pages, new_parent.as_deref());
                page.parent_id = new_parent;
            }
            changed.push("parent_id");
        }
        if changed.is_empty() {
            return Err("Nothing to update: pass title, icon, favorite or parent_id.".into());
        }
        page.updated_at = now_ms();
        self.save_page(&page)?;
        Ok(pretty(&json!({
            "page_id": id,
            "updated": changed,
            "title": page.display_title(),
            "icon": page.icon,
            "favorite": page.favorite,
            "parent_id": page.parent_id,
        })))
    }

    fn trash_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, _) = self.load()?;
        let page = pages.get(&id).ok_or_else(|| not_found(&id))?;
        if page.trashed {
            return Ok(format!("\"{}\" is already in the Trash.", page.display_title()));
        }
        let ids = descendants(&pages, &id);
        let now = now_ms();
        let mut count = 0;
        for pid in &ids {
            if let Some(p) = pages.get(pid).filter(|p| !p.trashed) {
                let mut p = p.clone();
                p.trashed = true;
                p.updated_at = now;
                self.save_page(&p)?;
                count += 1;
            }
        }
        Ok(pretty(&json!({
            "trashed_pages": count,
            "title": page.display_title(),
            "note": "Moved to the Trash (not deleted). Undo with restore_page or from the app's Trash.",
        })))
    }

    // ----- calendar --------------------------------------------------------

    /// An event for an agent. `first` is the first day of its repeating
    /// event (for words like "Every week on Tuesday").
    fn event_json(&self, e: &crate::model::Event, first: chrono::NaiveDate) -> Value {
        let end_shown = if e.all_day { e.end - 1 } else { e.end };
        json!({
            "id": e.id,
            "title": e.display_title(),
            "start": crate::when::iso(e.start, e.all_day),
            "end": crate::when::iso(end_shown, e.all_day),
            "all_day": e.all_day,
            "color": e.color,
            "notes": e.notes,
            "notes_page": e.page_id.as_ref().map(|p| format!("notion2://page/{p}")),
            "reminder_minutes": e.remind,
            "repeats": e.repeat.as_ref().map(|r| crate::repeat::describe(r, first)),
        })
    }

    /// The stored event an id names. A repeat's id ("<id>@<day>") names its
    /// repeating event.
    fn find_event(&self, id: &str) -> Result<crate::model::Event, String> {
        let (series, _) = crate::repeat::split_id(id.trim());
        self.store
            .load_events()
            .map_err(db_error)?
            .into_iter()
            .find(|e| e.id == series)
            .ok_or_else(|| format!("No event with id {id}. Use list_events to find event ids."))
    }

    /// `repeat` and `repeat_until` from the arguments, applied to `e`.
    fn set_repeat(e: &mut crate::model::Event, args: &Value) -> Result<(), String> {
        if let Some(v) = args.get("repeat") {
            let word = v.as_str().unwrap_or("none");
            let rule = crate::repeat::parse(word).map_err(|_| {
                format!("Couldn't read repeat \"{word}\". Use none, daily, weekdays, weekly, biweekly, monthly or yearly.")
            })?;
            e.repeat = rule.map(|mut r| {
                if let Some(old) = &e.repeat {
                    r.until = old.until.clone();
                    r.skip = old.skip.clone();
                }
                r
            });
        }
        if let Some(v) = args.get("repeat_until") {
            let until = match v.as_str().filter(|s| !s.trim().is_empty()) {
                None => None,
                Some(s) => Some(
                    crate::when::parse_iso(s)
                        .map(|(t, _)| crate::when::date_value(crate::when::date_of(t)))
                        .ok_or_else(|| format!("Couldn't read repeat_until \"{s}\". Use a date like 2026-12-20."))?,
                ),
            };
            if let Some(r) = &mut e.repeat {
                r.until = until;
            }
        }
        Ok(())
    }

    fn list_events(&self, args: &Value) -> ToolResult {
        let today = crate::when::today();
        let from = match arg_str(args, "from") {
            Some(s) => crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read the date \"{s}\". Use 2026-11-10 or 2026-11-10T14:00."))?.0,
            None => crate::when::midnight(today),
        };
        let to = match arg_str(args, "to") {
            Some(s) => {
                let (t, day) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read the date \"{s}\"."))?;
                if day {
                    crate::when::midnight(crate::when::next_day(crate::when::date_of(t)))
                } else {
                    t
                }
            }
            None => from + 30 * 24 * crate::when::HOUR,
        };
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).min(200) as usize;
        let stored = self.store.load_events().map_err(db_error)?;
        let firsts: HashMap<&str, chrono::NaiveDate> =
            stored.iter().map(|e| (e.id.as_str(), crate::when::date_of(e.start))).collect();
        let mut shown = crate::repeat::expand(&stored, from, to);
        shown.sort_by_key(|e| e.start);
        let list: Vec<Value> = shown
            .iter()
            .take(limit)
            .map(|e| {
                let first = firsts.get(crate::repeat::split_id(&e.id).0).copied().unwrap_or_else(|| crate::when::date_of(e.start));
                self.event_json(e, first)
            })
            .collect();
        Ok(pretty(&json!({
            "from": crate::when::iso(from, false),
            "to": crate::when::iso(to, false),
            "count": list.len(),
            "events": list,
            "timezone": "times are local to this Mac",
        })))
    }

    fn create_event(&self, args: &Value) -> ToolResult {
        let title = required_str(args, "title")?.trim().to_string();
        let start_s = required_str(args, "start")?;
        let (start, day) = crate::when::parse_iso(start_s).ok_or_else(|| format!("Couldn't read start \"{start_s}\". Use 2026-11-10 for an all-day event or 2026-11-10T14:00 for a time."))?;
        let all_day = args.get("all_day").and_then(Value::as_bool).unwrap_or(day);
        let end = match arg_str(args, "end") {
            Some(s) => {
                let (t, end_day) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read end \"{s}\"."))?;
                if all_day || end_day {
                    crate::when::midnight(crate::when::next_day(crate::when::date_of(t)))
                } else {
                    t
                }
            }
            None if all_day => crate::when::midnight(crate::when::next_day(crate::when::date_of(start))),
            None => start + crate::when::HOUR,
        };
        let start = if all_day { crate::when::midnight(crate::when::date_of(start)) } else { start };
        let space = self.store.get_setting("space").unwrap_or_default();
        let mut e = crate::model::Event::new(space, title, start, end, all_day);
        if let Some(n) = arg_str(args, "notes") {
            e.notes = n.to_string();
        }
        if let Some(c) = arg_str(args, "color").filter(|c| crate::ui::icons::ICON_COLORS.iter().any(|(id, _)| id == c)) {
            e.color = c.to_string();
        }
        e.remind = match args.get("reminder_minutes") {
            Some(Value::Null) => None,
            Some(v) => v.as_i64().filter(|m| *m >= 0),
            None => match self.store.get_setting("default_reminder") {
                Some(v) => crate::notify::parse_reminder(&v),
                None => Some(10),
            },
        };
        Self::set_repeat(&mut e, args)?;
        self.store.save_event(&e).map_err(db_error)?;
        Ok(pretty(&json!({ "created": self.event_json(&e, crate::when::date_of(e.start)) })))
    }

    fn update_event(&self, args: &Value) -> ToolResult {
        let id = required_str(args, "event_id")?;
        let mut series = self.find_event(id)?;
        // A change to one repeat goes to them all (moved by the same days and
        // time), as in the app.
        let day = crate::repeat::split_id(id.trim()).1.filter(|_| series.repeat.is_some());
        let mut e = match day {
            Some(d) => crate::repeat::at(&series, d),
            None => series.clone(),
        };
        if let Some(t) = arg_str(args, "title") {
            e.title = t.trim().to_string();
        }
        if let Some(n) = arg_str(args, "notes") {
            e.notes = n.to_string();
        }
        if let Some(c) = arg_str(args, "color").filter(|c| crate::ui::icons::ICON_COLORS.iter().any(|(id, _)| id == c)) {
            e.color = c.to_string();
        }
        if let Some(all_day) = args.get("all_day").and_then(Value::as_bool) {
            e.all_day = all_day;
        }
        match args.get("reminder_minutes") {
            Some(Value::Null) => e.remind = None,
            Some(v) => e.remind = v.as_i64().filter(|m| *m >= 0),
            None => {}
        }
        let duration = e.end - e.start;
        if let Some(s) = arg_str(args, "start") {
            let (t, _) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read start \"{s}\"."))?;
            e.start = if e.all_day { crate::when::midnight(crate::when::date_of(t)) } else { t };
            e.end = e.start + duration;
        }
        if let Some(s) = arg_str(args, "end") {
            let (t, day) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read end \"{s}\"."))?;
            e.end = if e.all_day || day { crate::when::midnight(crate::when::next_day(crate::when::date_of(t))) } else { t };
        }
        if e.end <= e.start {
            e.end = e.start + if e.all_day { 24 * crate::when::HOUR } else { crate::when::HOUR };
        }
        Self::set_repeat(&mut e, args)?;
        let e = match day {
            Some(d) => {
                let before = crate::repeat::at(&series, d);
                crate::repeat::apply(&mut series, &before, &e);
                series
            }
            None => e,
        };
        let mut e = e;
        e.updated_at = now_ms();
        self.store.save_event(&e).map_err(db_error)?;
        Ok(pretty(&json!({ "updated": self.event_json(&e, crate::when::date_of(e.start)) })))
    }

    fn delete_event(&self, args: &Value) -> ToolResult {
        let id = required_str(args, "event_id")?;
        let mut e = self.find_event(id)?;
        let all = args.get("all_repeats").and_then(Value::as_bool).unwrap_or(false);
        if let (Some(day), Some(rule), false) = (crate::repeat::split_id(id.trim()).1, e.repeat.as_mut(), all) {
            rule.skip.push(crate::when::date_value(day));
            e.updated_at = now_ms();
            self.store.save_event(&e).map_err(db_error)?;
            return Ok(format!("Deleted the {} repeat of \"{}\"; the other repeats stay.", day.format("%b %-d"), e.display_title()));
        }
        self.store.delete_event(&e.id).map_err(db_error)?;
        Ok(format!("Deleted the event \"{}\".", e.display_title()))
    }

    fn restore_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, _) = self.load()?;
        let page = pages.get(&id).ok_or_else(|| not_found(&id))?;
        if !page.trashed {
            return Ok(format!("\"{}\" is not in the Trash.", page.display_title()));
        }
        let parent_gone = page
            .parent_id
            .as_ref()
            .is_some_and(|parent| pages.get(parent).is_none_or(|pp| pp.trashed));
        let now = now_ms();
        let ids = descendants(&pages, &id);
        for pid in &ids {
            if let Some(p) = pages.get(pid) {
                let mut p = p.clone();
                p.trashed = false;
                p.updated_at = now;
                if parent_gone && p.id == id {
                    p.parent_id = None;
                }
                self.save_page(&p)?;
            }
        }
        Ok(pretty(&json!({
            "restored_pages": ids.len(),
            "title": page.display_title(),
            "moved_to_top_level": parent_gone,
        })))
    }
}

// ---------------------------------------------------------------------------
// Protocol pieces
// ---------------------------------------------------------------------------

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = if SUPPORTED_VERSIONS.contains(&requested) { requested } else { SUPPORTED_VERSIONS[0] };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "notion2", "title": "Rune", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_definitions() -> Value {
    let page_id = json!({ "type": "string", "description": "Page id (or a notion2://page/<id> link)." });
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    let write = |idempotent: bool| {
        json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": idempotent, "openWorldHint": false })
    };
    json!([
        {
            "name": "search_pages",
            "title": "Search pages",
            "description": "Search the workspace by page title and content (case-insensitive; every word must appear). Returns page ids, titles, their path of parent pages and a snippet. An empty query returns the most recently edited pages.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Most results to return (default 10)." }
                },
                "required": ["query"]
            },
            "annotations": read_only
        },
        {
            "name": "list_pages",
            "title": "List pages",
            "description": "List the page tree: top-level pages, or the sub-pages of parent_id, nested `depth` levels deep (default 3). Trashed pages are left out.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "parent_id": page_id,
                    "depth": { "type": "integer", "minimum": 1, "maximum": 10, "description": "Levels of sub-pages to include (default 3)." }
                }
            },
            "annotations": read_only
        },
        {
            "name": "read_page",
            "title": "Read a page",
            "description": "Read a page as Markdown, after a short header with its id, title, icon, parent and path. Links to other pages appear as [Title](notion2://page/<id>) on their own line.",
            "inputSchema": {
                "type": "object",
                "properties": { "page_id": page_id },
                "required": ["page_id"]
            },
            "annotations": read_only
        },
        {
            "name": "create_page",
            "title": "Create a page",
            "description": "Create a page with a title and optional Markdown content. With parent_id it becomes a sub-page and a link to it is added to the end of the parent page. Returns the new page's id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Page title." },
                    "markdown": { "type": "string", "description": "Page content as Markdown (don't repeat the title as a heading)." },
                    "parent_id": { "type": "string", "description": "Optional parent page id; omit for a top-level page." },
                    "icon": { "type": "string", "description": "Optional emoji icon, e.g. 📝." }
                },
                "required": ["title"]
            },
            "annotations": write(false)
        },
        {
            "name": "append_to_page",
            "title": "Append to a page",
            "description": "Add Markdown content to the end of a page, keeping what is already there.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "markdown": { "type": "string", "description": "Content to add, as Markdown." }
                },
                "required": ["page_id", "markdown"]
            },
            "annotations": write(false)
        },
        {
            "name": "replace_page_content",
            "title": "Replace a page's content",
            "description": "Replace everything in a page's body with new Markdown (optionally renaming it). Read the page first and keep its [Title](notion2://page/<id>) link lines to keep links to sub-pages; sub-pages themselves are never removed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "markdown": { "type": "string", "description": "The page's full new content, as Markdown." },
                    "title": { "type": "string", "description": "Optional new title." }
                },
                "required": ["page_id", "markdown"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "update_page",
            "title": "Update page details",
            "description": "Change a page's title, icon or favorite flag, or move it under another page (parent_id; empty string moves it to the top level). Content is unchanged.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "title": { "type": "string" },
                    "icon": { "type": "string", "description": "Emoji, or empty to remove the icon." },
                    "favorite": { "type": "boolean" },
                    "parent_id": { "type": "string", "description": "New parent page id, or empty for top level." }
                },
                "required": ["page_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "trash_page",
            "title": "Move a page to the Trash",
            "description": "Move a page and its sub-pages to the Trash. This is reversible (restore_page); pages are never permanently deleted by this server.",
            "inputSchema": {
                "type": "object",
                "properties": { "page_id": page_id },
                "required": ["page_id"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "restore_page",
            "title": "Restore a page from the Trash",
            "description": "Bring a trashed page and its sub-pages back. If its parent is gone it returns at the top level.",
            "inputSchema": {
                "type": "object",
                "properties": { "page_id": page_id },
                "required": ["page_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "list_events",
            "title": "List calendar events",
            "description": "Events in the Rune calendar between two dates (default: the next 30 days). Times are local to this Mac, written like 2026-11-10T14:00; all-day events use plain dates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Start of the range: 2026-11-10 or 2026-11-10T09:00. Default: today." },
                    "to": { "type": "string", "description": "End of the range (inclusive for a plain date). Default: 30 days after from." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                }
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "create_event",
            "title": "Add a calendar event",
            "description": "Add an event to the calendar. Give start as 2026-11-10T14:00 for a timed event (an hour long unless end is given) or 2026-11-10 for an all-day event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "start": { "type": "string", "description": "2026-11-10T14:00 (local time) or 2026-11-10 (all day)" },
                    "end": { "type": "string", "description": "Optional end, same format. For all-day events, the last day." },
                    "all_day": { "type": "boolean" },
                    "notes": { "type": "string" },
                    "color": { "type": "string", "enum": ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"] },
                    "reminder_minutes": { "type": ["integer", "null"], "minimum": 0, "description": "Minutes before the start for a Mac notification (0 = at the start; null = none). Default: the app's setting, normally 10." },
                    "repeat": { "type": "string", "enum": ["none", "daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"], "description": "How the event repeats, counted from its start. Default: none." },
                    "repeat_until": { "type": ["string", "null"], "description": "The last day it may repeat on, like 2026-12-20. Default: no end." }
                },
                "required": ["title", "start"]
            },
            "annotations": write(false)
        },
        {
            "name": "update_event",
            "title": "Change a calendar event",
            "description": "Change an event's title, time, notes, colour or how it repeats. Moving the start keeps the length unless end is also given. Given one repeat of a repeating event (an id ending in @<date>), the change goes to every repeat, moved by the same amount.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": { "type": "string" },
                    "title": { "type": "string" },
                    "start": { "type": "string" },
                    "end": { "type": "string" },
                    "all_day": { "type": "boolean" },
                    "notes": { "type": "string" },
                    "color": { "type": "string", "enum": ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"] },
                    "reminder_minutes": { "type": ["integer", "null"], "minimum": 0 },
                    "repeat": { "type": "string", "enum": ["none", "daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"] },
                    "repeat_until": { "type": ["string", "null"], "description": "The last day it may repeat on (null: no end)." }
                },
                "required": ["event_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "delete_event",
            "title": "Delete a calendar event",
            "description": "Remove an event from the calendar for good. Given one repeat of a repeating event (an id ending in @<date>), only that repeat is removed unless all_repeats is true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": { "type": "string" },
                    "all_repeats": { "type": "boolean", "description": "With a repeat's id: remove every repeat. Default: false." }
                },
                "required": ["event_id"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        }
    ])
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn db_error(e: rusqlite::Error) -> String {
    format!("Workspace database error: {e}")
}

fn not_found(id: &str) -> String {
    format!("No page with id {id}. Use search_pages or list_pages to find page ids.")
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(format!("`{key}` must be a string.")),
        None => Err(format!("Missing required argument `{key}`.")),
    }
}

fn arg_u64(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be a positive whole number.")),
    }
}

/// Accept a bare id or a notion2://page/<id> link.
fn normalize_id(raw: &str) -> String {
    let s = raw.trim();
    s.strip_prefix(LINK_PREFIX).unwrap_or(s).trim_end_matches('/').to_string()
}

fn required_page_id(args: &Value, key: &str) -> Result<String, String> {
    let id = normalize_id(required_str(args, key)?);
    if id.is_empty() {
        return Err(format!("`{key}` can't be empty."));
    }
    Ok(id)
}

fn optional_page_id(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(normalize_id(s))),
        Some(_) => Err(format!("`{key}` must be a page id string.")),
    }
}

/// The page, if it exists and is not in the Trash.
fn live_page<'a>(pages: &'a HashMap<Id, Page>, id: &str) -> Result<&'a Page, String> {
    let page = pages.get(id).ok_or_else(|| not_found(id))?;
    if page.trashed {
        return Err(format!(
            "\"{}\" ({id}) is in the Trash. Restore it with restore_page first.",
            page.display_title()
        ));
    }
    Ok(page)
}

fn path_of(pages: &HashMap<Id, Page>, id: &str) -> String {
    let mut parts: Vec<String> = ancestors(pages, id).iter().map(|p| p.display_title().to_string()).collect();
    if let Some(p) = pages.get(id) {
        parts.push(p.display_title().to_string());
    }
    parts.join(" / ")
}

fn next_position(pages: &HashMap<Id, Page>, parent: Option<&str>) -> f64 {
    children_of(pages, parent).last().map(|p| p.position + 1.0).unwrap_or(1.0)
}

/// A page whose only block is an empty paragraph (what a new page starts with).
fn is_blank(blocks: &[Block]) -> bool {
    blocks.is_empty() || (blocks.len() == 1 && blocks[0].kind == BlockKind::Text && blocks[0].text.trim().is_empty())
}

fn non_empty(blocks: Vec<Block>) -> Vec<Block> {
    if blocks.is_empty() {
        vec![Block::text("")]
    } else {
        blocks
    }
}

/// Agents often start content with the page title as a heading; the title is
/// already shown above the page, so drop that one heading.
pub(crate) fn strip_title_heading(markdown: &str, title: &str) -> String {
    let trimmed = markdown.trim_start();
    if let Some(first) = trimmed.lines().next() {
        let heading = first.trim_start_matches('#');
        if first.starts_with('#') && heading.starts_with(' ') && !title.is_empty() && heading.trim().eq_ignore_ascii_case(title.trim()) {
            return trimmed[first.len()..].trim_start_matches(['\r', '\n']).to_string();
        }
    }
    markdown.to_string()
}

/// Markdown → blocks. A line that is only a link to an existing page becomes a
/// page link block, so read → edit → write round-trips keep sub-page links.
/// The address a picture or drawing has in a page's Markdown.
const BLOCK_PREFIX: &str = "notion2://block/";

/// `[Image](notion2://block/<id>)` → `<id>`.
fn media_token(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix('[')?;
    let (_, rest) = rest.split_once("](")?;
    let id = rest.strip_prefix(BLOCK_PREFIX)?.strip_suffix(')')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(id)
}

/// A page's pictures and drawings cannot be written as Markdown. When new
/// Markdown takes the place of a page, each line that names one of them
/// (`[Image](notion2://block/<id>)`) becomes that picture or drawing again,
/// and those the Markdown does not name are kept at the end of the page:
/// replacing a page never loses a picture or a drawing.
pub(crate) fn keep_media(old: &[Block], new: Vec<Block>) -> Vec<Block> {
    let is_media = |b: &Block| matches!(b.kind, BlockKind::Image | BlockKind::Drawing);
    let mut used: Vec<&str> = Vec::new();
    let mut out: Vec<Block> = new
        .into_iter()
        .map(|b| {
            let found = (b.kind == BlockKind::Text).then(|| media_token(&crate::inline::plain(&b.text)).or_else(|| media_token(&b.text)).map(str::to_string)).flatten();
            match found.and_then(|id| old.iter().find(|o| o.id == id && is_media(o))) {
                Some(original) if !used.contains(&original.id.as_str()) => {
                    used.push(original.id.as_str());
                    let mut kept = original.clone();
                    kept.indent = b.indent;
                    kept
                }
                _ => b,
            }
        })
        .collect();
    out.extend(old.iter().filter(|o| is_media(o) && !used.contains(&o.id.as_str())).cloned());
    out
}

pub(crate) fn parse_body(pages: &HashMap<Id, Page>, markdown: &str) -> Vec<Block> {
    markdown::to_blocks(markdown)
        .into_iter()
        .map(|b| match page_link_target(&b.text) {
            Some(target) if b.kind == BlockKind::Text && pages.contains_key(&target) => {
                Block::page_link(target).with_indent(b.indent)
            }
            _ => b,
        })
        .collect()
}

/// `[Title](notion2://page/<id>)` → `<id>`.
fn page_link_target(text: &str) -> Option<Id> {
    let t = text.trim();
    if !t.starts_with('[') || !t.ends_with(')') {
        return None;
    }
    let at = t.rfind(&format!("]({LINK_PREFIX}"))?;
    let id = &t[at + 2 + LINK_PREFIX.len()..t.len() - 1];
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// Blocks → Markdown, with # / ## / ### headings (the title is reported
/// separately) and page links as `[Title](notion2://page/<id>)`.
pub(crate) fn render_body(pages: &HashMap<Id, Page>, blocks: &[Block]) -> String {
    let as_text = |b: &Block, text: String| {
        let mut t = Block::text(text);
        t.indent = b.indent;
        t
    };
    let converted: Vec<Block> = blocks
        .iter()
        .map(|b| match b.kind {
            BlockKind::Heading1 => as_text(b, format!("# {}", b.text)),
            BlockKind::Heading2 => as_text(b, format!("## {}", b.text)),
            BlockKind::Heading3 => as_text(b, format!("### {}", b.text)),
            BlockKind::PageLink => {
                let target = b.target.clone().unwrap_or_default();
                let title = match pages.get(&target) {
                    Some(p) if p.trashed => format!("{} (in Trash)", p.display_title()),
                    Some(p) => p.display_title().to_string(),
                    None => "Missing page".to_string(),
                };
                let title = title.replace('[', "\\[").replace(']', "\\]");
                as_text(b, format!("[{title}]({LINK_PREFIX}{target})"))
            }
            // With the block's id, so that replace_page_content can put the
            // same picture or drawing back (see `keep_media`).
            BlockKind::Drawing => as_text(b, format!("[Drawing]({BLOCK_PREFIX}{})", b.id)),
            BlockKind::Image => as_text(b, format!("[Image]({BLOCK_PREFIX}{})", b.id)),
            BlockKind::Table => as_text(b, crate::table::Table::parse(&b.data).to_markdown("")),
            _ => b.clone(),
        })
        .collect();
    let md = markdown::from_blocks("", &converted, |_| String::new());
    if md.trim().is_empty() {
        "(This page is empty.)".to_string()
    } else {
        md
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// About 140 characters of `text` around the byte offset `at` in its lowercase form.
fn snippet_around(text: &str, lower: &str, at: usize) -> String {
    let char_at = lower[..at.min(lower.len())].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let start = char_at.saturating_sub(40);
    let end = (char_at + 100).min(chars.len());
    let mut out: String = chars[start.min(chars.len())..end].iter().collect::<String>().replace('\n', " ");
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Milliseconds since the Unix epoch → `2026-11-09T07:52:00Z`.
fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// What to give an AI tool so it can use this workspace: the connector is
/// this program started with `--mcp` (see Settings ▸ AI ▸ Connect AI tools).
pub mod connect {
    /// The name the tools list the connector under.
    pub const NAME: &str = "notion2";

    /// Where this program is (the app's own executable).
    pub fn program() -> String {
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "notion2".into())
    }

    fn shell_quoted(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    fn json_quoted(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
    }

    /// The command as typed in Terminal.
    pub fn command(program: &str) -> String {
        format!("{} --mcp", shell_quoted(program))
    }

    /// For Claude Code: run once in Terminal.
    pub fn claude_code(program: &str) -> String {
        format!("claude mcp add --scope user {NAME} -- {} --mcp", shell_quoted(program))
    }

    /// For Codex: lines for ~/.codex/config.toml.
    pub fn codex(program: &str) -> String {
        format!("[mcp_servers.{NAME}]\ncommand = {}\nargs = [\"--mcp\"]\n", json_quoted(program))
    }

    /// For the Claude app, Cursor, VS Code and most others: the JSON their
    /// MCP settings file takes.
    pub fn json(program: &str) -> String {
        format!("{{\n  \"mcpServers\": {{\n    \"{NAME}\": {{\n      \"command\": {},\n      \"args\": [\"--mcp\"]\n    }}\n  }}\n}}\n", json_quoted(program))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new(Store::open_in_memory().unwrap())
    }

    fn request(server: &mut Server, id: i64, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        let reply = server.handle_line(&line).expect("a reply");
        let v: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], id);
        v
    }

    /// Call a tool and return (text, is_error).
    fn call(server: &mut Server, name: &str, args: Value) -> (String, bool) {
        let v = request(server, 99, "tools/call", json!({ "name": name, "arguments": args }));
        let result = &v["result"];
        (result["content"][0]["text"].as_str().unwrap().to_string(), result["isError"].as_bool().unwrap())
    }

    fn json_of(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn an_older_connector_reads_but_does_not_change_newer_notes() {
        let mut s = server();
        let (made, failed) = call(&mut s, "create_page", json!({ "title": "Before" }));
        assert!(!failed, "{made}");
        s.store.set_file_schema(crate::store::SCHEMA + 1);
        let (text, failed) = call(&mut s, "create_page", json!({ "title": "After" }));
        assert!(failed && text.contains("older than the Rune app"), "{text}");
        let (text, failed) = call(&mut s, "list_pages", json!({}));
        assert!(!failed && text.contains("Before") && !text.contains("After"), "{text}");
    }

    #[test]
    fn initialize_negotiates_version_and_lists_tools() {
        let mut s = server();
        let v = request(&mut s, 1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }));
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(v["result"]["serverInfo"]["name"], "notion2");
        assert!(v["result"]["capabilities"]["tools"].is_object());
        // Unknown versions get our newest.
        let v = request(&mut s, 2, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(v["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);

        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert_eq!(request(&mut s, 3, "ping", Value::Null)["result"], json!({}));

        let tools = request(&mut s, 4, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in ["search_pages", "list_pages", "read_page", "create_page", "append_to_page", "replace_page_content", "update_page", "trash_page", "restore_page"] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        for t in tools["result"]["tools"].as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn protocol_errors() {
        let mut s = server();
        let v: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
        assert_eq!(v["error"]["code"], -32700);
        assert_eq!(v["id"], Value::Null);
        assert_eq!(request(&mut s, 5, "resources/list", json!({}))["error"]["code"], -32601);
        assert_eq!(request(&mut s, 6, "tools/call", json!({ "name": "nope" }))["error"]["code"], -32602);
        // Unknown notifications and stray responses get no reply.
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"whatever"}"#).is_none());
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#).is_none());
        // Bad arguments are tool errors the model can read and fix.
        let (text, err) = call(&mut s, "read_page", json!({}));
        assert!(err && text.contains("page_id"));
        let (text, err) = call(&mut s, "read_page", json!({ "page_id": "missing" }));
        assert!(err && text.contains("No page"));
    }

    #[test]
    fn calendar_events_round_trip() {
        let mut s = server();
        let (text, err) = call(&mut s, "create_event", json!({ "title": "Design review", "start": "2026-11-12T14:00", "notes": "Bring sketches", "color": "green" }));
        assert!(!err, "{text}");
        let created = &json_of(&text)["created"];
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["start"], "2026-11-12T14:00");
        assert_eq!(created["end"], "2026-11-12T15:00");

        let (text, err) = call(&mut s, "create_event", json!({ "title": "Offsite", "start": "2026-11-13", "end": "2026-11-14" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["created"]["all_day"], true);
        assert_eq!(json_of(&text)["created"]["end"], "2026-11-14");

        let (text, err) = call(&mut s, "list_events", json!({ "from": "2026-11-12", "to": "2026-11-13" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["count"], 2);

        let (text, err) = call(&mut s, "update_event", json!({ "event_id": id, "start": "2026-11-12T16:30" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["updated"]["end"], "2026-11-12T17:30", "moving keeps the length");

        let (text, err) = call(&mut s, "delete_event", json!({ "event_id": id }));
        assert!(!err, "{text}");
        let (text, _) = call(&mut s, "list_events", json!({ "from": "2026-11-12", "to": "2026-11-12" }));
        assert_eq!(json_of(&text)["count"], 0);
        let (_, err) = call(&mut s, "create_event", json!({ "title": "x", "start": "next tuesday" }));
        assert!(err, "unreadable dates are reported, not guessed");
    }

    #[test]
    fn connector_snippets_are_well_formed() {
        let program = "/Applications/Rune.app/Contents/MacOS/notion2";
        assert_eq!(connect::command(program), "'/Applications/Rune.app/Contents/MacOS/notion2' --mcp");
        assert_eq!(
            connect::claude_code(program),
            "claude mcp add --scope user notion2 -- '/Applications/Rune.app/Contents/MacOS/notion2' --mcp"
        );
        let parsed: Value = serde_json::from_str(&connect::json(program)).unwrap();
        assert_eq!(parsed["mcpServers"]["notion2"]["command"], program);
        assert_eq!(parsed["mcpServers"]["notion2"]["args"], json!(["--mcp"]));
        let toml = connect::codex(program);
        assert!(toml.starts_with("[mcp_servers.notion2]\ncommand = \"/Applications/Rune.app/Contents/MacOS/notion2\"\nargs = [\"--mcp\"]"), "{toml}");
        // A quote in the path cannot end the quoting early.
        assert_eq!(connect::command("/tmp/it's/notion2"), "'/tmp/it'\\''s/notion2' --mcp");
    }

    #[test]
    fn repeating_events_list_each_repeat() {
        let mut s = server();
        let (text, err) = call(&mut s, "create_event", json!({ "title": "Standup", "start": "2026-11-03T09:00", "repeat": "weekly", "repeat_until": "2026-11-24" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["created"]["repeats"], "Every week on Tuesday, until Nov 24, 2026");

        let (text, _) = call(&mut s, "list_events", json!({ "from": "2026-11-01", "to": "2026-11-30" }));
        let listed = json_of(&text);
        assert_eq!(listed["count"], 4, "{text}");
        let second = listed["events"][1]["id"].as_str().unwrap().to_string();
        assert!(second.ends_with("@2026-11-10"), "{second}");
        assert_eq!(listed["events"][1]["start"], "2026-11-10T09:00");

        // Moving one repeat moves them all.
        let (text, err) = call(&mut s, "update_event", json!({ "event_id": second, "start": "2026-11-10T10:00" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["updated"]["start"], "2026-11-03T10:00");

        // Deleting one repeat keeps the rest.
        let (text, err) = call(&mut s, "delete_event", json!({ "event_id": second }));
        assert!(!err, "{text}");
        let (text, _) = call(&mut s, "list_events", json!({ "from": "2026-11-01", "to": "2026-11-30" }));
        assert_eq!(json_of(&text)["count"], 3, "{text}");

        let (_, err) = call(&mut s, "update_event", json!({ "event_id": second, "repeat": "sometimes" }));
        assert!(err, "unknown repeat words are reported");
    }

    #[test]
    fn create_read_search_edit_and_trash_round_trip() {
        let mut s = server();
        let (text, err) = call(&mut s, "create_page", json!({
            "title": "Trip plan",
            "icon": "✈️",
            "markdown": "# Trip plan\n\nPack early.\n\n## Checklist\n\n- [ ] Passport\n- [x] Tickets\n\n```rust\nfn main() {}\n```"
        }));
        assert!(!err, "{text}");
        let parent = json_of(&text)["id"].as_str().unwrap().to_string();

        let (text, err) = call(&mut s, "create_page", json!({ "title": "Packing", "parent_id": format!("notion2://page/{parent}") }));
        assert!(!err, "{text}");
        let child = json_of(&text)["id"].as_str().unwrap().to_string();

        let (page, err) = call(&mut s, "read_page", json!({ "page_id": parent }));
        assert!(!err);
        assert!(page.contains("title: \"Trip plan\"") && page.contains("icon: ✈️"), "{page}");
        assert!(!page.contains("# Trip plan\n"), "title heading should be stripped: {page}");
        assert!(page.contains("## Checklist") && page.contains("- [ ] Passport") && page.contains("- [x] Tickets"), "{page}");
        assert!(page.contains("```rust\nfn main() {}\n```"), "{page}");
        let link = format!("[Packing](notion2://page/{child})");
        assert!(page.contains(&link), "{page}");

        // Writing back what was read keeps the sub-page link as a link block.
        let body = page.split("---\n\n").nth(1).unwrap().to_string();
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": parent, "markdown": format!("{body}\n\nAdded line.") }));
        assert!(!err);
        let (_, blocks) = s.store.load().unwrap();
        assert!(blocks[&parent].iter().any(|b| b.kind == BlockKind::PageLink && b.target.as_deref() == Some(child.as_str())));
        assert!(blocks[&parent].iter().any(|b| b.kind == BlockKind::Heading2 && b.text == "Checklist"));

        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": child, "markdown": "- socks\n- charger" }));
        assert!(!err);
        let (_, blocks) = s.store.load().unwrap();
        assert_eq!(blocks[&child].len(), 2, "blank starter block is replaced");

        let (text, _) = call(&mut s, "search_pages", json!({ "query": "charger" }));
        let found = json_of(&text);
        assert_eq!(found["results"][0]["id"], child.as_str());
        assert_eq!(found["results"][0]["path"], "Trip plan / Packing");
        assert!(found["results"][0]["snippet"].as_str().unwrap().contains("charger"));

        let (text, _) = call(&mut s, "list_pages", json!({}));
        let tree = json_of(&text);
        assert_eq!(tree["pages"][0]["children"][0]["id"], child.as_str());

        let (_, err) = call(&mut s, "update_page", json!({ "page_id": child, "title": "Packing list", "favorite": true, "parent_id": "" }));
        assert!(!err);
        let (pages, _) = s.store.load().unwrap();
        assert_eq!(pages[&child].title, "Packing list");
        assert!(pages[&child].favorite && pages[&child].parent_id.is_none());
        let (text, err) = call(&mut s, "update_page", json!({ "page_id": parent, "parent_id": parent }));
        assert!(err && text.contains("inside itself"));

        let (text, err) = call(&mut s, "trash_page", json!({ "page_id": parent }));
        assert!(!err && json_of(&text)["trashed_pages"] == 1);
        let (_, err) = call(&mut s, "read_page", json!({ "page_id": parent }));
        assert!(err);
        let (text, _) = call(&mut s, "search_pages", json!({ "query": "passport" }));
        assert_eq!(json_of(&text)["count"], 0);
        let (_, err) = call(&mut s, "restore_page", json!({ "page_id": parent }));
        assert!(!err);
        let (pages, _) = s.store.load().unwrap();
        assert!(!pages[&parent].trashed);
    }

    #[test]
    fn serve_reads_lines_and_answers_requests_only() {
        let mut s = server();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#, "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
            "\n",
            r#"{"jsonrpc":"2.0","id":"b","method":"tools/list"}"#, "\n",
        );
        let mut out = Vec::new();
        s.serve(io::Cursor::new(input), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["id"], "b");
    }

    #[test]
    fn helpers() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_794_239_520_000), "2026-11-09T15:52:00Z");
        assert_eq!(page_link_target("[A](notion2://page/abc123)"), Some("abc123".into()));
        assert_eq!(page_link_target("[A](https://x.com)"), None);
        assert_eq!(page_link_target("see [A](notion2://page/abc)"), None);
        assert_eq!(strip_title_heading("# Hello\n\nbody", "hello"), "body");
        assert_eq!(strip_title_heading("## Other\nbody", "hello"), "## Other\nbody");
        assert_eq!(normalize_id(" notion2://page/abc/ "), "abc");
        let text = "Some text with Needle inside";
        let lower = text.to_lowercase();
        assert!(snippet_around(text, &lower, lower.find("needle").unwrap()).contains("Needle"));
    }
}
