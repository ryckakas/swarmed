//! A WebSocket client for one server only: the DevTools endpoint of a Chrome Swarmed started,
//! on this machine.
//!
//! Chrome speaks its DevTools protocol over a WebSocket on the port it writes into its
//! profile. The protocol needs little of WebSocket: text messages both ways, a ping now and
//! then, and a close. That much is written here rather than taken from a crate: the core links
//! no network client, and this one only ever talks to `127.0.0.1`.
//!
//! What the standard asks of a client against servers it does not know is left out on
//! purpose. The server's `Sec-WebSocket-Accept` is not checked, which needs SHA-1 and guards
//! against a server that is no WebSocket server at all, which the port Chrome wrote down is
//! not. The masking key, which keeps proxies from caching what a page sent, is not random:
//! there is no proxy between two processes on one machine.

use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

/// The longest the server's answer to the handshake may be.
const MAX_HANDSHAKE: usize = 16 * 1024;

/// The largest message taken: a picture of a large page, at twice its size, is a few
/// megabytes once it is written in base64.
pub(crate) const MAX_MESSAGE: usize = 64 * 1024 * 1024;

/// What a frame's first byte says it carries.
const CONTINUATION: u8 = 0x0;
const TEXT: u8 = 0x1;
const BINARY: u8 = 0x2;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
const PONG: u8 = 0xA;

/// A whole message from the server.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Message {
    /// A text message: in the DevTools protocol, one JSON object.
    Text(String),
    /// A ping, to be answered with a pong that carries the same.
    Ping(Vec<u8>),
}

/// Connects to `ws://127.0.0.1:<port><path>` and opens a WebSocket there, within `limit`:
/// a Chrome that runs and does not answer, stopped or hung, would otherwise hold whoever
/// connects, and the task's locks with them, for good.
pub(crate) async fn connect(
    port: u16,
    path: &str,
    limit: Duration,
) -> io::Result<(Reader, Writer)> {
    let late = || io::Error::new(io::ErrorKind::TimedOut, "it did not answer in time");
    tokio::time::timeout(limit, opened(port, path))
        .await
        .map_err(|_| late())?
}

async fn opened(port: u16, path: &str) -> io::Result<(Reader, Writer)> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await?;
    stream.set_nodelay(true)?;
    let (read, mut write) = stream.into_split();
    write.write_all(handshake(port, path).as_bytes()).await?;
    let mut reader = BufReader::new(read);
    let rest = read_answer(&mut reader).await?;
    Ok((
        Reader {
            inner: reader,
            pending: rest,
            gathering: Gathered::default(),
        },
        Writer { inner: write },
    ))
}

/// The request that asks the server to speak WebSocket. No `Origin` is sent: Chrome refuses
/// a connection from an origin it was not told to allow, and a client that is no web page
/// has none.
fn handshake(port: u16, path: &str) -> String {
    let key = base64::engine::general_purpose::STANDARD.encode(mask_key().repeat(4));
    format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
}

/// Reads the server's answer to the handshake, refusing anything but a switch to WebSocket.
/// Returns what came after it, which is the start of the first message.
async fn read_answer(reader: &mut BufReader<OwnedReadHalf>) -> io::Result<Vec<u8>> {
    let mut answer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = find(&answer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&answer[..end]).into_owned();
            switched(&head)?;
            return Ok(answer[end + 4..].to_vec());
        }
        if answer.len() > MAX_HANDSHAKE {
            return Err(invalid("the answer to the handshake is too long"));
        }
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        answer.extend_from_slice(&chunk[..read]);
    }
}

/// Whether the head of an answer switches to WebSocket; why not if it does not.
fn switched(head: &str) -> io::Result<()> {
    let status = head.lines().next().unwrap_or_default();
    if status.split_whitespace().nth(1) == Some("101") {
        Ok(())
    } else {
        Err(invalid(&format!("the server answered `{status}`")))
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

/// Four bytes that differ from one frame to the next. See the module's documentation for why
/// they need not be random.
fn mask_key() -> [u8; 4] {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    (nanos ^ COUNT.fetch_add(0x9E37_79B9, Ordering::Relaxed)).to_be_bytes()
}

/// The side of a connection that messages are read from.
#[derive(Debug)]
pub(crate) struct Reader {
    inner: BufReader<OwnedReadHalf>,
    /// What came with the handshake's answer and has not been read yet.
    pending: Vec<u8>,
    /// The message being put together. Kept here, not for one read: a ping may come between
    /// two of a message's frames, and is handed out before the message is whole.
    gathering: Gathered,
}

impl Reader {
    /// The next whole message; `None` once the server closed the connection.
    pub(crate) async fn next(&mut self) -> io::Result<Option<Message>> {
        next_message(self).await
    }

    /// The next frame; `None` if the connection ended between two frames.
    async fn frame(&mut self) -> io::Result<Option<Frame>> {
        let mut head = [0u8; 2];
        if !self.fill(&mut head).await? {
            return Ok(None);
        }
        let length = match head[1] & 0x7F {
            126 => u64::from(u16::from_be_bytes(self.exactly::<2>().await?)),
            127 => u64::from_be_bytes(self.exactly::<8>().await?),
            short => u64::from(short),
        };
        let length = usize::try_from(length)
            .ok()
            .filter(|length| *length <= MAX_MESSAGE)
            .ok_or_else(|| invalid("a frame larger than is taken"))?;
        // A server never masks what it sends; one that did would still be read right.
        let mask = if head[1] & 0x80 == 0 {
            None
        } else {
            Some(self.exactly::<4>().await?)
        };
        let mut payload = vec![0u8; length];
        if !self.fill(&mut payload).await? && length > 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if let Some(mask) = mask {
            masked(&mut payload, mask);
        }
        Ok(Some(Frame {
            fin: head[0] & 0x80 != 0,
            opcode: head[0] & 0x0F,
            payload,
        }))
    }

    async fn exactly<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut bytes = [0u8; N];
        if self.fill(&mut bytes).await? {
            Ok(bytes)
        } else {
            Err(io::ErrorKind::UnexpectedEof.into())
        }
    }

    /// Fills `buffer`, from what came with the handshake first. Says `false` if the connection
    /// ended before anything was read; ending part of the way through is an error.
    async fn fill(&mut self, buffer: &mut [u8]) -> io::Result<bool> {
        let taken = self.pending.len().min(buffer.len());
        buffer[..taken].copy_from_slice(&self.pending[..taken]);
        self.pending.drain(..taken);
        if taken == buffer.len() {
            return Ok(true);
        }
        match self.inner.read_exact(&mut buffer[taken..]).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof && taken == 0 => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// The next whole message `reader` reads, put together from its frames.
async fn next_message(reader: &mut Reader) -> io::Result<Option<Message>> {
    loop {
        let Some(frame) = reader.frame().await? else {
            return Ok(None);
        };
        match reader.gathering.take(frame)? {
            Taken::More => {}
            Taken::Message(message) => return Ok(Some(message)),
            Taken::Closed => return Ok(None),
        }
    }
}

/// A message's bytes as what it is.
fn whole(message: Vec<u8>, text: bool) -> io::Result<Message> {
    if !text {
        return Err(invalid(
            "a binary message, which the DevTools protocol never sends",
        ));
    }
    String::from_utf8(message)
        .map(Message::Text)
        .map_err(|_| invalid("a text message that is not UTF-8"))
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

/// A message being put together from its frames.
#[derive(Debug, Default)]
struct Gathered {
    bytes: Vec<u8>,
    binary: bool,
}

/// What a frame made of the message being put together.
enum Taken {
    /// More frames are to come.
    More,
    /// A whole message, or a ping.
    Message(Message),
    /// The server closed the connection.
    Closed,
}

impl Gathered {
    /// Takes in one frame, and says whether there is something to hand on.
    fn take(&mut self, frame: Frame) -> io::Result<Taken> {
        match frame.opcode {
            CLOSE => return Ok(Taken::Closed),
            PING => return Ok(Taken::Message(Message::Ping(frame.payload))),
            PONG => return Ok(Taken::More),
            TEXT | BINARY => self.binary = frame.opcode == BINARY,
            CONTINUATION => {}
            _ => return Err(invalid("a frame of an unknown kind")),
        }
        if self.bytes.len() + frame.payload.len() > MAX_MESSAGE {
            return Err(invalid("a message larger than is taken"));
        }
        self.bytes.extend_from_slice(&frame.payload);
        if !frame.fin {
            return Ok(Taken::More);
        }
        whole(std::mem::take(&mut self.bytes), !self.binary).map(Taken::Message)
    }
}

/// The side of a connection that messages are written to.
#[derive(Debug)]
pub(crate) struct Writer {
    inner: OwnedWriteHalf,
}

impl Writer {
    /// Sends one text message.
    pub(crate) async fn text(&mut self, text: &str) -> io::Result<()> {
        self.inner
            .write_all(&frame(TEXT, text.as_bytes(), mask_key()))
            .await
    }

    /// Answers a ping.
    pub(crate) async fn pong(&mut self, payload: &[u8]) -> io::Result<()> {
        self.inner
            .write_all(&frame(PONG, payload, mask_key()))
            .await
    }

    /// Says the connection is over, and ends it.
    pub(crate) async fn close(&mut self) {
        self.inner
            .write_all(&frame(CLOSE, &[], mask_key()))
            .await
            .ok();
        self.inner.shutdown().await.ok();
    }
}

/// One whole frame as a client sends it: masked, as the standard requires of a client.
fn frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    match u16::try_from(payload.len()) {
        Ok(short) if short < 126 => frame.push(0x80 | u8::try_from(short).unwrap_or(0)),
        Ok(medium) => {
            frame.push(0x80 | 0x7E);
            frame.extend_from_slice(&medium.to_be_bytes());
        }
        Err(_) => {
            frame.push(0x80 | 0x7F);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    let start = frame.len();
    frame.extend_from_slice(payload);
    masked(&mut frame[start..], mask);
    frame
}

fn masked(bytes: &mut [u8], mask: [u8; 4]) {
    for (at, byte) in bytes.iter_mut().enumerate() {
        *byte ^= mask[at % 4];
    }
}

/// The largest message [`Unframer`] reads; a larger one passes unread. What it looks for is
/// a few hundred bytes, and a picture of a page is megabytes.
pub(crate) const MAX_READ: usize = 256 * 1024;

/// Reads the text messages out of one direction of a WebSocket connection as it passes,
/// without changing what passes: a client's frames, masked, or a server's.
///
/// Only what is short enough is read; a longer message is passed over, frame by frame, and
/// never held. With `after_head`, the server's answer to the handshake comes first and is
/// passed over too.
#[derive(Debug, Default)]
pub(crate) struct Unframer {
    buffer: Vec<u8>,
    /// Whether the answer to the handshake is still to be passed over.
    in_head: bool,
    /// How much of a frame too long to read is still to come.
    passing: usize,
    /// The message being put together from its frames, while it is short enough.
    gathering: Option<Vec<u8>>,
}

impl Unframer {
    /// Reads a server's side, which starts with its answer to the handshake.
    pub(crate) fn after_head() -> Self {
        Self {
            in_head: true,
            ..Self::default()
        }
    }

    /// Takes in what passed next, and returns the text messages it completed.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let passed = self.passing.min(bytes.len());
        self.passing -= passed;
        self.buffer.extend_from_slice(&bytes[passed..]);
        let mut read = Vec::new();
        if !self.past_head() {
            return read;
        }
        loop {
            match self.next_frame() {
                Step::Wait => return read,
                Step::Read(message) => read.extend(message),
            }
        }
    }

    /// Passes over the answer to the handshake, if it is still to come. Says whether it has
    /// been.
    fn past_head(&mut self) -> bool {
        if !self.in_head {
            return true;
        }
        let Some(end) = find(&self.buffer, b"\r\n\r\n") else {
            return false;
        };
        self.buffer.drain(..end + 4);
        self.in_head = false;
        true
    }

    /// Takes the next whole frame off the buffer, with the message it completed if any.
    fn next_frame(&mut self) -> Step {
        let Some((head, length, mask)) = frame_head(&self.buffer) else {
            return Step::Wait;
        };
        let (fin, opcode) = (self.buffer[0] & 0x80 != 0, self.buffer[0] & 0x0F);
        if length > MAX_READ {
            self.pass_over(head, length, opcode);
            return Step::Read(None);
        }
        if self.buffer.len() < head + length {
            return Step::Wait;
        }
        let mut payload: Vec<u8> = self.buffer.drain(..head + length).skip(head).collect();
        if let Some(mask) = mask {
            masked(&mut payload, mask);
        }
        Step::Read(self.gathered(opcode, fin, payload))
    }

    /// Passes over a frame too long to read, and the message it belongs to.
    fn pass_over(&mut self, head: usize, length: usize, opcode: u8) {
        let here = self.buffer.len().saturating_sub(head).min(length);
        self.buffer.drain(..head + here);
        self.passing = length - here;
        let control = matches!(opcode, PING | PONG | CLOSE);
        if !control {
            self.gathering = None;
        }
    }

    /// Adds a frame's payload to the message it belongs to, and returns the message once it
    /// is whole.
    fn gathered(&mut self, opcode: u8, fin: bool, payload: Vec<u8>) -> Option<Vec<u8>> {
        match opcode {
            TEXT => self.gathering = Some(payload),
            CONTINUATION => self.gathering.as_mut()?.extend_from_slice(&payload),
            BINARY => self.gathering = None,
            _ => {}
        }
        let long = self
            .gathering
            .as_ref()
            .is_some_and(|so_far| so_far.len() > MAX_READ);
        if long {
            self.gathering = None;
        }
        let ends = fin && matches!(opcode, TEXT | CONTINUATION);
        if ends { self.gathering.take() } else { None }
    }
}

/// What reading the next frame came to.
enum Step {
    /// The frame is not whole yet.
    Wait,
    /// It was taken off, with the message it completed, if any.
    Read(Option<Vec<u8>>),
}

/// A frame's head at the start of `bytes`: its length, the length of its payload and its
/// mask; `None` until the whole head is there.
fn frame_head(bytes: &[u8]) -> Option<(usize, usize, Option<[u8; 4]>)> {
    let second = *bytes.get(1)?;
    let (mut head, length) = match second & 0x7F {
        126 => (
            4,
            u64::from(u16::from_be_bytes(bytes.get(2..4)?.try_into().ok()?)),
        ),
        127 => (10, u64::from_be_bytes(bytes.get(2..10)?.try_into().ok()?)),
        short => (2, u64::from(short)),
    };
    let mask = if second & 0x80 == 0 {
        None
    } else {
        let mask: [u8; 4] = bytes.get(head..head + 4)?.try_into().ok()?;
        head += 4;
        Some(mask)
    };
    let length = usize::try_from(length).unwrap_or(usize::MAX);
    Some((head, length, mask))
}

/// The server's side of the protocol, for the stand-in Chrome the tests talk to.
#[cfg(test)]
pub(crate) mod serving {
    use super::{CLOSE, PING, TEXT, find, invalid, masked};
    use std::io;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    /// Answers a client's handshake on `stream`.
    pub(crate) async fn accept(stream: &mut TcpStream) -> io::Result<()> {
        let mut asked = Vec::new();
        let mut chunk = [0u8; 512];
        while find(&asked, b"\r\n\r\n").is_none() {
            let read = stream.read(&mut chunk).await?;
            let more = chunk.get(..read).filter(|more| !more.is_empty());
            asked.extend_from_slice(more.ok_or(io::ErrorKind::UnexpectedEof)?);
        }
        let answer = "HTTP/1.1 101 WebSocket Protocol Handshake\r\nUpgrade: WebSocket\r\n\
                      Connection: Upgrade\r\nSec-WebSocket-Accept: unchecked\r\n\r\n";
        stream.write_all(answer.as_bytes()).await
    }

    /// The next text message a client sent; `None` once it closed.
    pub(crate) async fn read(stream: &mut TcpStream) -> io::Result<Option<String>> {
        let mut head = [0u8; 2];
        if stream.read_exact(&mut head).await.is_err() {
            return Ok(None);
        }
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0u8; 2];
                stream.read_exact(&mut bytes).await?;
                usize::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0u8; 8];
                stream.read_exact(&mut bytes).await?;
                usize::try_from(u64::from_be_bytes(bytes)).map_err(|_| invalid("too long"))?
            }
            short => usize::from(short),
        };
        let mut mask = [0u8; 4];
        stream.read_exact(&mut mask).await?;
        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload).await?;
        masked(&mut payload, mask);
        if head[0] & 0x0F == CLOSE {
            return Ok(None);
        }
        String::from_utf8(payload)
            .map(Some)
            .map_err(|_| invalid("not UTF-8"))
    }

    /// Sends a text message, unmasked, as a server does; in two frames when `split`.
    pub(crate) async fn send(stream: &mut TcpStream, text: &str, split: bool) -> io::Result<()> {
        let bytes = text.as_bytes();
        let (first, second) = bytes.split_at(if split { bytes.len() / 2 } else { bytes.len() });
        let mut out = unmasked(if split { TEXT } else { 0x80 | TEXT }, first);
        if split {
            out.extend(unmasked(0x80, second));
        }
        stream.write_all(&out).await
    }

    /// Sends a text message in two frames with a ping between them, which the standard allows.
    pub(crate) async fn send_around_ping(stream: &mut TcpStream, text: &str) -> io::Result<()> {
        let (first, second) = text.as_bytes().split_at(text.len() / 2);
        let mut out = unmasked(TEXT, first);
        out.extend(unmasked(0x80 | PING, b"still there?"));
        out.extend(unmasked(0x80, second));
        stream.write_all(&out).await
    }

    fn unmasked(first: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![first];
        match u16::try_from(payload.len()) {
            Ok(short) if short < 126 => frame.push(u8::try_from(short).unwrap_or(0)),
            Ok(medium) => {
                frame.push(126);
                frame.extend_from_slice(&medium.to_be_bytes());
            }
            Err(_) => {
                frame.push(127);
                frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
            }
        }
        frame.extend_from_slice(payload);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn a_frame_says_its_length_in_as_few_bytes_as_it_can() {
        let mask = [1, 2, 3, 4];
        assert_eq!(frame(TEXT, b"hi", mask)[..2], [0x81, 0x82]);
        let medium = frame(TEXT, &[0; 300], mask);
        assert_eq!(medium[..4], [0x81, 0xFE, 1, 44]);
        let long = frame(TEXT, &vec![0; 70_000], mask);
        assert_eq!(long[1], 0xFF);
        assert_eq!(long[2..10], 70_000u64.to_be_bytes());
    }

    #[test]
    fn what_a_client_sends_is_masked_and_unmasks_again() {
        let mask = [0x37, 0xFA, 0x21, 0x3D];
        let sent = frame(TEXT, b"Hello", mask);
        assert_eq!(sent[2..6], mask);
        let mut payload = sent[6..].to_vec();
        assert_ne!(payload, b"Hello");
        masked(&mut payload, mask);
        assert_eq!(payload, b"Hello");
    }

    /// A frame as a server sends it, unmasked.
    fn unmasked(first: u8, payload: &[u8]) -> Vec<u8> {
        let mut sent = frame(first & 0x0F, payload, [0; 4]);
        sent[0] = first;
        // The same frame with no mask: the bit off, and the four bytes of the key gone.
        let at = sent.len() - payload.len() - 4;
        sent[1] &= 0x7F;
        sent.drain(at..at + 4);
        sent
    }

    #[test]
    fn messages_are_read_as_they_pass_however_the_bytes_are_cut() {
        let mut sent = frame(TEXT, br#"{"id":1}"#, [9, 8, 7, 6]);
        sent.extend(frame(TEXT, &[b'x'; 300], [1, 2, 3, 4]));
        let mut unframer = Unframer::default();
        let mut read = Vec::new();
        for byte in &sent {
            read.extend(unframer.push(std::slice::from_ref(byte)));
        }
        assert_eq!(read, [br#"{"id":1}"#.to_vec(), vec![b'x'; 300]]);
    }

    #[test]
    fn a_servers_side_starts_after_its_answer_and_puts_pieces_together() {
        let mut passed = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n".to_vec();
        passed.extend(unmasked(TEXT, b"{\"method\":"));
        passed.extend(unmasked(0x80 | PING, b"hi"));
        passed.extend(unmasked(0x80 | CONTINUATION, b"\"x\"}"));
        let mut unframer = Unframer::after_head();
        let (head, rest) = passed.split_at(20);
        assert_eq!(unframer.push(head), Vec::<Vec<u8>>::new());
        assert_eq!(unframer.push(rest), [br#"{"method":"x"}"#.to_vec()]);
    }

    #[test]
    fn a_message_too_long_to_read_passes_and_the_next_is_read() {
        let mut passed = unmasked(0x80 | TEXT, &vec![b'p'; MAX_READ + 10]);
        passed.extend(unmasked(0x80 | TEXT, b"next"));
        let mut unframer = Unframer::default();
        let mut read = Vec::new();
        for piece in passed.chunks(4096) {
            read.extend(unframer.push(piece));
        }
        assert_eq!(read, [b"next".to_vec()]);
        assert!(unframer.buffer.len() < 4096);
    }

    #[test]
    fn only_a_switch_of_protocols_is_taken_as_an_answer() {
        assert!(
            switched("HTTP/1.1 101 WebSocket Protocol Handshake\r\nUpgrade: websocket").is_ok()
        );
        let refused = switched("HTTP/1.1 403 Forbidden").expect_err("refused");
        assert!(refused.to_string().contains("403 Forbidden"), "{refused}");
    }

    /// Hears one message from a client, then sends it a short one and a long one in two
    /// frames. Returns what it heard.
    async fn serve_once(listener: TcpListener) -> Option<String> {
        let (mut stream, _) = listener.accept().await.expect("accepts");
        serving::accept(&mut stream).await.expect("answers");
        let heard = serving::read(&mut stream).await.expect("reads");
        serving::send(&mut stream, "whole", false)
            .await
            .expect("sends");
        let long = "x".repeat(70_000);
        serving::send(&mut stream, &long, true)
            .await
            .expect("sends");
        serving::send_around_ping(&mut stream, "in two")
            .await
            .expect("sends");
        heard
    }

    const LIMIT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn a_server_that_takes_the_connection_and_says_nothing_is_given_up_on() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listens");
        let port = listener.local_addr().expect("has an address").port();
        // Held open, never answered: as a Chrome that is stopped or hung.
        let held = tokio::spawn(async move { listener.accept().await });
        let refused = connect(port, "/devtools/browser/1", Duration::from_millis(50)).await;
        assert_eq!(
            refused.err().map(|error| error.kind()),
            Some(io::ErrorKind::TimedOut)
        );
        held.abort();
    }

    #[tokio::test]
    async fn messages_go_both_ways_whole_and_in_pieces() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listens");
        let port = listener.local_addr().expect("has an address").port();
        let server = tokio::spawn(serve_once(listener));
        let (mut reader, mut writer) = connect(port, "/devtools/browser/1", LIMIT)
            .await
            .expect("opens");
        writer.text(r#"{"id":1}"#).await.expect("writes");
        assert_eq!(
            reader.next().await.expect("reads"),
            Some(Message::Text("whole".into()))
        );
        let long = reader.next().await.expect("reads");
        assert_eq!(long, Some(Message::Text("x".repeat(70_000))));
        // A ping between a message's two frames is handed out, and the message still whole.
        assert_eq!(
            reader.next().await.expect("reads"),
            Some(Message::Ping(b"still there?".to_vec()))
        );
        assert_eq!(
            reader.next().await.expect("reads"),
            Some(Message::Text("in two".into()))
        );
        assert_eq!(
            server.await.expect("served").as_deref(),
            Some(r#"{"id":1}"#)
        );
        assert_eq!(reader.next().await.expect("ends"), None);
    }
}
