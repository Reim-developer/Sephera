//! The run epoch, which is how a count is cancelled.
//!
//! A count is one call into Rust that cannot be interrupted: `Progress` has
//! `set_total` and `advance` and no way to say stop, and adding one to
//! `sephera_core` means touching a published crate's trait for a GUI that is the
//! only caller that wants it. So the epoch does not stop the work. It marks its
//! result stale.
//!
//! The counter is a *cancellation generation*, not a request counter. A handler
//! snapshots it when it starts and compares that snapshot when it finishes: if
//! they differ, a `cancel_current` happened in between and the reply is emptied
//! rather than written into the window. The UI closes the dialog at once, and the
//! seconds the work spent are seconds nobody watched.
//!
//! That is the honest trade, and it is the right one at this scale: a count is
//! milliseconds, so the wasted work is bounded and small, while the alternative --
//! a cancellation token threaded through four crates -- is unbounded and permanent.
//!
//! What was wrong before, and why this shape: the handler used to compare the
//! *front end's* stamp against *this* counter. Those are two counters that never
//! meet -- the client numbers its first run 1 and this one starts at 0 -- so
//! `is_current(1)` was false on the very first request, every reply came back
//! emptied, and nothing ever loaded. Comparing two counters that are supposed to
//! agree is the bug; a snapshot and a re-read is the version that holds.

use std::sync::atomic::{AtomicU64, Ordering};

/// The cancellation generation every in-flight request is measured against.
#[derive(Debug, Default)]
pub struct Epoch {
    value: AtomicU64,
}

impl Epoch {
    /// The generation a request starts in.
    ///
    /// Taken at the start rather than derived from anything the caller sends,
    /// because the caller's numbering is the caller's business. This one only
    /// ever answers "has anything cancelled since".
    pub fn begin(&self) -> u64 {
        self.value.load(Ordering::SeqCst)
    }

    /// Whether a request that began at `began` is still the one being waited for.
    ///
    /// The only read side. A reply carrying `0` was already stale when it
    /// arrived, which is how the front end tells a discarded run from a live one
    /// without a second field.
    pub fn is_current(&self, began: u64) -> bool {
        self.value.load(Ordering::SeqCst) == began
    }

    /// Invalidate whatever is running.
    ///
    /// Bumps the generation, so every request that snapshotted the previous one
    /// -- which is every request in flight -- finds itself stale when it next
    /// checks.
    pub fn cancel(&self) {
        self.value.fetch_add(1, Ordering::SeqCst);
    }
}

/// The epoch state, registered on the app.
#[derive(Debug, Default)]
pub struct Progress(pub Epoch);

/// # Errors
///
/// Returns an error when the state cannot be reached, which is a programming
/// error rather than a runtime one.
// The owned `State` wrapper is deliberate. A plain `&Progress` reads more
// naturally and cannot be registered through `manage()` by the handler macro, so
// this keeps the wrapper and takes the lint rather than losing the seam.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn cancel_current(state: tauri::State<'_, Progress>) {
    state.0.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arithmetic a command performs, written out.
    ///
    /// The handler cannot be driven from a unit test -- it needs a window, a
    /// webview and a Tauri state handle -- so what is pinned here is the sequence
    /// it runs: snapshot, work, echo the front end's stamp only if the snapshot
    /// still holds. The old code skipped the snapshot and compared the front
    /// end's stamp against the counter directly, which is the line this test
    /// exists to stop someone writing again.
    #[test]
    fn the_sequence_a_command_follows_echoes_the_first_request() {
        let epoch = Epoch::default();
        // The front end's first stamp. Irrelevant to the host, and that is the
        // whole point.
        let client_stamp = 1u64;

        let began = epoch.begin();
        let echoed = if epoch.is_current(began) { client_stamp } else { 0 };

        assert_eq!(echoed, client_stamp);
    }

    #[test]
    fn a_cancel_empties_the_reply_a_command_would_have_sent() {
        let epoch = Epoch::default();
        let client_stamp = 1u64;

        let began = epoch.begin();
        epoch.cancel();
        let echoed = if epoch.is_current(began) { client_stamp } else { 0 };

        assert_eq!(echoed, 0);
    }

    #[test]
    fn a_request_is_current_when_nothing_has_cancelled() {
        let epoch = Epoch::default();
        let began = epoch.begin();
        assert!(epoch.is_current(began));
    }

    /// The bug that shipped, pinned.
    ///
    /// The handler compared the front end's stamp against this counter. The front
    /// end numbers its first run 1, this counter starts at 0, and `is_current(1)`
    /// therefore returned false on the very first request -- so the reply was
    /// emptied, the store dropped it, and `busy` never cleared. Nothing loaded.
    ///
    /// The snapshot severs the two counters: the handler no longer cares what the
    /// front end called its run, only whether a cancel happened since it began.
    #[test]
    fn a_snapshot_does_not_depend_on_the_callers_numbering() {
        let epoch = Epoch::default();
        let began = epoch.begin();
        // The front end's first stamp is 1. It is *not* the counter's value, and
        // it does not have to be.
        assert_ne!(began, 1);
        assert!(epoch.is_current(began));
    }

    #[test]
    fn a_cancel_makes_every_request_in_flight_stale() {
        let epoch = Epoch::default();
        let first = epoch.begin();
        let second = epoch.begin();

        epoch.cancel();

        assert!(!epoch.is_current(first));
        assert!(!epoch.is_current(second));
    }

    #[test]
    fn a_request_started_after_a_cancel_is_still_current() {
        let epoch = Epoch::default();
        epoch.cancel();

        let began = epoch.begin();
        assert!(epoch.is_current(began));
    }

    #[test]
    fn two_cancels_move_the_generation_twice() {
        let epoch = Epoch::default();
        let began = epoch.begin();
        epoch.cancel();
        epoch.cancel();
        // A later request is current even though an earlier one is not: the
        // generation moved twice and both snapshots are on the old side.
        assert!(!epoch.is_current(began));
        let after = epoch.begin();
        assert!(epoch.is_current(after));
    }
}
