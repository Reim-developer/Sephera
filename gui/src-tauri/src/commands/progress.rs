//! The run epoch, which is how a count is cancelled.
//!
//! A count is one call into Rust that cannot be interrupted: `Progress` has
//! `set_total` and `advance` and no way to say stop, and adding one to
//! `sephera_core` means touching a published crate's trait for a GUI that is the
//! only caller that wants it. So the epoch does not stop the work. It marks its
//! result stale.
//!
//! A request takes the current epoch, the frontend stores it, and `cancel_current`
//! bumps the counter. When the work finishes and the reply arrives, the handler
//! compares the epoch it captured against the live one; if they differ, the work
//! ran for a request nobody is waiting for and its output is discarded rather than
//! written into the window. The UI closes the dialog at once, and the seconds the
//! work spent are seconds nobody watched.
//!
//! That is the honest trade, and it is the right one at this scale: a count is
//! milliseconds, so the wasted work is bounded and small, while the alternative --
//! a cancellation token threaded through four crates -- is unbounded and permanent.

use std::sync::atomic::{AtomicU64, Ordering};

/// The counter every run is measured against.
#[derive(Debug, Default)]
pub struct Epoch {
    value: AtomicU64,
}

impl Epoch {
    /// Whether a request started at `started` is still the one being waited for.
    ///
    /// The only read side. A reply carrying `0` was already stale when it
    /// arrived, which is how the front end tells a discarded run from a live one
    /// without a second field.
    pub fn is_current(&self, started: u64) -> bool {
        started != 0 && self.value.load(Ordering::SeqCst) == started
    }

    /// Invalidate whatever is running.
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
