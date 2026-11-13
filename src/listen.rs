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

/// Whether Apple's speech recognition turns speech into text on this Mac
/// for the language of this Mac (0: not known yet, 1: yes, 2: no, it uses
/// Apple's servers). The listener program says; it is asked one time.
static ON_THIS_MAC: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// None until the listener program was asked (and in a test run).
pub fn on_this_mac() -> Option<bool> {
    match ON_THIS_MAC.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// What `n2listen --check` says about recognition on this Mac.
fn on_this_mac_in(answer: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(answer.trim()).ok()?.get("on_device")?.as_bool()
}

/// Ask the listener program, off the calling thread. It asks for no
/// permission and hears nothing.
pub fn learn_where_speech_goes() {
    if testing() || std::env::var_os("NOTION2_SELFTEST").is_some() {
        return;
    }
    let Some(helper) = helper() else { return };
    std::thread::spawn(move || {
        let out = std::process::Command::new(helper).arg("--check").stdin(std::process::Stdio::null()).output();
        if let Some(yes) = out.ok().and_then(|o| on_this_mac_in(&String::from_utf8_lossy(&o.stdout))) {
            ON_THIS_MAC.store(if yes { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
        }
    });
}

/// The sentence that tells the person where the sound of a meeting goes.
pub fn where_speech_goes() -> &'static str {
    match on_this_mac() {
        Some(false) => "Tell the others first. For your language, Apple turns the speech into text on its servers.",
        _ => "Tell the others first. The sound stays on this Mac.",
    }
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
    start_from(if meeting { Source::Meeting } else { Source::Mic })
}

/// What the listener hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The microphone (the voice box, a line of a page).
    Mic,
    /// The microphone and what the Mac plays (a meeting).
    Meeting,
    /// Only what Rune itself plays (a video in its player): not the
    /// microphone, and not a call or anything else on the Mac.
    Sound,
}

/// Start listening to a source. What is heard arrives on the receiver.
pub fn start_from(source: Source) -> Result<(Listener, UnboundedReceiver<Heard>), String> {
    let (tx, rx) = unbounded();
    if testing() {
        return Ok((stand_in(tx, source), rx));
    }
    let program = helper().ok_or("This copy of Rune has no listener. Use the installed app.")?;
    let mut command = Command::new(program);
    command.arg(match source {
        Source::Mic => "--mic",
        Source::Meeting => "--meeting",
        Source::Sound => "--system",
    });
    if source == Source::Sound {
        command.arg("--from-app").arg(std::process::id().to_string());
    }
    let mut child = command
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
fn stand_in(tx: UnboundedSender<Heard>, source: Source) -> Listener {
    let meeting = source == Source::Meeting;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let wait = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
        if source == Source::Sound {
            // A video: only what the Mac plays, and nobody is "me".
            let _ = tx.unbounded_send(Heard::Listening { others: true });
            wait(150);
            let _ = tx.unbounded_send(Heard::Final { who: Who::Others, text: "Warm up for four minutes before the first block.".into(), at: 1.0 });
            wait(150);
            let _ = tx.unbounded_send(Heard::Final { who: Who::Others, text: "Then do two rounds of the power circuit, with forty seconds of work.".into(), at: 4.0 });
            while !flag.load(Ordering::Relaxed) {
                wait(50);
            }
            let _ = tx.unbounded_send(Heard::Done);
            return;
        }
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
/// The share of a line's words that must be the others' words, in the order
/// the others said them. (Counted in any order, and at 0.6, an answer that
/// uses the words of the question was taken for an echo and left out:
/// "Yes I can do that by Friday" after "Can you do that by Friday?".)
const ECHO_SHARE: f64 = 0.75;
/// A line this close (seconds) to a line of the others is heard at the same
/// moment: an echo and not an answer, also with a word or two heard
/// differently.
const SAME_MOMENT: f64 = 1.5;
const SAME_MOMENT_SHARE: f64 = 0.6;
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
    /// the share of its words that are the others' words in the same order
    /// (the longest run of words, not always side by side, that both have).
    fn echoed(&self, i: usize, seconds: f64) -> f64 {
        let me = &self.lines[i];
        if me.words.is_empty() {
            return 0.0;
        }
        let theirs: Vec<&str> = self
            .lines
            .iter()
            .filter(|l| l.who == Who::Others && (l.at - me.at).abs() <= seconds)
            .flat_map(|l| l.words.iter().map(String::as_str))
            .collect();
        // (One row of the table at a time: the lines are short.)
        let mut best = vec![0usize; theirs.len() + 1];
        for word in &me.words {
            let mut before = 0;
            for (j, other) in theirs.iter().enumerate() {
                let kept = best[j + 1];
                best[j + 1] = if other == word { before + 1 } else { best[j + 1].max(best[j]) };
                before = kept;
            }
        }
        best[theirs.len()] as f64 / me.words.len() as f64
    }

    /// Work out the turns again from all the lines.
    fn rebuild(&mut self) {
        let mine: Vec<usize> = (0..self.lines.len()).filter(|&i| self.lines[i].who == Who::Me).collect();
        let share = |i: usize| {
            let at = self.lines[i].at;
            let same_moment = self.lines.iter().any(|l| l.who == Who::Others && (l.at - at).abs() <= SAME_MOMENT);
            if same_moment {
                SAME_MOMENT_SHARE
            } else {
                ECHO_SHARE
            }
        };
        let long_echo: Vec<usize> =
            mine.iter().copied().filter(|&i| self.lines[i].words.len() > SHORT_WORDS && self.echoed(i, ECHO_SECONDS) >= share(i)).collect();
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

impl Transcript {
    /// For a video (one side only, with no name): the words with their time,
    /// one block for each turn, to go inside a toggle.
    pub fn plain_blocks(&self) -> Vec<Block> {
        self.turns.iter().map(|t| Block::text(format!("{} — {}", Self::clock(t.at), crate::inline::escape(&t.text))).with_indent(1)).collect()
    }

    /// The words alone, a turn on each line (for the AI that writes the notes of a video).
    pub fn plain_text(&self) -> String {
        self.turns.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

/// What the AI is told when it writes the notes of a video.
pub const VIDEO_NOTES: &str = "You write notes of a video from its transcript. The transcript came from speech recognition, so it has wrong words and little punctuation. Reply with Markdown only, with no preamble, no heading and no closing remark: the main points, steps, numbers and names as `-` bullets, in the order of the video, the most at fifteen. A detail of a point is a bullet under it, indented by two spaces. Use only what the transcript says: do not add facts or guess names. Write in the language of the video.";

/// Put lines inside a toggle of a page, in place of what the toggle held
/// before. The toggle is made the first time: right under the block `after`
/// and what is inside that block (the line of a video and its notes), or at
/// the end of the page when that block is gone. `toggle` remembers which
/// block the toggle is. The lines come one step in (`indent` 1).
pub fn put_under(page: &mut Vec<Block>, toggle: &mut Option<Id>, title: &str, collapsed: bool, after: Option<&str>, lines: Vec<Block>) {
    if lines.is_empty() {
        return;
    }
    let at = toggle.as_ref().and_then(|id| page.iter().position(|b| &b.id == id));
    let at = match at {
        Some(i) => i,
        None => {
            let mut head = Block::new(BlockKind::Toggle, title);
            head.collapsed = collapsed;
            *toggle = Some(head.id.clone());
            match after.and_then(|id| page.iter().position(|b| b.id == id)) {
                Some(line) => {
                    head.indent = page[line].indent;
                    page.insert(line + 1, head);
                    line + 1
                }
                None => {
                    page.push(head);
                    page.len() - 1
                }
            }
        }
    };
    let depth = page[at].indent;
    // What the toggle holds: the deeper blocks after it.
    let end = page[at + 1..].iter().position(|b| b.indent <= depth).map(|i| at + 1 + i).unwrap_or(page.len());
    let mut lines = lines;
    for (i, line) in lines.iter_mut().enumerate() {
        line.indent += depth;
        // Lines already there keep their ids, so the page is not redrawn for them.
        if let Some(old) = page[at + 1..end].get(i) {
            line.id = old.id.clone();
        }
    }
    page.splice(at + 1..end, lines);
}

/// Put the transcript into a page: inside a "Transcript" toggle at the end
/// (made the first time, closed), in place of what the toggle held before.
/// `toggle` remembers which block it is.
/// The "Transcript" toggle that a page has already (the last one), and how
/// many lines it holds. The app listened to this page before: what it hears
/// now goes on in the same toggle (see `put_transcript`).
pub fn transcript_so_far(page: &[Block]) -> (Option<Id>, usize) {
    let Some(at) = page.iter().rposition(|b| b.kind == BlockKind::Toggle && b.indent == 0 && b.plain_text().trim().eq_ignore_ascii_case("Transcript")) else {
        return (None, 0);
    };
    let lines = page[at + 1..].iter().take_while(|b| b.indent > 0).count();
    (Some(page[at].id.clone()), lines)
}

/// What a page's "Transcript" toggle holds from its line `from` on, as the
/// AI is given a transcript: a line "Me: words" for each turn.
pub fn heard_before(page: &[Block], from: usize) -> String {
    let Some(at) = page.iter().rposition(|b| b.kind == BlockKind::Toggle && b.indent == 0 && b.plain_text().trim().eq_ignore_ascii_case("Transcript")) else {
        return String::new();
    };
    page[at + 1..]
        .iter()
        .take_while(|b| b.indent > 0)
        .skip(from)
        .filter_map(|b| {
            let plain = b.plain_text();
            let (who, rest) = plain.split_once(" · ")?;
            let (_, said) = rest.split_once(" — ")?;
            Some(format!("{}: {}", who.trim(), said.trim()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The transcript for the notes of a meeting that was heard in two parts:
/// what the page held before (`heard_before`), then what was heard now.
pub fn all_heard(earlier: &str, now: &str) -> String {
    if earlier.is_empty() { now.to_string() } else { format!("{earlier}\n{now}") }
}

/// The line of a page's "Transcript" toggle where the part that was heard
/// last begins: the last line that says when the app listened again (0 for
/// a transcript in one part).
pub fn last_part(page: &[Block]) -> usize {
    let Some(at) = page.iter().rposition(|b| b.kind == BlockKind::Toggle && b.indent == 0 && b.plain_text().trim().eq_ignore_ascii_case("Transcript")) else {
        return 0;
    };
    page[at + 1..].iter().take_while(|b| b.indent > 0).enumerate().filter(|(_, b)| b.text.starts_with(AGAIN)).map(|(i, _)| i).last().unwrap_or(0)
}

const AGAIN: &str = "_Listening started again at ";

/// The line between what was heard before and what is heard now.
pub fn again_line(time: &str) -> Block {
    Block::text(format!("{AGAIN}{time}_")).with_indent(1)
}

/// Put the lines of a transcript in the page's "Transcript" toggle (made at
/// the end of the page the first time). The first `kept` lines of the toggle
/// are from an earlier time of listening: they stay as they are, and `lines`
/// take the place of what comes after them.
pub fn put_transcript(page: &mut Vec<Block>, toggle: &mut Option<Id>, kept: usize, lines: Vec<Block>) {
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
    let from = (at + 1 + kept).min(end);
    // Lines already there keep their ids, so the page is not redrawn for them.
    let mut lines = lines;
    for (new, old) in lines.iter_mut().zip(&page[from..end]) {
        new.id = old.id.clone();
    }
    page.splice(from..end, lines);
}

/// Before `put_transcript` while the app listens: a line that the person
/// changed in the page (a word put right, a voice given a name) stays as they
/// made it, for as long as the turn it shows has not changed. `written`: the
/// lines as they were put there the last time. (Every write put the heard
/// words back, so a change in the open toggle was undone within 20 seconds.)
/// Only while the toggle has the lines that were written, one for one: with
/// a line added or taken out by hand, the lines are written as before.
pub fn keep_changes(page: &[Block], toggle: Option<&Id>, kept: usize, written: &mut Vec<String>, lines: &mut [Block]) {
    let heard: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
    if let Some(at) = toggle.and_then(|id| page.iter().position(|b| &b.id == id)) {
        let end = page[at + 1..].iter().position(|b| b.indent == 0).map(|i| at + 1 + i).unwrap_or(page.len());
        let from = (at + 1 + kept).min(end);
        if end - from == written.len() {
            for ((line, old), was) in lines.iter_mut().zip(&page[from..end]).zip(written.iter()) {
                if line.text == *was && old.text != *was {
                    line.text = old.text.clone();
                }
            }
        }
    }
    *written = heard;
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
        put_transcript(&mut blocks, &mut toggle, 0, transcript.blocks());
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
    #[test]
    fn the_notes_and_the_words_of_a_video_go_under_its_line() {
        use super::*;
        let text = |b: &Block| format!("{}{}:{}", "  ".repeat(b.indent as usize), b.kind.as_str(), b.text);
        let mut page = vec![Block::text("Before"), Block::text("https://youtu.be/jNQXAC9IVRw"), Block::text("After")];
        let video = page[1].id.clone();
        let mut heard = Transcript::default();
        heard.add(Who::Others, "Warm up for four minutes.", 1.0);
        let (mut words, mut notes) = (None, None);
        // While it listens: the words, in a closed toggle right under the video's line.
        put_under(&mut page, &mut words, "Transcript of the video", true, Some(&video), heard.plain_blocks());
        assert_eq!(page.iter().map(text).collect::<Vec<_>>(), ["text:Before", "text:https://youtu.be/jNQXAC9IVRw", "toggle:Transcript of the video", "  text:0:01 — Warm up for four minutes.", "text:After"]);
        assert!(page[2].collapsed);
        // More is heard: the same toggle, with the new words in place of the old.
        heard.add(Who::Others, "Then two rounds.", 70.0);
        let first_line = page[3].id.clone();
        put_under(&mut page, &mut words, "Transcript of the video", true, Some(&video), heard.plain_blocks());
        assert_eq!(page.len(), 6);
        assert_eq!(page[3].id, first_line);
        assert_eq!(page[4].text, "1:10 — Then two rounds.");
        // At the end: the notes, in an open toggle between the line and the words.
        let mut point = Block::new(BlockKind::Bulleted, "Warm up: four minutes");
        point.indent = 1;
        put_under(&mut page, &mut notes, "Notes of the video", false, Some(&video), vec![point]);
        assert_eq!(
            page.iter().map(text).collect::<Vec<_>>(),
            ["text:Before", "text:https://youtu.be/jNQXAC9IVRw", "toggle:Notes of the video", "  bulleted:Warm up: four minutes", "toggle:Transcript of the video", "  text:0:01 — Warm up for four minutes.", "  text:1:10 — Then two rounds.", "text:After"]
        );
        assert!(!page[2].collapsed);
        // The line of the video is gone: a new toggle goes to the end of the page.
        let mut late = None;
        put_under(&mut page, &mut late, "Transcript of the video", true, Some("no such block"), heard.plain_blocks());
        assert_eq!(page[page.len() - 3].text, "Transcript of the video");
        // A video inside a list: its toggles are as deep as its line.
        let mut deep = vec![Block::new(BlockKind::Bulleted, "https://youtu.be/jNQXAC9IVRw").with_indent(1)];
        let line = deep[0].id.clone();
        put_under(&mut deep, &mut None, "Transcript of the video", true, Some(&line), heard.plain_blocks());
        assert_eq!((deep[1].indent, deep[2].indent), (1, 2));
        assert_eq!(heard.plain_text(), "Warm up for four minutes.\nThen two rounds.");
    }

    #[test]
    fn the_listener_says_where_speech_is_turned_into_text() {
        assert_eq!(on_this_mac_in(r#"{"microphone":"not asked","on_device":true,"language":"en_US","speech":"not asked","t":"check"}"#), Some(true));
        assert_eq!(on_this_mac_in("{\"on_device\":false,\"t\":\"check\"}\n"), Some(false));
        assert_eq!(on_this_mac_in(""), None);
        assert_eq!(on_this_mac_in("{\"t\":\"check\"}"), None);
    }

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
        // An answer that uses the words of the question is an answer.
        t.add(Who::Others, "Can you do that by Friday?", 70.0);
        t.add(Who::Me, "Yes I can do that by Friday", 73.0);
        assert!(t.text().ends_with("Others: Can you do that by Friday?\nMe: Yes I can do that by Friday"), "{}", t.text());
        // At the same moment as the others' line, two words heard
        // differently are still an echo.
        let before = t.echoes();
        t.add(Who::Others, "We need to move lunch to next week", 76.0);
        t.add(Who::Me, "need to move launch to next weak", 76.5);
        assert_eq!(t.echoes(), before + 1, "{}", t.text());
        // The same words in another order are not an echo.
        t.add(Who::Others, "The report goes to Sam on Monday and then to the board", 80.0);
        t.add(Who::Me, "On Monday the report goes to the board and then to Sam", 84.0);
        assert!(t.text().ends_with("Me: On Monday the report goes to the board and then to Sam"), "{}", t.text());

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
        put_transcript(&mut page, &mut toggle, 0, t.blocks());
        assert_eq!(page.len(), 3, "the empty last line gives way to the toggle");
        assert_eq!((page[1].kind, page[1].text.as_str(), page[1].collapsed), (BlockKind::Toggle, "Transcript", true));
        let first_line = page[2].id.clone();
        // More is said; then the person writes under the page.
        t.add(Who::Others, "Hi there.", 4.0);
        page.push(Block::text("My own note"));
        put_transcript(&mut page, &mut toggle, 0, t.blocks());
        assert_eq!(page.iter().map(|b| b.text.clone()).collect::<Vec<_>>(), vec!["Notes", "Transcript", "**Me** · 0:01 — Hello.", "**Others** · 0:04 — Hi there.", "My own note"]);
        assert_eq!(page[2].id, first_line, "lines already shown are not redrawn");
        // Nothing heard: nothing is added.
        let mut empty = vec![Block::text("x")];
        put_transcript(&mut empty, &mut None, 0, Vec::new());
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn listening_again_goes_on_in_the_same_transcript() {
        let texts = |page: &[Block]| page.iter().map(|b| b.text.clone()).collect::<Vec<_>>();
        // A page with no transcript: there is nothing to go on from.
        let mut page = vec![Block::new(BlockKind::Heading2, "Notes"), Block::text("")];
        assert_eq!(transcript_so_far(&page), (None, 0));
        // The first time of listening.
        let mut toggle = None;
        let mut first = Transcript::default();
        first.add(Who::Me, "Hello.", 1.0);
        first.add(Who::Others, "Hi there.", 4.0);
        put_transcript(&mut page, &mut toggle, 0, first.blocks());
        page.push(Block::text("My own note"));
        // The second time (the person stopped and pressed Listen again, or
        // the meeting went on): the same toggle, the old lines kept, a line
        // that says when, then the new words. Its clock starts again.
        let (mut again_in, kept) = transcript_so_far(&page);
        assert_eq!((again_in.clone(), kept), (toggle.clone(), 2));
        let marker = again_line("2:31 PM");
        let mut second = Transcript::default();
        second.add(Who::Others, "Back again.", 2.0);
        let lines = |t: &Transcript| std::iter::once(marker.clone()).chain(t.blocks()).collect::<Vec<_>>();
        put_transcript(&mut page, &mut again_in, kept, lines(&second));
        let old_ids: Vec<Id> = page[2..4].iter().map(|b| b.id.clone()).collect();
        second.add(Who::Me, "Good.", 5.0);
        put_transcript(&mut page, &mut again_in, kept, lines(&second));
        assert_eq!(
            texts(&page),
            vec!["Notes", "Transcript", "**Me** · 0:01 — Hello.", "**Others** · 0:04 — Hi there.", "_Listening started again at 2:31 PM_", "**Others** · 0:02 — Back again.", "**Me** · 0:05 — Good.", "My own note"]
        );
        assert_eq!(page.iter().filter(|b| b.kind == BlockKind::Toggle).count(), 1, "one transcript");
        assert_eq!(page[2..4].iter().map(|b| b.id.clone()).collect::<Vec<_>>(), old_ids, "the old lines are not touched");
        assert!(page[4..7].iter().all(|b| b.indent == 1));
        // A third time counts all of it.
        assert_eq!(transcript_so_far(&page), (toggle, 5));
        // The transcript of a video has a name of its own: it is not gone on from.
        let video = vec![Block::new(BlockKind::Toggle, "Transcript of the video"), Block::text("x").with_indent(1)];
        assert_eq!(transcript_so_far(&video), (None, 0));
    }

    #[test]
    fn a_line_changed_in_the_page_stays_while_the_app_listens() {
        use super::*;
        let mut t = Transcript::default();
        t.add(Who::Others, "The lunch moves to March.", 1.0);
        t.add(Who::Me, "Good.", 5.0);
        let mut page = vec![Block::text("Monday")];
        let (mut toggle, mut written) = (None, Vec::new());
        let write = |page: &mut Vec<Block>, toggle: &mut Option<Id>, written: &mut Vec<String>, t: &Transcript| {
            let mut lines = t.blocks();
            keep_changes(page, toggle.as_ref(), 0, written, &mut lines);
            put_transcript(page, toggle, 0, lines);
        };
        write(&mut page, &mut toggle, &mut written, &t);
        // The person puts a word right, and gives the voice a name.
        page[2].text = page[2].text.replace("lunch", "launch");
        assert_eq!(rename_voice(&mut page, "Others", "Sam"), 1);
        let fixed = page[2].text.clone();
        assert!(fixed.contains("**Sam**") && fixed.contains("launch"));
        // More is heard: the line they changed stays, the new line comes.
        t.add(Who::Others, "And the budget is agreed.", 70.0);
        write(&mut page, &mut toggle, &mut written, &t);
        assert_eq!(page[2].text, fixed);
        assert!(page[3].text.contains("Good.") && page[4].text.contains("budget"));
        // A turn that goes on is written again as heard (its words changed).
        t.add(Who::Others, "Sam sends it.", 75.0);
        page[4].text = "typed over".into();
        write(&mut page, &mut toggle, &mut written, &t);
        assert!(page[4].text.contains("And the budget is agreed. Sam sends it."));
        assert_eq!(page[2].text, fixed);
        // A line added by hand in the toggle: written as before, by place.
        page.insert(3, Block::text("my own line").with_indent(1));
        write(&mut page, &mut toggle, &mut written, &t);
        assert_eq!(page.len(), 5);
        assert!(page[2].text.contains("lunch") && page[3].text.contains("Good."));
    }

    #[test]
    fn the_notes_of_a_meeting_heard_in_two_parts_are_written_from_both() {
        use super::*;
        // The app was closed while it listened: the first part is in the page.
        let mut first = Transcript::default();
        first.add(Who::Me, "So the budget is agreed.", 1.0);
        first.add(Who::Others, "Yes, and Sam sends the numbers by Friday.", 3.0);
        let mut page = vec![Block::text("Monday"), Block::new(BlockKind::Heading2, "Notes")];
        let mut toggle = None;
        put_transcript(&mut page, &mut toggle, 0, first.blocks());
        let earlier = heard_before(&page, 0);
        assert_eq!(earlier, "Me: So the budget is agreed.\nOthers: Yes, and Sam sends the numbers by Friday.");
        // It listens again: the notes are written from both parts, in order.
        let mut second = Transcript::default();
        second.add(Who::Others, "The launch moves to March.", 2.0);
        let (mut toggle, kept) = transcript_so_far(&page);
        let mut lines = second.blocks();
        lines.insert(0, again_line("3:20 PM"));
        put_transcript(&mut page, &mut toggle, kept, lines);
        assert_eq!(all_heard(&earlier, &second.text()), "Me: So the budget is agreed.\nOthers: Yes, and Sam sends the numbers by Friday.\nOthers: The launch moves to March.");
        // (The line that says when it listened again is not a turn.)
        assert_eq!(heard_before(&page, 0).lines().count(), 3);
        // A first part that had its notes is not given again.
        assert_eq!(all_heard("", &second.text()), "Others: The launch moves to March.");
        assert_eq!(heard_before(&[Block::text("No transcript here")], 0), "");
        // The first part had its notes (Stop), and the app was closed while
        // it heard the second: the next notes take the second part, not the
        // first one again.
        assert_eq!(last_part(&page), 2);
        assert_eq!(heard_before(&page, last_part(&page)), "Others: The launch moves to March.");
        // A third part: the part that was heard last is the third.
        let (mut toggle, kept) = transcript_so_far(&page);
        let mut third = Transcript::default();
        third.add(Who::Me, "Thanks, all.", 1.0);
        let mut lines = third.blocks();
        lines.insert(0, again_line("3:40 PM"));
        put_transcript(&mut page, &mut toggle, kept, lines);
        assert_eq!(heard_before(&page, last_part(&page)), "Me: Thanks, all.");
        // Known from this run where the words with no notes begin: from there.
        assert_eq!(heard_before(&page, 2).lines().count(), 2);
        assert_eq!(last_part(&[Block::text("No transcript here")]), 0);
    }

    #[test]
    fn a_long_transcript_is_cut_in_the_middle_for_the_ai() {
        assert_eq!(for_ai("short", 100), "short");
        let long = "a".repeat(60) + &"b".repeat(60);
        let cut = for_ai(&long, 40);
        assert!(cut.starts_with(&"a".repeat(20)) && cut.ends_with(&"b".repeat(20)) && cut.contains("left out"));
    }
}
