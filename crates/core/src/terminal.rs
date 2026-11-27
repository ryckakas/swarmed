//! A shell in the task's worktree for each session, and what of it the agent gets to see.
//!
//! The user types in the terminal as in any other. What happened there since the agent's last
//! turn travels with the user's next message, so the agent can act on a command it asked for
//! without the user pasting anything. Its output is published and never stored, and nothing
//! gives an agent a way to type into it.
//!
//! A session on Claude Code can also open Claude Code itself in a terminal of its own, on the
//! screen of a command that has one only there, such as `/vim`. It runs as the user would
//! start it, with their own sign-in, and nothing it shows is given to the agent: a screen such
//! as `/usage-credits` shows a balance and card digits.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pty_process::{Command, OwnedReadPty, OwnedWritePty, Pty, Size};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::error::{Error, Result};
use crate::{Core, lock};

/// How much of a terminal's output is kept to redraw it when it is opened again.
const REPLAY_BYTES: usize = 200_000;
/// How much terminal output one message carries to the agent; older output is dropped.
const SHARED_BYTES: usize = 16_000;
/// How many chunks of output a slow front end may fall behind.
const OUTPUT_BUFFER: usize = 4096;
/// How long a program that has ended may be silent before all it printed counts as read.
const LAST_WORDS: Duration = Duration::from_millis(100);

/// A piece of what a session's terminal printed, as published to every front end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalOutput {
    /// The session the terminal belongs to.
    pub session_id: i64,
    /// The text, with the escape sequences a terminal emulator expects.
    pub data: String,
    /// How many bytes the terminal had printed before this piece, since its shell started.
    pub offset: u64,
    /// What runs in it has ended: `data` is empty, and nothing follows.
    pub ended: bool,
    /// Which run of the terminal printed this, as [`TerminalOpened::run`] names it. A piece of
    /// a run that another has since taken the place of may still be on its way.
    pub run: u64,
}

/// A terminal as it stands when a front end opens it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TerminalOpened {
    /// What the terminal has printed so far, or the end of it, to redraw it from.
    pub replay: String,
    /// How many bytes it had printed when `replay` was taken. A piece of output whose
    /// `offset` is lower is already in the replay; any other came after it.
    pub end: u64,
    /// Which run of the terminal this is: only output of the same run belongs to it.
    pub run: u64,
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

    /// What there is to redraw of run `run`, with where it ends: taken together, so no output
    /// falls between.
    fn opened(&self, run: u64) -> TerminalOpened {
        TerminalOpened {
            replay: self.replay.clone(),
            end: self.end,
            run,
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

/// What a terminal runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Program {
    /// The user's shell, as a login shell.
    Shell,
    /// Claude Code with its screen, opened on `command`, such as `/vim`.
    Claude {
        /// The `claude` executable.
        program: PathBuf,
        /// The command, slash and all.
        command: String,
    },
}

/// A running shell: the way to reach it, and what it has printed.
struct Terminal {
    /// Tells this shell apart from one opened later for the same session.
    run: u64,
    /// What it runs.
    program: Program,
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

/// Starts `program` in `cwd` on a new pseudo-terminal, and returns a hold on the program's
/// side of it too, for [`pump`] to keep open.
fn start_in(cwd: &Path, size: Size, program: &Program) -> Result<(Pty, OwnedFd, Child)> {
    let (pty, pts) = pty_process::open().map_err(failed)?;
    pty.resize(size).map_err(failed)?;
    let slave = pts.as_fd().try_clone_to_owned().map_err(failed)?;
    let child = command(program)
        .current_dir(cwd)
        .env("TERM", "xterm-256color")
        // The terminal draws 24-bit colour, and tools on macOS colour nothing unless asked.
        .env("COLORTERM", "truecolor")
        .env("CLICOLOR", "1")
        .kill_on_drop(true)
        .spawn(pts)
        .map_err(failed)?;
    Ok((pty, slave, child))
}

/// What starts `program`. Claude Code gets the `PATH` a shell would give it, and none of the
/// variables of Swarmed's own: no credential, and no mark of a Claude Code that Swarmed itself
/// may have been started from, so that it runs as the user's own does in a terminal.
fn command(program: &Program) -> Command {
    match program {
        Program::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            Command::new(shell).arg("-l")
        }
        Program::Claude { program, command } => {
            let started = Command::new(program)
                .arg(command)
                .env("PATH", crate::shellenv::path());
            claude_variables().fold(started, Command::env_remove)
        }
    }
}

/// The variables of this process that would decide how Claude Code signs in or behaves.
fn claude_variables() -> impl Iterator<Item = OsString> {
    std::env::vars_os().map(|(name, _)| name).filter(|name| {
        let name = name.to_string_lossy();
        name.starts_with("ANTHROPIC_") || name.starts_with("CLAUDE")
    })
}

/// Passes on what is typed until the terminal is closed or the program has ended. Owns the
/// program: when this ends, it is killed if it is still there, and `_gone` is let go of, which
/// tells [`pump`] either way.
async fn feed(
    mut writer: OwnedWritePty,
    mut input: mpsc::UnboundedReceiver<Input>,
    mut child: Child,
    _gone: oneshot::Sender<()>,
) {
    loop {
        let typed = tokio::select! {
            typed = input.recv() => typed,
            _ = child.wait() => None,
        };
        let written = match typed {
            Some(Input::Typed(text)) => writer.write_all(text.as_bytes()).await.is_ok(),
            Some(Input::Resized(size)) => writer.resize(size).is_ok(),
            None => false,
        };
        if !written {
            break;
        }
    }
}

/// One run of a terminal, as [`pump`] publishes it.
struct Printing {
    terminals: Arc<Terminals>,
    session_id: i64,
    run: u64,
    log: Arc<Mutex<Log>>,
    /// Bytes read that end in the middle of a character.
    pending: Vec<u8>,
}

impl Printing {
    /// Keeps and publishes `bytes`, as far as they are whole characters.
    fn print(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let data = take_text(&mut self.pending);
        let offset = lock(&self.log).push(&data);
        let output = TerminalOutput {
            session_id: self.session_id,
            data,
            offset,
            ended: false,
            run: self.run,
        };
        self.terminals.publish(output);
    }

    /// Publishes that the run has ended, and forgets the terminal, unless another run has
    /// taken its place.
    fn end(self) {
        let ended = TerminalOutput {
            session_id: self.session_id,
            data: String::new(),
            offset: lock(&self.log).end,
            ended: true,
            run: self.run,
        };
        let mut open = lock(&self.terminals.open);
        let current = open.get(&self.session_id);
        if current.is_some_and(|terminal| terminal.run == self.run) {
            open.remove(&self.session_id);
            // An error only means nobody is listening right now.
            self.terminals.bus.send(ended).ok();
        }
    }
}

/// Publishes what the program prints until it is gone, then that it ended, and forgets the
/// terminal. Once another run has taken the terminal's place, nothing more of this one is
/// published: it would be drawn as the new one's.
///
/// The program's side of the pseudo-terminal, `slave`, is held open until all it printed is
/// read. macOS throws away what is unread when the last holder of that side lets go, so a
/// program that prints and ends at once, with why it could not start for one, would be seen to
/// end without a word. Held open, the reading side never says it is over; `gone` does, and
/// what is printed by then can be read at once.
async fn pump(
    mut printing: Printing,
    (mut reader, slave): (OwnedReadPty, OwnedFd),
    mut gone: oneshot::Receiver<()>,
) {
    let mut buffer = [0_u8; 8192];
    loop {
        let read = tokio::select! {
            read = reader.read(&mut buffer) => read,
            _ = &mut gone => break,
        };
        match read {
            Ok(read) if read > 0 => printing.print(&buffer[..read]),
            _ => break,
        }
    }
    while let Ok(Ok(read)) = tokio::time::timeout(LAST_WORDS, reader.read(&mut buffer)).await
        && read > 0
    {
        printing.print(&buffer[..read]);
    }
    drop(slave);
    printing.end();
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
    /// Publishes `output`, unless another run has taken its terminal's place.
    fn publish(&self, output: TerminalOutput) {
        let open = lock(&self.open);
        let current = open.get(&output.session_id);
        if current.is_some_and(|terminal| terminal.run == output.run) {
            // An error only means nobody is listening right now.
            self.bus.send(output).ok();
        }
    }

    /// The terminal of `session_id` running `program`, told its size, and what it printed so
    /// far. It is started in `cwd` if none runs, or if the one that runs runs something else,
    /// in whose place it then is.
    ///
    /// Must be called from within a tokio runtime.
    fn open(
        self: &Arc<Self>,
        session_id: i64,
        (cwd, size): (&Path, Size),
        program: &Program,
    ) -> Result<TerminalOpened> {
        let mut open = lock(&self.open);
        let running = open.get(&session_id);
        if let Some(terminal) = running.filter(|terminal| terminal.program == *program) {
            terminal.input.send(Input::Resized(size)).ok();
            return Ok(lock(&terminal.log).opened(terminal.run));
        }
        let (pty, slave, child) = start_in(cwd, size, program)?;
        let (reader, writer) = pty.into_split();
        let (input, typed) = mpsc::unbounded_channel();
        let log = Arc::<Mutex<Log>>::default();
        let run = self.runs.fetch_add(1, Ordering::Relaxed);
        let terminal = Terminal {
            run,
            program: program.clone(),
            input,
            log: Arc::clone(&log),
        };
        // One this takes the place of ends, as its way in is dropped.
        open.insert(session_id, terminal);
        let (gone, is_gone) = oneshot::channel();
        drop(tokio::spawn(feed(writer, typed, child, gone)));
        let printing = Printing {
            terminals: Arc::clone(self),
            session_id,
            run,
            log,
            pending: vec![],
        };
        drop(tokio::spawn(pump(printing, (reader, slave), is_gone)));
        Ok(TerminalOpened {
            run,
            ..TerminalOpened::default()
        })
    }

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
        let worktree = self.worktree_of(session_id)?;
        let size = size(cols, rows);
        self.terminals
            .open(session_id, (&worktree, size), &Program::Shell)
    }

    /// The task's worktree, where a session's terminals start.
    fn worktree_of(&self, session_id: i64) -> Result<PathBuf> {
        let session = self.session(session_id)?;
        let task = self.store().task_by_id(session.task_id)?;
        let task = task.ok_or(Error::SessionNotFound(session_id))?;
        Ok(task.worktree_path)
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

    /// Ends the session's shell, and Claude Code if the session opened it in a terminal.
    /// Does nothing if there is neither.
    pub fn terminal_close(&self, session_id: i64) {
        self.terminals.close(session_id);
        self.claude_terminals.close(session_id);
    }

    /// Opens Claude Code with its screen, on `command`, in a terminal of the session's own:
    /// for a command whose screen Claude Code has only there, such as `vim`, named without its
    /// slash. One the session opened on the same command and that still runs is shown as it
    /// stands; any other is replaced. It starts in the task's worktree as the user would start
    /// it, with their own sign-in. Nothing it shows is given to the agent.
    ///
    /// It is a new conversation, never the session's: resuming that would put two Claude Codes
    /// on one conversation. In a new worktree it first asks whether to trust the folder, which
    /// is the user's to answer.
    ///
    /// Must be called from within a tokio runtime.
    pub fn claude_terminal_open(
        &self,
        session_id: i64,
        command: &str,
        (cols, rows): (u16, u16),
    ) -> Result<TerminalOpened> {
        let session = self.session(session_id)?;
        let refused = || Error::NoClaudeScreen(command.to_string());
        if !crate::commands::opens_its_screen(&session.agent, command) {
            return Err(refused());
        }
        let backend = self.agents_now().backend(&session.agent)?;
        let program = Program::Claude {
            program: backend.interactive().ok_or_else(refused)?,
            command: format!("/{command}"),
        };
        let worktree = self.worktree_of(session_id)?;
        let size = size(cols, rows);
        self.claude_terminals
            .open(session_id, (&worktree, size), &program)
    }

    /// Types `data` into the Claude Code the session opened.
    pub fn claude_terminal_write(&self, session_id: i64, data: &str) -> Result<()> {
        self.claude_terminals
            .send(session_id, Input::Typed(data.to_string()))
    }

    /// Tells the Claude Code the session opened how much room it has.
    pub fn claude_terminal_resize(&self, session_id: i64, cols: u16, rows: u16) -> Result<()> {
        self.claude_terminals
            .send(session_id, Input::Resized(size(cols, rows)))
    }

    /// Ends the Claude Code the session opened. Does nothing if there is none.
    pub fn claude_terminal_close(&self, session_id: i64) {
        self.claude_terminals.close(session_id);
    }

    /// What every Claude Code the sessions opened prints, from now on, and when each ends.
    #[must_use]
    pub fn subscribe_claude_terminals(&self) -> broadcast::Receiver<TerminalOutput> {
        self.claude_terminals.bus.subscribe()
    }

    /// What every session's terminal prints, from now on.
    #[must_use]
    pub fn subscribe_terminals(&self) -> broadcast::Receiver<TerminalOutput> {
        self.terminals.bus.subscribe()
    }
}

/// What `rounds` runs of a program that prints `last words` and ends at once were each heard
/// to print by the time they said they ended.
#[cfg(test)]
async fn last_words_of_each(terminals: &Arc<Terminals>, rounds: i64) -> Vec<String> {
    let mut output = terminals.bus.subscribe();
    let echo = Program::Claude {
        program: PathBuf::from("/bin/echo"),
        command: "last words".to_string(),
    };
    let at = (Path::new("/"), size(80, 24));
    let mut printed = vec![];
    for round in 0..rounds {
        let opened = terminals.open(round, at, &echo).expect("opens");
        printed.push(all_of_a_run(&mut output, (round, opened.run)).await);
    }
    printed
}

/// Everything the run printed, once it has said it ended. Nothing else may be published.
#[cfg(test)]
async fn all_of_a_run(output: &mut broadcast::Receiver<TerminalOutput>, run: (i64, u64)) -> String {
    let mut seen = String::new();
    loop {
        let heard = output.recv().await.expect("published");
        assert_eq!((heard.session_id, heard.run), run);
        seen.push_str(&heard.data);
        if heard.ended {
            return seen;
        }
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
        let opened = log.opened(3);
        assert_eq!((opened.replay.as_str(), opened.end), ("$ ls\r\nné", 9));
        assert_eq!(opened.run, 3);
        // What comes after the replay was taken starts where it ends, and is not in it.
        assert_eq!(log.push("late"), opened.end);
    }

    #[test]
    fn a_replay_cut_short_still_ends_where_the_output_does() {
        let mut log = Log::default();
        let filler = "x".repeat(REPLAY_BYTES);
        log.push(&filler);
        log.push("tail");
        let opened = log.opened(0);
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

    /// A program that stays, as Claude Code does on a screen: `sleep`, for `seconds`.
    fn staying(seconds: &str) -> Program {
        Program::Claude {
            program: PathBuf::from("/bin/sleep"),
            command: seconds.to_string(),
        }
    }

    #[tokio::test]
    async fn a_run_is_shown_again_on_the_same_command_and_replaced_on_another() {
        let terminals = Arc::<Terminals>::default();
        let mut output = terminals.bus.subscribe();
        let at = (Path::new("/"), size(80, 24));
        let first = terminals.open(7, at, &staying("60")).expect("opens");
        let again = terminals
            .open(7, at, &staying("60"))
            .expect("is shown again");
        assert_eq!(again.run, first.run);
        let other = terminals
            .open(7, at, &staying("61"))
            .expect("takes its place");
        assert_ne!(other.run, first.run);
        // Another session's is its own.
        let apart = terminals.open(8, at, &staying("60")).expect("opens");
        assert_ne!(apart.run, other.run);
        let typed = || Input::Typed("q".to_string());
        terminals.send(7, typed()).expect("types");

        terminals.close(7);
        let closed = terminals.send(7, typed());
        assert!(matches!(closed, Err(Error::TerminalNotOpen(7))));
        terminals.send(8, typed()).expect("types");
        // Neither the one replaced nor the one closed says it ended: whoever shows the
        // terminal by then shows another run, or none.
        tokio::time::sleep(LAST_WORDS * 4).await;
        let heard: Vec<TerminalOutput> = std::iter::from_fn(|| output.try_recv().ok()).collect();
        assert!(heard.iter().all(|piece| !piece.ended), "{heard:?}");
    }

    #[tokio::test]
    async fn what_a_program_prints_as_it_ends_is_published_before_its_end() {
        let terminals = Arc::<Terminals>::default();
        // Often, since what is lost is lost only when the program is gone before it is read.
        let printed = last_words_of_each(&terminals, 20).await;
        let whole = |seen: &String| seen.contains("last words");
        assert!(printed.iter().all(whole), "{printed:?}");
        assert_eq!(terminals.take_unshared(0), "");
    }

    #[test]
    fn only_the_end_of_long_output_is_shared_from_a_line_start() {
        let text = "first line\nsecond line\nthird line\n".to_string();
        assert_eq!(last_lines(text.clone(), 14), "third line\n");
        assert_eq!(last_lines(text.clone(), 1000), text);
    }
}
