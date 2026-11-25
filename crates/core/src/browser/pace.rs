//! Pictures at two qualities. A page that stands still is sent at [`STILL`], which keeps small
//! text sharp; one that moves, at [`MOVING`], which keeps up with it. Chrome sends one picture
//! when pictures start, so starting them again at [`STILL`] once the page has not moved for
//! [`SETTLE`] is what sends the sharp one, and it sets off nothing more. A screenshot instead
//! would: Chrome draws the page again for it, which reads as the page moving.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::Browser;
use crate::lock;

/// The JPEG quality of a page that stands still, from 0 to 100.
pub(super) const STILL: u8 = 90;

/// The JPEG quality of a page that moves.
pub(super) const MOVING: u8 = 60;

/// How long a page must not have moved to be sent at [`STILL`] again.
const SETTLE: Duration = Duration::from_millis(200);

/// How many pictures within [`WINDOW`] make a page one that moves: one or two are a click.
const MOVING_PICTURES: usize = 3;

/// See [`MOVING_PICTURES`].
const WINDOW: Duration = Duration::from_millis(300);

/// When the latest pictures of the tab shown came.
#[derive(Debug, Default)]
pub(super) struct Motion {
    heard: VecDeque<Instant>,
}

impl Motion {
    /// Notes a picture heard at `now`, and says whether the page moves.
    pub(super) fn heard(&mut self, now: Instant) -> bool {
        self.heard.push_back(now);
        while self.heard.len() > MOVING_PICTURES {
            self.heard.pop_front();
        }
        let first = self.heard.front().copied().unwrap_or(now);
        self.heard.len() == MOVING_PICTURES && now.duration_since(first) <= WINDOW
    }

    /// The page stands still: what came before counts no more, so that the one picture sent
    /// for it does not make it one that moves with the last of those.
    pub(super) fn settled(&mut self) {
        self.heard.clear();
    }
}

/// Asks for `tab` at [`MOVING`]: pictures of it come close together.
pub(super) async fn moving(browser: Arc<Browser>, tab: String) {
    browser.recast(&tab, MOVING).await;
}

/// After [`SETTLE`] with no picture of `tab` after picture `seen`, asks for it at [`STILL`].
pub(super) async fn settled(browser: Arc<Browser>, tab: String, seen: u64) {
    tokio::time::sleep(SETTLE).await;
    if browser.still_due(&tab, seen) {
        lock(&browser.motion).settled();
        browser.recast(&tab, STILL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_moves_when_pictures_come_close_together_and_not_for_a_click() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut motion = Motion::default();
        assert!(!motion.heard(at(0)));
        assert!(!motion.heard(at(40)), "a click draws once or twice");
        assert!(motion.heard(at(80)));
        assert!(motion.heard(at(120)));
        // Pictures far apart are a page that changes now and then, not one that moves.
        let mut now_and_then = Motion::default();
        for ms in [0, 200, 400, 600] {
            assert!(!now_and_then.heard(at(ms)));
        }
        // Settled, the picture sent for it is not taken with the last that moved.
        motion.settled();
        assert!(!motion.heard(at(330)));
    }
}
