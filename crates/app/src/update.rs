//! Updating Swarmed from its GitHub releases, with Tauri's updater.
//!
//! Each release publishes `latest.json`, which names its version and an archive of the app,
//! signed with a key that never leaves the maintainer: the public half is in `tauri.conf.json`.
//! An archive is installed only if its signature matches, version included, so neither a
//! tampered release nor an older one passed off as newer gets in.
//!
//! With automatic updates on, a release build looks at launch and every few hours, downloads
//! what it finds, and installs it as Swarmed quits. With them off, nothing is fetched until the
//! About view looks. Either way, the About view can install a new version and restart at once.
//!
//! Nothing is installed while Swarmed runs on: agents start this very executable to reach
//! Swarmed's tools, and a newer one would answer them for an app that is not running. So the
//! archive waits, checked, in memory, and goes in on the way out.
//!
//! The archive that waits is the newest: each look asks again, and a newer release takes the
//! place of the one held. With automatic updates switched off, one downloaded before goes in
//! only if the user asks for it.
//!
//! A version that could not be put in is remembered with the reason (`run/update-refused` in
//! the data directory), so the next launch says why and does not download it again by itself.
//!
//! This is the one part of the app crate that decides something: the updater is the window's,
//! and the core links no HTTP client. What it decides is in [`step`] and [`progress_step`],
//! which are pure and tested here: add to those rather than to the code around them.
//!
//! The plugin's own commands are in no capability: the window reaches updating through
//! `update_state`, `update_check` and `update_restart` alone.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use swarmed_core::{Core, UpdateMode};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The event every change of [`UpdateState`] is emitted as.
pub(crate) const UPDATE_STATE: &str = "update-state";

/// How long automatic updates wait before looking again.
const EVERY: Duration = Duration::from_hours(6);

/// How long a look may take, and a download: a request that hangs must not leave the About
/// view looking for ever, nor hold up the next look.
const ASKING: Duration = Duration::from_secs(30);
const DOWNLOADING: Duration = Duration::from_mins(10);

/// Where updating stands, as the About view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum UpdateState {
    /// Nobody has looked yet.
    #[default]
    Unknown,
    Checking,
    /// The installed version is the newest.
    UpToDate,
    /// `version` is out, and not downloaded. `failed` is why it did not go in when Swarmed
    /// last quit, if it was tried then.
    Available {
        version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        failed: Option<String>,
    },
    /// `version` is coming in: `received` bytes so far, of `total` if the server said.
    Downloading {
        version: String,
        received: u64,
        total: Option<u64>,
    },
    /// `version` is downloaded and its signature checked: it goes in when Swarmed quits.
    Ready {
        version: String,
    },
    /// The last look or download failed, for `reason`.
    Failed {
        reason: String,
    },
}

/// The update found last and, once downloaded and checked, its archive.
struct Found {
    update: Update,
    archive: Option<Vec<u8>>,
}

/// A version that could not be put in when Swarmed quit, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Refused {
    version: String,
    reason: String,
}

impl Refused {
    /// What `file` holds: the version on its first line, the reason after it.
    fn read(file: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(file).ok()?;
        let (version, reason) = text.split_once('\n')?;
        Some(Self {
            version: version.to_string(),
            reason: reason.trim().to_string(),
        })
    }

    fn write(&self, file: &Path) -> std::io::Result<()> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(file, format!("{}\n{}\n", self.version, self.reason))
    }
}

/// Where a version that could not be put in is remembered until the next launch.
fn refusal_file<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    let core = app.state::<Arc<Core>>();
    core.paths().data_dir.join("run").join("update-refused")
}

/// What updating keeps between calls: managed by the app, one for its whole life.
#[derive(Default)]
pub(crate) struct Updates {
    state: Mutex<UpdateState>,
    found: Mutex<Option<Found>>,
    /// The version that did not go in when Swarmed last quit, read once at launch.
    refused: Mutex<Option<Refused>>,
    /// Whether the user asked for the version held to be installed: by then nothing looks.
    asked: AtomicBool,
    /// Held while a look or a download runs, so that two never do.
    busy: tokio::sync::Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // What the mutex holds is replaced whole, so a panic cannot leave half of it written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Updates {
    pub(crate) fn state(&self) -> UpdateState {
        lock(&self.state).clone()
    }

    fn set<R: Runtime>(&self, app: &AppHandle<R>, state: UpdateState) {
        lock(&self.state).clone_from(&state);
        // No window to hear it is no reason to stop: the state is kept for the next ask.
        app.emit(UPDATE_STATE, state).ok();
    }

    fn ready(&self) -> bool {
        self.held().is_some()
    }

    /// The version downloaded and checked, waiting to go in.
    fn held(&self) -> Option<String> {
        let found = lock(&self.found);
        let held = found.as_ref().filter(|found| found.archive.is_some());
        held.map(|found| found.update.version.clone())
    }

    /// Why `version` did not go in when Swarmed last quit, if it was tried then.
    fn refusal(&self, version: &str) -> Option<String> {
        let refused = lock(&self.refused);
        let same = refused
            .as_ref()
            .filter(|refused| refused.version == version);
        same.map(|refused| refused.reason.clone())
    }

    fn asked(&self) -> bool {
        self.asked.load(Ordering::Relaxed)
    }

    /// The update found last, downloaded or not.
    fn found(&self) -> Option<Update> {
        lock(&self.found).as_ref().map(|found| found.update.clone())
    }
}

/// Whether this is a build from the sources: anything `scripts/build-release.sh` did not
/// build, so `pnpm app` and a bundle from `pnpm app:build` alike. A release must not take the
/// place of what somebody built themselves.
fn from_sources() -> bool {
    option_env!("SWARMED_RELEASE").is_none()
}

/// Whether the user has automatic updates on.
fn automatic<R: Runtime>(app: &AppHandle<R>) -> bool {
    let core = app.state::<Arc<Core>>();
    core.settings()
        .is_ok_and(|settings| settings.updates == UpdateMode::Automatic)
}

/// Starts looking every few hours, when automatic updates are on, for as long as the app
/// runs. A build run from the sources never looks by itself.
pub(crate) fn watch<R: Runtime>(app: AppHandle<R>) {
    *lock(&app.state::<Updates>().refused) = Refused::read(&refusal_file(&app));
    if from_sources() {
        return;
    }
    drop(tauri::async_runtime::spawn(async move {
        loop {
            look_by_itself(&app).await;
            tokio::time::sleep(EVERY).await;
        }
    }));
}

/// One of the looks [`watch`] makes: none at all unless automatic updates are on.
async fn look_by_itself<R: Runtime>(app: &AppHandle<R>) {
    if automatic(app) {
        look(app, true).await;
    }
}

/// Looks for a new version now, as the About view asks: downloading it too when automatic
/// updates are on, so that it is ready to go in when Swarmed quits.
pub(crate) async fn look_now<R: Runtime>(app: &AppHandle<R>) -> UpdateState {
    let download = automatic(app) && !from_sources();
    look(app, download).await
}

/// Looks for a new version and, if `download`, fetches it. While another look or download
/// runs, or the user has asked for the version held, it only says where things stand. With a
/// version held it looks without a word: what is held still goes in if GitHub does not answer.
async fn look<R: Runtime>(app: &AppHandle<R>, download: bool) -> UpdateState {
    let updates = app.state::<Updates>();
    let Ok(_busy) = updates.busy.try_lock() else {
        return updates.state();
    };
    if updates.asked() {
        return updates.state();
    }
    let held = updates.held();
    match check(app, &updates, held.is_none()).await {
        Ok(newest) => settle(app, &updates, newest, held.as_deref(), download).await,
        Err(reason) if held.is_none() => updates.set(app, UpdateState::Failed { reason }),
        Err(_) => {}
    }
    updates.state()
}

/// Asks for the newest release: `None` when there is nothing newer, `Err` with the reason when
/// there was no answer. The UI is told it is being asked only if `announce`.
async fn check<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
    announce: bool,
) -> Result<Option<Update>, String> {
    if announce {
        updates.set(app, UpdateState::Checking);
    }
    let asking = app.updater().map_err(|error| error.to_string())?;
    match tokio::time::timeout(ASKING, asking.check()).await {
        Ok(checked) => checked.map_err(|error| error.to_string()),
        Err(_) => Err("GitHub did not answer in time".to_string()),
    }
}

/// What a look does with what it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The newest version is the one held: nothing changes.
    Hold,
    /// Download the newest version, in place of whatever is held.
    Fetch,
    /// Say the newest version is out, and hold nothing.
    Offer,
    /// Nothing newer is out any more: hold nothing.
    Forget,
}

/// Decides a look's [`Step`]: `held` is the version waiting to go in, `newest` the one the
/// release names if it is newer than this one, `refused` whether that one failed to go in when
/// Swarmed last quit, `download` whether new versions are downloaded unasked.
fn step(held: Option<&str>, newest: Option<&str>, refused: bool, download: bool) -> Step {
    match newest {
        None => Step::Forget,
        // Automatic updates are off: what was downloaded while they were on does not go in.
        Some(_) if !download => Step::Offer,
        Some(newest) if held == Some(newest) => Step::Hold,
        // It would fail again the same way; the user can still ask for it.
        Some(_) if refused => Step::Offer,
        Some(_) => Step::Fetch,
    }
}

/// Acts on what a look found, by its [`step`].
async fn settle<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
    newest: Option<Update>,
    held: Option<&str>,
    download: bool,
) {
    let version = newest.as_ref().map(|update| update.version.as_str());
    let refused = version.is_some_and(|version| updates.refusal(version).is_some());
    match (step(held, version, refused, download), newest) {
        (Step::Hold, _) => {}
        (Step::Fetch, Some(update)) => keep(app, updates, update, true).await,
        (Step::Offer, Some(update)) => keep(app, updates, update, false).await,
        (_, _) => {
            *lock(&updates.found) = None;
            updates.set(app, UpdateState::UpToDate);
        }
    }
}

/// Keeps `update`, downloaded and checked first if `download`, and says how it stands.
async fn keep<R: Runtime>(app: &AppHandle<R>, updates: &Updates, update: Update, download: bool) {
    let version = update.version.clone();
    let available = UpdateState::Available {
        failed: updates.refusal(&version),
        version: version.clone(),
    };
    updates.set(app, available);
    let archive = if download {
        fetch(app, updates, &update).await
    } else {
        None
    };
    let ready = archive.is_some();
    *lock(&updates.found) = Some(Found { update, archive });
    if ready {
        updates.set(app, UpdateState::Ready { version });
    }
}

/// The step of a download the UI is told of: each whole percent, or each MiB when the size is
/// not known.
fn progress_step(received: u64, total: Option<u64>) -> u64 {
    total.map_or(received >> 20, |total| {
        received.saturating_mul(100) / total.max(1)
    })
}

/// Downloads `update` and checks its signature, telling the UI at once that it has begun and
/// then how far it got, a [`progress_step`] at a time. A download that fails, or does not
/// match its signature, leaves nothing behind but the reason.
async fn fetch<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
    update: &Update,
) -> Option<Vec<u8>> {
    let version = update.version.clone();
    let mut received: u64 = 0;
    let mut told = None;
    // Said before the first piece arrives: the button that asked for it goes at once.
    let begun = UpdateState::Downloading {
        version: version.clone(),
        received,
        total: None,
    };
    updates.set(app, begun);
    let progress = |chunk: usize, total: Option<u64>| {
        received = received.saturating_add(u64::try_from(chunk).unwrap_or(u64::MAX));
        let step = progress_step(received, total);
        if told != Some(step) {
            told = Some(step);
            let version = version.clone();
            let state = UpdateState::Downloading {
                version,
                received,
                total,
            };
            updates.set(app, state);
        }
    };
    let downloaded = tokio::time::timeout(DOWNLOADING, update.download(progress, || ())).await;
    let reason = match downloaded {
        Ok(Ok(archive)) => return Some(archive),
        Ok(Err(error)) => error.to_string(),
        Err(_) => "the download took longer than ten minutes".to_string(),
    };
    updates.set(app, UpdateState::Failed { reason });
    None
}

/// Downloads the version found, unless it is ready already, then restarts Swarmed: it goes
/// in on the way out, by [`install_ready`]. The `Err` says, for the user, why not.
pub(crate) async fn restart<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if from_sources() {
        return Err("A build run from the sources is not updated: install a release.".into());
    }
    let updates = app.state::<Updates>();
    // A look or download under way finishes first; it may make this one unnecessary.
    let busy = updates.busy.lock().await;
    if !updates.ready() {
        let update = updates
            .found()
            .ok_or("There is no new version to install: check for one first.")?;
        let version = update.version.clone();
        let archive = fetch(app, &updates, &update)
            .await
            .ok_or("The new version could not be downloaded.")?;
        *lock(&updates.found) = Some(Found {
            update,
            archive: Some(archive),
        });
        updates.set(app, UpdateState::Ready { version });
    }
    // From here on no look may put the version held away: it goes in as Swarmed quits.
    updates.asked.store(true, Ordering::Relaxed);
    drop(busy);
    app.request_restart();
    Ok(())
}

/// Installs the version that is ready, if one is, as Swarmed quits: because the user asked, or
/// because updating automatically is still on. One that fails to go in leaves the installed
/// version as it was, and the reason where the next launch finds it.
pub(crate) fn install_ready<R: Runtime>(app: &AppHandle<R>) {
    let updates = app.state::<Updates>();
    let found = lock(&updates.found).take();
    let Some(Found {
        update,
        archive: Some(archive),
    }) = found
    else {
        return;
    };
    if !(updates.asked() || automatic(app)) {
        return;
    }
    let file = refusal_file(app);
    // Whatever was remembered is of an earlier try; with no file, nothing was refused.
    std::fs::remove_file(&file).ok();
    if let Err(error) = update.install(archive) {
        let refused = Refused {
            version: update.version.clone(),
            reason: error.to_string(),
        };
        // Nowhere to say it: the next launch then looks and downloads as if nothing happened.
        refused.write(&file).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{Refused, Step, progress_step, step};

    #[test]
    fn a_look_downloads_what_is_newer_than_the_version_held() {
        assert_eq!(step(None, Some("0.3.0"), false, true), Step::Fetch);
        assert_eq!(step(Some("0.3.0"), Some("0.3.0"), false, true), Step::Hold);
        assert_eq!(step(Some("0.3.0"), Some("0.3.1"), false, true), Step::Fetch);
    }

    #[test]
    fn a_release_that_was_withdrawn_is_not_held_on_to() {
        assert_eq!(step(Some("0.3.0"), None, false, true), Step::Forget);
        assert_eq!(step(None, None, false, false), Step::Forget);
    }

    #[test]
    fn nothing_is_downloaded_or_held_with_automatic_updates_off() {
        assert_eq!(step(None, Some("0.3.0"), false, false), Step::Offer);
        assert_eq!(
            step(Some("0.3.0"), Some("0.3.0"), false, false),
            Step::Offer
        );
    }

    #[test]
    fn a_version_that_did_not_go_in_is_offered_and_not_downloaded_again() {
        assert_eq!(step(None, Some("0.3.0"), true, true), Step::Offer);
    }

    #[test]
    fn a_download_is_told_by_the_percent_or_by_the_mebibyte() {
        assert_eq!(progress_step(0, Some(200)), 0);
        assert_eq!(progress_step(50, Some(200)), 25);
        assert_eq!(progress_step(200, Some(200)), 100);
        // A size of nothing must not divide by it.
        assert_eq!(progress_step(10, Some(0)), 1000);
        assert_eq!(progress_step((3 << 20) + 5, None), 3);
        assert_eq!(progress_step(u64::MAX, Some(1)), u64::MAX);
    }

    #[test]
    fn a_version_that_was_refused_is_read_back_with_its_reason() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let file = dir.path().join("run").join("update-refused");
        assert_eq!(Refused::read(&file), None);
        let refused = Refused {
            version: "0.3.0".to_string(),
            reason: "the app is on a read-only disk".to_string(),
        };
        refused.write(&file).expect("written");
        assert_eq!(Refused::read(&file), Some(refused));
    }
}
