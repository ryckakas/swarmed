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
    /// `version` is out, and not downloaded.
    Available {
        version: String,
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

/// What updating keeps between calls: managed by the app, one for its whole life.
#[derive(Default)]
pub(crate) struct Updates {
    state: Mutex<UpdateState>,
    found: Mutex<Option<Found>>,
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
        let found = lock(&self.found);
        found.as_ref().is_some_and(|found| found.archive.is_some())
    }

    /// The update found last, downloaded or not.
    fn found(&self) -> Option<Update> {
        lock(&self.found).as_ref().map(|found| found.update.clone())
    }
}

/// Whether this is a build run from the sources, which has no app bundle to replace.
fn from_sources() -> bool {
    tauri::is_dev()
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
/// runs, or a version is ready already, it only says where things stand.
async fn look<R: Runtime>(app: &AppHandle<R>, download: bool) -> UpdateState {
    let updates = app.state::<Updates>();
    let Ok(_busy) = updates.busy.try_lock() else {
        return updates.state();
    };
    if !updates.ready()
        && let Some(update) = check(app, &updates).await
    {
        keep(app, &updates, update, download).await;
    }
    updates.state()
}

/// Asks for the newest release: `None` when there is nothing newer, or no answer, which the
/// state then says.
async fn check<R: Runtime>(app: &AppHandle<R>, updates: &Updates) -> Option<Update> {
    updates.set(app, UpdateState::Checking);
    let checked = match app.updater() {
        Ok(asking) => tokio::time::timeout(ASKING, asking.check()).await,
        Err(error) => Ok(Err(error)),
    };
    match checked {
        Ok(Ok(Some(update))) => Some(update),
        Ok(Ok(None)) => {
            updates.set(app, UpdateState::UpToDate);
            None
        }
        Ok(Err(error)) => {
            let reason = error.to_string();
            updates.set(app, UpdateState::Failed { reason });
            None
        }
        Err(_) => {
            let reason = "GitHub did not answer in time".to_string();
            updates.set(app, UpdateState::Failed { reason });
            None
        }
    }
}

/// Keeps `update`, downloaded and checked first if `download`, and says how it stands.
async fn keep<R: Runtime>(app: &AppHandle<R>, updates: &Updates, update: Update, download: bool) {
    let version = update.version.clone();
    let available = UpdateState::Available {
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

/// Downloads `update` and checks its signature, telling the UI how far it got: a step at each
/// whole percent, or each MiB when the size is not known. A download that fails, or does not
/// match its signature, leaves nothing behind but the reason.
async fn fetch<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
    update: &Update,
) -> Option<Vec<u8>> {
    let version = update.version.clone();
    let mut received: u64 = 0;
    let mut told = None;
    let progress = |chunk: usize, total: Option<u64>| {
        received = received.saturating_add(u64::try_from(chunk).unwrap_or(u64::MAX));
        let step = total.map_or(received >> 20, |total| {
            received.saturating_mul(100) / total.max(1)
        });
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
    drop(busy);
    app.request_restart();
    Ok(())
}

/// Installs the version that is ready, if one is, as Swarmed quits. One that fails to go in
/// leaves the installed version as it was, and the next launch looks again.
pub(crate) fn install_ready<R: Runtime>(app: &AppHandle<R>) {
    let found = lock(&app.state::<Updates>().found).take();
    if let Some(Found {
        update,
        archive: Some(archive),
    }) = found
    {
        update.install(archive).ok();
    }
}
