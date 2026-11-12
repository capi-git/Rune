//! AI features. They run through an AI tool already installed on this Mac
//! (Claude Code, then Codex), headless, with every tool switched off.
//!
//! Requests run on a dedicated Tokio runtime and stream text back to the UI over
//! a channel, so the interface stays responsive and a request can be stopped by
//! dropping the receiver.

pub mod local;

use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use serde::Serialize;
use std::sync::OnceLock;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Which AI answers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::ClaudeCode, Provider::Codex];

    pub fn id(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "claude",
            Provider::Codex => "codex",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Provider::ALL.into_iter().find(|p| p.id() == id.trim())
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    /// Settings key holding this provider's model name (blank = its default).
    pub fn model_setting(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "ai_model_claude",
            Provider::Codex => "ai_model_codex",
        }
    }
}

/// Settings key for the chosen provider; absent means "pick automatically".
pub const PROVIDER_SETTING: &str = "ai_provider";
pub const EFFORT_SETTING: &str = "ai_effort";

/// How hard the AI thinks before it answers: what is kept in settings, its
/// name, a few words on it, and how many of four dots stand for it. (The
/// names and words are Aven's.) Empty: the tool's own setting.
pub const EFFORTS: [(&str, &str, &str, u8); 5] = [
    ("", "Default", "the tool's own", 0),
    ("low", "Low", "quick", 1),
    ("medium", "Medium", "balanced", 2),
    ("high", "High", "thinks more", 3),
    ("max", "Max", "thinks longest", 4),
];

/// How many of four dots an effort has (none for the tool's own setting).
pub fn effort_dots(effort: &str) -> u8 {
    EFFORTS.iter().find(|(id, ..)| *id == effort).map(|e| e.3).unwrap_or(0)
}

/// AI settings, read on the UI thread (the store lives there) and handed to
/// the request.
#[derive(Clone, Debug, Default)]
pub struct Prefs {
    pub provider: Option<Provider>,
    pub claude_model: String,
    pub codex_model: String,
    /// One of `EFFORTS` (empty: the tool's own setting).
    pub effort: String,
}

impl Prefs {
    pub fn load() -> Self {
        let get = |k: &str| crate::store::setting(k).map(|s| s.trim().to_string()).unwrap_or_default();
        Prefs {
            provider: crate::store::setting(PROVIDER_SETTING).and_then(|s| Provider::from_id(&s)),
            claude_model: normalize_model(Provider::ClaudeCode, &get(Provider::ClaudeCode.model_setting())),
            codex_model: get(Provider::Codex.model_setting()),
            effort: Some(get(EFFORT_SETTING)).filter(|e| EFFORTS.iter().any(|(id, ..)| id == e)).unwrap_or_default(),
        }
    }

    pub fn model(&self, provider: Provider) -> &str {
        match provider {
            Provider::ClaudeCode => &self.claude_model,
            Provider::Codex => &self.codex_model,
        }
    }
}

/// Claude's current models: (full name passed to Claude Code, short name, label).
const CLAUDE_MODELS: [(&str, &str, &str); 4] = [
    ("claude-fable-5-1", "fable", "Fable 5.1"),
    ("claude-opus-5-5", "opus", "Opus 5.5"),
    ("claude-sonnet-5-5", "sonnet", "Sonnet 5.5"),
    ("claude-haiku-5-5", "haiku", "Haiku 5.5"),
];

/// Models offered in the pickers: (value passed to the tool, label with its
/// version). The first is the tool's own default, named after the model it
/// is set to. Codex's list is the one it keeps for your account
/// (~/.codex/models_cache.json), so it stays current as Codex updates.
pub fn model_choices(provider: Provider) -> Vec<(String, String)> {
    let default = match default_model_label(provider) {
        Some(label) => format!("Default · {label}"),
        None => "Default".to_string(),
    };
    let mut out = vec![(String::new(), default)];
    match provider {
        Provider::ClaudeCode => out.extend(CLAUDE_MODELS.iter().map(|(id, _, label)| (id.to_string(), label.to_string()))),
        Provider::Codex => out.extend(codex_models()),
    }
    out
}

/// "Opus 5.5" for "opus", "claude-opus-5-5" or "Opus"; a 1M-context variant
/// (written "fable[1m]") is labelled as such.
fn claude_label(model: &str) -> Option<String> {
    let m = model.trim().to_ascii_lowercase();
    let (base, wide) = match m.strip_suffix("[1m]") {
        Some(b) => (b.to_string(), true),
        None => (m.clone(), false),
    };
    let (_, _, label) = CLAUDE_MODELS.iter().find(|(id, short, label)| *id == base || *short == base || label.to_ascii_lowercase() == base)?;
    Some(if wide { format!("{label} (1M context)") } else { label.to_string() })
}

/// The model each tool uses when none is chosen here, from its own settings.
fn default_model_label(provider: Provider) -> Option<String> {
    match provider {
        Provider::ClaudeCode => {
            let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(std::path::PathBuf::from).or_else(|| dirs::home_dir().map(|h| h.join(".claude")))?;
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).ok()?).ok()?;
            let model = v["model"].as_str()?;
            claude_label(model).or_else(|| Some(model.to_string()))
        }
        Provider::Codex => {
            let text = std::fs::read_to_string(codex_home()?.join("config.toml")).ok()?;
            let slug = text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == "model").then(|| v.trim().trim_matches('"').to_string())
            })?;
            Some(codex_models().into_iter().find(|(id, _)| *id == slug).map(|(_, label)| label).unwrap_or(slug))
        }
    }
}

fn codex_home() -> Option<std::path::PathBuf> {
    std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from).or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
}

fn codex_models() -> Vec<(String, String)> {
    let Some(text) = codex_home().and_then(|h| std::fs::read_to_string(h.join("models_cache.json")).ok()) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    v["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter(|m| m["visibility"].as_str() == Some("list"))
                .filter_map(|m| {
                    let slug = m["slug"].as_str()?;
                    Some((slug.to_string(), m["display_name"].as_str().unwrap_or(slug).to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A model as typed by hand ("Opus") or chosen before ("opus") in the form
/// the picker uses ("claude-opus-5-5").
pub fn normalize_model(provider: Provider, model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return String::new();
    }
    if provider == Provider::ClaudeCode {
        let lower = m.to_ascii_lowercase();
        if let Some((id, _, _)) = CLAUDE_MODELS.iter().find(|(id, short, label)| *id == lower || *short == lower || label.to_ascii_lowercase() == lower || label.split(' ').next().is_some_and(|w| w.to_ascii_lowercase() == lower)) {
            return id.to_string();
        }
        return m.to_string();
    }
    model_choices(provider)
        .into_iter()
        .find(|(id, label)| !id.is_empty() && (id.eq_ignore_ascii_case(m) || label.eq_ignore_ascii_case(m)))
        .map(|(id, _)| id)
        .unwrap_or_else(|| m.to_string())
}

/// The chosen provider, or the first installed tool (Claude Code first).
pub fn pick_provider(prefs: &Prefs, installed: &[local::Installed]) -> Option<Provider> {
    if let Some(p) = prefs.provider {
        return Some(p);
    }
    [Provider::ClaudeCode, Provider::Codex]
        .into_iter()
        .find(|p| installed.iter().any(|i| i.provider == *p))
}

pub const NO_AI: &str = "No AI tool found. Install Claude Code (claude.com/claude-code) or Codex and sign in once in Terminal.";

/// A reply to give instead of asking an AI (the self-test, which must not
/// depend on which AI tools are installed or spend their time).
static CANNED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_canned_reply(reply: Option<String>) {
    *CANNED.lock().unwrap_or_else(|e| e.into_inner()) = reply;
}

/// A test can pretend there is no AI tool on this Mac.
static PRETEND_NONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn pretend_no_ai(on: bool) {
    PRETEND_NONE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether there is an AI to ask: an installed tool that the settings can
/// use. (True until the look for installed tools has finished, so nothing is
/// switched off by mistake; a request then says so itself if none is found.)
/// The voice features that need the AI are offered only when this is true;
/// without it the microphones type what is said, as it is said.
pub fn has_ai() -> bool {
    if PRETEND_NONE.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    if CANNED.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return true;
    }
    match local::installed_cached() {
        Some(found) => pick_provider(&Prefs::load(), &found).is_some_and(|p| found.iter().any(|i| i.provider == p)),
        None => true,
    }
}

/// The setting that holds what the app found the last time: "1" when an AI
/// tool was connected, "0" when none was.
pub const FOUND_SETTING: &str = "ai_found";

/// For what shows on the screen: is an AI tool connected? In the first
/// moments after the app opens it does not know yet, and what it found the
/// last time is the answer (so nothing of the AI shows and then goes, or
/// comes late, each time the app opens).
pub fn shown() -> bool {
    if PRETEND_NONE.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    if CANNED.lock().unwrap_or_else(|e| e.into_inner()).is_some() || std::env::var_os("NOTION2_SELFTEST").is_some() {
        return true;
    }
    match local::installed_cached() {
        Some(found) => pick_provider(&Prefs::load(), &found).is_some_and(|p| found.iter().any(|i| i.provider == p)),
        None => crate::store::setting(FOUND_SETTING).as_deref() == Some("1"),
    }
}

/// Start a request with whichever AI the settings pick. Text arrives as
/// `Delta` events, then `Done` or `Error`. Dropping the receiver stops it.
pub fn start(prefs: Prefs, messages: Vec<ChatMessage>) -> UnboundedReceiver<AiEvent> {
    start_with(prefs, messages, None, Vec::new())
}

/// As `start`. With `tools` the AI can use the tools of Rune, and the
/// `pictures` go to it with the request.
pub fn start_with(prefs: Prefs, messages: Vec<ChatMessage>, tools: Option<Tools>, pictures: Vec<Picture>) -> UnboundedReceiver<AiEvent> {
    let (tx, rx) = unbounded();
    if let Some(reply) = CANNED.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        // A test can say what the stand-in AI answers: "[[reply: text]]" in
        // what it sends (with \n for a new line) is answered with that text.
        let asked = messages.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
        let asked = asked.split("\n\nHow to reply.").next().unwrap_or(asked);
        // The request itself, where it has such a mark: not a page given
        // with it that holds a mark from an earlier test.
        let asked = asked.rsplit_once("</page>").map(|(_, after)| after).filter(|t| t.contains("[[reply:") || t.contains("[[tool:")).unwrap_or(asked);
        let reply = match asked.split_once("[[reply:").and_then(|(_, rest)| rest.rsplit_once("]]")) {
            Some((text, _)) => text.trim().replace("\\n", "\n"),
            None => reply,
        };
        // (A test can have the stand-in say how many pictures it was given.)
        let reply = reply.replace("{pictures}", &pictures.len().to_string());
        // (…and what else it was given, see attach.rs: the name, the kind and
        // the first words of each thing; and the first words of the page.)
        let whole = messages.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
        let given: Vec<String> = whole
            .split("<given name=\"")
            .skip(1)
            .filter_map(|part| {
                let (name, rest) = part.split_once("\" kind=\"")?;
                let (kind, rest) = rest.split_once("\">\n")?;
                let body = rest.split("\n</given>").next().unwrap_or("");
                Some(format!("{name} ({kind}): {}", body.chars().take(60).collect::<String>().replace('\n', " / ")))
            })
            .collect();
        let reply = reply.replace("{given}", &given.join(" | "));
        let page = whole.split_once("<page>\n").and_then(|(_, rest)| rest.split_once("\n</page>")).map(|(page, _)| page).unwrap_or("");
        let reply = reply.replace("{page}", &page.chars().take(80).collect::<String>().replace('\n', " / "));
        // A test of a large job: the reply is a plan ("@plan …"). The tools
        // are used only after the yes, and the answer is then "[[then: text]]".
        let is_plan = split_plan(&reply).is_some();
        let yes = asked.contains(PLAN_OK);
        let reply = if is_plan && yes {
            asked.split_once("[[then:").and_then(|(_, rest)| rest.split_once("]]")).map(|(text, _)| text.trim().to_string()).unwrap_or_else(|| "Done.".into())
        } else {
            reply
        };
        let tools = if is_plan && !yes { None } else { tools };
        // A test can also say which tools the stand-in AI uses, before its
        // answer: "[[tool: create_page {"title":"Trip"}]]". They are the
        // real tools, on the notes of the test, with the record of changes.
        let mut steps: Vec<(String, serde_json::Value)> = Vec::new();
        if let Some(tools) = &tools {
            let mut rest = asked.split("[[reply:").next().unwrap_or(asked);
            let mut calls: Vec<String> = Vec::new();
            while let Some((_, after)) = rest.split_once("[[tool:") {
                let Some((call, tail)) = after.split_once("]]") else { break };
                calls.push(call.trim().to_string());
                rest = tail;
            }
            if !calls.is_empty() {
                if let Ok(store) = crate::store::Store::open(crate::store::Store::default_path()) {
                    let mut server = crate::mcp::Server::new(store).with_journal(tools.journal.clone());
                    for call in calls {
                        let (name, args) = call.split_once(' ').unwrap_or((call.as_str(), "{}"));
                        let args: serde_json::Value = serde_json::from_str(args.trim()).unwrap_or_else(|_| serde_json::json!({}));
                        server.tool(name, &args);
                        steps.push((format!("mcp__{TOOLS_NAME}__{name}"), args));
                    }
                }
            }
        }
        runtime().spawn(async move {
            for (name, args) in steps {
                let title = args.get("title").and_then(serde_json::Value::as_str).map(str::to_string);
                let _ = tx.unbounded_send(AiEvent::Working { tool: name.clone(), title, chars: 0 });
                tokio::time::sleep(Duration::from_millis(220)).await;
                let _ = tx.unbounded_send(AiEvent::Step(name, args));
                tokio::time::sleep(Duration::from_millis(160)).await;
            }
            // In a few pieces, as a real reply arrives.
            let chars: Vec<char> = reply.chars().collect();
            for piece in chars.chunks(24) {
                tokio::time::sleep(Duration::from_millis(40)).await;
                let _ = tx.unbounded_send(AiEvent::Delta(piece.iter().collect()));
            }
            let _ = tx.unbounded_send(AiEvent::Done);
        });
        return rx;
    }
    runtime().spawn(async move {
        let result = dispatch(&prefs, &messages, tools.as_ref(), &pictures, &tx).await;
        let _ = tx.unbounded_send(match result {
            Ok(()) => AiEvent::Done,
            Err(e) => AiEvent::Error(e),
        });
    });
    rx
}

async fn dispatch(prefs: &Prefs, messages: &[ChatMessage], tools: Option<&Tools>, pictures: &[Picture], tx: &UnboundedSender<AiEvent>) -> Result<(), String> {
    let provider = match prefs.provider {
        Some(p) => p,
        None => {
            let found = local::installed_async(false).await;
            pick_provider(prefs, &found).ok_or_else(|| NO_AI.to_string())?
        }
    };
    local::run(provider, prefs.model(provider), &prefs.effort, messages, tools, pictures, tx).await
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        ChatMessage { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage { role: "user".into(), content: content.into() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AiEvent {
    Delta(String),
    /// The AI used one of the tools of Rune: its name, and what it gave it.
    Step(String, serde_json::Value),
    /// The AI is writing what it gives to a tool (a long page takes a
    /// while): the tool, the name of the page when it is known, and how
    /// many characters so far.
    Working { tool: String, title: Option<String>, chars: usize },
    Done,
    Error(String),
}

/// The tools of Rune for the app's own AI (see mcp.rs). With them it can do
/// anything in the app: read and search the notes, make, change, move and
/// remove pages, and work with the calendar. What it changes is written
/// down in `journal`, so that the app can say what was done and take it back.
#[derive(Clone, Debug, PartialEq)]
pub struct Tools {
    pub journal: std::path::PathBuf,
}

/// The model that answers now, for a quiet line beside a box to ask in: how
/// hard it thinks (dots lit, of four) and its name. Nothing when no AI tool
/// is known.
pub fn chosen_label() -> Option<(u8, String)> {
    let prefs = Prefs::load();
    let installed = local::installed_cached().unwrap_or_default();
    let provider = pick_provider(&prefs, &installed)?;
    let model = prefs.model(provider).to_string();
    let label = model_choices(provider).into_iter().find(|(id, _)| *id == model).map(|(_, label)| label)?;
    let label = label.strip_prefix("Default · ").map(str::to_string).unwrap_or(label);
    Some((effort_dots(&prefs.effort), label))
}

/// The first line of a reply that is a plan for a large job (see `PLAN_RULE`).
pub const PLAN_MARK: &str = "@plan";

/// The words that tell the AI that the person said yes to its plan.
pub const PLAN_OK: &str = "The plan is approved.";

/// For a run with the tools: a large job is shown as a plan first.
pub const PLAN_RULE: &str = "A large job needs the person's yes first. If the request will make or change more than six pages or events, do not change anything yet: reply with a first line that is exactly `@plan`, then one or two sentences on what you will build, then the steps as a list (one line each, seven lines at most, plain words, no ids). When the request says that the plan is approved, do the work at once with the tools and do not ask again.";

/// If a reply is a plan for a large job: what it will build, in a sentence
/// or two, and its steps.
pub fn split_plan(reply: &str) -> Option<(String, Vec<String>)> {
    let mut lines = reply.lines().map(str::trim).skip_while(|l| l.is_empty());
    if lines.next()?.trim_matches('`') != PLAN_MARK {
        return None;
    }
    let mut about: Vec<&str> = Vec::new();
    let mut steps: Vec<String> = Vec::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let item = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).or_else(|| {
            let (number, rest) = line.split_once(". ")?;
            number.chars().all(|c| c.is_ascii_digit()).then_some(rest)
        });
        match item {
            Some(step) => steps.push(step.trim().trim_start_matches("[ ] ").to_string()),
            None => about.push(line),
        }
    }
    Some((about.join(" "), steps))
}

/// A request with the person's yes to the plan that the AI gave for it.
pub fn approved(request: &str, plan: &str) -> String {
    format!("{}\n\n{PLAN_OK} Do it now with the tools, and do not ask again. The plan:\n{}", request.trim(), plan.trim())
}

/// A picture that goes to the AI with a request: a screenshot that the
/// person pasted or dropped into the box.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub id: String,
    /// "image/png", "image/jpeg", …
    pub media: String,
    /// The picture, in base64.
    pub data: String,
}

/// No more pictures than this go with one request.
pub const MOST_PICTURES: usize = 4;

impl Picture {
    /// From a data address ("data:image/jpeg;base64,…"), as the page script
    /// sends it. Nothing for another kind of file, or one that is too large.
    pub fn from_data_url(url: &str) -> Option<Picture> {
        let (head, data) = url.strip_prefix("data:")?.split_once(',')?;
        let media = head.strip_suffix(";base64")?;
        let known = matches!(media, "image/png" | "image/jpeg" | "image/gif" | "image/webp");
        (known && !data.is_empty() && data.len() <= 4_000_000).then(|| Picture { id: crate::model::new_id(), media: media.to_string(), data: data.to_string() })
    }

    /// As an address that a page can show.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media, self.data)
    }
}

/// The name of the tools as the AI tool sees them ("mcp__rune__read_page").
pub const TOOLS_NAME: &str = "rune";

/// Can the AI that Settings picks use the tools of Rune? Claude Code and
/// Codex both can. (Not known yet, in the first moments: no, and the AI
/// answers from the page alone that one time.)
pub fn takes_tools() -> bool {
    if PRETEND_NONE.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    if CANNED.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return true;
    }
    match local::installed_cached() {
        Some(found) => pick_provider(&Prefs::load(), &found).is_some(),
        None => false,
    }
}

/// What the AI is at while it writes what it gives to a tool, in plain
/// words ("Writing the page “Trading journal” · 2,400 characters").
pub fn working_label(tool: &str, title: Option<&str>, chars: usize) -> String {
    let tool = tool.rsplit("__").next().unwrap_or(tool);
    let named = title.map(str::trim).filter(|t| !t.is_empty());
    let what = match (tool, named) {
        ("create_page", Some(t)) => format!("Writing the page “{t}”"),
        ("create_page", None) => "Writing a new page".to_string(),
        ("append_to_page" | "replace_page_content" | "update_page", _) => "Writing to a page".to_string(),
        ("create_event" | "update_event" | "delete_event", _) => "Working in the calendar".to_string(),
        ("add_table_row" | "update_table_row", _) => "Filling a table".to_string(),
        ("search_pages" | "list_pages" | "read_page" | "list_events", _) => "Looking at your notes".to_string(),
        _ => "Working".to_string(),
    };
    if chars < 300 {
        return what;
    }
    let count = chars.to_string();
    let mut grouped = String::new();
    for (i, c) in count.chars().enumerate() {
        if i > 0 && (count.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{what} · {grouped} characters")
}

/// One use of a tool, in plain words ("Made the page “Fitness”"). `title`
/// gives the name of a page for its id.
pub fn step_label(tool: &str, input: &serde_json::Value, title: impl Fn(&str) -> Option<String>) -> String {
    let tool = tool.rsplit("__").next().unwrap_or(tool);
    let text = |key: &str| input.get(key).and_then(serde_json::Value::as_str).unwrap_or("").trim().to_string();
    let page = || {
        let id = text("page_id");
        let id = id.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        match title(&id).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            Some(t) => format!("“{t}”"),
            None => "a page".to_string(),
        }
    };
    match tool {
        "search_pages" => format!("Looked in the notes for “{}”", text("query")),
        "list_pages" => "Looked at the list of pages".into(),
        "read_page" => format!("Read {}", page()),
        "create_page" => format!("Made the page “{}”", text("title")),
        "append_to_page" => format!("Added to {}", page()),
        "update_page" if input.get("parent_id").is_some() => format!("Moved {}", page()),
        "update_page" if input.get("title").is_some() => format!("Named {} “{}”", page(), text("title")),
        "replace_page_content" | "update_page" => format!("Changed {}", page()),
        "trash_page" => format!("Moved {} to the Trash", page()),
        "restore_page" => format!("Took {} out of the Trash", page()),
        "list_events" => "Looked at the calendar".into(),
        "create_event" => format!("Added “{}” to the calendar", text("title")),
        "update_event" => "Changed an event in the calendar".into(),
        "delete_event" => "Removed an event from the calendar".into(),
        "add_table_row" => format!("Added a row to {}", page()),
        "update_table_row" => format!("Changed a row of {}", page()),
        "set_todo" if input.get("done").and_then(serde_json::Value::as_bool) == Some(true) => format!("Ticked “{}” in {}", text("text"), page()),
        "set_todo" => format!("Changed a to-do in {}", page()),
        other => format!("Used {other}"),
    }
}

/// A model with no tools sometimes writes out pretend tool calls and then
/// invents their results (files, folders, command output). Remove the calls
/// and report that it happened, so the panel can warn that the reply may be
/// made up.
pub fn clean_reply(text: &str) -> (String, bool) {
    const OPEN: [&str; 5] = ["<function_calls", "<invoke", "<function_results", "<tool_use", "<tool_result"];
    const CLOSE: [&str; 5] = ["</function_calls>", "</invoke>", "</function_results>", "</tool_use>", "</tool_result>"];
    let mut out: Vec<&str> = Vec::new();
    // The lines of a call that is still open: they go only when it closes.
    let mut held: Vec<&str> = Vec::new();
    let mut depth = 0usize;
    let mut found = false;
    let mut in_code = false;
    for line in text.lines() {
        let t = line.trim_start();
        // Code that is shown is the person's own (notes on such calls, in a
        // page that is written again): it is kept as it is.
        if depth == 0 && t.starts_with("```") {
            in_code = !in_code;
            out.push(line);
            continue;
        }
        if in_code {
            out.push(line);
            continue;
        }
        let opens = OPEN.iter().filter(|o| t.starts_with(*o)).count();
        let closes = CLOSE.iter().filter(|c| t.contains(*c)).count();
        let tool_line = opens > 0 || (depth > 0 && closes > 0) || (depth > 0 && (t.starts_with("<parameter") || t.starts_with("</parameter>")));
        if tool_line || depth > 0 {
            depth = (depth + opens).saturating_sub(closes);
            if depth == 0 {
                // A whole call, from its first line to its last: taken out.
                found = true;
                held.clear();
            } else {
                held.push(line);
            }
            continue;
        }
        out.push(line);
    }
    // A call that never closed took every line after it away, to the end of
    // the page: those lines are kept. Only the line that opened it goes.
    if !held.is_empty() {
        found = true;
        out.extend(held.into_iter().skip(1));
    }
    let mut cleaned = out.join("\n");
    if text.ends_with('\n') && !cleaned.is_empty() {
        cleaned.push('\n');
    }
    (cleaned, found)
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// What the AI is told when it turns speech into a page.
pub const SPEECH_TO_PAGE: &str = "You turn a spoken note into a clear, well-organized page. The note was dictated, so it may have misheard words, repeats and little punctuation. Reply with Markdown only, with no preamble and no closing remark. The first line is `# ` and a short title (at most 8 words). Then the content: short paragraphs, `##` headings when there are several topics, `-` bullets for lists, and `- [ ]` to-dos for things to do. Keep every fact, name, number and intention the speaker gave. Fix obvious dictation errors. Do not add facts, do not answer questions the speaker asked, and do not speak to the speaker. Write in the language the speaker used.";

/// What the AI is told when speech is written into a page that exists.
pub const SPEECH_INTO_PAGE: &str = "You turn a spoken note into clear, well-organized text that will be put into a page the person is writing. The note was dictated, so it may have misheard words, repeats and little punctuation. Reply with Markdown only, with no preamble, no closing remark and no title line. Use short paragraphs, `##` headings only when there are several topics, `-` bullets for lists, and `- [ ]` to-dos for things to do. A short note is one sentence or one list: do not pad it. Keep every fact, name, number and intention the speaker gave. Fix obvious dictation errors. Do not add facts, do not answer questions the speaker asked, and do not speak to the speaker. Write in the language the speaker used.";

#[derive(Clone, Debug, PartialEq)]
pub enum AiAction {
    Ask,
    Continue,
    Summarize,
    ActionItems,
    Improve,
    FixGrammar,
    Shorter,
    Longer,
    Translate(String),
    /// What happened in a meeting, said or typed: it becomes the notes of
    /// the meeting's page (see AppState::speech_recap).
    Recap,
}

impl AiAction {
    pub fn label(&self) -> String {
        match self {
            AiAction::Ask => "Ask AI".into(),
            AiAction::Continue => "Continue writing".into(),
            AiAction::Summarize => "Summarize page".into(),
            AiAction::ActionItems => "Find action items".into(),
            AiAction::Improve => "Improve writing".into(),
            AiAction::FixGrammar => "Fix spelling & grammar".into(),
            AiAction::Shorter => "Make shorter".into(),
            AiAction::Longer => "Make longer".into(),
            AiAction::Translate(lang) => format!("Translate to {lang}"),
            AiAction::Recap => "Meeting recap".into(),
        }
    }

    /// Actions that rewrite the selected block rather than add new content.
    pub fn rewrites_block(&self) -> bool {
        matches!(
            self,
            AiAction::Improve
                | AiAction::FixGrammar
                | AiAction::Shorter
                | AiAction::Longer
                | AiAction::Translate(_)
        )
    }
}

const SYSTEM_PROMPT: &str = "You are the writing assistant inside a notes app. \
You cannot read files, run commands, browse the web or use any tools, and you cannot see anything \
on this computer except the page text included in the request. Never write tool calls, commands, \
file paths or file contents you were not given, and never invent files, folders, people or facts \
about the user's work. If the request needs information that is not in the page, say in one line \
what is missing. Reply in Markdown using headings (#, ##, ###), bulleted lists, numbered lists, \
to-do items (- [ ] task), block quotes, fenced code blocks and plain paragraphs; inside text you may \
use **bold**, _italic_, `code` and [links](https://example.com). Do not add a preamble or closing \
remarks; return only the content to insert into the page.";

/// For a run with the tools of Rune (see `Tools`).
const AGENT_PROMPT: &str = "You are the assistant inside Rune, a notes app with pages and a calendar, on the person's Mac. \
You have the tools of Rune. With them you can do anything in Rune that the person asks: search and read the notes, \
make, change, name, move and remove pages, and add, change and remove calendar events. Do the work yourself, with the tools. \
Never tell the person to do a step that you can do, and never say that you cannot see or change the notes: look, and do it. \
You have no other tools: you cannot read files, run commands or browse the web, and you see nothing of this Mac but Rune. \
Never invent what is in the notes: look it up with the tools. A page that you remove goes to the Trash. \
Do what the request asks for and no more. Make each page with a tool call of its own, the main page first, \
so that the person sees each page as it is made. When the work is done, say in one or two plain sentences what you did, \
with the names of the pages and events, and with no ids and no links; but a reply that is a page (one that starts with @replace) says nothing else. \
When you reply with content for a page, use Markdown: headings (#, ##, ###), bulleted lists, numbered lists, \
to-do items (- [ ] task), block quotes, fenced code blocks and plain paragraphs; inside text you may \
use **bold**, _italic_, `code` and [links](https://example.com). No preamble and no closing remarks.";

/// Added to the instructions of the box that asks all the notes, for a
/// run with the tools of Rune.
pub const ASK_NOTES_TOOLS: &str = "You also have the tools of Rune. With them you can do anything in Rune that the person asks: \
search and read more of the notes, make, change, name, move and remove pages, and add, change and remove calendar events. \
When the request asks you to do something, do it yourself with the tools; then say in one or two plain sentences what you did, \
with the names of the pages and events, with no ids and no links, and with no Sources list. \
When it is a question that the pages below do not answer, look in the other notes with the tools before you say that the notes \
do not hold the answer. Never tell the person to do a step that you can do. A page that you remove goes to the Trash. \
You have no other tools: you cannot read files, run commands or browse the web. \
A large job needs the person's yes first. If the request will make or change more than six pages or events, do not change anything yet: reply with a first line that is exactly `@plan`, then one or two sentences on what you will build, then the steps as a list (one line each, seven lines at most, plain words, no ids). When the request says that the plan is approved, do the work at once with the tools and do not ask again.";

pub const MAX_CONTEXT_CHARS: usize = 24_000;

/// The first line of a reply that is the whole page after a change (see the
/// instructions for `AiAction::Ask`).
pub const REVISE_MARK: &str = "@replace";

/// If a reply is the page as it must be after a change: the page (the text
/// after the mark). `None` for an answer or new content.
pub fn split_revision(reply: &str) -> Option<String> {
    let rest = reply.trim_start();
    let (first, body) = rest.split_once('\n').unwrap_or((rest, ""));
    (first.trim().trim_matches('`') == REVISE_MARK).then(|| body.trim_start_matches(['\r', '\n']).to_string())
}

/// In a reply that changes the page, the line that starts a sub-page: the
/// mark, then the name of the sub-page. The lines after it are that page.
pub const PAGE_MARK: &str = "@page ";
/// No more sub-pages than this are made from one reply.
pub const MOST_PAGES: usize = 12;

/// A reply that changes the page, split into the page itself and the
/// sub-pages it asks for (the name and the Markdown of each).
pub fn split_pages(body: &str) -> (String, Vec<(String, String)>) {
    let mut main = String::new();
    let mut pages: Vec<(String, String)> = Vec::new();
    let mut in_code = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_code = !in_code;
        }
        let name = (!in_code).then(|| t.trim_matches('`').strip_prefix(PAGE_MARK)).flatten().map(|n| n.trim().trim_matches(['"', '“', '”', '*', '#', ' ']).to_string());
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            pages.push((name, String::new()));
            continue;
        }
        let into = match pages.last_mut() {
            Some((_, text)) => text,
            None => &mut main,
        };
        into.push_str(line);
        into.push('\n');
    }
    pages.truncate(MOST_PAGES);
    (main, pages)
}

/// Put links in place of the names of sub-pages: `[[Workout A]]` becomes a
/// link to the page made for "Workout A". A name with no page stays as it is.
pub fn link_pages(text: &str, made: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else { break };
        let name = rest[start + 2..start + 2 + len].trim();
        out.push_str(&rest[..start]);
        match made.iter().find(|(n, _)| n.trim().eq_ignore_ascii_case(name)) {
            Some((n, id)) => out.push_str(&format!("[{n}]({})", crate::inline::page_url(id))),
            None => out.push_str(&rest[start..start + len + 4]),
        }
        rest = &rest[start + len + 4..];
    }
    out.push_str(rest);
    out
}

fn clip(s: &str) -> &str {
    match s.char_indices().nth(MAX_CONTEXT_CHARS) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// A request on a page, for a run with the tools of Rune: the page with
/// its name and id, the time now, the request, and how to reply.
pub fn build_agent_messages(page_id: &str, title: &str, page_md: &str, prompt: &str, now: &str) -> Vec<ChatMessage> {
    let ChatMessage { content: plain, .. } = build_messages(&AiAction::Ask, page_md, "", prompt).pop().unwrap_or_else(|| ChatMessage::user(""));
    let (before, how) = plain.split_once("\n\nHow to reply.").unwrap_or((plain.as_str(), ""));
    let before = before.replacen("Here is the page I'm working on:", &format!("Here is the page I'm working on. Its name is \"{}\" and its id is {page_id}.", title.trim()), 1);
    let task = format!(
        "{before}\n\nHow to reply.{how} \
         That is for this page and its sub-pages only: do not use the tools to change this page. \
         For everything else that the request asks for (another page, a new page that is not a sub-page of this page, \
         a move, a new name, the Trash, the calendar, something to find in the other notes), use the tools, \
         and then say shortly what you did. {PLAN_RULE}\n\nNow it is {now} (local time)."
    );
    vec![ChatMessage::system(AGENT_PROMPT), ChatMessage::user(task)]
}

pub fn build_messages(action: &AiAction, page_md: &str, block_text: &str, prompt: &str) -> Vec<ChatMessage> {
    let page = clip(page_md);
    let task = match action {
        AiAction::Ask => format!(
            "Here is the page I'm working on:\n\n<page>\n{page}\n</page>\n\nRequest: {}\n\n\
             How to reply. If the request asks to change what is already on the page (edit, rewrite, \
             shorten, extend, reorder, correct, update, or take out a part of it), reply with a first \
             line that is exactly `{REVISE_MARK}`, and after it the whole page as it must be after the \
             change, in the same Markdown: keep every part that does not change word for word, and \
             keep lines such as `[Image](rune://block/...)` and links to other pages as they are. \
             You can also make sub-pages of this page, when the request asks for pages of their own \
             (sub-pages, a page for each part): after the whole page, write a line that is exactly \
             `{PAGE_MARK}` followed by the name of the sub-page, and under it that sub-page in Markdown; \
             do this for each sub-page. Link each sub-page from the page: a line that is only \
             `[[the name]]`, or `[[the name]]` inside a sentence. Do the work yourself: never tell \
             the person to make or link the pages. A reply that starts with `{REVISE_MARK}` is only \
             the pages: write nothing before the mark and nothing after the last page, not even a line \
             on what you did, because every line after it goes into that page. \
             If the request is a question, or asks for something new that changes nothing that is \
             there, reply with only the answer or the new content, and no `{REVISE_MARK}` line. \
             Never draw a picture with characters (boxes, lines, arrows, shading), even when a diagram or a picture \
             is asked for: a diagram is a code block of the language mermaid, which the app draws in its own colours (write no style \
             or classDef lines); levels, prices or amounts go in a small table.",
            prompt.trim()
        ),
        AiAction::Continue => format!(
            "Continue writing this page naturally from where it ends, in the same voice. \
             Return only the new text.\n\n<page>\n{page}\n</page>"
        ),
        AiAction::Summarize => format!(
            "Summarize this page: one short paragraph, then the key points as a bulleted list.\n\n<page>\n{page}\n</page>"
        ),
        AiAction::ActionItems => format!(
            "List the action items in this page as to-do items. If there are none, suggest sensible next steps as to-do items.\n\n<page>\n{page}\n</page>"
        ),
        AiAction::Improve => format!("Rewrite this text to improve clarity and flow while keeping its meaning. Return only the rewritten text.\n\n{block_text}"),
        AiAction::FixGrammar => format!("Fix spelling and grammar in this text. Change nothing else. Return only the corrected text.\n\n{block_text}"),
        AiAction::Shorter => format!("Make this text shorter while keeping the key points. Return only the new text.\n\n{block_text}"),
        AiAction::Longer => format!("Expand this text with more detail. Return only the new text.\n\n{block_text}"),
        AiAction::Translate(lang) => format!("Translate this text to {lang}. Return only the translation.\n\n{block_text}"),
        // Not sent from here: a recap has its own instructions (voice::RECAP).
        AiAction::Recap => prompt.trim().to_string(),
    };
    vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(task)]
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("ai")
            .build()
            .expect("start AI runtime")
    })
}

/// Wait without blocking the UI thread (works on any executor).
pub async fn sleep_ms(ms: u64) {
    let (tx, rx) = futures_channel::oneshot::channel::<()>();
    runtime().spawn(async move {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        let _ = tx.send(());
    });
    let _ = rx.await;
}

/// Warm the installed-tool lookup in the background so the first request
/// doesn't wait on the login shell.
pub fn warm_up() {
    if local::installed_cached().is_none() {
        runtime().spawn_blocking(|| {
            local::installed(false);
        });
    }
}

/// Short name of the AI that will answer, for labels. Uses the last lookup.
pub fn active_label(prefs: &Prefs) -> String {
    let found = local::installed_cached().unwrap_or_default();
    match pick_provider(prefs, &found) {
        Some(p) => {
            let model = prefs.model(p);
            match model_choices(p).into_iter().find(|(id, _)| id == model) {
                Some((id, _)) if id.is_empty() => match default_model_label(p) {
                    Some(label) => format!("{} · {label}", p.name()),
                    None => p.name().into(),
                },
                Some((_, label)) => format!("{} · {label}", p.name()),
                None => format!("{} · {model}", p.name()),
            }
        }
        None => "AI".into(),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_reply_is_cleaned_of_pretend_calls_and_of_nothing_else() {
        // A pretend call, whole: taken out, and said.
        let (text, found) = clean_reply("Before\n<function_calls>\n<invoke name=\"x\">\n<parameter name=\"a\">1</parameter>\n</invoke>\n</function_calls>\nAfter\n");
        assert_eq!((text.as_str(), found), ("Before\nAfter\n", true));
        // Shown as code, such lines are the person's notes: kept.
        let code = "Notes on tools\n```xml\n<invoke name=\"x\">\n</invoke>\n```\nMore";
        assert_eq!(clean_reply(code), (code.to_string(), false));
        // A call that never closes does not take the rest of the page with it.
        let (text, found) = clean_reply("One\n<tool_use>\nTwo\nThree");
        assert_eq!((text.as_str(), found), ("One\nTwo\nThree", true));
        // The name of a call inside a sentence is words.
        let said = "Use <invoke name=\"x\"> to call it.\nNext line";
        assert_eq!(clean_reply(said), (said.to_string(), false));
    }
    #[test]
    fn a_picture_for_the_ai_comes_from_a_data_address() {
        let p = Picture::from_data_url("data:image/jpeg;base64,AAAA").unwrap();
        assert_eq!((p.media.as_str(), p.data.as_str()), ("image/jpeg", "AAAA"));
        assert_eq!(p.data_url(), "data:image/jpeg;base64,AAAA");
        assert!(Picture::from_data_url("data:text/html;base64,AAAA").is_none());
        assert!(Picture::from_data_url("data:image/png;base64,").is_none());
        assert!(Picture::from_data_url("https://example.com/a.png").is_none());
    }

    #[test]
    fn the_words_for_what_the_ai_is_at() {
        assert_eq!(working_label("mcp__rune__create_page", Some("Trading journal"), 2400), "Writing the page “Trading journal” · 2,400 characters");
        assert_eq!(working_label("mcp__rune__create_page", None, 0), "Writing a new page");
        assert_eq!(working_label("mcp__rune__replace_page_content", None, 12345), "Writing to a page · 12,345 characters");
        assert_eq!(working_label("mcp__rune__list_pages", None, 0), "Looking at your notes");
        assert_eq!(working_label("mcp__rune__something_new", None, 10), "Working");
    }

    #[test]
    fn a_large_job_is_a_plan_first() {
        let reply = "@plan\nEight weeks from 3 km to 10 km, three runs a week.\n\n- New page: 10K Training\n- 8 week pages, Week 1 to Week 8\n1. 24 runs in the calendar\n";
        let (about, steps) = split_plan(reply).unwrap();
        assert_eq!(about, "Eight weeks from 3 km to 10 km, three runs a week.");
        assert_eq!(steps, vec!["New page: 10K Training", "8 week pages, Week 1 to Week 8", "24 runs in the calendar"]);
        assert_eq!(split_plan("Here is the answer.\n- one"), None);
        assert_eq!(split_plan("@replace\n# Page"), None);
        // The yes goes with the request and the plan, and the rule is in what a run with the tools is told.
        let yes = approved("build it [[reply: x]]", "- New page: A");
        assert!(yes.starts_with("build it") && yes.contains(PLAN_OK) && yes.ends_with("- New page: A"));
        let msgs = build_agent_messages("abc", "Plan", "# Plan", "build me a journal", "2026-11-10 17:30, Tuesday");
        assert!(msgs[1].content.contains(PLAN_MARK) && msgs[1].content.contains("more than six pages or events"));
        assert!(ASK_NOTES_TOOLS.contains(PLAN_MARK));
    }

    #[test]
    fn a_run_with_tools_is_told_the_page_the_time_and_what_the_tools_are_for() {
        let msgs = build_agent_messages("abc123", "Workout Plan", "# Workout Plan\n\n- Squats", "add a run on Monday at 7 [[reply: ok]]", "2026-11-10 17:30, Tuesday");
        assert!(msgs[0].content.contains("tools of Rune") && !msgs[0].content.contains("cannot read files, run commands, browse the web or use any tools"));
        let user = &msgs[1].content;
        assert!(user.contains("Its name is \"Workout Plan\" and its id is abc123."), "{user}");
        assert!(user.contains("Now it is 2026-11-10 17:30, Tuesday (local time)."));
        assert!(user.contains("use the tools") && user.contains(REVISE_MARK) && user.contains(PAGE_MARK.trim()));
        // The stand-in AI of a test reads its answer from the request alone.
        assert_eq!(user.split("\n\nHow to reply.").count(), 2);
        // A step in plain words, with the name of the page.
        let title = |id: &str| (id == "abc123").then(|| "Workout Plan".to_string());
        assert_eq!(step_label("mcp__rune__read_page", &serde_json::json!({ "page_id": "notion2://page/abc123" }), title), "Read “Workout Plan”");
        assert_eq!(step_label("mcp__rune__create_page", &serde_json::json!({ "title": "Fitness" }), title), "Made the page “Fitness”");
        assert_eq!(step_label("mcp__rune__trash_page", &serde_json::json!({ "page_id": "zzz" }), title), "Moved a page to the Trash");
    }

    #[test]
    fn a_reply_can_ask_for_sub_pages() {
        let body = "# Plan\n\n1. [[Workout A]]\n2. [[workout b]]\n\n```\n@page not this, it is code\n```\n@page Workout A\nTen squats.\n\n`@page \"Workout B\"`\nFive push-ups.\n";
        let (main, pages) = split_pages(body);
        assert!(main.contains("[[Workout A]]") && main.contains("@page not this") && !main.contains("Ten squats"), "{main}");
        assert_eq!(pages, vec![("Workout A".to_string(), "Ten squats.\n\n".to_string()), ("Workout B".to_string(), "Five push-ups.\n".to_string())]);
        // The names become links to the pages that were made (in any letter case); a name with no page stays.
        let made = vec![("Workout A".to_string(), "p1".to_string()), ("Workout B".to_string(), "p2".to_string())];
        let linked = link_pages("1. [[Workout A]]\n2. [[workout b]] and [[Other]] and [[open", &made);
        assert_eq!(linked, "1. [Workout A](rune://page/p1)\n2. [Workout B](rune://page/p2) and [[Other]] and [[open");
        // A reply with no sub-pages is the page alone.
        assert_eq!(split_pages("One\nTwo"), ("One\nTwo\n".to_string(), Vec::new()));
        // Never more than a few.
        let many: String = (0..40).map(|i| format!("@page P{i}\nx\n")).collect();
        assert_eq!(split_pages(&many).1.len(), MOST_PAGES);
    }

    use super::*;

    #[test]
    fn prompts_include_context() {
        // A reply that is the page after a change, and one that is not.
        assert_eq!(split_revision("@replace\n# Trip\n\n- Rome"), Some("# Trip\n\n- Rome".to_string()));
        assert_eq!(split_revision("\n  `@replace`  \n\nBody"), Some("Body".to_string()));
        assert_eq!(split_revision("@replace"), Some(String::new()));
        assert_eq!(split_revision("Add Rome.\n@replace"), None);
        assert_eq!(split_revision("@replaced text"), None);
        let msgs = build_messages(&AiAction::Ask, "# Trip\n\n- Paris", "", "What's missing?");
        assert!(msgs.last().is_some_and(|m| m.content.contains(REVISE_MARK)));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("cannot read files"));
        assert!(msgs[1].content.contains("- Paris") && msgs[1].content.contains("What's missing?"));
        let msgs = build_messages(&AiAction::FixGrammar, "ignored", "teh cat", "");
        assert!(msgs[1].content.ends_with("teh cat"));
    }

    #[test]
    fn provider_choice() {
        let claude = local::Installed { provider: Provider::ClaudeCode, path: "/x/claude".into(), version: None };
        let codex = local::Installed { provider: Provider::Codex, path: "/x/codex".into(), version: None };
        let auto = Prefs::default();
        assert_eq!(pick_provider(&auto, &[codex.clone(), claude.clone()]), Some(Provider::ClaudeCode));
        assert_eq!(pick_provider(&auto, &[codex]), Some(Provider::Codex));
        assert_eq!(pick_provider(&auto, &[]), None);
        let chosen = Prefs { provider: Some(Provider::Codex), ..Prefs::default() };
        assert_eq!(pick_provider(&chosen, &[claude]), Some(Provider::Codex));
        assert_eq!(Provider::from_id("codex"), Some(Provider::Codex));
        assert_eq!(Provider::from_id("api"), None);
    }

    #[test]
    fn claude_models_have_version_numbers() {
        assert_eq!(normalize_model(Provider::ClaudeCode, "Opus"), "claude-opus-5-5");
        assert_eq!(normalize_model(Provider::ClaudeCode, "sonnet"), "claude-sonnet-5-5");
        assert_eq!(normalize_model(Provider::ClaudeCode, ""), "");
        assert_eq!(claude_label("fable[1m]").as_deref(), Some("Fable 5.1 (1M context)"));
        assert_eq!(claude_label("claude-haiku-5-5").as_deref(), Some("Haiku 5.5"));
        let choices = model_choices(Provider::ClaudeCode);
        assert!(choices[0].1.starts_with("Default"));
        assert!(choices.iter().any(|(id, label)| id == "claude-opus-5-5" && label == "Opus 5.5"));
    }

    #[test]
    fn pretend_tool_calls_are_removed() {
        let reply = "## Plan\n- one\n<invoke name=\"Bash\">\n<parameter name=\"command\">ls ~/Documents</parameter>\n</invoke>\n- two\n";
        let (clean, found) = clean_reply(reply);
        assert!(found);
        assert_eq!(clean, "## Plan\n- one\n- two\n");
        let (same, found) = clean_reply("Just text.");
        assert!(!found);
        assert_eq!(same, "Just text.");
    }
}
