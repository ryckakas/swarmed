//! Images a tool returns, such as a browser's screenshot, kept so that the user sees what the
//! agent looked at.
//!
//! An agent's backend reads them out of a tool's result as it reads the rest ([`Taking`]),
//! each checked as a pasted image is: PNG, JPEG, GIF or WebP, told by its first bytes, at most
//! [`MAX_RETURNED_MB`] each and [`MAX_PER_RESULT`] in one result. Where each was, the result's
//! text says `[image]`, as it did before any was kept, or why it was not kept. The core then
//! writes them into the session's own folder beside its pasted images ([`kept`]), named by the
//! call and a number and never over a file, and lists their paths on
//! [`AgentEvent::ToolResult`]. They go with that folder.
//!
//! An image can show anything a page or a file showed, a password included. It is shown to
//! the user and nothing else: never logged, nor handed to another session, a fork, the cloud
//! or search, and [`Returned`]'s `Debug` says only what it is.

use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::agent::AgentEvent;
use crate::error::Result;
use crate::pasted::{Format, created, format_of, private_folder, written};
use crate::paths::Paths;

/// The largest image a tool returns that is kept, in megabytes.
pub const MAX_RETURNED_MB: usize = 10;

/// The most images kept of one tool result.
pub const MAX_PER_RESULT: usize = 8;

const MAX_RETURNED_BYTES: usize = MAX_RETURNED_MB * 1024 * 1024;

/// What stands in a result's text where an image was kept, as it stood before any was.
pub(crate) const IMAGE: &str = "[image]";

/// What every returned image's file name starts with; the call, a number and the extension
/// follow.
const NAME_START: &str = "tool-";

/// How much of a call's id goes into the name of an image it returned.
const CALL_IN_NAME: usize = 64;

/// An image a tool returned, checked and not kept yet. It goes from an agent's backend to the
/// core with the result it came in, and is never stored or published.
#[derive(Clone, PartialEq, Eq)]
pub struct Returned {
    bytes: Vec<u8>,
    format: Format,
}

impl fmt::Debug for Returned {
    /// What it is and how large, never what it shows.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Returned")
            .field("format", &self.format)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// The images of one tool result, taken as its parts are read.
#[derive(Debug, Default)]
pub(crate) struct Taking {
    images: Vec<Returned>,
}

impl Taking {
    /// Takes an image given in base64, of the type `media_type` says, and returns what stands
    /// for it in the result's text: [`IMAGE`] for one kept, else why it is not.
    pub(crate) fn image(&mut self, media_type: &str, data: &str) -> String {
        match self.checked(media_type, data) {
            Ok(image) => {
                self.images.push(image);
                IMAGE.to_string()
            }
            Err(why) => format!("[image not shown: {why}]"),
        }
    }

    /// The image as it is kept, or why it is not: one too many, too large, or not a format
    /// that is shown, by its first bytes whatever its type says.
    fn checked(&self, media_type: &str, data: &str) -> std::result::Result<Returned, String> {
        let larger = || format!("larger than {MAX_RETURNED_MB} MB");
        if self.images.len() >= MAX_PER_RESULT {
            return Err(format!("more than {MAX_PER_RESULT} in one result"));
        }
        let data = data.trim();
        // Base64 is a third longer than what it stands for: one far too long is not decoded.
        if data.len() / 4 * 3 > MAX_RETURNED_BYTES + 3 {
            return Err(larger());
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| "it could not be read".to_string())?;
        if bytes.len() > MAX_RETURNED_BYTES {
            return Err(larger());
        }
        let format = format_of(&bytes).map_err(|_| unshown(media_type))?;
        Ok(Returned { bytes, format })
    }

    /// The images taken, in the order they came.
    pub(crate) fn images(self) -> Vec<Returned> {
        self.images
    }
}

/// Why an image of a format that is not shown is not: its type, as the tool named it.
fn unshown(media_type: &str) -> String {
    let named = media_type.trim();
    let printable = !named.is_empty() && named.chars().all(|c| c.is_ascii_graphic());
    if printable && named.len() <= 64 {
        named.to_string()
    } else {
        "a format that is not shown".to_string()
    }
}

/// The images a tool returned in `event` kept in session `session`'s own folder, and listed on
/// the event by their paths in place of what was read. One that cannot be written is left
/// out; its result still says `[image]` where it was. A sub-agent's result is kept as the
/// agent's own is, and every other event is passed on as it is.
pub(crate) fn kept(paths: &Paths, session: i64, event: AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
            mut images,
            returned,
        } => {
            let folder = paths.attachments(session);
            images.extend(
                returned
                    .iter()
                    .filter_map(|image| keep(&folder, &tool_use_id, image).ok()),
            );
            AgentEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                images,
                returned: Vec::new(),
            }
        }
        AgentEvent::Sub { parent, event } => AgentEvent::Sub {
            parent,
            event: Box::new(kept(paths, session, *event)),
        },
        other => other,
    }
}

/// A call's id as it can stand in a file's name: its letters, digits, dashes and underscores.
fn call_in_name(call: &str) -> String {
    let named = |c: &char| c.is_ascii_alphanumeric() || *c == '-' || *c == '_';
    let kept: String = call.chars().filter(named).take(CALL_IN_NAME).collect();
    if kept.is_empty() {
        "call".to_string()
    } else {
        kept
    }
}

/// Writes `image`, returned by call `call`, into `folder` as `tool-<call>-<n>.<ext>` and returns
/// its path. Never writes over a file: the same call's earlier images, and another Swarmed on
/// the same data directory, keep theirs.
fn keep(folder: &Path, call: &str, image: &Returned) -> Result<PathBuf> {
    private_folder(folder)?;
    let start = format!("{NAME_START}{}-", call_in_name(call));
    let mut number = 1_u64;
    loop {
        let path = folder.join(format!("{start}{number}.{}", image.format.extension()));
        if let Some(file) = created(&path)? {
            return written(file, path, &image.bytes);
        }
        number += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn base64(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    #[test]
    fn an_image_is_kept_by_what_it_is_and_named_where_it_is_not() {
        let mut taking = Taking::default();
        assert_eq!(taking.image("image/png", &base64(PNG)), IMAGE);
        // Its first bytes decide, not the type it is given.
        assert_eq!(taking.image("image/jpeg", &base64(PNG)), IMAGE);
        assert_eq!(
            taking.image("image/svg+xml", &base64(b"<svg xmlns=")),
            "[image not shown: image/svg+xml]"
        );
        assert_eq!(
            taking.image("", &base64(b"II*\0rest")),
            "[image not shown: a format that is not shown]"
        );
        assert_eq!(
            taking.image("image/png", "not base64!"),
            "[image not shown: it could not be read]"
        );
        let mut large = PNG.to_vec();
        large.resize(MAX_RETURNED_BYTES + 1, 0);
        assert_eq!(
            taking.image("image/png", &base64(&large)),
            "[image not shown: larger than 10 MB]"
        );
        let images = taking.images();
        assert_eq!(images.len(), 2);
        // What it shows is never printed.
        assert_eq!(
            format!("{:?}", images[0]),
            "Returned { format: Png, bytes: 16 }"
        );
    }

    #[test]
    fn at_most_eight_images_of_one_result_are_kept() {
        let mut taking = Taking::default();
        let said: Vec<String> = (0..10)
            .map(|_| taking.image("image/png", &base64(PNG)))
            .collect();
        assert!(said[..MAX_PER_RESULT].iter().all(|said| said == IMAGE));
        assert_eq!(said[8], "[image not shown: more than 8 in one result]");
        assert_eq!(taking.images().len(), MAX_PER_RESULT);
    }

    #[test]
    fn images_are_named_by_their_call_and_never_written_over() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("attachments").join("7");
        let image = Returned {
            bytes: PNG.to_vec(),
            format: Format::Png,
        };
        let first = keep(&folder, "toolu_01A/b..c", &image).expect("kept");
        let second = keep(&folder, "toolu_01A/b..c", &image).expect("kept");
        assert_eq!(first, folder.join("tool-toolu_01Abc-1.png"));
        assert_eq!(second, folder.join("tool-toolu_01Abc-2.png"));
        assert_eq!(std::fs::read(&first).expect("reads"), PNG);
        let mode = |path: &Path| std::fs::metadata(path).expect("there").permissions().mode();
        assert_eq!(mode(&folder) & 0o777, 0o700);
        assert_eq!(mode(&first) & 0o777, 0o600);
        assert_eq!(call_in_name("../.."), "call");
        assert_eq!(call_in_name(&"a".repeat(100)).len(), CALL_IN_NAME);
    }

    #[test]
    fn the_core_keeps_what_a_result_returned_and_lists_it_in_place_of_the_bytes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let paths = Paths {
            data_dir: dir.path().to_path_buf(),
            worktrees_root: dir.path().join("worktrees"),
            claude_config: dir.path().join("claude.json"),
            codex_config: dir.path().join("codex.toml"),
            opencode_config_dir: dir.path().join("opencode"),
        };
        let mut taking = Taking::default();
        let content = taking.image("image/png", &base64(PNG));
        let result = AgentEvent::ToolResult {
            tool_use_id: "t1".into(),
            content,
            is_error: false,
            images: Vec::new(),
            returned: taking.images(),
        };
        let sub = AgentEvent::Sub {
            parent: "agent".into(),
            event: Box::new(result.clone()),
        };
        let AgentEvent::ToolResult {
            images, returned, ..
        } = kept(&paths, 3, result)
        else {
            panic!("not a result");
        };
        assert_eq!(images, vec![paths.attachments(3).join("tool-t1-1.png")]);
        assert_eq!(returned, Vec::new());
        let AgentEvent::Sub { event, .. } = kept(&paths, 3, sub) else {
            panic!("not a sub-agent's");
        };
        assert!(matches!(
            *event,
            AgentEvent::ToolResult { ref images, .. }
                if images == &vec![paths.attachments(3).join("tool-t1-2.png")]
        ));
        let text = AgentEvent::Text { text: "hi".into() };
        assert_eq!(kept(&paths, 3, text.clone()), text);
    }
}
