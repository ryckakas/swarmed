//! Which Swarmed looks after what an earlier one left behind.
//!
//! Two can run on one data directory: the installed app beside one built for development, or
//! the app beside a command. To the second, a session that counts as busy and a stack it did
//! not start are the first one's work, not leftovers. So only the first to arrive settles
//! sessions when it starts and ends stacks it finds when it quits. It is known by a lock on a
//! file, which the system lets go of when the process ends, however it ends.

use std::fs::{File, TryLockError};

use crate::error::Result;
use crate::{Core, lock};

impl Core {
    /// Says a front end that stays, such as the app, has started: sessions left busy by a
    /// Swarmed that ended in the middle of a turn are put back to idle, unless another Swarmed
    /// is running on the same data, whose sessions they may be. Says how many there were.
    /// Worktrees that removals moved aside and did not see deleted are deleted.
    pub fn arrive(&self) -> Result<usize> {
        if self.keeps_house()? {
            self.finish_removals();
            self.sessions_settle()
        } else {
            Ok(0)
        }
    }

    /// Ends the stacks Swarmed leaves behind when it quits: every one if this was the first
    /// Swarmed to [`arrive`](Core::arrive), and otherwise only the ones it started itself.
    pub async fn leave(&self) {
        let first = lock(&self.house).is_some();
        if first {
            self.stacks_stop_all().await;
        } else {
            self.stacks_stop_started().await;
        }
    }

    /// Whether this is the Swarmed that arrived first, taking that place if nobody has it.
    fn keeps_house(&self) -> Result<bool> {
        let mut house = lock(&self.house);
        if house.is_some() {
            return Ok(true);
        }
        let folder = self.paths.data_dir.join("run");
        std::fs::create_dir_all(&folder)?;
        let file = File::create(folder.join("first.lock"))?;
        match file.try_lock() {
            Ok(()) => {
                *house = Some(file);
                Ok(true)
            }
            Err(TryLockError::WouldBlock) => Ok(false),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}
