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

/// Start a request with whichever AI the settings pick. Text arrives as
/// `Delta` events, then `Done` or `Error`. Dropping the receiver stops it.
pub fn start(prefs: Prefs, messages: Vec<ChatMessage>) -> UnboundedReceiver<AiEvent> {
    let (tx, rx) = unbounded();
    if let Some(reply) = CANNED.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        // A test can say what the stand-in AI answers: "[[reply: text]]" in
        // what it sends (with \n for a new line) is answered with that text.
        let asked = messages.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
        let reply = match asked.split_once("[[reply:").and_then(|(_, rest)| rest.rsplit_once("]]")) {
            Some((text, _)) => text.trim().replace("\\n", "\n"),
            None => reply,
        };
        runtime().spawn(async move {
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
        let result = dispatch(&prefs, &messages, &tx).await;
        let _ = tx.unbounded_send(match result {
            Ok(()) => AiEvent::Done,
            Err(e) => AiEvent::Error(e),
        });
    });
    rx
}

async fn dispatch(prefs: &Prefs, messages: &[ChatMessage], tx: &UnboundedSender<AiEvent>) -> Result<(), String> {
    let provider = match prefs.provider {
        Some(p) => p,
        None => {
            let found = local::installed_async(false).await;
            pick_provider(prefs, &found).ok_or_else(|| NO_AI.to_string())?
        }
    };
    local::run(provider, prefs.model(provider), &prefs.effort, messages, tx).await
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
    Done,
    Error(String),
}

/// A model with no tools sometimes writes out pretend tool calls and then
/// invents their results (files, folders, command output). Remove the calls
/// and report that it happened, so the panel can warn that the reply may be
/// made up.
pub fn clean_reply(text: &str) -> (String, bool) {
    const OPEN: [&str; 5] = ["<function_calls", "<invoke", "<function_results", "<tool_use", "<tool_result"];
    const CLOSE: [&str; 5] = ["</function_calls>", "</invoke>", "</function_results>", "</tool_use>", "</tool_result>"];
    let mut out: Vec<&str> = Vec::new();
    let mut depth = 0usize;
    let mut found = false;
    for line in text.lines() {
        let t = line.trim_start();
        let opens = OPEN.iter().filter(|o| t.contains(*o)).count();
        let closes = CLOSE.iter().filter(|c| t.contains(*c)).count();
        let tool_line = opens > 0 || closes > 0 || t.starts_with("<parameter") || t.starts_with("</parameter>");
        if tool_line || depth > 0 {
            found = true;
            depth = (depth + opens).saturating_sub(closes);
            continue;
        }
        out.push(line);
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

const MAX_CONTEXT_CHARS: usize = 24_000;

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

fn clip(s: &str) -> &str {
    match s.char_indices().nth(MAX_CONTEXT_CHARS) {
        Some((i, _)) => &s[..i],
        None => s,
    }
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
             keep lines such as `[Image](notion2://block/...)` and links to other pages as they are. \
             If the request is a question, or asks for something new that changes nothing that is \
             there, reply with only the answer or the new content, and no `{REVISE_MARK}` line.",
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
