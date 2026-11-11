//! AI tools already installed on this Mac (Claude Code and Codex), run headless.
//!
//! An app opened from Finder starts with a bare PATH, so tools are found by
//! asking the user's login shell for its PATH (once) and by checking the usual
//! install folders. Each request runs the tool in a fresh empty folder with all
//! of its tools switched off (Claude Code) or a read-only sandbox (Codex), feeds
//! the prompt on stdin and streams the reply back. Dropping the receiver kills
//! the process.

use super::{AiEvent, ChatMessage, Provider, Tools};
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
    /// The tool reported a failure.
    Fail(String),
    /// The tool finished its turn successfully.
    Finished,
    Ignore,
}

/// Claude Code `--output-format stream-json` lines.
#[derive(Default)]
pub struct ClaudeParser {
    streamed_current: bool,
    emitted: bool,
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

enum Attempt {
    Done,
    Stopped,
    /// The tool rejected a flag; retry without the optional ones.
    UnknownFlag(String),
    Failed(String),
}

/// Run one request through an installed tool, streaming its reply into `tx`.
pub async fn run(provider: Provider, model: &str, effort: &str, messages: &[ChatMessage], tools: Option<&Tools>, tx: &UnboundedSender<AiEvent>) -> Result<(), String> {
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

    let mut optional = true;
    loop {
        match attempt(provider, &path, model, effort, &system, &user, optional, tools, tx).await {
            Attempt::Done | Attempt::Stopped => return Ok(()),
            Attempt::Failed(e) => return Err(e),
            Attempt::UnknownFlag(flag) => {
                if optional && flag != "--tools" {
                    optional = false;
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
    optional: bool,
    tools: Option<&Tools>,
    tx: &UnboundedSender<AiEvent>,
) -> Attempt {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => return Attempt::Failed(e),
    };
    let (args, input, mut parser) = match provider {
        Provider::ClaudeCode => (claude_args(system, model, effort, optional, tools), user.to_string(), Parser::Claude(ClaudeParser::default())),
        _ => (codex_args(&dir.0, model, effort, optional, tools), codex_prompt(system, user, tools.is_some()), Parser::Codex(CodexParser::default())),
    };

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
    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => match parser.line(&line) {
                        Line::Delta(text) => {
                            emitted = true;
                            if tx.unbounded_send(AiEvent::Delta(text)).is_err() {
                                return Attempt::Stopped; // the child is killed when dropped
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
            }
        }
    }

    let status = child.wait().await.ok();
    let stderr = match stderr_task {
        Some(t) => t.await.unwrap_or_default(),
        None => String::new(),
    };
    if finished && failure.is_none() {
        return Attempt::Done;
    }
    if !emitted {
        if let Some(flag) = unknown_option(&stderr) {
            return Attempt::UnknownFlag(flag);
        }
    }
    if let Some(e) = failure {
        return Attempt::Failed(friendly_error(provider, &e));
    }
    match status {
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
        let rx = start_with(prefs, messages, Some(Tools { journal: journal.clone() }));
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
