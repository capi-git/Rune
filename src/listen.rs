//! Listening to a meeting (or to the person, for the voice box).
//!
//! The hearing is done by a small program of its own, `n2listen`
//! (helper/n2listen.swift, built into the app beside this one): it hears the
//! microphone and, for a meeting, the sound the Mac plays, turns speech into
//! text on this Mac, and prints what it hears a line at a time. This module
//! starts it, reads those lines, and keeps the transcript.
//!
//! Nothing here runs until the person asks to be listened to, so there is no
//! cost while the app is merely open.

use crate::model::{Block, BlockKind, Id};
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    /// The microphone.
    Me,
    /// What the Mac plays: the other people in a meeting.
    Others,
}

/// What the listener says.
#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    /// It has begun. `others`: it hears the Mac's sound too.
    Listening { others: bool },
    /// Words still being worked out (they may change).
    Partial { who: Who, text: String },
    /// A finished stretch of speech, `at` seconds after it began.
    Final { who: Who, text: String, at: f64 },
    /// Something the person should know; it carries on.
    Note(String),
    /// It could not listen (a permission, no microphone).
    Error(String),
    /// It has stopped.
    Done,
}

/// One line from the listener.
pub fn parse(line: &str) -> Option<Heard> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let text = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let who = || if v.get("who").and_then(Value::as_str) == Some("others") { Who::Others } else { Who::Me };
    Some(match v.get("t")?.as_str()? {
        "status" => Heard::Listening { others: v.get("others").and_then(Value::as_bool).unwrap_or(false) },
        "partial" => Heard::Partial { who: who(), text: text("text") },
        "final" => Heard::Final { who: who(), text: text("text"), at: v.get("at").and_then(Value::as_f64).unwrap_or(0.0) },
        "note" => Heard::Note(text("message")),
        "error" => Heard::Error(text("message")),
        "done" => Heard::Done,
        _ => return None,
    })
}

/// Where the listener program is: beside this one, inside the app.
pub fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join("n2listen");
    path.is_file().then_some(path)
}

/// A test run hears a stand-in, unless it asks for the real listener
/// (`NOTION2_REAL_LISTENER`, to prove the real one end to end).
fn testing() -> bool {
    std::env::var_os("NOTION2_SELFTEST").is_some() && std::env::var_os("NOTION2_REAL_LISTENER").is_none()
}

/// Whether the app's own listener hears the speaker, and not the Mac's
/// Dictation: what Settings says, and with nothing chosen this listener
/// wherever the app has one (a test run stays with Dictation until it chooses).
pub fn own_ears() -> bool {
    match crate::store::setting("voice_engine").as_deref() {
        Some("builtin") => available(),
        Some("dictation") => false,
        _ => std::env::var_os("NOTION2_SELFTEST").is_none() && helper().is_some(),
    }
}

/// Whether this copy of the app can listen (an installed build can; a plain
/// development build has no listener program).
pub fn available() -> bool {
    testing() || helper().is_some()
}

/// A listener that is running.
pub enum Listener {
    Program { child: Child, input: Option<ChildStdin> },
    /// The self-test's stand-in: a few lines, as if heard.
    StandIn { stop: Arc<AtomicBool> },
}

impl Listener {
    /// Ask it to finish: the last words become final, then it says Done.
    pub fn stop(&mut self) {
        match self {
            Listener::Program { input, .. } => {
                if let Some(mut pipe) = input.take() {
                    let _ = pipe.write_all(b"stop\n");
                }
            }
            Listener::StandIn { stop } => stop.store(true, Ordering::Relaxed),
        }
    }

    /// End it now (it did not finish when asked, or the app is closing).
    pub fn end(&mut self) {
        self.stop();
        if let Listener::Program { child, .. } = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.end();
    }
}

/// Start listening: to the microphone, and with `meeting` to the Mac's sound
/// too. What is heard arrives on the receiver.
pub fn start(meeting: bool) -> Result<(Listener, UnboundedReceiver<Heard>), String> {
    let (tx, rx) = unbounded();
    if testing() {
        return Ok((stand_in(tx, meeting), rx));
    }
    let program = helper().ok_or("This copy of Rune has no listener. Use the installed app.")?;
    let mut child = Command::new(program)
        .arg(if meeting { "--meeting" } else { "--mic" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't start the listener ({e})."))?;
    let input = child.stdin.take();
    let output = child.stdout.take().ok_or("Couldn't read from the listener.")?;
    std::thread::spawn(move || {
        let mut done = false;
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if let Some(heard) = parse(&line) {
                done |= matches!(heard, Heard::Done | Heard::Error(_));
                if tx.unbounded_send(heard).is_err() {
                    return;
                }
            }
        }
        // It went away without saying so.
        if !done {
            let _ = tx.unbounded_send(Heard::Done);
        }
    });
    Ok((Listener::Program { child, input }, rx))
}

/// `notion2 --listen-test <report> [--file <sound file>] [--seconds <n>]`: run
/// the real listener and write every line it says to `report`, with no window.
/// With a sound file it hears that file in place of the microphone; without
/// one it hears the microphone for `n` seconds (8 if not given), and with
/// `--meeting` the sound the Mac plays as well. Start it with
/// `open -n <app> --args …`, so macOS treats the listener as Rune's.
pub fn test(args: &[String]) -> i32 {
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let Some(report) = value("--listen-test") else { return 2 };
    let mut lines = Vec::new();
    let code = match helper() {
        None => {
            lines.push(r#"{"t":"error","what":"helper","message":"This copy of Rune has no listener."}"#.to_string());
            2
        }
        Some(program) => {
            let file = value("--file");
            let seconds = value("--seconds").and_then(|s| s.parse::<u64>().ok()).unwrap_or(8).min(120);
            let mut command = Command::new(program);
            match &file {
                Some(file) => command.arg("--file").arg(file),
                None => command.arg(if args.iter().any(|a| a == "--meeting") { "--meeting" } else { "--mic" }),
            };
            if args.iter().any(|a| a == "--debug") {
                command.arg("--debug");
            }
            match command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                Err(e) => {
                    lines.push(format!(r#"{{"t":"error","what":"start","message":"{e}"}}"#));
                    2
                }
                Ok(mut child) => {
                    let mut input = child.stdin.take();
                    let output = child.stdout.take();
                    // The microphone is told to stop after the seconds asked
                    // for, counted from when it says it is listening.
                    let (heard_tx, heard_rx) = std::sync::mpsc::channel::<()>();
                    let stopper = (file.is_none()).then(|| {
                        std::thread::spawn(move || {
                            if heard_rx.recv_timeout(std::time::Duration::from_secs(90)).is_ok() {
                                std::thread::sleep(std::time::Duration::from_secs(seconds));
                            }
                            if let Some(mut pipe) = input.take() {
                                let _ = pipe.write_all(b"stop\n");
                            }
                        })
                    });
                    if let Some(output) = output {
                        for line in BufReader::new(output).lines().map_while(Result::ok) {
                            if line.contains(r#""listening""#) {
                                let _ = heard_tx.send(());
                            }
                            lines.push(line);
                            // Each line is written at once, so a run that waits
                            // (for a permission answer, say) still shows how far it got.
                            let _ = std::fs::write(&report, lines.join("\n") + "\n");
                        }
                    }
                    let status = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
                    drop(stopper);
                    status
                }
            }
        }
    };
    lines.push(format!(r#"{{"t":"exit","code":{code}}}"#));
    let _ = std::fs::write(&report, lines.join("\n") + "\n");
    code
}

/// For the self-test: says what a short meeting might, then waits to be stopped.
fn stand_in(tx: UnboundedSender<Heard>, meeting: bool) -> Listener {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let wait = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
        let _ = tx.unbounded_send(Heard::Listening { others: meeting });
        wait(150);
        let _ = tx.unbounded_send(Heard::Partial { who: Who::Me, text: "so the budget".into() });
        wait(150);
        let _ = tx.unbounded_send(Heard::Final { who: Who::Me, text: "So the budget is agreed.".into(), at: 1.0 });
        if meeting {
            wait(150);
            let _ = tx.unbounded_send(Heard::Final { who: Who::Others, text: "Yes, and Sam sends the numbers by Friday.".into(), at: 3.0 });
        }
        while !flag.load(Ordering::Relaxed) {
            wait(50);
        }
        let _ = tx.unbounded_send(Heard::Done);
    });
    Listener::StandIn { stop }
}

// ---------------------------------------------------------------------------
// The transcript
// ---------------------------------------------------------------------------

/// A stretch of speech by one side.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub who: Who,
    pub text: String,
    /// Seconds after the listening began.
    pub at: f64,
    /// When its last words were said.
    until: f64,
}

/// A finished stretch of speech as the listener gave it.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    who: Who,
    text: String,
    at: f64,
    /// Its words, lower case, letters and digits only.
    words: Vec<String>,
}

/// What was said, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    /// Every line heard, echoes too (a later line can show that an earlier
    /// one was an echo).
    lines: Vec<Line>,
    /// The lines without the echoes, joined into turns.
    pub turns: Vec<Turn>,
}

/// Letters and digits only, lower case: for telling the same words apart
/// from punctuation and spacing.
fn bare(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The words of a line, for telling how alike two lines are.
fn words_of(text: &str) -> Vec<String> {
    text.split_whitespace().map(bare).filter(|w| !w.is_empty()).collect()
}

/// With speakers rather than headphones the microphone hears the others too,
/// a moment before or after the Mac's own sound is heard, and often with a
/// word or two heard differently. A line of "me" is such an echo when most
/// of its words are in what the others said within these seconds of it.
const ECHO_SECONDS: f64 = 8.0;
/// The share of a line's words that must be the others' words.
const ECHO_SHARE: f64 = 0.6;
/// A short line ("OK", "Sounds good") is too little to tell by itself: it
/// counts as an echo only close in time, and only in a meeting where longer
/// echoes show that the microphone hears the speakers.
const SHORT_WORDS: usize = 3;
const SHORT_SECONDS: f64 = 3.0;

impl Transcript {
    /// Add a finished stretch of speech. The same side carrying on (within a
    /// minute) joins its last turn; what the microphone heard of the others
    /// is left out (see `ECHO_SECONDS`).
    pub fn add(&mut self, who: Who, text: &str, at: f64) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let line = Line { who, text: text.to_string(), at, words: words_of(text) };
        // Kept in the order of their times (they nearly always arrive so).
        let after = self.lines.iter().rposition(|l| l.at <= at).map(|i| i + 1).unwrap_or(0);
        self.lines.insert(after, line);
        self.rebuild();
    }

    /// How much of line `i` (of "me") the others said within `seconds` of it:
    /// the share of its words, each of the others' words counted once.
    fn echoed(&self, i: usize, seconds: f64) -> f64 {
        let me = &self.lines[i];
        if me.words.is_empty() {
            return 0.0;
        }
        let mut theirs: Vec<&str> = self
            .lines
            .iter()
            .filter(|l| l.who == Who::Others && (l.at - me.at).abs() <= seconds)
            .flat_map(|l| l.words.iter().map(String::as_str))
            .collect();
        let mut found = 0usize;
        for word in &me.words {
            if let Some(at) = theirs.iter().position(|w| w == word) {
                theirs.swap_remove(at);
                found += 1;
            }
        }
        found as f64 / me.words.len() as f64
    }

    /// Work out the turns again from all the lines.
    fn rebuild(&mut self) {
        let mine: Vec<usize> = (0..self.lines.len()).filter(|&i| self.lines[i].who == Who::Me).collect();
        let long_echo: Vec<usize> =
            mine.iter().copied().filter(|&i| self.lines[i].words.len() > SHORT_WORDS && self.echoed(i, ECHO_SECONDS) >= ECHO_SHARE).collect();
        // Word for word what the others said in the last moments, as before.
        let said_again = |i: usize| {
            let me = &self.lines[i];
            let all = bare(&me.text);
            all.len() >= 12 && self.lines.iter().any(|l| l.who == Who::Others && l.at <= me.at && me.at - l.at < 20.0 && bare(&l.text).contains(&all))
        };
        let speakers_heard = long_echo.len() >= 2;
        let echo: Vec<bool> = (0..self.lines.len())
            .map(|i| {
                let l = &self.lines[i];
                l.who == Who::Me
                    && (long_echo.contains(&i)
                        || said_again(i)
                        || (speakers_heard && l.words.len() <= SHORT_WORDS && self.echoed(i, SHORT_SECONDS) >= 1.0))
            })
            .collect();
        let mut turns: Vec<Turn> = Vec::new();
        for (line, _) in self.lines.iter().zip(&echo).filter(|(_, echo)| !**echo) {
            match turns.last_mut() {
                Some(last) if last.who == line.who && line.at - last.until < 60.0 => {
                    last.text.push(' ');
                    last.text.push_str(&line.text);
                    last.until = line.at;
                }
                _ => turns.push(Turn { who: line.who, text: line.text.clone(), at: line.at, until: line.at }),
            }
        }
        self.turns = turns;
    }

    pub fn words(&self) -> usize {
        self.turns.iter().map(|t| t.text.split_whitespace().count()).sum()
    }

    /// How many lines were left out as echoes.
    pub fn echoes(&self) -> usize {
        let kept: usize = self.turns.iter().map(|t| t.text.split_whitespace().count()).sum();
        let all: usize = self.lines.iter().map(|l| l.text.split_whitespace().count()).sum();
        // Counted in lines: those whose words are not in the turns.
        if all == kept {
            return 0;
        }
        let mut left = self.turns.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ");
        self.lines
            .iter()
            .filter(|l| match left.find(l.text.as_str()) {
                Some(at) => {
                    left.replace_range(at..at + l.text.len(), "");
                    false
                }
                None => true,
            })
            .count()
    }

    fn label(who: Who) -> &'static str {
        match who {
            Who::Me => "Me",
            Who::Others => "Others",
        }
    }

    /// "3:05" for a number of seconds.
    fn clock(seconds: f64) -> String {
        let s = seconds.max(0.0) as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }

    /// For the AI: one line for each turn.
    pub fn text(&self) -> String {
        self.turns.iter().map(|t| format!("{}: {}", Self::label(t.who), t.text)).collect::<Vec<_>>().join("\n")
    }

    /// For the page: one block for each turn, to go inside the "Transcript" toggle.
    pub fn blocks(&self) -> Vec<Block> {
        self.turns
            .iter()
            .map(|t| {
                Block::text(format!("**{}** · {} — {}", Self::label(t.who), Self::clock(t.at), crate::inline::escape(&t.text))).with_indent(1)
            })
            .collect()
    }
}

/// Put the transcript into a page: inside a "Transcript" toggle at the end
/// (made the first time, closed), in place of what the toggle held before.
/// `toggle` remembers which block it is.
pub fn put_transcript(page: &mut Vec<Block>, toggle: &mut Option<Id>, lines: Vec<Block>) {
    if lines.is_empty() {
        return;
    }
    let at = toggle.as_ref().and_then(|id| page.iter().position(|b| &b.id == id));
    let at = match at {
        Some(i) => i,
        None => {
            if page.last().is_some_and(|b| b.kind == BlockKind::Text && b.text.trim().is_empty()) {
                page.pop();
            }
            let mut head = Block::new(BlockKind::Toggle, "Transcript");
            head.collapsed = true;
            *toggle = Some(head.id.clone());
            page.push(head);
            page.len() - 1
        }
    };
    // What the toggle holds: the indented blocks after it.
    let end = page[at + 1..].iter().position(|b| b.indent == 0).map(|i| at + 1 + i).unwrap_or(page.len());
    // Lines already there keep their ids, so the page is not redrawn for them.
    let mut lines = lines;
    for (new, old) in lines.iter_mut().zip(&page[at + 1..end]) {
        new.id = old.id.clone();
    }
    page.splice(at + 1..end, lines);
}

/// `notion2 --transcript-into <page id> <lines file> [--notes <markdown
/// file>] [--fresh-notes]`: put a transcript kept outside the app into a
/// page's "Transcript" toggle, with no window. Each line of the file is
/// `seconds<TAB>me|others<TAB>words` (as made from `--listen-test` reports).
/// With `--notes`, that Markdown (`## Notes` and `## Action items` parts) goes
/// under the page's headings of those names; with `--fresh-notes` it takes
/// the place of what is there. An open app shows the change by itself.
pub fn into_page(args: &[String]) -> i32 {
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(page), Some(file)) = (value("--transcript-into"), args.iter().position(|a| a == "--transcript-into").and_then(|i| args.get(i + 2)).cloned()) else {
        eprintln!("usage: notion2 --transcript-into <page id> <lines file> [--notes <markdown file>] [--fresh-notes]");
        return 2;
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Could not read {file}: {e}");
            return 2;
        }
    };
    let mut transcript = Transcript::default();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(at), Some(who), Some(words)) = (parts.next().and_then(|s| s.trim().parse::<f64>().ok()), parts.next(), parts.next()) else { continue };
        let who = if who.trim() == "me" { Who::Me } else { Who::Others };
        transcript.add(who, words, at);
    }
    let notes = match value("--notes").map(std::fs::read_to_string) {
        Some(Ok(markdown)) => crate::markdown::to_blocks(markdown.trim()),
        Some(Err(e)) => {
            eprintln!("Could not read the notes: {e}");
            return 2;
        }
        None => Vec::new(),
    };
    let done = (|| -> rusqlite::Result<Option<(usize, usize)>> {
        let store = crate::store::Store::open(crate::store::Store::default_path())?;
        let (pages, mut all) = store.load()?;
        if !pages.contains_key(&page) {
            return Ok(None);
        }
        let mut blocks = all.remove(&page).unwrap_or_default();
        if args.iter().any(|a| a == "--fresh-notes") {
            crate::voice::clear_recap(&mut blocks);
        }
        let added = crate::voice::fill_recap(&mut blocks, notes);
        let mut toggle = blocks
            .iter()
            .find(|b| b.kind == BlockKind::Toggle && b.indent == 0 && b.plain_text().trim().eq_ignore_ascii_case("Transcript"))
            .map(|b| b.id.clone());
        put_transcript(&mut blocks, &mut toggle, transcript.blocks());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        store.save_blocks_at(&page, &blocks, now)?;
        Ok(Some((transcript.turns.len(), added)))
    })();
    match done {
        Ok(Some((turns, added))) => {
            println!("{turns} turns of the transcript and {added} lines of notes are in the page ({} words; {} lines were echoes).", transcript.words(), transcript.echoes());
            0
        }
        Ok(None) => {
            eprintln!("There is no page with the id {page}.");
            1
        }
        Err(e) => {
            eprintln!("Could not write the page: {e}");
            1
        }
    }
}

/// What the AI is told to make the notes of a meeting from its transcript.
pub const NOTES: &str = "You write the notes of a meeting from its transcript. The transcript came from speech recognition, so it has wrong words and little punctuation. \"Me\" is the person these notes are for; \"Others\" is everyone else in the meeting (it does not say who). The microphone may have heard the others too, so some words may appear twice. Reply with Markdown only, with no preamble and no closing remark, in exactly this shape: a line `## Notes`, then the main points, decisions, numbers and dates as `-` bullets, the most important first, at most twelve; then a line `## Action items`, then each thing someone agreed to do as a `- [ ]` to-do, starting with who does it when that was said (write \"I\" for Me). When a day was agreed for it, end its line with \u{1F4C5} and that day as YYYY-MM-DD (the first line you are given says what day today is); when no day was said, add nothing. If nothing was agreed, leave that part empty. Use only what the transcript says: do not add facts or guess names. Write in the language of the meeting.";

/// One turn of a meeting as its page keeps it: a line "**Me** · 0:42 — words"
/// in the page's "Transcript" toggle.
#[derive(Clone, Debug, PartialEq)]
pub struct Spoken {
    pub who: String,
    /// Seconds after the listening began.
    pub at: f64,
    pub words: usize,
}

/// One voice of a meeting: when it spoke, and how much.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub name: String,
    /// The stretches it spoke in, as (start, end) in seconds.
    pub parts: Vec<(f64, f64)>,
    pub seconds: f64,
    pub words: usize,
}

/// Who spoke when in a meeting.
#[derive(Clone, Debug, PartialEq)]
pub struct Voices {
    pub voices: Vec<Voice>,
    /// How long the meeting was, in seconds (to the end of the last words).
    pub length: f64,
    pub words: usize,
}

impl Voices {
    /// A voice's part of all the speaking, in hundredths.
    pub fn share(&self, voice: &Voice) -> u32 {
        let all: f64 = self.voices.iter().map(|v| v.seconds).sum();
        if all <= 0.0 { 0 } else { (voice.seconds / all * 100.0).round() as u32 }
    }
}

/// Words a second in speech, to judge how long a turn took from its words
/// (the page keeps when a turn began, not when it ended).
const WORDS_A_SECOND: f64 = 2.6;

/// The lines a page's "Transcript" toggle holds, as (place in the page, turn).
/// A page with two transcripts gives the later one.
fn transcript_lines(page: &[Block]) -> Vec<(usize, Spoken)> {
    let Some(head) = page.iter().rposition(|b| b.kind == BlockKind::Toggle && b.indent == 0 && b.plain_text().trim().eq_ignore_ascii_case("transcript")) else {
        return Vec::new();
    };
    page.iter()
        .enumerate()
        .skip(head + 1)
        .take_while(|(_, b)| b.indent > 0)
        .filter_map(|(i, b)| {
            let plain = b.plain_text();
            let (who, rest) = plain.split_once(" · ")?;
            let (clock, said) = rest.split_once(" — ")?;
            let (minutes, seconds) = clock.trim().split_once(':')?;
            let at = minutes.parse::<u64>().ok()? * 60 + seconds.parse::<u64>().ok()?;
            Some((i, Spoken { who: who.trim().to_string(), at: at as f64, words: said.split_whitespace().count() }))
        })
        .collect()
}

/// The turns of the meeting a page holds (none for a page that is not a meeting's).
pub fn spoken_in(page: &[Block]) -> Vec<Spoken> {
    transcript_lines(page).into_iter().map(|(_, s)| s).collect()
}

/// Who spoke when, from the turns. "Me" comes first; the others in the
/// order they first spoke.
pub fn voices(turns: &[Spoken]) -> Option<Voices> {
    if turns.is_empty() {
        return None;
    }
    let mut voices: Vec<Voice> = Vec::new();
    let mut length: f64 = 0.0;
    for (i, t) in turns.iter().enumerate() {
        // A turn ends when its words are said, or when the next one begins.
        let by_words = t.at + (t.words as f64 / WORDS_A_SECOND).max(1.5);
        let end = match turns.get(i + 1) {
            Some(next) if next.at > t.at => by_words.min(next.at),
            _ => by_words,
        };
        length = length.max(end);
        let at = match voices.iter().position(|v| v.name == t.who) {
            Some(at) => at,
            None => {
                voices.push(Voice { name: t.who.clone(), parts: Vec::new(), seconds: 0.0, words: 0 });
                voices.len() - 1
            }
        };
        let v = &mut voices[at];
        v.seconds += end - t.at;
        v.words += t.words;
        // Turns of one voice with less than two seconds between them are one stretch.
        match v.parts.last_mut() {
            Some(last) if t.at - last.1 < 2.0 => last.1 = last.1.max(end),
            _ => v.parts.push((t.at, end)),
        }
    }
    if let Some(me) = voices.iter().position(|v| v.name == "Me") {
        let me = voices.remove(me);
        voices.insert(0, me);
    }
    let words = voices.iter().map(|v| v.words).sum();
    Some(Voices { voices, length: length.max(1.0), words })
}

/// Give a voice of the page's transcript another name (the lines of "Others"
/// become the lines of "Sam"). Says how many lines changed.
pub fn rename_voice(page: &mut [Block], old: &str, new: &str) -> usize {
    // A name is one short line with no signs that mean something in a line.
    let new: String = new.chars().filter(|c| !c.is_control() && !matches!(c, '*' | '·' | '—')).take(40).collect();
    let new = new.trim();
    if new.is_empty() || new == old {
        return 0;
    }
    let mine: Vec<usize> = transcript_lines(page).into_iter().filter(|(_, s)| s.who == old).map(|(i, _)| i).collect();
    let mut changed = 0;
    for i in mine {
        // The stored line is "**name** · time — words": the name ends at "** · ".
        const END: &str = "** · ";
        if let Some(at) = page[i].text.find(END).filter(|_| page[i].text.starts_with("**")) {
            page[i].text = format!("**{}{}", crate::inline::escape(new), &page[i].text[at..]);
            changed += 1;
        }
    }
    changed
}

/// The transcript as sent to the AI: all of a short one; of a very long one
/// the beginning and the end (where meetings are set up and summed up).
pub fn for_ai(transcript: &str, budget_chars: usize) -> String {
    let count = transcript.chars().count();
    if count <= budget_chars {
        return transcript.to_string();
    }
    let head: String = transcript.chars().take(budget_chars / 2).collect();
    let tail: String = transcript.chars().skip(count - budget_chars / 2).collect();
    format!("{head}\n\n[… the middle of the meeting is left out …]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page as a meeting leaves it: notes, then the transcript in its toggle.
    fn meeting_page() -> Vec<Block> {
        let line = |who: &str, at: &str, said: &str| Block::text(format!("**{who}** · {at} — {said}")).with_indent(1);
        vec![
            Block::new(BlockKind::Heading2, "Notes"),
            Block::text("The budget is agreed."),
            Block::new(BlockKind::Toggle, "Transcript"),
            line("Others", "0:02", "Good morning to all of you"),
            line("Me", "0:05", "Let us start with the budget"),
            line("Me", "0:08", "It is agreed"),
            line("Others", "0:40", "The numbers are final on Friday and I send them to all of you after lunch"),
            Block::text("A line that is not a turn").with_indent(1),
        ]
    }

    #[test]
    fn the_voices_of_a_meeting_are_read_from_its_page() {
        let page = meeting_page();
        let turns = spoken_in(&page);
        assert_eq!(turns.len(), 4, "the line that is not a turn is left out");
        assert_eq!((turns[0].who.as_str(), turns[0].at, turns[0].words), ("Others", 2.0, 6));
        let v = voices(&turns).unwrap();
        // "Me" is first, whoever spoke first.
        assert_eq!(v.voices.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Me", "Others"]);
        // Others: 0:02 for six words (2.3 s), then 0:40 for sixteen words (6.2 s).
        let others = &v.voices[1];
        assert_eq!(others.parts.len(), 2);
        assert!((others.parts[0].0 - 2.0).abs() < 0.01 && others.parts[0].1 > 4.0 && others.parts[0].1 <= 5.0, "{:?}", others.parts);
        // Me: two turns three seconds apart, the first cut short by the second: one stretch.
        let me = &v.voices[0];
        assert_eq!(me.parts.len(), 1, "{:?}", me.parts);
        assert!((me.parts[0].0 - 5.0).abs() < 0.01 && me.parts[0].1 > 8.0);
        assert_eq!((me.words, others.words, v.words), (9, 22, 31));
        assert!(v.length > 45.0 && v.length < 48.0, "{}", v.length);
        assert_eq!(v.share(me) + v.share(others), 100);
        // A page with no transcript is not a meeting's.
        assert!(voices(&spoken_in(&[Block::text("**Me** · 0:05 — not in a transcript")])).is_none());
    }

    #[test]
    fn a_voice_can_be_given_a_name() {
        let mut page = meeting_page();
        assert_eq!(rename_voice(&mut page, "Others", "  Sam *Lee* "), 2);
        assert_eq!(page[3].text, "**Sam Lee** · 0:02 — Good morning to all of you");
        assert_eq!(page[4].text, "**Me** · 0:05 — Let us start with the budget", "the other voice is as it was");
        let v = voices(&spoken_in(&page)).unwrap();
        assert_eq!(v.voices.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Me", "Sam Lee"]);
        // No name, the same name, a voice that is not there: nothing changes.
        assert_eq!(rename_voice(&mut page, "Me", "   "), 0);
        assert_eq!(rename_voice(&mut page, "Me", "Me"), 0);
        assert_eq!(rename_voice(&mut page, "Nobody", "Ann"), 0);
    }

    #[test]
    fn the_listener_s_lines_are_read() {
        assert_eq!(parse(r#"{"t":"status","state":"listening","on_device":true,"others":true}"#), Some(Heard::Listening { others: true }));
        assert_eq!(parse(r#"{"t":"partial","who":"me","text":"so the"}"#), Some(Heard::Partial { who: Who::Me, text: "so the".into() }));
        assert_eq!(
            parse(r#"{"t":"final","who":"others","text":"Agreed.","at":12.4}"#),
            Some(Heard::Final { who: Who::Others, text: "Agreed.".into(), at: 12.4 })
        );
        assert_eq!(parse(r#"{"t":"error","what":"microphone","message":"Not allowed."}"#), Some(Heard::Error("Not allowed.".into())));
        assert_eq!(parse(r#"{"t":"done"}"#), Some(Heard::Done));
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"t":"something new"}"#), None);
    }

    #[test]
    fn the_transcript_joins_turns_and_drops_echoes() {
        let mut t = Transcript::default();
        t.add(Who::Me, "So the budget is agreed.", 1.0);
        t.add(Who::Me, "We ship in March.", 6.0);
        t.add(Who::Others, "Yes, and Sam sends the numbers by Friday.", 10.0);
        // The microphone heard the speakers: not "me".
        t.add(Who::Me, "and Sam sends the numbers by Friday", 11.0);
        t.add(Who::Me, "  ", 12.0);
        t.add(Who::Me, "Good.", 13.0);
        assert_eq!(t.turns.len(), 3);
        assert_eq!(t.turns[0].text, "So the budget is agreed. We ship in March.");
        assert_eq!(t.text(), "Me: So the budget is agreed. We ship in March.\nOthers: Yes, and Sam sends the numbers by Friday.\nMe: Good.");
        assert_eq!(t.words(), 18);
        assert_eq!(t.echoes(), 1);
        // After a long silence the same side starts a new turn.
        t.add(Who::Me, "One more thing.", 200.0);
        assert_eq!(t.turns.len(), 4);
        let blocks = t.blocks();
        assert_eq!(blocks[1].text, "**Others** · 0:10 — Yes, and Sam sends the numbers by Friday.");
        assert!(blocks.iter().all(|b| b.indent == 1));
    }

    #[test]
    fn what_the_microphone_heard_of_the_others_is_left_out() {
        // On speakers the microphone hears the others: a little before or
        // after the Mac's own sound, and with words heard differently.
        let mut t = Transcript::default();
        t.add(Who::Others, "Because if the name is there already, I do not want one more name to learn, right", 10.0);
        t.add(Who::Me, "Because if name is there already I don't want one more name to learn right", 10.6);
        t.add(Who::Me, "OK, I see what you mean.", 14.0);
        // The microphone's line can come first: it goes when the others' line arrives.
        t.add(Who::Me, "then the dates are matched against their name and brought in", 20.0);
        assert_eq!(t.turns.len(), 2, "kept until the others' line shows that it was an echo");
        t.add(Who::Others, "Then the dates are matched against the name and then brought in", 20.4);
        assert_eq!(t.text(), "Others: Because if the name is there already, I do not want one more name to learn, right\nMe: OK, I see what you mean.\nOthers: Then the dates are matched against the name and then brought in");
        // One long line of the others heard as two.
        t.add(Who::Others, "So take a look at the story and see what you think, and then we can go from there", 30.0);
        t.add(Who::Me, "so take a look at the story and", 29.5);
        t.add(Who::Me, "see what you think and then we can go from there", 31.0);
        assert_eq!(t.turns.len(), 3);
        // Short lines: in a meeting with such echoes, the same word at the same moment is one too.
        t.add(Who::Others, "OK cool, awesome.", 40.0);
        t.add(Who::Me, "Awesome", 40.5);
        t.add(Who::Me, "Thanks", 41.0);
        assert_eq!(t.turns.last().map(|l| (l.who, l.text.as_str())), Some((Who::Me, "Thanks")));
        assert_eq!(t.echoes(), 5);
        // What is really said by "me" stays, also when it shares some words.
        t.add(Who::Others, "Can you send the list today", 60.0);
        t.add(Who::Me, "I forgot to send it, I will send the list every day", 63.0);
        assert!(t.text().ends_with("Me: I forgot to send it, I will send the list every day"));

        // With headphones there are no echoes: a short answer in the others' words stays.
        let mut t = Transcript::default();
        t.add(Who::Others, "Is Friday OK?", 5.0);
        t.add(Who::Me, "OK", 6.0);
        assert_eq!(t.text(), "Others: Is Friday OK?\nMe: OK");
        assert_eq!(t.echoes(), 0);
    }

    #[test]
    fn the_transcript_goes_in_a_toggle_and_is_kept_up_to_date() {
        let mut page = vec![Block::new(BlockKind::Heading2, "Notes"), Block::text("")];
        let mut toggle = None;
        let mut t = Transcript::default();
        t.add(Who::Me, "Hello.", 1.0);
        put_transcript(&mut page, &mut toggle, t.blocks());
        assert_eq!(page.len(), 3, "the empty last line gives way to the toggle");
        assert_eq!((page[1].kind, page[1].text.as_str(), page[1].collapsed), (BlockKind::Toggle, "Transcript", true));
        let first_line = page[2].id.clone();
        // More is said; then the person writes under the page.
        t.add(Who::Others, "Hi there.", 4.0);
        page.push(Block::text("My own note"));
        put_transcript(&mut page, &mut toggle, t.blocks());
        assert_eq!(page.iter().map(|b| b.text.clone()).collect::<Vec<_>>(), vec!["Notes", "Transcript", "**Me** · 0:01 — Hello.", "**Others** · 0:04 — Hi there.", "My own note"]);
        assert_eq!(page[2].id, first_line, "lines already shown are not redrawn");
        // Nothing heard: nothing is added.
        let mut empty = vec![Block::text("x")];
        put_transcript(&mut empty, &mut None, Vec::new());
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn a_long_transcript_is_cut_in_the_middle_for_the_ai() {
        assert_eq!(for_ai("short", 100), "short");
        let long = "a".repeat(60) + &"b".repeat(60);
        let cut = for_ai(&long, 40);
        assert!(cut.starts_with(&"a".repeat(20)) && cut.ends_with(&"b".repeat(20)) && cut.contains("left out"));
    }
}
