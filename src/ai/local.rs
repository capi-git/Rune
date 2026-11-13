//! AI tools already installed on this Mac (Claude Code and Codex), run headless.
//!
//! An app opened from Finder starts with a bare PATH, so tools are found by
//! asking the user's login shell for its PATH (once) and by checking the usual
//! install folders. Each request runs the tool in a fresh empty folder with all
//! of its tools switched off (Claude Code) or a read-only sandbox (Codex), feeds
//! the prompt on stdin and streams the reply back. Dropping the receiver kills
//! the process.

use super::{AiEvent, ChatMessage, Picture, Provider, Tools};
use futures_channel::mpsc::UnboundedSender;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// An AI command-line tool found on this Mac.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub provider: Provider,
    pub path: PathBuf,
    /// Version number reported by `--version`, e.g. "2.1.319".
    pub version: Option<String>,
}

pub fn binary_name(provider: Provider) -> &'static str {
    match provider {
        Provider::ClaudeCode => "claude",
        Provider::Codex => "codex",
    }
}

// ---------------------------------------------------------------------------
// Finding the tools
// ---------------------------------------------------------------------------

const MARK_A: &str = "__N2_PATH_";
const START: &str = "__N2_PATH_START__";
const END: &str = "__N2_PATH_END__";

/// Pull the PATH printed between the markers out of a login shell's output,
/// ignoring anything the shell's startup files printed around it.
pub fn parse_marked_path(output: &str) -> Option<String> {
    let start = output.rfind(START)? + START.len();
    let rest = &output[start..];
    let end = rest.find(END)?;
    let path = rest[..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// The PATH the user's login shell sets up, or None if it can't be read
/// within a few seconds.
fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/') && Path::new(s).exists())
        .unwrap_or_else(|| "/bin/zsh".into());
    // The markers are split so a shell that echoes commands can't fake them.
    let script = format!("printf '\\n%s%s%s%s%s\\n' {MARK_A} START__ \"$PATH\" {MARK_A} END__");
    let mut child = std::process::Command::new(&shell)
        .args(["-i", "-l", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
        let _ = tx.send(out);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    // A background process started by the shell can keep the pipe open; don't wait on it forever.
    let out = rx.recv_timeout(Duration::from_millis(500)).ok()?;
    parse_marked_path(&out)
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Usual install folders, for when the shell's PATH can't be read.
fn common_dirs() -> Vec<PathBuf> {
    let h = home();
    let mut dirs = vec![
        h.join(".local/bin"),
        h.join(".claude/local"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        h.join(".npm-global/bin"),
        h.join(".bun/bin"),
        h.join(".volta/bin"),
        h.join("Library/pnpm"),
        h.join(".cargo/bin"),
    ];
    // nvm: newest Node version first.
    if let Ok(entries) = std::fs::read_dir(h.join(".nvm/versions/node")) {
        let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("bin")).collect();
        versions.sort();
        versions.reverse();
        dirs.extend(versions);
    }
    dirs.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
    dirs
}

/// Join PATH lists in order, keeping the first copy of each folder.
pub fn merge_paths<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut seen = Vec::<&str>::new();
    for part in parts {
        for dir in part.split(':') {
            let dir = dir.trim();
            if !dir.is_empty() && !seen.contains(&dir) {
                seen.push(dir);
            }
        }
    }
    seen.join(":")
}

/// PATH used to find the tools and handed to them when they run: the login
/// shell's first, then this process's, then the usual install folders.
pub fn search_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let login = login_shell_path().unwrap_or_default();
        let current = std::env::var("PATH").unwrap_or_default();
        let common: Vec<String> = common_dirs().iter().map(|p| p.display().to_string()).collect();
        let common = common.join(":");
        merge_paths([login.as_str(), current.as_str(), common.as_str()])
    })
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub fn find_in(path: &str, name: &str) -> Option<PathBuf> {
    path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(name)).find(|p| is_executable(p))
}

/// "2.1.319 (Claude Code)" -> "2.1.319"; "codex-cli 0.167.0" -> "0.167.0".
pub fn parse_version(output: &str) -> Option<String> {
    let line = output.lines().find(|l| !l.trim().is_empty())?;
    line.split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.').to_string())
}

fn read_version(path: &Path) -> Option<String> {
    let mut child = std::process::Command::new(path)
        .arg("--version")
        .env("PATH", search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    parse_version(&out)
}

fn locate(provider: Provider) -> Option<PathBuf> {
    find_in(search_path(), binary_name(provider))
}

static DETECTED: Mutex<Option<Vec<Installed>>> = Mutex::new(None);

/// Find the installed tools (blocking: may start a shell and run `--version`).
/// Results are cached until `refresh` is true.
pub fn installed(refresh: bool) -> Vec<Installed> {
    if !refresh {
        if let Some(found) = DETECTED.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return found;
        }
    }
    let found: Vec<Installed> = [Provider::ClaudeCode, Provider::Codex]
        .into_iter()
        .filter_map(|provider| {
            let path = locate(provider)?;
            let version = read_version(&path);
            Some(Installed { provider, path, version })
        })
        .collect();
    *DETECTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(found.clone());
    found
}

/// The last detection result, without doing any work.
pub fn installed_cached() -> Option<Vec<Installed>> {
    DETECTED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `installed` off the calling thread; safe to await from the UI.
pub async fn installed_async(refresh: bool) -> Vec<Installed> {
    let (tx, rx) = futures_channel::oneshot::channel();
    super::runtime().spawn_blocking(move || {
        let _ = tx.send(installed(refresh));
    });
    rx.await.unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Command lines
// ---------------------------------------------------------------------------

/// Flags that make Claude Code a plain text generator: no tools, no MCP
/// servers, hooks, plugins or CLAUDE.md, nothing saved.
pub fn claude_args(system: &str, model: &str, effort: &str, optional: bool, tools: Option<&Tools>) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose", "--tools", "", "--permission-mode", "dontAsk"]
        .map(String::from)
        .to_vec();
    if optional {
        args.extend(["--include-partial-messages", "--no-session-persistence"].map(String::from));
    }
    match tools {
        // The tools of Rune, and no other: none of Claude Code's own tools
        // (--tools ""), none of the person's own connectors (--strict-mcp-config).
        // Safe mode would switch these tools off with the person's own, so
        // this run reads none of the person's settings in its place.
        Some(tools) => args.extend(
            ["--setting-sources", "", "--mcp-config", &tools_config(tools), "--allowedTools", &format!("mcp__{}", super::TOOLS_NAME)].map(String::from),
        ),
        None if optional => args.push("--safe-mode".into()),
        None => {}
    }
    args.extend(["--strict-mcp-config".into(), "--disable-slash-commands".into()]);
    if !system.trim().is_empty() {
        args.extend(["--system-prompt".into(), system.to_string()]);
    }
    if !model.trim().is_empty() {
        args.extend(["--model".into(), model.trim().to_string()]);
    }
    // How hard it thinks (low, medium, high, max), when one is chosen.
    if !effort.trim().is_empty() {
        args.extend(["--effort".into(), effort.trim().to_string()]);
    }
    args
}

/// How Claude Code starts the tools of Rune: this program as a connector
/// (`--mcp`), on the same notes, with the file for the record of changes.
pub fn tools_config(tools: &Tools) -> String {
    // (A test names the program: the program of a test is not the app.)
    let program = std::env::var("NOTION2_TOOLS_PROGRAM")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "notion2".into()));
    let mut env = serde_json::Map::new();
    env.insert(crate::mcp::JOURNAL_ENV.into(), Value::String(tools.journal.display().to_string()));
    if let Some(db) = std::env::var_os("NOTION2_DB").filter(|d| !d.is_empty()) {
        env.insert("NOTION2_DB".into(), Value::String(db.to_string_lossy().into_owned()));
    }
    serde_json::json!({ "mcpServers": { super::TOOLS_NAME: { "command": program, "args": ["--mcp"], "env": env } } }).to_string()
}

/// One message for Claude Code (`--input-format stream-json`): the pictures,
/// then the words.
pub fn claude_message(text: &str, pictures: &[Picture]) -> String {
    let mut content: Vec<Value> = pictures
        .iter()
        .map(|p| serde_json::json!({ "type": "image", "source": { "type": "base64", "media_type": p.media, "data": p.data } }))
        .collect();
    content.push(serde_json::json!({ "type": "text", "text": text }));
    serde_json::json!({ "type": "user", "message": { "role": "user", "content": content } }).to_string() + "\n"
}

/// The pictures as files in `dir` (for Codex): the paths of the ones that
/// could be written.
fn picture_files(dir: &Path, pictures: &[Picture]) -> Vec<String> {
    use base64::Engine;
    let mut files = Vec::new();
    for (i, p) in pictures.iter().enumerate() {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(p.data.trim()) else { continue };
        let kind = p.media.rsplit('/').next().unwrap_or("png").replace("jpeg", "jpg");
        let path = dir.join(format!("picture-{}.{kind}", i + 1));
        if std::fs::write(&path, bytes).is_ok() {
            files.push(path.display().to_string());
        }
    }
    files
}

/// Codex runs in a read-only sandbox in an empty folder, prints JSON events
/// and keeps no session files. The prompt is read from stdin ("-").
pub fn codex_args(dir: &Path, model: &str, effort: &str, optional: bool, tools: Option<&Tools>) -> Vec<String> {
    let mut args: Vec<String> = ["exec", "--json", "--skip-git-repo-check", "--sandbox", "read-only", "--color", "never"]
        .map(String::from)
        .to_vec();
    if optional {
        args.push("--ephemeral".into());
    }
    // The tools of Rune: this program as a connector for this run only.
    if let Some(tools) = tools {
        let config: Value = serde_json::from_str(&tools_config(tools)).unwrap_or_default();
        let server = &config["mcpServers"][super::TOOLS_NAME];
        let env = server["env"].as_object().map(|e| e.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")).unwrap_or_default();
        let name = super::TOOLS_NAME;
        for setting in [format!("mcp_servers.{name}.command={}", server["command"]), format!("mcp_servers.{name}.args={}", server["args"]), format!("mcp_servers.{name}.env={{{env}}}")] {
            args.extend(["-c".into(), setting]);
        }
    }
    args.extend(["-C".into(), dir.display().to_string()]);
    if !model.trim().is_empty() {
        args.extend(["-m".into(), model.trim().to_string()]);
    }
    // Codex has no level above "high".
    if !effort.trim().is_empty() {
        let level = if effort.trim() == "max" { "high" } else { effort.trim() };
        args.extend(["-c".into(), format!("model_reasoning_effort=\"{level}\"")]);
    }
    args.push("-".into());
    args
}

/// Codex has no system-prompt flag, so the instructions lead the prompt.
pub fn codex_prompt(system: &str, user: &str, tools: bool) -> String {
    if system.trim().is_empty() {
        return user.to_string();
    }
    let rule = if tools {
        "The tools of Rune are the tools named rune. Use only those. Do not run commands, read files or browse."
    } else {
        "Answer directly from the text below. Do not run commands, read files or browse."
    };
    format!("{system}\n\n{rule}\n\n---\n\n{user}")
}

/// Environment variables a parent Claude Code session sets that would make
/// the child think it is nested or running in a host's special mode.
fn strip_from_env(name: &str) -> bool {
    const KEEP: [&str; 7] = [
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
        "CLAUDE_CODE_SKIP_VERTEX_AUTH",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ];
    name.starts_with("CLAUDE") && !KEEP.contains(&name)
}

// ---------------------------------------------------------------------------
// Output parsing
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Line {
    Delta(String),
    /// The AI used tools: the name of each, and what it gave it.
    Steps(Vec<(String, Value)>),
    /// The AI is writing what it gives to a tool: the tool, the name of the
    /// page when it is known, and how many characters so far.
    Working(String, Option<String>, usize),
    /// The tool reported a failure.
    Fail(String),
    /// The tool finished its turn successfully.
    Finished,
    Ignore,
}

/// Claude Code `--output-format stream-json` lines.
#[derive(Default)]
pub struct ClaudeParser {
    /// The tool call that is being written now, what it is given so far,
    /// and at what length the app was last told.
    tool: Option<String>,
    tool_input: String,
    tool_said: usize,
    streamed_current: bool,
    emitted: bool,
}

/// The "title" in the start of a JSON text that is not whole yet.
fn title_in(partial: &str) -> Option<String> {
    let rest = partial.split_once("\"title\"")?.1.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' | 't' => out.push(' '),
                'u' => return None,
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

impl ClaudeParser {
    pub fn line(&mut self, line: &str) -> Line {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Line::Ignore };
        match v["type"].as_str() {
            Some("stream_event") => {
                let event = &v["event"];
                match event["type"].as_str() {
                    Some("message_start") => {
                        self.streamed_current = false;
                        Line::Ignore
                    }
                    // A tool call starts: its name is known, what it is
                    // given comes in pieces (a long page takes a while).
                    Some("content_block_start") if event["content_block"]["type"] == "tool_use" => {
                        self.tool = event["content_block"]["name"].as_str().map(str::to_string);
                        self.tool_input.clear();
                        self.tool_said = 0;
                        match &self.tool {
                            Some(name) => Line::Working(name.clone(), None, 0),
                            None => Line::Ignore,
                        }
                    }
                    Some("content_block_delta") if event["delta"]["type"] == "input_json_delta" => {
                        let Some(name) = self.tool.clone() else { return Line::Ignore };
                        self.tool_input.push_str(event["delta"]["partial_json"].as_str().unwrap_or(""));
                        // Not at every piece: every few hundred characters.
                        if self.tool_input.len() < self.tool_said + 300 {
                            return Line::Ignore;
                        }
                        self.tool_said = self.tool_input.len();
                        Line::Working(name, title_in(&self.tool_input), self.tool_input.len())
                    }
                    Some("content_block_stop") => {
                        self.tool = None;
                        Line::Ignore
                    }
                    Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                        match event["delta"]["text"].as_str() {
                            Some(t) if !t.is_empty() => {
                                self.streamed_current = true;
                                self.emitted = true;
                                Line::Delta(t.to_string())
                            }
                            _ => Line::Ignore,
                        }
                    }
                    _ => Line::Ignore,
                }
            }
            // A whole message that uses tools: its steps.
            Some("assistant") if v["message"]["content"].as_array().is_some_and(|b| b.iter().any(|b| b["type"] == "tool_use")) => {
                let steps = v["message"]["content"]
                    .as_array()
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter(|b| b["type"] == "tool_use")
                            .filter_map(|b| Some((b["name"].as_str()?.to_string(), b["input"].clone())))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Line::Steps(steps)
            }
            // A whole message: used only when partial messages aren't streamed.
            Some("assistant") if !self.streamed_current => {
                let text: String = v["message"]["content"]
                    .as_array()
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();
                if text.is_empty() || v["message"]["model"] == "<synthetic>" {
                    Line::Ignore
                } else {
                    self.emitted = true;
                    Line::Delta(text)
                }
            }
            Some("result") => {
                let failed = v["is_error"].as_bool().unwrap_or(false)
                    || v["subtype"].as_str().is_some_and(|s| s != "success");
                if failed {
                    let msg = v["result"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .map(String::from)
                        .or_else(|| {
                            v["errors"].as_array().map(|e| {
                                e.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("; ")
                            })
                        })
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| v["subtype"].as_str().unwrap_or("error").to_string());
                    Line::Fail(msg)
                } else if !self.emitted {
                    match v["result"].as_str() {
                        Some(t) if !t.is_empty() => {
                            self.emitted = true;
                            Line::Delta(t.to_string())
                        }
                        _ => Line::Finished,
                    }
                } else {
                    Line::Finished
                }
            }
            _ => Line::Ignore,
        }
    }
}

/// Codex `exec --json` events (current item/turn events and the older
/// `{"msg": {...}}` shape).
#[derive(Default)]
pub struct CodexParser {
    sent: HashMap<String, usize>,
    order: Vec<String>,
}

impl CodexParser {
    fn message_text(&mut self, id: &str, text: &str) -> Line {
        let separator = if !self.sent.contains_key(id) && !self.order.is_empty() { "\n\n" } else { "" };
        if !self.sent.contains_key(id) {
            self.order.push(id.to_string());
        }
        let already = self.sent.get(id).copied().unwrap_or(0);
        let new = if text.len() >= already && text.is_char_boundary(already) { &text[already..] } else { text };
        self.sent.insert(id.to_string(), text.len());
        if new.is_empty() {
            Line::Ignore
        } else {
            Line::Delta(format!("{separator}{new}"))
        }
    }

    pub fn line(&mut self, line: &str) -> Line {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Line::Ignore };
        match v["type"].as_str() {
            Some("item.started" | "item.updated" | "item.completed") => {
                let item = &v["item"];
                // A use of a tool of Rune is a step (said one time, as it starts).
                if item["type"] == "mcp_tool_call" {
                    return match (v["type"].as_str(), item["tool"].as_str()) {
                        (Some("item.started"), Some(tool)) => {
                            Line::Steps(vec![(format!("mcp__{}__{tool}", item["server"].as_str().unwrap_or(super::TOOLS_NAME)), item["arguments"].clone())])
                        }
                        _ => Line::Ignore,
                    };
                }
                if item["type"] != "agent_message" {
                    return Line::Ignore;
                }
                let id = item["id"].as_str().unwrap_or("item").to_string();
                match item["text"].as_str() {
                    Some(t) => self.message_text(&id, t),
                    None => Line::Ignore,
                }
            }
            Some("turn.completed") => Line::Finished,
            Some("turn.failed") => Line::Fail(
                v["error"]["message"].as_str().or(v["error"].as_str()).unwrap_or("the turn failed").to_string(),
            ),
            Some("error") => Line::Fail(v["message"].as_str().unwrap_or("unknown error").to_string()),
            _ => {
                // Older Codex: {"id":"0","msg":{"type":"agent_message","message":"..."}}
                let msg = &v["msg"];
                match msg["type"].as_str() {
                    Some("agent_message_delta") => match msg["delta"].as_str() {
                        Some(d) if !d.is_empty() => {
                            let id = format!("legacy-{}", v["id"].as_str().unwrap_or("0"));
                            let total = self.sent.get(&id).copied().unwrap_or(0) + d.len();
                            if !self.sent.contains_key(&id) {
                                self.order.push(id.clone());
                            }
                            self.sent.insert(id, total);
                            Line::Delta(d.to_string())
                        }
                        _ => Line::Ignore,
                    },
                    Some("agent_message") => {
                        let id = format!("legacy-{}", v["id"].as_str().unwrap_or("0"));
                        match msg["message"].as_str() {
                            Some(t) => self.message_text(&id, t),
                            None => Line::Ignore,
                        }
                    }
                    Some("task_complete") => Line::Finished,
                    Some("error") => Line::Fail(msg["message"].as_str().unwrap_or("unknown error").to_string()),
                    _ => Line::Ignore,
                }
            }
        }
    }
}

enum Parser {
    Claude(ClaudeParser),
    Codex(CodexParser),
}

impl Parser {
    fn line(&mut self, line: &str) -> Line {
        match self {
            Parser::Claude(p) => p.line(line),
            Parser::Codex(p) => p.line(line),
        }
    }
}

/// Turn a tool's raw error into something a person can act on.
pub fn friendly_error(provider: Provider, raw: &str) -> String {
    let raw = raw.trim();
    let lower = raw.to_lowercase();
    let name = provider.name();
    match provider {
        Provider::ClaudeCode
            if lower.contains("/login")
                || lower.contains("not logged in")
                || lower.contains("invalid api key")
                || lower.contains("oauth token")
                || lower.contains("authentication") =>
        {
            "Claude Code isn't signed in. Run `claude` in Terminal once and sign in, then try again.".into()
        }
        Provider::Codex
            if lower.contains("401")
                || lower.contains("unauthorized")
                || lower.contains("not logged in")
                || lower.contains("codex login")
                || lower.contains("please log in") =>
        {
            "Codex isn't signed in. Run `codex login` in Terminal once, then try again.".into()
        }
        _ if lower.contains("usage limit") || lower.contains("rate limit") => {
            format!("{name} has reached its usage limit: {}", clip_chars(raw, 300))
        }
        _ if raw.is_empty() => format!("{name} stopped without a reply."),
        _ => format!("{name} stopped with an error: {}", clip_chars(raw, 400)),
    }
}

fn clip_chars(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

/// The last meaningful lines of stderr (Codex logs noise like skill-loading
/// warnings there).
pub fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !(l.contains(" WARN ") || (l.contains(" ERROR ") && l.contains("failed to load skill"))))
        .collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].join("\n")
}

fn unknown_option(stderr: &str) -> Option<String> {
    let lower = stderr.to_lowercase();
    let at = lower.find("unknown option").or_else(|| lower.find("unexpected argument"))?;
    let rest = &stderr[at..];
    let flag = rest.split(['\'', '"', '`']).nth(1)?;
    Some(flag.to_string())
}

// ---------------------------------------------------------------------------
// Running a request
// ---------------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("notion2-ai-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create a work folder: {e}"))?;
        Ok(TempDir(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The tool may give no line at all for this long; then the run is taken as
/// hung and is stopped. (It waited for ever: the card said "Writing" with
/// its clock running and nothing more came.)
#[cfg(not(test))]
const SILENT_MOST: Duration = Duration::from_secs(300);
#[cfg(test)]
const SILENT_MOST: Duration = Duration::from_secs(2);
/// After the tool said that it is done, it has this long to end by itself.
/// (A tool that gave its whole answer and then did not close kept the card
/// at "Writing" too.)
const END_GRACE: Duration = Duration::from_secs(3);

#[cfg_attr(test, derive(Debug, PartialEq))]
enum Attempt {
    Done,
    Stopped,
    /// The tool rejected a flag; retry without it when it is an optional one.
    UnknownFlag(String),
    Failed(String),
}

/// The flags that an older version of a tool may not know (see
/// `claude_args` and `codex_args`): a run goes on without one that the tool
/// refuses.
const OPTIONAL_FLAGS: [&str; 4] = ["--include-partial-messages", "--no-session-persistence", "--safe-mode", "--ephemeral"];

/// Run one request through an installed tool, streaming its reply into `tx`.
pub async fn run(provider: Provider, model: &str, effort: &str, messages: &[ChatMessage], tools: Option<&Tools>, pictures: &[Picture], tx: &UnboundedSender<AiEvent>) -> Result<(), String> {
    let path = {
        let (otx, orx) = futures_channel::oneshot::channel();
        super::runtime().spawn_blocking(move || {
            let _ = otx.send(locate(provider));
        });
        orx.await.ok().flatten()
    };
    let Some(path) = path else {
        return Err(match provider {
            Provider::ClaudeCode => "Claude Code isn't installed on this Mac. Install it from claude.com/claude-code, or choose another AI in Settings (⌘,).".into(),
            Provider::Codex => "Codex isn't installed on this Mac. Install it with `npm install -g @openai/codex`, or choose another AI in Settings (⌘,).".into(),
        });
    };

    let system = messages.iter().filter(|m| m.role == "system").map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n\n");
    let user = messages.iter().filter(|m| m.role != "system").map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n\n");
    run_at(provider, &path, model, effort, &system, &user, tools, pictures, tx).await
}

/// The tries of one request with the tool at `path`.
async fn run_at(provider: Provider, path: &Path, model: &str, effort: &str, system: &str, user: &str, tools: Option<&Tools>, pictures: &[Picture], tx: &UnboundedSender<AiEvent>) -> Result<(), String> {
    // The optional flags that the tool refused: only those are left out of
    // the next try. (After one refused flag, all of them were left out: also
    // the ones that keep the tool from saving the page's words in its own
    // history, and from loading the person's own settings and hooks.)
    let mut refused: Vec<String> = Vec::new();
    loop {
        match attempt(provider, path, model, effort, system, user, &refused, tools, pictures, tx).await {
            Attempt::Done | Attempt::Stopped => return Ok(()),
            Attempt::Failed(e) => return Err(e),
            Attempt::UnknownFlag(flag) => {
                if OPTIONAL_FLAGS.contains(&flag.as_str()) && !refused.contains(&flag) {
                    refused.push(flag);
                    continue;
                }
                return Err(format!(
                    "This version of {} doesn't support `{flag}`. Update it ({}), then try again.",
                    provider.name(),
                    match provider {
                        Provider::ClaudeCode => "run `claude update` in Terminal",
                        _ => "reinstall the latest version",
                    }
                ));
            }
        }
    }
}

async fn attempt(
    provider: Provider,
    path: &Path,
    model: &str,
    effort: &str,
    system: &str,
    user: &str,
    refused: &[String],
    tools: Option<&Tools>,
    pictures: &[Picture],
    tx: &UnboundedSender<AiEvent>,
) -> Attempt {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => return Attempt::Failed(e),
    };
    // With pictures, the request says so.
    let said;
    let user = if pictures.is_empty() {
        user
    } else {
        said = format!("{user}\n\n(With this request the person gave {} picture{}: look at {}.)", pictures.len(), if pictures.len() == 1 { "" } else { "s" }, if pictures.len() == 1 { "it" } else { "them" });
        said.as_str()
    };
    let (args, input, mut parser) = match provider {
        Provider::ClaudeCode => {
            let mut args = claude_args(system, model, effort, true, tools);
            // Claude Code takes pictures in a message, not in plain words.
            let input = if pictures.is_empty() {
                user.to_string()
            } else {
                args.extend(["--input-format".into(), "stream-json".into()]);
                claude_message(user, pictures)
            };
            (args, input, Parser::Claude(ClaudeParser::default()))
        }
        _ => {
            let mut args = codex_args(&dir.0, model, effort, true, tools);
            // Codex takes pictures as files.
            let files = picture_files(&dir.0, pictures);
            let at = args.len().saturating_sub(1);
            for file in files.into_iter().rev() {
                args.insert(at, file);
                args.insert(at, "-i".into());
            }
            (args, codex_prompt(system, user, tools.is_some()), Parser::Codex(CodexParser::default()))
        }
    };

    // (Each optional flag is a word of its own, with no value after it.)
    let args: Vec<String> = args.into_iter().filter(|a| !refused.contains(a)).collect();
    let mut cmd = tokio::process::Command::new(path);
    cmd.args(&args)
        .current_dir(&dir.0)
        .env("PATH", search_path())
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (name, _) in std::env::vars_os() {
        if let Some(n) = name.to_str() {
            if strip_from_env(n) {
                cmd.env_remove(n);
            }
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Attempt::Failed(format!("Couldn't start {}: {e}", provider.name())),
    };

    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            let _ = stdin.write_all(input.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let stderr_task = child.stderr.take().map(|mut err| {
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf).await;
            let s = String::from_utf8_lossy(&buf).into_owned();
            // Keep only the end; that's where the reason is.
            let skip = s.len().saturating_sub(16_000);
            let mut start = skip;
            while !s.is_char_boundary(start) {
                start += 1;
            }
            s[start..].to_string()
        })
    });
    let Some(stdout) = child.stdout.take() else {
        return Attempt::Failed(format!("Couldn't read {}'s output.", provider.name()));
    };
    let mut lines = BufReader::new(stdout).lines();

    let mut emitted = false;
    let mut finished = false;
    let mut failure: Option<String> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    // When the tool gave its last line, and when it said that it is done.
    let mut heard = Instant::now();
    let mut done_at: Option<Instant> = None;
    let mut hung = false;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                heard = Instant::now();
                match line {
                    Ok(Some(line)) => match parser.line(&line) {
                        Line::Delta(text) => {
                            emitted = true;
                            if tx.unbounded_send(AiEvent::Delta(text)).is_err() {
                                return Attempt::Stopped; // the child is killed when dropped
                            }
                        }
                        Line::Working(tool, title, chars) => {
                            emitted = true;
                            if tx.unbounded_send(AiEvent::Working { tool, title, chars }).is_err() {
                                return Attempt::Stopped;
                            }
                        }
                        Line::Steps(steps) => {
                            emitted = true;
                            for (name, input) in steps {
                                if tx.unbounded_send(AiEvent::Step(name, input)).is_err() {
                                    return Attempt::Stopped;
                                }
                            }
                        }
                        Line::Fail(e) => failure = Some(e),
                        Line::Finished => {
                            finished = true;
                            failure = None;
                            done_at.get_or_insert_with(Instant::now);
                        }
                        Line::Ignore => {}
                    },
                    Ok(None) | Err(_) => break,
                }
            }
            _ = tick.tick() => {
                if tx.is_closed() {
                    return Attempt::Stopped;
                }
                // It said that it is done, and does not end: the answer is whole.
                if done_at.is_some_and(|at| at.elapsed() >= END_GRACE) {
                    break;
                }
                if heard.elapsed() >= SILENT_MOST {
                    hung = true;
                    break;
                }
            }
        }
    }

    // (The tool goes when `child` does: see `kill_on_drop`.)
    if finished && failure.is_none() {
        let _ = child.start_kill();
        return Attempt::Done;
    }
    if hung {
        let _ = child.start_kill();
        let minutes = (SILENT_MOST.as_secs() / 60).max(1);
        // (With the tools of Rune it may have changed things before it
        // hung: the card lists them, with Undo. "Nothing was changed" was
        // said over that list.)
        return Attempt::Failed(format!(
            "{} gave no sign for {minutes} minute{}, so the run was stopped.{} Try again.",
            provider.name(),
            if minutes == 1 { "" } else { "s" },
            if tools.is_some() { "" } else { " Nothing was changed." }
        ));
    }
    // Its end, and what it said of a fault: not waited for without end either.
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status.ok(),
        Err(_) => {
            let _ = child.start_kill();
            None
        }
    };
    let stderr = match stderr_task {
        Some(t) => tokio::time::timeout(Duration::from_secs(3), t).await.ok().and_then(Result::ok).unwrap_or_default(),
        None => String::new(),
    };
    if !emitted {
        if let Some(flag) = unknown_option(&stderr) {
            return Attempt::UnknownFlag(flag);
        }
    }
    if let Some(e) = failure {
        return Attempt::Failed(friendly_error(provider, &e));
    }
    match status {
        // Claude Code ends each answer with a line that says so. With no
        // such line the answer was cut off (the tool was ended from outside,
        // or lost its connection): shown as done, half a page stood there
        // with "Replace" under it.
        Some(s) if s.success() && provider == Provider::ClaudeCode && !finished => {
            Attempt::Failed(format!("{} stopped before its answer was whole. Try again.", provider.name()))
        }
        Some(s) if s.success() => Attempt::Done,
        Some(s) => {
            let tail = stderr_tail(&stderr);
            let detail = if tail.is_empty() {
                format!("exit code {}", s.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".into()))
            } else {
                tail
            };
            Attempt::Failed(friendly_error(provider, &detail))
        }
        None => Attempt::Failed(format!("{} stopped unexpectedly.", provider.name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_path_ignores_shell_noise() {
        let out = "Welcome!\nlast login...\n__N2_PATH_START__/opt/homebrew/bin:/usr/bin__N2_PATH_END__\nbye\n";
        assert_eq!(parse_marked_path(out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse_marked_path("no markers here"), None);
        assert_eq!(parse_marked_path("__N2_PATH_START____N2_PATH_END__"), None);
        // An echoed command shows the split markers, never the joined ones.
        let echoed = "+zsh:1> printf '\\n%s%s%s%s%s\\n' __N2_PATH_ START__ /bin __N2_PATH_ END__\n__N2_PATH_START__/bin__N2_PATH_END__\n";
        assert_eq!(parse_marked_path(echoed).as_deref(), Some("/bin"));
    }

    #[test]
    fn paths_merge_without_duplicates() {
        assert_eq!(merge_paths(["/a:/b", "/b:/c:", ":/a:/d"]), "/a:/b:/c:/d");
    }

    #[test]
    fn versions_parse() {
        assert_eq!(parse_version("2.1.319 (Claude Code)\n").as_deref(), Some("2.1.319"));
        assert_eq!(parse_version("codex-cli 0.167.0").as_deref(), Some("0.167.0"));
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn claude_stream_with_partial_messages() {
        let mut p = ClaudeParser::default();
        let lines = [
            r#"{"type":"system","subtype":"init","tools":[]}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"po"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ng"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}"#,
            r#"{"duration_ms":1,"type":"result","subtype":"success","is_error":false,"result":"pong"}"#,
        ];
        let out: Vec<Line> = lines.iter().map(|l| p.line(l)).collect();
        assert_eq!(
            out,
            vec![
                Line::Ignore,
                Line::Ignore,
                Line::Delta("po".into()),
                Line::Delta("ng".into()),
                Line::Ignore,
                Line::Finished
            ]
        );
    }

    /// A stand-in for an AI tool: a shell script that takes the request and
    /// prints the lines it is given, then does what `then` says.
    fn stand_in(name: &str, lines: &[&str], then: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("n2-stand-in-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tool.sh");
        let mut script = String::from("#!/bin/sh\ncat > /dev/null\n");
        for line in lines {
            script.push_str(&format!("printf '%s\\n' '{line}'\n"));
        }
        script.push_str(then);
        script.push('\n');
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_tool_that_does_not_end_does_not_hold_the_run_for_ever() {
        let run = |path: PathBuf| {
            let (tx, rx) = futures_channel::mpsc::unbounded();
            let started = Instant::now();
            let how = super::super::runtime().block_on(async { attempt(Provider::ClaudeCode, &path, "", "", "", "Hello", &[], None, &[], &tx).await });
            drop(tx);
            let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
            (how, events, started.elapsed())
        };
        // It gives its whole answer, says that it is done, and then stays: the answer counts.
        let text = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#;
        let done = r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello"}"#;
        let (how, events, took) = run(stand_in("stays", &[text, done], "sleep 60"));
        assert_eq!(how, Attempt::Done);
        assert!(events.contains(&AiEvent::Delta("Hello".into())), "{events:?}");
        assert!(took < Duration::from_secs(10), "{took:?}");
        // It starts to answer and then gives no sign: the run is stopped, and says why.
        let (how, _, took) = run(stand_in("silent", &[text], "sleep 60"));
        assert!(matches!(&how, Attempt::Failed(why) if why.contains("gave no sign")), "{how:?}");
        assert!(took < Duration::from_secs(10), "{took:?}");
        // One that ends as it must is as before.
        let (how, _, took) = run(stand_in("ends", &[text, done], "exit 0"));
        assert_eq!(how, Attempt::Done);
        assert!(took < Duration::from_secs(5), "{took:?}");
        // It ends with no line that says the answer is whole: not done.
        let (how, _, _) = run(stand_in("cut", &[text], "exit 0"));
        assert!(matches!(&how, Attempt::Failed(why) if why.contains("before its answer was whole")), "{how:?}");
        // A run with no tools that hung changed nothing, and says so; one
        // with the tools of Rune may have, and does not say that.
        let (how, _, _) = run(stand_in("silent-plain", &[text], "sleep 60"));
        assert!(matches!(&how, Attempt::Failed(why) if why.contains("Nothing was changed")), "{how:?}");
        let tools = Tools { journal: std::env::temp_dir().join("n2-stand-in-none.jsonl") };
        let path = stand_in("silent-tools", &[text], "sleep 60");
        let (tx, _rx) = futures_channel::mpsc::unbounded();
        let how = super::super::runtime().block_on(async { attempt(Provider::ClaudeCode, &path, "", "", "", "Hello", &[], Some(&tools), &[], &tx).await });
        assert!(matches!(&how, Attempt::Failed(why) if why.contains("gave no sign") && !why.contains("Nothing was changed")), "{how:?}");
    }

    #[test]
    fn a_refused_flag_is_left_out_and_the_others_stay() {
        // A tool that does not know "--safe-mode", and says what it was started with.
        let says = r#"case " $* " in *" --safe-mode "*) echo "error: unknown option '--safe-mode'" >&2; exit 1;; esac
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"%s"}]}}\n' "$*"
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"x"}'"#;
        let path = stand_in("refuses", &[], says);
        let (tx, rx) = futures_channel::mpsc::unbounded();
        let how = super::super::runtime().block_on(async { run_at(Provider::ClaudeCode, &path, "", "", "", "Hello", None, &[], &tx).await });
        drop(tx);
        assert_eq!(how, Ok(()));
        let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
        let started_with: String = events.iter().filter_map(|e| if let AiEvent::Delta(text) = e { Some(text.as_str()) } else { None }).collect();
        assert!(!started_with.contains("--safe-mode"), "{started_with}");
        // The flag that keeps the request out of the tool's own history is still given.
        assert!(started_with.contains("--no-session-persistence") && started_with.contains("--include-partial-messages"), "{started_with}");
        // A flag that is not an optional one is said to the person, with no second try.
        let never = r#"echo "error: unknown option '--strict-mcp-config'" >&2; exit 1"#;
        let path = stand_in("refuses-all", &[], never);
        let (tx, _rx) = futures_channel::mpsc::unbounded();
        let how = super::super::runtime().block_on(async { run_at(Provider::ClaudeCode, &path, "", "", "", "Hello", None, &[], &tx).await });
        assert!(matches!(&how, Err(why) if why.contains("doesn't support `--strict-mcp-config`")), "{how:?}");
    }

    #[test]
    fn claude_without_partial_messages_uses_whole_message() {
        let mut p = ClaudeParser::default();
        assert_eq!(
            p.line(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#),
            Line::Delta("Hello".into())
        );
        assert_eq!(p.line(r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello"}"#), Line::Finished);
        // Only a result line: its text is the reply.
        let mut p = ClaudeParser::default();
        assert_eq!(p.line(r#"{"type":"result","subtype":"success","result":"Hi"}"#), Line::Delta("Hi".into()));
    }

    #[test]
    fn claude_errors() {
        let mut p = ClaudeParser::default();
        assert_eq!(
            p.line(r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#),
            Line::Fail("Not logged in · Please run /login".into())
        );
        assert_eq!(
            p.line(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["boom"]}"#),
            Line::Fail("boom".into())
        );
        assert!(friendly_error(Provider::ClaudeCode, "Not logged in · Please run /login").contains("isn't signed in"));
    }

    #[test]
    fn codex_events() {
        let mut p = CodexParser::default();
        let lines = [
            r#"{"type":"thread.started","thread_id":"t"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"thinking"}}"#,
            r#"{"type":"item.updated","item":{"id":"item_1","type":"agent_message","text":"po"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"pong"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"again"}}"#,
            r#"{"type":"turn.completed","usage":{}}"#,
        ];
        let out: Vec<Line> = lines.iter().map(|l| p.line(l)).collect();
        assert_eq!(
            out,
            vec![
                Line::Ignore,
                Line::Ignore,
                Line::Ignore,
                Line::Delta("po".into()),
                Line::Delta("ng".into()),
                Line::Delta("\n\nagain".into()),
                Line::Finished
            ]
        );
        assert_eq!(
            p.line(r#"{"type":"turn.failed","error":{"message":"401 Unauthorized"}}"#),
            Line::Fail("401 Unauthorized".into())
        );
        assert_eq!(p.line(r#"{"type":"error","message":"stream closed"}"#), Line::Fail("stream closed".into()));
        assert!(friendly_error(Provider::Codex, "401 Unauthorized").contains("codex login"));
    }

    #[test]
    fn codex_legacy_events() {
        let mut p = CodexParser::default();
        assert_eq!(p.line(r#"{"id":"0","msg":{"type":"agent_message_delta","delta":"Hi"}}"#), Line::Delta("Hi".into()));
        assert_eq!(p.line(r#"{"id":"0","msg":{"type":"agent_message","message":"Hi there"}}"#), Line::Delta(" there".into()));
        assert_eq!(p.line(r#"{"id":"0","msg":{"type":"task_complete"}}"#), Line::Finished);
    }

    #[test]
    fn command_lines() {
        let a = claude_args("Be brief.", "", "", true, None);
        let tools = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[tools + 1], "");
        assert!(a.contains(&"--safe-mode".to_string()) && a.contains(&"--no-session-persistence".to_string()));
        assert!(a.windows(2).any(|w| w[0] == "--system-prompt" && w[1] == "Be brief."));
        assert!(!a.contains(&"--model".to_string()) && !a.contains(&"--effort".to_string()));
        let b = claude_args("", "sonnet", "high", false, None);
        // With the tools of Rune: this program as the one connector, its tools allowed, no safe mode (it would switch them off).
        let tools = Tools { journal: PathBuf::from("/tmp/run.jsonl") };
        let t = claude_args("Be brief.", "", "", true, Some(&tools));
        assert!(!t.contains(&"--safe-mode".to_string()) && t.contains(&"--strict-mcp-config".to_string()));
        assert!(t.windows(2).any(|w| w[0] == "--tools" && w[1].is_empty()) && t.windows(2).any(|w| w[0] == "--setting-sources" && w[1].is_empty()));
        assert!(t.windows(2).any(|w| w[0] == "--allowedTools" && w[1] == "mcp__rune"));
        let config: Value = serde_json::from_str(&t[t.iter().position(|x| x == "--mcp-config").unwrap() + 1]).unwrap();
        assert_eq!(config["mcpServers"]["rune"]["args"][0], "--mcp");
        assert_eq!(config["mcpServers"]["rune"]["env"]["NOTION2_AI_JOURNAL"], "/tmp/run.jsonl");
        // A use of a tool in the answer of Claude Code is a step.
        let mut parser = ClaudeParser::default();
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"mcp__rune__create_page","input":{"title":"Fitness"}}]}}"#;
        assert!(matches!(parser.line(line), Line::Steps(steps) if steps.len() == 1 && steps[0].0 == "mcp__rune__create_page" && steps[0].1["title"] == "Fitness"));
        // While it writes what it gives to a tool: the tool at once, then how far it is, with the name of the page.
        let mut parser = ClaudeParser::default();
        let start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"mcp__rune__create_page","input":{}}}}"#;
        assert!(matches!(parser.line(start), Line::Working(tool, None, 0) if tool == "mcp__rune__create_page"));
        let piece = |text: &str| serde_json::json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": text}}}).to_string();
        assert!(matches!(parser.line(&piece("{\"title\": \"Trading jour")), Line::Ignore));
        let long = format!("nal\", \"markdown\": \"{}", "x".repeat(400));
        assert!(matches!(parser.line(&piece(&long)), Line::Working(tool, Some(title), chars) if tool == "mcp__rune__create_page" && title == "Trading journal" && chars > 400));
        assert!(matches!(parser.line(&piece("y")), Line::Ignore));
        assert_eq!(title_in("{\"title\":\"A \\\"good\\\" day\",\"mark"), Some("A \"good\" day".to_string()));
        assert_eq!(title_in("{\"title\":\"Not whole"), None);
        assert!(b.windows(2).any(|w| w[0] == "--effort" && w[1] == "high"));
        assert!(!b.contains(&"--safe-mode".to_string()) && b.contains(&"--tools".to_string()));
        assert!(b.windows(2).any(|w| w[0] == "--model" && w[1] == "sonnet"));

        let c = codex_args(Path::new("/tmp/x"), "", "", true, None);
        // Codex with the tools of Rune: the connector is set for this run, and a use of a tool is a step.
        let ct = codex_args(Path::new("/tmp/x"), "", "", true, Some(&tools));
        assert!(ct.windows(2).any(|w| w[0] == "-c" && w[1].starts_with("mcp_servers.rune.command=\"")));
        assert!(ct.windows(2).any(|w| w[0] == "-c" && w[1] == "mcp_servers.rune.args=[\"--mcp\"]"));
        assert!(ct.windows(2).any(|w| w[0] == "-c" && w[1].starts_with("mcp_servers.rune.env={") && w[1].contains("NOTION2_AI_JOURNAL=\"/tmp/run.jsonl\"")), "{ct:?}");
        assert_eq!(ct.last().map(String::as_str), Some("-"));
        let mut cp = CodexParser::default();
        let started = r#"{"type":"item.started","item":{"id":"item_1","type":"mcp_tool_call","server":"rune","tool":"create_page","arguments":{"title":"Fitness"},"status":"in_progress"}}"#;
        assert!(matches!(cp.line(started), Line::Steps(steps) if steps[0].0 == "mcp__rune__create_page" && steps[0].1["title"] == "Fitness"));
        assert!(matches!(cp.line(&started.replace("item.started", "item.completed")), Line::Ignore));
        assert!(codex_prompt("Be brief.", "Hello", true).contains("tools named rune") && codex_prompt("Be brief.", "Hello", false).contains("Answer directly"));
        // A picture goes to Claude Code in a message: the picture first, then the words.
        let shot = Picture { id: "p1".into(), media: "image/png".into(), data: "AAAA".into() };
        let message: Value = serde_json::from_str(&claude_message("What is this?", &[shot.clone()])).unwrap();
        assert_eq!(message["type"], "user");
        assert_eq!(message["message"]["content"][0]["source"]["media_type"], "image/png");
        assert_eq!(message["message"]["content"][0]["source"]["data"], "AAAA");
        assert_eq!(message["message"]["content"][1]["text"], "What is this?");
        // …and to Codex as a file.
        let dir = std::env::temp_dir().join(format!("n2-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let files = picture_files(&dir, &[shot]);
        assert!(files.len() == 1 && files[0].ends_with("picture-1.png") && std::fs::read(&files[0]).unwrap() == vec![0, 0, 0]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!c.iter().any(|x| x.contains("reasoning_effort")));
        let d = codex_args(Path::new("/tmp/x"), "", "max", true, None);
        assert!(d.windows(2).any(|w| w[0] == "-c" && w[1] == "model_reasoning_effort=\"high\"") && d.last().map(String::as_str) == Some("-"));
        assert!(c.windows(2).any(|w| w[0] == "--sandbox" && w[1] == "read-only"));
        assert_eq!(c.last().map(String::as_str), Some("-"));
        assert!(c.contains(&"--ephemeral".to_string()));
        assert!(codex_prompt("SYS", "Hello", false).starts_with("SYS") && codex_prompt("SYS", "Hello", false).ends_with("Hello"));
    }

    /// Sends one tiny prompt through every installed tool, exactly as the app
    /// does. Uses the signed-in account, so it only runs when asked:
    /// `cargo test live_installed_tools -- --ignored --nocapture`
    #[test]
    #[ignore = "calls the installed AI tools"]
    fn live_installed_tools() {
        let found = installed(true);
        eprintln!("PATH used: {}", search_path());
        eprintln!("found: {found:?}");
        assert!(!found.is_empty(), "no AI tool found");
        for tool in found {
            let prefs = super::super::Prefs { provider: Some(tool.provider), ..Default::default() };
            let messages = vec![
                ChatMessage::system("You are the writing assistant inside a notes app."),
                ChatMessage::user("Reply with the single word: pong"),
            ];
            let started = Instant::now();
            let rx = super::super::start(prefs, messages);
            let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
            eprintln!("{:?} in {:.1}s: {events:?}", tool.provider, started.elapsed().as_secs_f32());
            let text: String = events
                .iter()
                .filter_map(|e| match e {
                    AiEvent::Delta(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(events.last(), Some(&AiEvent::Done));
            assert!(text.to_lowercase().contains("pong"), "{text}");
        }
    }

    /// The real tool, asked for pages of their own: its reply must be one
    /// that the app can turn into a changed page with linked sub-pages.
    #[test]
    #[ignore = "calls the installed AI tools"]
    fn live_sub_pages() {
        use super::super::{build_messages, clean_reply, link_pages, split_pages, split_revision, AiAction};
        let Some(tool) = installed(true).into_iter().next() else { panic!("no AI tool found") };
        let page = "# Workout Plan\n\n## Overview\n\nA three day plan.\n\n## Workout Details\n\n### Workout A: Full body\n\n- 20 burpees\n- 15 pull-ups\n- 40 jump squats\n\n### Workout B: Upper body\n\n- 25 diamond push-ups\n- 12 chin-ups\n\n### Workout C: Legs\n\n- 30 pistol squats\n- 50 lunges\n\n## Workouts\n\nTo finish setting this up, turn each workout under Workout Details into its own subpage, then link it in the Workouts list below.\n";
        let asked = "This is too hard. Make it easier, keep it to calisthenics, and make this page the main page with a sub-page for each workout, linked from the Workouts list.";
        let prefs = super::super::Prefs { provider: Some(tool.provider), ..Default::default() };
        let started = Instant::now();
        let rx = super::super::start(prefs, build_messages(&AiAction::Ask, page, "", asked));
        let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                AiEvent::Delta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        eprintln!("{:?} in {:.1}s:\n{text}", tool.provider, started.elapsed().as_secs_f32());
        assert_eq!(events.last(), Some(&AiEvent::Done));
        let body = split_revision(&clean_reply(&text).0).expect("the reply changes the page");
        let (main, pages) = split_pages(&body);
        assert_eq!(pages.len(), 3, "one sub-page for each workout: {:?}", pages.iter().map(|(n, _)| n).collect::<Vec<_>>());
        assert!(pages.iter().all(|(_, text)| text.trim().len() > 20), "each sub-page has its workout");
        let made: Vec<(String, String)> = pages.iter().enumerate().map(|(i, (n, _))| (n.clone(), format!("p{i}"))).collect();
        let linked = link_pages(&main, &made);
        let found = made.iter().filter(|(_, id)| linked.contains(&format!("notion2://page/{id}"))).count();
        eprintln!("linked in the page: {found} of {}\n{linked}", made.len());
        assert_eq!(found, 3, "the page links to each sub-page by its name");
        assert!(!main.contains("turn each workout"), "the page no longer tells the person to do it");
    }

    /// The real tool with the tools of Rune, on notes of its own: it must
    /// do the work itself, and the record must say what it did.
    /// Build the app first (`cargo build`): its program serves the tools.
    #[test]
    #[ignore = "calls the installed AI tools"]
    fn live_agent_tools() {
        use super::super::{build_agent_messages, start_with, Tools};
        let dir = std::env::temp_dir().join(format!("n2-live-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("ws.db");
        let journal = dir.join("run.jsonl");
        let program = std::env::current_dir().unwrap().join("target/debug/notion2");
        assert!(program.exists(), "build the app first: {}", program.display());
        std::env::set_var("NOTION2_DB", &db);
        std::env::set_var("NOTION2_TOOLS_PROGRAM", &program);
        // The notes: a page the person is on, and two pages about running.
        let store = crate::store::Store::open(db.clone()).unwrap();
        let mut server = crate::mcp::Server::new(store);
        let id_of = |v: serde_json::Value| serde_json::from_str::<Value>(v["content"][0]["text"].as_str().unwrap()).unwrap()["id"].as_str().unwrap().to_string();
        let here = id_of(server.tool("create_page", &serde_json::json!({ "title": "Workout Plan", "markdown": "Three days a week." })));
        server.tool("create_page", &serde_json::json!({ "title": "Run log March", "markdown": "5 km on Monday." }));
        server.tool("create_page", &serde_json::json!({ "title": "Run log April", "markdown": "8 km on Friday." }));
        drop(server);

        let asked = "Make a new page named Fitness at the top level, and move the two run log pages into it. Then add a run to my calendar for tomorrow at 7 in the morning.";
        let messages = build_agent_messages(&here, "Workout Plan", "# Workout Plan\n\nThree days a week.", asked, "2026-11-10 17:30, Tuesday");
        let prefs = super::super::Prefs { provider: Some(Provider::ClaudeCode), ..Default::default() };
        let started = Instant::now();
        let rx = start_with(prefs, messages, Some(Tools { journal: journal.clone() }), Vec::new());
        let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
        let steps: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                AiEvent::Step(name, input) => Some(super::super::step_label(name, input, |_| None)),
                _ => None,
            })
            .collect();
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                AiEvent::Delta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let changes = crate::mcp::read_journal(&journal);
        let said = crate::mcp::describe(&changes);
        eprintln!("in {:.1}s\nsteps: {steps:#?}\nrecord: {said:#?}\ncount: {}\nanswer: {text}", started.elapsed().as_secs_f32(), crate::mcp::count_line(&changes));
        assert_eq!(events.last(), Some(&AiEvent::Done), "{events:?}");
        assert!(said.iter().any(|l| l == "Made the page “Fitness”"), "{said:?}");
        assert_eq!(said.iter().filter(|l| l.starts_with("Moved the page “Run log")).count(), 2, "{said:?}");
        assert!(said.iter().any(|l| l.starts_with("Added “") && l.ends_with("to the calendar")), "{said:?}");
        let store = crate::store::Store::open(db.clone()).unwrap();
        let (pages, _) = store.load().unwrap();
        let fitness = pages.values().find(|p| p.title == "Fitness").expect("the page Fitness").id.clone();
        assert_eq!(pages.values().filter(|p| p.parent_id.as_deref() == Some(fitness.as_str())).count(), 2);
        let event = store.load_events().unwrap().pop().expect("an event");
        assert_eq!(crate::when::iso(event.start, false), "2026-11-11T07:00", "{event:?}");
        // All of it can be taken back.
        crate::mcp::undo_changes(&store, &changes).unwrap();
        let (pages, _) = store.load().unwrap();
        assert!(pages[&fitness].trashed && pages.values().filter(|p| p.title.starts_with("Run log")).all(|p| p.parent_id.is_none()));
        assert!(store.load_events().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real tool asked for a tracker: the table it makes must be a
    /// database (kinds, coloured options, money, totals, a name), not a
    /// plain grid. `N2_ASK` changes the request; `N2_KEEP` names a folder
    /// to keep the notes in (to look at them in the app).
    #[test]
    #[ignore = "calls the installed AI tools"]
    fn live_agent_table() {
        use super::super::{build_agent_messages, start_with, Tools};
        let dir = std::env::var_os("N2_KEEP").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("n2-live-table-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("ws.db");
        let journal = dir.join("run.jsonl");
        let program = std::env::current_dir().unwrap().join("target/debug/notion2");
        assert!(program.exists(), "build the app first: {}", program.display());
        std::env::set_var("NOTION2_DB", &db);
        std::env::set_var("NOTION2_TOOLS_PROGRAM", &program);
        let store = crate::store::Store::open(db.clone()).unwrap();
        let mut server = crate::mcp::Server::new(store);
        let id_of = |v: serde_json::Value| serde_json::from_str::<Value>(v["content"][0]["text"].as_str().unwrap()).unwrap()["id"].as_str().unwrap().to_string();
        let here = id_of(server.tool("create_page", &serde_json::json!({ "title": "Home", "markdown": "My notes." })));
        drop(server);

        let asked = std::env::var("N2_ASK").unwrap_or_else(|_| "Make me a trading journal to log my trades.".to_string());
        let messages = build_agent_messages(&here, "Home", "# Home\n\nMy notes.", &asked, "2026-11-11 09:30, Wednesday");
        let prefs = super::super::Prefs { provider: Some(Provider::ClaudeCode), ..Default::default() };
        let started = Instant::now();
        let rx = start_with(prefs, messages, Some(Tools { journal }), Vec::new());
        let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
        let text: String = events.iter().filter_map(|e| if let AiEvent::Delta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(events.last(), Some(&AiEvent::Done), "{events:?}");
        let store = crate::store::Store::open(db.clone()).unwrap();
        let (pages, blocks) = store.load().unwrap();
        let mut tables = Vec::new();
        for page in pages.values().filter(|p| !p.trashed && p.id != here) {
            let md = crate::markdown::from_blocks(&page.title, blocks.get(&page.id).map(Vec::as_slice).unwrap_or(&[]), |id| id.to_string());
            eprintln!("===== {} =====\n{md}\n", page.title);
            for block in blocks.get(&page.id).into_iter().flatten().filter(|b| b.kind == crate::model::BlockKind::Table) {
                tables.push(crate::table::Table::parse(&block.data));
            }
        }
        eprintln!("in {:.1}s\nanswer: {text}", started.elapsed().as_secs_f32());
        // The pages a reply asks for are made by the app (see `split_pages`).
        if let Some(body) = super::super::split_revision(&text) {
            let (main, made) = super::super::split_pages(&body);
            for md in std::iter::once(main).chain(made.into_iter().map(|(_, md)| md)) {
                for block in crate::markdown::to_blocks(&md).into_iter().filter(|b| b.kind == crate::model::BlockKind::Table) {
                    tables.push(crate::table::Table::parse(&block.data));
                }
            }
        }
        if let Some(keep) = std::env::var_os("N2_KEEP") {
            let _ = std::fs::write(std::path::Path::new(&keep).join("answer.md"), &text);
        }
        let table = tables.first().expect("a table");
        eprintln!("view: {:?}\nicon: {}", table.view, table.icon);
        assert!(!table.name.is_empty(), "the table has a name");
        assert!((0..table.columns()).any(|c| !table.column(c).options.is_empty() && table.column(c).options.iter().any(|o| o.color != crate::table::Color::Default)), "a choice column with colours");
        assert!((0..table.columns()).any(|c| table.kind(c) == crate::table::Kind::Date), "a date column");
        assert!(std::env::var_os("N2_ASK").is_some() || (0..table.columns()).any(|c| !table.column(c).format.is_empty()), "a money column");
        assert!((0..table.columns()).any(|c| table.column(c).total.is_some()), "a total at the foot");
    }

    /// The real tool on one request, with the time of each thing it sends:
    /// to see where a slow run spends its time. The request, the model and
    /// the strength come from N2_LIVE_REQUEST, N2_LIVE_MODEL, N2_LIVE_EFFORT.
    /// Build the app first (`cargo build`).
    #[test]
    #[ignore = "calls the installed AI tools"]
    fn live_agent_timeline() {
        use super::super::{build_agent_messages, start_with, Tools};
        let dir = std::env::temp_dir().join(format!("n2-live-time-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("ws.db");
        let journal = dir.join("run.jsonl");
        let program = std::env::current_dir().unwrap().join("target/debug/notion2");
        assert!(program.exists(), "build the app first: {}", program.display());
        std::env::set_var("NOTION2_DB", &db);
        std::env::set_var("NOTION2_TOOLS_PROGRAM", &program);
        let store = crate::store::Store::open(db.clone()).unwrap();
        let mut server = crate::mcp::Server::new(store);
        let made = server.tool("create_page", &serde_json::json!({ "title": "" }));
        let here = serde_json::from_str::<Value>(made["content"][0]["text"].as_str().unwrap()).unwrap()["id"].as_str().unwrap().to_string();
        drop(server);
        let asked = std::env::var("N2_LIVE_REQUEST").unwrap_or_else(|_| "create me a trading journal".into());
        let model = std::env::var("N2_LIVE_MODEL").unwrap_or_default();
        let effort = std::env::var("N2_LIVE_EFFORT").unwrap_or_default();
        let messages = build_agent_messages(&here, "", "", &asked, "2026-11-10 19:50, Tuesday");
        let prefs = super::super::Prefs { provider: Some(Provider::ClaudeCode), claude_model: model.clone(), effort: effort.clone(), ..Default::default() };
        let started = Instant::now();
        let mut rx = start_with(prefs, messages, Some(Tools { journal: journal.clone() }), Vec::new());
        let mut text = String::new();
        let mut first_text = None;
        let mut steps = 0;
        super::super::runtime().block_on(async {
            while let Some(event) = futures_util::StreamExt::next(&mut rx).await {
                let at = started.elapsed().as_secs_f32();
                match event {
                    AiEvent::Delta(t) => {
                        if first_text.is_none() {
                            first_text = Some(at);
                            eprintln!("{at:6.1}s  first words");
                        }
                        text.push_str(&t);
                    }
                    AiEvent::Step(name, input) => {
                        steps += 1;
                        eprintln!("{at:6.1}s  step {steps}: {}", super::super::step_label(&name, &input, |_| None));
                    }
                    AiEvent::Working { tool, title, chars } => eprintln!("{at:6.1}s    {}", super::super::working_label(&tool, title.as_deref(), chars)),
                    AiEvent::Done => eprintln!("{at:6.1}s  done"),
                    AiEvent::Error(e) => eprintln!("{at:6.1}s  ERROR {e}"),
                }
            }
        });
        let changes = crate::mcp::read_journal(&journal);
        eprintln!("model {model:?}, strength {effort:?}: {:.1}s in all, {steps} steps\nrecord: {}\nanswer:\n{}", started.elapsed().as_secs_f32(), crate::mcp::count_line(&changes), text.chars().take(1200).collect::<String>());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real tool with a picture: it must say what is in it. The picture
    /// is the file named by N2_LIVE_PICTURE (a PNG), the words that must be
    /// in the answer are in N2_LIVE_EXPECT.
    #[test]
    #[ignore = "calls the installed AI tools"]
    fn live_picture() {
        use base64::Engine;
        let file = std::env::var("N2_LIVE_PICTURE").expect("N2_LIVE_PICTURE names a PNG file");
        let expect = std::env::var("N2_LIVE_EXPECT").unwrap_or_default();
        let data = base64::engine::general_purpose::STANDARD.encode(std::fs::read(&file).unwrap());
        let picture = Picture { id: "p1".into(), media: "image/png".into(), data };
        let messages = super::super::build_messages(&super::super::AiAction::Ask, "# Notes", "", "What words are in the picture? Write them out.");
        for provider in [Provider::ClaudeCode, Provider::Codex] {
            if locate(provider).is_none() {
                continue;
            }
            let prefs = super::super::Prefs { provider: Some(provider), ..Default::default() };
            let started = Instant::now();
            let rx = super::super::start_with(prefs, messages.clone(), None, vec![picture.clone()]);
            let events: Vec<AiEvent> = super::super::runtime().block_on(futures_util::StreamExt::collect(rx));
            let text: String = events
                .iter()
                .filter_map(|e| match e {
                    AiEvent::Delta(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            eprintln!("{provider:?} in {:.1}s: {text} | last: {:?}", started.elapsed().as_secs_f32(), events.last());
            if provider == Provider::ClaudeCode {
                assert_eq!(events.last(), Some(&AiEvent::Done));
                assert!(text.to_lowercase().contains(&expect.to_lowercase()), "{text}");
            }
        }
    }

    #[test]
    fn stderr_and_flags() {
        assert_eq!(unknown_option("error: unknown option '--safe-mode'").as_deref(), Some("--safe-mode"));
        assert_eq!(unknown_option("error: unexpected argument '--ephemeral' found").as_deref(), Some("--ephemeral"));
        assert_eq!(unknown_option("all good"), None);
        let noisy = "2026 ERROR codex_core::session: failed to load skill /x: bad\nreal problem\n";
        assert_eq!(stderr_tail(noisy), "real problem");
        assert!(strip_from_env("CLAUDECODE") && strip_from_env("CLAUDE_CODE_SIMPLE") && strip_from_env("CLAUDE_CODE_ENTRYPOINT"));
        assert!(!strip_from_env("CLAUDE_CONFIG_DIR") && !strip_from_env("ANTHROPIC_API_KEY") && !strip_from_env("PATH"));
    }
}
