//! A shell in the task's worktree for each session, and what of it the agent gets to see.
//!
//! The user types in the terminal as in any other. What happened there since the agent's last
//! turn travels with the user's next message, so the agent can act on a command it asked for
//! without the user pasting anything.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pty_process::{Command, OwnedReadPty, OwnedWritePty, Pty, Size};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::{broadcast, mpsc};

use crate::error::{Error, Result};
use crate::{Core, lock};

/// How much of a terminal's output is kept to redraw it when it is opened again.
const REPLAY_BYTES: usize = 200_000;
/// How much terminal output one message carries to the agent; older output is dropped.
const SHARED_BYTES: usize = 16_000;
/// How many chunks of output a slow front end may fall behind.
const OUTPUT_BUFFER: usize = 4096;

/// A piece of what a session's terminal printed, as published to every front end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalOutput {
    /// The session the terminal belongs to.
    pub session_id: i64,
    /// The text, with the escape sequences a terminal emulator expects.
    pub data: String,
    /// How many bytes the terminal had printed before this piece, since its shell started.
    pub offset: u64,
}

/// A terminal as it stands when a front end opens it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalOpened {
    /// What the terminal has printed so far, or the end of it, to redraw it from.
    pub replay: String,
    /// How many bytes it had printed when `replay` was taken. A piece of output whose
    /// `offset` is lower is already in the replay; any other came after it.
    pub end: u64,
}

/// What a terminal has printed: all of it for redrawing, and what the agent has not seen.
#[derive(Debug, Default)]
struct Log {
    replay: String,
    unshared: String,
    /// How many bytes were printed in all, whatever of them `replay` still holds.
    end: u64,
}

impl Log {
    /// Keeps `text`, and returns how much had been printed before it.
    fn push(&mut self, text: &str) -> u64 {
        let offset = self.end;
        self.end += text.len() as u64;
        self.replay.push_str(text);
        self.unshared.push_str(text);
        keep_end(&mut self.replay, REPLAY_BYTES);
        // Twice what is shared: escape sequences are stripped before the limit applies.
        keep_end(&mut self.unshared, SHARED_BYTES * 2);
        offset
    }

    /// What there is to redraw, with where it ends: taken together, so no output falls between.
    fn opened(&self) -> TerminalOpened {
        TerminalOpened {
            replay: self.replay.clone(),
            end: self.end,
        }
    }
}

/// Drops the start of `text` so that at most `limit` bytes remain.
fn keep_end(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut cut = text.len() - limit;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    text.drain(..cut);
}

/// What a front end sends to a terminal.
enum Input {
    Typed(String),
    Resized(Size),
}

/// A running shell: the way to reach it, and what it has printed.
struct Terminal {
    /// Tells this shell apart from one opened later for the same session.
    run: u64,
    /// Dropping this ends the shell.
    input: mpsc::UnboundedSender<Input>,
    log: Arc<Mutex<Log>>,
}

/// Every session's terminal, and the channel their output is published on.
pub(crate) struct Terminals {
    open: Mutex<HashMap<i64, Terminal>>,
    runs: AtomicU64,
    bus: broadcast::Sender<TerminalOutput>,
}

impl Default for Terminals {
    fn default() -> Self {
        Self {
            open: Mutex::default(),
            runs: AtomicU64::new(0),
            bus: broadcast::Sender::new(OUTPUT_BUFFER),
        }
    }
}

impl fmt::Debug for Terminals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sessions: Vec<i64> = lock(&self.open).keys().copied().collect();
        f.debug_struct("Terminals")
            .field("open", &sessions)
            .finish_non_exhaustive()
    }
}

fn failed(reason: impl fmt::Display) -> Error {
    Error::Terminal(reason.to_string())
}

fn size(cols: u16, rows: u16) -> Size {
    Size::new(rows.max(2), cols.max(10))
}

/// Starts the user's shell in `cwd` on a new pseudo-terminal.
fn start(cwd: &Path, size: Size) -> Result<(Pty, Child)> {
    let (pty, pts) = pty_process::open().map_err(failed)?;
    pty.resize(size).map_err(failed)?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let child = Command::new(shell)
        .arg("-l")
        .current_dir(cwd)
        .env("TERM", "xterm-256color")
        // The terminal draws 24-bit colour, and tools on macOS colour nothing unless asked.
        .env("COLORTERM", "truecolor")
        .env("CLICOLOR", "1")
        .kill_on_drop(true)
        .spawn(pts)
        .map_err(failed)?;
    Ok((pty, child))
}

/// Passes on what is typed until the terminal is closed. Owns the shell: when this ends, the
/// shell is killed, which is what ends the reading side too.
async fn feed(mut writer: OwnedWritePty, mut input: mpsc::UnboundedReceiver<Input>, child: Child) {
    while let Some(input) = input.recv().await {
        let written = match input {
            Input::Typed(text) => writer.write_all(text.as_bytes()).await.is_ok(),
            Input::Resized(size) => writer.resize(size).is_ok(),
        };
        if !written {
            break;
        }
    }
    drop(child);
}

/// Publishes what the shell prints until it exits, then forgets the terminal.
async fn pump(
    terminals: Arc<Terminals>,
    (session_id, run): (i64, u64),
    mut reader: OwnedReadPty,
    log: Arc<Mutex<Log>>,
) {
    let mut pending: Vec<u8> = vec![];
    let mut buffer = [0_u8; 8192];
    while let Ok(read) = reader.read(&mut buffer).await
        && read > 0
    {
        pending.extend_from_slice(&buffer[..read]);
        let data = take_text(&mut pending);
        let offset = lock(&log).push(&data);
        let output = TerminalOutput {
            session_id,
            data,
            offset,
        };
        // An error only means nobody is listening right now.
        terminals.bus.send(output).ok();
    }
    let mut open = lock(&terminals.open);
    if open.get(&session_id).is_some_and(|t| t.run == run) {
        open.remove(&session_id);
    }
}

/// Takes the text at the start of `bytes`, leaving a character that is still arriving.
fn take_text(bytes: &mut Vec<u8>) -> String {
    let whole = bytes.len() - unfinished(bytes);
    let text = String::from_utf8_lossy(&bytes[..whole]).into_owned();
    bytes.drain(..whole);
    text
}

/// How many bytes at the end of `bytes` are the start of a character whose rest has not come
/// yet. A byte that can never be part of text is not waited for: it is passed on as it is.
fn unfinished(bytes: &[u8]) -> usize {
    let cut_short = |held: &usize| {
        let end = std::str::from_utf8(&bytes[bytes.len() - held..]);
        // No length for the error means the bytes ran out in the middle of a character.
        end.is_err_and(|error| error.valid_up_to() == 0 && error.error_len().is_none())
    };
    // A character is four bytes at most, so its start is three at most.
    (1..=bytes.len().min(3)).find(cut_short).unwrap_or(0)
}

/// What a person would read on the screen: the text without colours, cursor movement and
/// window titles, and with lines a program redrew reduced to their last state.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => skip_escape(&mut chars),
            '\r' if chars.peek() != Some(&'\n') => erase_line(&mut out),
            '\r' | '\u{7}' => {}
            '\u{8}' => {
                out.pop();
            }
            _ => out.push(c),
        }
    }
    out
}

/// Removes what was written of the current line, as a carriage return lets a program overwrite.
fn erase_line(out: &mut String) {
    let line_start = out.rfind('\n').map_or(0, |at| at + 1);
    out.truncate(line_start);
}

/// Skips one escape sequence, of which the escape character itself was just read.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        // Control sequence: parameters, then one letter or symbol that ends it.
        Some('[') => {
            chars.find(|c| ('\u{40}'..='\u{7e}').contains(c));
        }
        // Operating system command, such as a window title: ends with a bell or `ESC \`.
        Some(']') => {
            let mut previous = ' ';
            chars.find(|c| {
                let ended = *c == '\u{7}' || (previous == '\u{1b}' && *c == '\\');
                previous = *c;
                ended
            });
        }
        // Character set choices take one more character; everything else is complete.
        Some('(' | ')') => {
            chars.next();
        }
        _ => {}
    }
}

/// The last `limit` bytes of `text`, starting at a line.
fn last_lines(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    keep_end(&mut text, limit);
    let first_whole_line = text.find('\n').map_or(0, |at| at + 1);
    text.split_off(first_whole_line)
}

impl Terminals {
    /// What the session's terminal printed since this was last asked, as plain text, and
    /// forgets it. Empty if there is no terminal or nothing happened.
    pub(crate) fn take_unshared(&self, session_id: i64) -> String {
        let open = lock(&self.open);
        let Some(terminal) = open.get(&session_id) else {
            return String::new();
        };
        let raw = std::mem::take(&mut lock(&terminal.log).unshared);
        last_lines(plain(&raw), SHARED_BYTES).trim().to_string()
    }

    /// Ends the session's shell, by letting go of the only way to reach it.
    pub(crate) fn close(&self, session_id: i64) {
        lock(&self.open).remove(&session_id);
    }

    fn send(&self, session_id: i64, input: Input) -> Result<()> {
        let open = lock(&self.open);
        let terminal = open
            .get(&session_id)
            .ok_or(Error::TerminalNotOpen(session_id))?;
        terminal
            .input
            .send(input)
            .map_err(|_| Error::TerminalNotOpen(session_id))
    }
}

impl Core {
    /// Opens the session's terminal, starting the user's shell in the task's worktree if it is
    /// not running, and returns what it has printed so far so it can be redrawn.
    ///
    /// Output goes on being published while the answer travels. A front end that listens
    /// before it asks draws the replay and then every piece from `end` on, and so shows
    /// each piece once.
    ///
    /// Must be called from within a tokio runtime.
    pub fn terminal_open(&self, session_id: i64, cols: u16, rows: u16) -> Result<TerminalOpened> {
        let session = self.session(session_id)?;
        let task = self.store().task_by_id(session.task_id)?;
        let task = task.ok_or(Error::SessionNotFound(session_id))?;
        let size = size(cols, rows);
        let mut open = lock(&self.terminals.open);
        if let Some(terminal) = open.get(&session_id) {
            terminal.input.send(Input::Resized(size)).ok();
            return Ok(lock(&terminal.log).opened());
        }
        let (pty, child) = start(&task.worktree_path, size)?;
        let (reader, writer) = pty.into_split();
        let (input, typed) = mpsc::unbounded_channel();
        let log = Arc::<Mutex<Log>>::default();
        let run = self.terminals.runs.fetch_add(1, Ordering::Relaxed);
        let terminal = Terminal {
            run,
            input,
            log: Arc::clone(&log),
        };
        open.insert(session_id, terminal);
        drop(tokio::spawn(feed(writer, typed, child)));
        let terminals = Arc::clone(&self.terminals);
        drop(tokio::spawn(pump(
            terminals,
            (session_id, run),
            reader,
            log,
        )));
        Ok(TerminalOpened {
            replay: String::new(),
            end: 0,
        })
    }

    /// Types `data` into the session's terminal.
    pub fn terminal_write(&self, session_id: i64, data: &str) -> Result<()> {
        self.terminals
            .send(session_id, Input::Typed(data.to_string()))
    }

    /// Tells the session's terminal how much room it has.
    pub fn terminal_resize(&self, session_id: i64, cols: u16, rows: u16) -> Result<()> {
        self.terminals
            .send(session_id, Input::Resized(size(cols, rows)))
    }

    /// Ends the session's shell. Does nothing if there is none.
    pub fn terminal_close(&self, session_id: i64) {
        self.terminals.close(session_id);
    }

    /// What every session's terminal prints, from now on.
    #[must_use]
    pub fn subscribe_terminals(&self) -> broadcast::Receiver<TerminalOutput> {
        self.terminals.bus.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_titles_and_redrawn_lines_are_not_shown_to_the_agent() {
        let raw = "\u{1b}]0;title\u{7}\u{1b}[1;32m$\u{1b}[0m ls\r\nDownloading 10%\rDownloading 100%\r\ndone\u{8}\u{8}ne\r\n";
        assert_eq!(plain(raw), "$ ls\nDownloading 100%\ndone\n");
    }

    #[test]
    fn a_character_split_across_two_reads_is_put_together() {
        let mut bytes = "né".as_bytes()[..2].to_vec();
        assert_eq!(take_text(&mut bytes), "n");
        bytes.push("é".as_bytes()[1]);
        assert_eq!(take_text(&mut bytes), "é");
        let mut binary = vec![0xff, 0xfe, 0xfd, 0xfc, b'a'];
        assert_eq!(take_text(&mut binary).chars().last(), Some('a'));
    }

    #[test]
    fn each_piece_of_output_knows_where_it_starts_and_a_replay_where_it_ends() {
        let mut log = Log::default();
        assert_eq!(log.push("$ ls\r\n"), 0);
        // Counted in bytes, as the text is published: "é" is two.
        assert_eq!(log.push("né"), 6);
        let opened = log.opened();
        assert_eq!((opened.replay.as_str(), opened.end), ("$ ls\r\nné", 9));
        // What comes after the replay was taken starts where it ends, and is not in it.
        assert_eq!(log.push("late"), opened.end);
    }

    #[test]
    fn a_replay_cut_short_still_ends_where_the_output_does() {
        let mut log = Log::default();
        let filler = "x".repeat(REPLAY_BYTES);
        log.push(&filler);
        log.push("tail");
        let opened = log.opened();
        assert_eq!(opened.replay.len(), REPLAY_BYTES);
        assert_eq!(opened.end, (REPLAY_BYTES + 4) as u64);
    }

    #[test]
    fn a_byte_that_is_not_text_does_not_hold_back_what_follows_it() {
        // Latin-1 for é, then a prompt: nothing more will come to complete it.
        let mut bytes = vec![0xe9, b'$', b' '];
        assert_eq!(take_text(&mut bytes), "\u{fffd}$ ");
        assert_eq!(bytes, Vec::<u8>::new());
        // A character cut short after such a byte is still waited for.
        let mut mixed = vec![0xe9, b'a', 0xc3];
        assert_eq!(take_text(&mut mixed), "\u{fffd}a");
        mixed.push(0xa9);
        assert_eq!(take_text(&mut mixed), "é");
    }

    #[test]
    fn only_the_end_of_long_output_is_shared_from_a_line_start() {
        let text = "first line\nsecond line\nthird line\n".to_string();
        assert_eq!(last_lines(text.clone(), 14), "third line\n");
        assert_eq!(last_lines(text.clone(), 1000), text);
    }
}
