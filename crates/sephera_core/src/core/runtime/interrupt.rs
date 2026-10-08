//! One Ctrl+C listener for the process, and what it does when it fires.
//!
//! Tokio installs an OS signal handler the first time a `ctrl_c()` future is
//! polled, and never removes it. Its own documentation is explicit about the
//! consequence: *"Even if this `Signal` instance is dropped, subsequent SIGINT
//! deliveries will end up captured by Tokio, and the default platform behavior
//! will NOT be reset."* So a listener scoped to the clone would not restore the
//! old behaviour afterwards -- it would remove it. Every Ctrl+C pressed during
//! the analysis that follows would be swallowed by a handler with no reader, and
//! a user who pressed it would be ignored.
//!
//! Hence one listener, installed on first use, and a single question when it
//! fires: is a cleanup in flight right now?
//!
//! - Yes, which is the clone unwinding. Request the stop and return. The clone
//!   watches the flag, kills its child, and lets its guard delete the directory.
//!   Exiting from here would race that deletion and leave the directory behind,
//!   which is the thing this change exists to prevent.
//! - No. Stop, which is what the platform's own handler would have done. The
//!   analyses are synchronous and would not notice a flag until they finished,
//!   so `exit` is the honest answer rather than a wait.
//!
//! The question is about a deletion *in progress*, not about whether a directory
//! exists. `ResolvedSource` holds the checkout for the whole run, so "is one on
//! disk" would be true during the analysis too, and answering that way would
//! swallow Ctrl+C exactly when there is most reason to honour it. What a hard
//! exit would skip is the clone's own removal, and that is the only window this
//! guard covers.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::watch;

/// The exit code for a run stopped at the user's request.
///
/// 130 is what a shell reports for a program killed by SIGINT, and a script that
/// already knows how to read it can tell "you stopped this" from "this failed".
pub const INTERRUPTED_EXIT_CODE: u8 = 130;

/// A handle to the process's one Ctrl+C listener.
///
/// Cheap to clone and safe to hold, which is what [`Interrupt::cleanup_guard`]
/// is for.
#[derive(Debug, Clone)]
pub struct Interrupt {
    requested: Arc<AtomicBool>,
    /// True while a deletion is under way that a hard exit would skip. Set
    /// before the clone starts, cleared once its guard has removed the checkout.
    cleanup_in_flight: Arc<AtomicBool>,
    fired: watch::Sender<bool>,
}

impl Interrupt {
    /// Spawn the listener on the current runtime and return the handle.
    fn install() -> Self {
        let interrupt = Self {
            requested: Arc::new(AtomicBool::new(false)),
            cleanup_in_flight: Arc::new(AtomicBool::new(false)),
            fired: watch::channel(false).0,
        };
        tokio::spawn(listen(interrupt.clone()));
        interrupt
    }

    /// Whether the user has asked to stop.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    /// Whether a deletion is under way that a hard exit would skip.
    #[must_use]
    fn cleanup_in_flight(&self) -> bool {
        self.cleanup_in_flight.load(Ordering::Acquire)
    }

    /// Claim the right to clean up, for as long as the returned guard lives.
    ///
    /// Taken before the clone starts and released after its directory is gone,
    /// which is what makes the listener's decision race-free: every instant a
    /// git child is running, this is held.
    #[must_use]
    pub fn cleanup_guard(&self) -> CleanupGuard {
        self.cleanup_in_flight.store(true, Ordering::Release);
        CleanupGuard {
            cleanup_in_flight: Arc::clone(&self.cleanup_in_flight),
        }
    }

    /// Resolve when the user asks to stop, immediately if they already have.
    ///
    /// The check has to be here rather than left to `changed`. `subscribe` marks
    /// the new receiver as having seen the *current* value, so a signal that
    /// arrived before this call leaves `changed` waiting for a second one that
    /// never comes. That is not a hypothetical window: it is the gap between
    /// `cleanup_guard` and the `select!` that waits on this.
    ///
    /// The other side -- a signal arriving between `subscribe` and the poll --
    /// needs no check of its own. Tokio's `changed_impl` requests its
    /// notification before it compares versions, which closes that window
    /// already; a second look here would be redundant, and worse, it would hide
    /// whether the first one is doing any work.
    pub async fn fired(&self) {
        if self.requested() {
            return;
        }
        let mut receiver = self.fired.subscribe();
        // An error means the sender was dropped, which only happens when the
        // process is on its way out. Either way there is nothing left to wait for.
        let _ = receiver.changed().await;
    }
}

/// Holds the right to clean up until it is dropped.
#[derive(Debug)]
pub struct CleanupGuard {
    cleanup_in_flight: Arc<AtomicBool>,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        self.cleanup_in_flight.store(false, Ordering::Release);
    }
}

/// The process-wide handle, installing the listener on first use.
///
/// A global rather than a parameter because the signal really is process-wide,
/// and because threading a handle through `resolve_source` would touch every
/// call site in three crates to carry something that is the same everywhere.
///
/// Nothing installs it until a clone asks for it, so a local analysis pays
/// nothing and keeps the platform's own Ctrl+C behaviour untouched.
pub fn interrupt() -> &'static Interrupt {
    static INTERRUPT: OnceLock<Interrupt> = OnceLock::new();
    INTERRUPT.get_or_init(Interrupt::install)
}

/// What a Ctrl+C means, given whether a deletion is under way.
///
/// Split out from `listen` because `listen` ends in `exit`, and an exit cannot be
/// observed from inside the process that performs it. This is the decision that
/// was wrong once already -- a listener scoped to the clone swallowed every
/// Ctrl+C afterwards -- so it is worth being able to read it, and to test it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Course {
    /// A checkout is being removed by the clone that was just interrupted. Let
    /// that finish; exiting here would abandon the removal mid-way and leave the
    /// directory behind.
    LetTheCloneCleanUp,
    /// Nothing is being removed, so stop the way the platform's own handler would
    /// have. The analyses are synchronous and would not notice a flag until they
    /// finished, so there is nothing to wait for.
    StopNow,
}

#[must_use]
const fn course(cleanup_in_flight: bool) -> Course {
    if cleanup_in_flight {
        Course::LetTheCloneCleanUp
    } else {
        Course::StopNow
    }
}

/// Wait for Ctrl+C, then decide whether this process can afford to stop yet.
async fn listen(interrupt: Interrupt) {
    if tokio::signal::ctrl_c().await.is_err() {
        // No signal handling available, so the platform's default remains in
        // place. That is the behaviour to fall back to, not an error.
        return;
    }

    interrupt.requested.store(true, Ordering::Release);
    // `send_replace` rather than `send`: `send` reports an error when no receiver
    // exists, and dropping that error would make the flag look recorded while the
    // notification went nowhere. `send_replace` always records and always wakes
    // whoever is there, which is what "record that the user asked to stop" means.
    interrupt.fired.send_replace(true);

    match course(interrupt.cleanup_in_flight()) {
        Course::LetTheCloneCleanUp => {}
        Course::StopNow => std::process::exit(i32::from(INTERRUPTED_EXIT_CODE)),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use super::{Course, Interrupt, course};

    /// A handle with no signal attached, so the policy can be tested without one.
    ///
    /// `listen` is what calls `exit`, and there is no way to observe an exit from
    /// inside the process that performs it. What is testable is the decision it
    /// makes beforehand, and that is where the bug would be: a guard that reports
    /// the wrong thing turns "clean up, then stop" into "stop without cleaning up",
    /// which is the whole regression this module exists to prevent.
    fn handle() -> Interrupt {
        Interrupt {
            requested: Arc::new(AtomicBool::new(false)),
            cleanup_in_flight: Arc::new(AtomicBool::new(false)),
            fired: tokio::sync::watch::channel(false).0,
        }
    }

    #[test]
    fn a_guard_is_held_only_while_it_exists() {
        let interrupt = handle();
        assert!(
            !interrupt.cleanup_in_flight(),
            "nothing is in flight before a clone starts"
        );

        let guard = interrupt.cleanup_guard();
        assert!(
            interrupt.cleanup_in_flight(),
            "a running git always has a deletion behind it"
        );

        drop(guard);
        assert!(
            !interrupt.cleanup_in_flight(),
            "a guard that outlived its clone would swallow every later Ctrl+C"
        );
    }

    #[test]
    fn an_interruption_during_a_cleanup_lets_the_cleanup_finish() {
        // The case that leaked a checkout: exiting here takes the process out
        // from under the removal, which on Windows also fails because git still
        // holds handles into the directory.
        assert_eq!(
            course(true),
            Course::LetTheCloneCleanUp,
            "a deletion is under way, so the listener must not exit"
        );
    }

    #[test]
    fn an_interruption_with_nothing_pending_stops_at_once() {
        // The other case, and the one a listener scoped to the clone got wrong:
        // with nobody left to notify, stopping is the only honest answer. Waiting
        // for a flag nothing reads would swallow the Ctrl+C entirely.
        assert_eq!(course(false), Course::StopNow);
    }

    #[test]
    fn no_signal_has_been_requested_before_one_arrives() {
        assert!(
            !handle().requested(),
            "the flag must start clear, or every clone would stop on its own"
        );
    }

    #[tokio::test]
    async fn a_signal_that_arrived_before_the_wait_resolves_at_once() {
        // The window this covers is the one that bites: the listener fires between
        // `cleanup_guard` and the `select!` that waits on it. Without the check
        // ahead of `subscribe`, this waits for a *second* signal that never comes.
        let interrupt = handle();
        let _guard = interrupt.cleanup_guard();
        interrupt.requested.store(true, Ordering::Release);
        interrupt.fired.send_replace(true);

        tokio::time::timeout(Duration::from_secs(5), interrupt.fired())
            .await
            .expect("waiting on a signal that already arrived must not block");
    }

    #[tokio::test]
    async fn a_signal_that_arrives_after_the_wait_is_still_seen() {
        // The other side of the same gap: subscribe, then be signalled, then poll.
        // A notification missed here would leave a clone running that the user
        // asked to stop.
        let interrupt = handle();
        let _guard = interrupt.cleanup_guard();
        let waiter = {
            let interrupt = interrupt.clone();
            tokio::spawn(async move { interrupt.fired().await })
        };
        // Let the task reach `subscribe` before the signal lands, so this covers
        // the notification rather than the check.
        tokio::task::yield_now().await;

        interrupt.requested.store(true, Ordering::Release);
        interrupt.fired.send_replace(true);

        tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("a signal after subscribing must wake the waiter")
            .expect("the waiter must not panic");
    }
}
