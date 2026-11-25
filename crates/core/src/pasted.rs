//! Images pasted into a message box: the one part of a message Swarmed keeps.
//!
//! Attaching a file is naming its path ([`crate::with_files`]): nothing is copied, since the
//! agent reads the file where it is. A pasted image, most often a screenshot, has no path, so
//! it is given one. The core writes it into a folder of the session's own in the data
//! directory, `attachments/<session>/`, readable by the user alone, and its path is listed under
//! the message like any file's. Nothing is written into a repository or a worktree. The folder
//! goes when the session goes for good, and a fork is given copies of its own. The images a
//! session's tools return are kept in the same folder ([`crate::returned`]), and read back
//! the same way.
//!
//! Only the formats agents read are kept, PNG, JPEG, GIF and WebP, told by their first bytes and
//! not by a name, and only up to [`MAX_IMAGE_MB`]. Nothing is converted: there is no image
//! library here, so anything else is refused with what it is.

use std::ffi::OsString;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::{Core, open};

/// The largest image kept, in megabytes: a full screenshot of a large Retina screen fits.
pub const MAX_IMAGE_MB: usize = 20;

const MAX_IMAGE_BYTES: usize = MAX_IMAGE_MB * 1024 * 1024;

/// What every pasted image's file name starts with; a number and the format's extension follow.
const NAME_START: &str = "pasted-";

/// What is said of something pasted that is no image anybody could name.
const UNKNOWN: &str = "what was pasted";

/// A format agents read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl Format {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}

/// The format of an image, told by its first bytes, or why it is not one agents read.
pub(crate) fn format_of(bytes: &[u8]) -> Result<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(Format::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok(Format::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Ok(Format::Gif)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Ok(Format::Webp)
    } else {
        Err(Error::ImageNotRead(named(bytes).to_string()))
    }
}

/// What an image agents do not read is, for saying why it is refused: macOS puts TIFF on the
/// clipboard beside PNG, and a photo from an iPhone is HEIC.
fn named(bytes: &[u8]) -> &'static str {
    const HEIF_BRANDS: [&[u8]; 6] = [b"heic", b"heix", b"hevc", b"heim", b"mif1", b"msf1"];
    let brand = bytes.get(8..12);
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        "a TIFF image"
    } else if bytes.starts_with(b"BM") {
        "a BMP image"
    } else if bytes.get(4..8) == Some(b"ftyp") && brand == Some(b"avif") {
        "an AVIF image"
    } else if bytes.get(4..8) == Some(b"ftyp") && brand.is_some_and(|b| HEIF_BRANDS.contains(&b)) {
        "a HEIC image"
    } else {
        UNKNOWN
    }
}

/// A pasted image as the window hands it over, in base64, checked: its bytes and its format.
#[derive(Debug, Clone)]
pub(crate) struct Pasted {
    bytes: Vec<u8>,
    format: Format,
}

impl Pasted {
    /// Reads and checks an image given in base64: a format agents read, and not too large.
    pub(crate) fn from_base64(data: &str) -> Result<Self> {
        let data = data.trim();
        // Base64 is a third longer than what it stands for: one far too long is not decoded.
        if data.len() / 4 * 3 > MAX_IMAGE_BYTES + 3 {
            return Err(Error::ImageTooLarge(MAX_IMAGE_MB));
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| Error::ImageNotRead(UNKNOWN.to_string()))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(Error::ImageTooLarge(MAX_IMAGE_MB));
        }
        let format = format_of(&bytes)?;
        Ok(Self { bytes, format })
    }

    /// Checks every image of a message at once, so that none is kept if one is refused.
    pub(crate) fn all(images: &[String]) -> Result<Vec<Self>> {
        images.iter().map(|data| Self::from_base64(data)).collect()
    }
}

/// Makes `folder` and the folders above it for the user alone.
pub(crate) fn private_folder(folder: &Path) -> Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    Ok(())
}

/// The number of a pasted image's file, from its name: 3 for `pasted-3.png`.
fn number_of(name: &str) -> Option<u64> {
    let rest = name.strip_prefix(NAME_START)?;
    rest.split_once('.')?.0.parse().ok()
}

/// The number after the highest one in `folder`, so that no image takes another's name.
fn next_number(folder: &Path) -> Result<u64> {
    let mut highest = 0;
    for entry in std::fs::read_dir(folder)? {
        let name = entry?.file_name();
        highest = highest.max(name.to_str().and_then(number_of).unwrap_or(0));
    }
    Ok(highest + 1)
}

/// A new file at `path` for the user alone, or `None` if there is one there already.
pub(crate) fn created(path: &Path) -> Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    match options.open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Writes `image` into `folder` under a name of its own and returns its path. Never writes
/// over a file: another Swarmed on the same data directory may be keeping one at the same time.
fn keep(folder: &Path, image: &Pasted) -> Result<PathBuf> {
    private_folder(folder)?;
    let mut number = next_number(folder)?;
    loop {
        let name = format!("{NAME_START}{number}.{}", image.format.extension());
        let path = folder.join(name);
        if let Some(file) = created(&path)? {
            return written(file, path, &image.bytes);
        }
        number += 1;
    }
}

/// `path` once `bytes` are written into its `file`. A file only half written is no image, so
/// it goes again.
pub(crate) fn written(mut file: File, path: PathBuf, bytes: &[u8]) -> Result<PathBuf> {
    if let Err(error) = file.write_all(bytes) {
        std::fs::remove_file(&path).ok();
        return Err(error.into());
    }
    Ok(path)
}

/// `path` as it really is, if it is a file right in `folder`: not in a folder below it, and not
/// a link that leads out of it.
fn inside(folder: &Path, path: &Path) -> Result<PathBuf> {
    let not_pasted = || Error::NotPastedImage(path.to_path_buf());
    let root = folder.canonicalize().map_err(|_| not_pasted())?;
    let real = path.canonicalize().map_err(|_| not_pasted())?;
    if real.parent() == Some(root.as_path()) && real.is_file() {
        Ok(real)
    } else {
        Err(not_pasted())
    }
}

/// `path` written with a slash after it, as it stands before a file's name in a message.
fn folder_in_text(path: &Path) -> String {
    format!("{}/", path.display())
}

impl Core {
    /// Keeps images pasted into a message to session `id`, each given in base64, and returns
    /// their paths in the same order, to be attached to the message like any file.
    ///
    /// Each goes into the session's own folder, `attachments/<id>/` in the data directory,
    /// which only the user can read, under a name the core gives it. Every image is checked
    /// before any is kept: a format agents read, told by its first bytes, and at most
    /// [`MAX_IMAGE_MB`] megabytes.
    pub fn session_images_keep(&self, id: i64, images: &[String]) -> Result<Vec<PathBuf>> {
        let pasted = Pasted::all(images)?;
        self.session(id)?;
        self.images_kept(id, &pasted)
    }

    /// Writes images already checked into session `id`'s folder.
    pub(crate) fn images_kept(&self, id: i64, images: &[Pasted]) -> Result<Vec<PathBuf>> {
        let folder = self.paths.attachments(id);
        images.iter().map(|image| keep(&folder, image)).collect()
    }

    /// An image kept for session `id`, pasted into it or returned by one of its tools, as a
    /// `data:` address the window can show it from, since its policy loads images from nowhere
    /// else. Only a file right in the session's own folder is read, links followed: anything
    /// else is [`Error::NotPastedImage`].
    pub fn session_image(&self, id: i64, path: &Path) -> Result<String> {
        let real = inside(&self.paths.attachments(id), path)?;
        if std::fs::metadata(&real)?.len() > MAX_IMAGE_BYTES as u64 {
            return Err(Error::ImageTooLarge(MAX_IMAGE_MB));
        }
        let bytes = std::fs::read(&real)?;
        let format = format_of(&bytes)?;
        Ok(format!(
            "data:{};base64,{}",
            format.mime(),
            STANDARD.encode(bytes)
        ))
    }

    /// Opens an image kept for session `id` in the system's viewer, Preview on a Mac. Only one
    /// right in the session's own folder, as [`Core::session_image`] reads.
    pub fn session_image_open(&self, id: i64, path: &Path) -> Result<()> {
        open::in_viewer(&inside(&self.paths.attachments(id), path)?)
    }

    /// The folder that holds the images kept for every session, each in a folder named by the
    /// session's id. A front end tells a message's pasted images from other files by it.
    #[must_use]
    pub fn attachments_root(&self) -> PathBuf {
        self.paths.attachments_root()
    }

    /// Gives a fork its own copies of the images pasted into session `from` that `events`
    /// name, and has the messages among them name the copies, in session `to`'s folder. The
    /// fork then loses nothing when the first session is deleted with its folder. What the
    /// first session's tools returned is not named by a message, and stays behind.
    pub(crate) fn images_forked(
        &self,
        from: i64,
        to: i64,
        events: &mut [AgentEvent],
    ) -> Result<()> {
        let (source, copy) = (self.paths.attachments(from), self.paths.attachments(to));
        let (was, now) = (folder_in_text(&source), folder_in_text(&copy));
        let messages: String = events
            .iter_mut()
            .filter_map(|event| match event {
                AgentEvent::UserMessage { text, .. } if text.contains(&was) => {
                    *text = text.replace(&was, &now);
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        if messages.is_empty() {
            return Ok(());
        }
        let named = |name: &OsString| messages.contains(&copy.join(name).display().to_string());
        let images: Vec<OsString> = std::fs::read_dir(&source)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .filter(|name| name.as_ref().map_or(true, named))
            .collect::<std::io::Result<_>>()?;
        private_folder(&copy)?;
        for name in images {
            // On a Mac's own disk a copy is a clone, which takes no room until one changes.
            std::fs::copy(source.join(&name), copy.join(&name))?;
        }
        Ok(())
    }

    /// Deletes the images kept for session `id`, pasted and returned by its tools, for when it
    /// goes for good. Best effort: the session is going, and nothing reads them again.
    pub(crate) fn forget_images(&self, id: i64) {
        std::fs::remove_dir_all(self.paths.attachments(id)).ok();
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
    fn formats_are_told_by_their_first_bytes() {
        assert_eq!(format_of(PNG).expect("png"), Format::Png);
        assert_eq!(format_of(b"\xff\xd8\xff\xe0").expect("jpeg"), Format::Jpeg);
        assert_eq!(format_of(b"GIF89a....").expect("gif"), Format::Gif);
        assert_eq!(
            format_of(b"RIFF\0\0\0\0WEBPVP8 ").expect("webp"),
            Format::Webp
        );
        let refused = |bytes: &[u8]| match format_of(bytes) {
            Err(Error::ImageNotRead(what)) => what,
            other => panic!("not refused: {other:?}"),
        };
        assert_eq!(refused(b"II*\0rest"), "a TIFF image");
        assert_eq!(refused(b"MM\0*rest"), "a TIFF image");
        assert_eq!(refused(b"BM......"), "a BMP image");
        assert_eq!(refused(b"\0\0\0\x18ftypheic...."), "a HEIC image");
        assert_eq!(refused(b"\0\0\0\x18ftypavif...."), "an AVIF image");
        assert_eq!(refused(b"<svg xmlns="), UNKNOWN);
        assert_eq!(refused(b""), UNKNOWN);
        // A name says nothing: a PNG called `.jpg` is a PNG, a text called `.png` is no image.
        assert_eq!(refused(b"RIFF\0\0\0\0WAVE"), UNKNOWN);
    }

    #[test]
    fn what_is_handed_over_is_checked_before_it_is_kept() {
        let pasted = Pasted::from_base64(&format!(" {} ", base64(PNG))).expect("a png");
        assert_eq!((pasted.format, pasted.bytes.as_slice()), (Format::Png, PNG));
        assert!(matches!(
            Pasted::from_base64("not base64!"),
            Err(Error::ImageNotRead(what)) if what == UNKNOWN
        ));
        let mut large = PNG.to_vec();
        large.resize(MAX_IMAGE_BYTES + 1, 0);
        assert!(matches!(
            Pasted::from_base64(&base64(&large)),
            Err(Error::ImageTooLarge(MAX_IMAGE_MB))
        ));
        // Too long to be small enough is refused before it is decoded.
        assert!(matches!(
            Pasted::from_base64(&"A".repeat(MAX_IMAGE_BYTES / 3 * 4 + 16)),
            Err(Error::ImageTooLarge(_))
        ));
        // One refused image refuses them all.
        let both = [base64(PNG), base64(b"II*\0")];
        assert!(matches!(Pasted::all(&both), Err(Error::ImageNotRead(_))));
    }

    #[test]
    fn each_image_gets_a_name_of_its_own_for_the_user_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("attachments").join("7");
        let png = Pasted::from_base64(&base64(PNG)).expect("a png");
        let first = keep(&folder, &png).expect("kept");
        let second = keep(&folder, &png).expect("kept");
        assert_eq!(first, folder.join("pasted-1.png"));
        assert_eq!(second, folder.join("pasted-2.png"));
        assert_eq!(std::fs::read(&first).expect("reads"), PNG);
        let mode = |path: &Path| std::fs::metadata(path).expect("there").permissions().mode();
        assert_eq!(mode(&folder) & 0o777, 0o700);
        assert_eq!(mode(&first) & 0o777, 0o600);
        // Numbers go on after the highest, whatever else is in the folder.
        std::fs::write(folder.join("pasted-9.gif"), "").expect("writes");
        std::fs::write(folder.join("notes.txt"), "").expect("writes");
        assert_eq!(
            keep(&folder, &png).expect("kept"),
            folder.join("pasted-10.png")
        );
        assert_eq!(number_of("pasted-x.png"), None);
        assert_eq!(number_of("pasted-3"), None);
    }

    #[test]
    fn only_a_file_right_in_the_folder_is_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("7");
        std::fs::create_dir_all(folder.join("deeper")).expect("makes");
        let image = folder.join("pasted-1.png");
        std::fs::write(&image, PNG).expect("writes");
        std::fs::write(dir.path().join("secret.png"), PNG).expect("writes");
        std::fs::write(folder.join("deeper").join("x.png"), PNG).expect("writes");
        std::os::unix::fs::symlink(dir.path().join("secret.png"), folder.join("link.png"))
            .expect("links");
        assert!(inside(&folder, &image).is_ok());
        for refused in [
            dir.path().join("secret.png"),
            folder.join("..").join("secret.png"),
            folder.join("deeper").join("x.png"),
            folder.join("deeper"),
            folder.join("link.png"),
            folder.join("missing.png"),
        ] {
            assert!(
                matches!(inside(&folder, &refused), Err(Error::NotPastedImage(_))),
                "{}",
                refused.display()
            );
        }
        // A folder not made yet holds nothing.
        assert!(inside(&dir.path().join("8"), &image).is_err());
    }
}
