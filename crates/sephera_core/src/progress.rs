//! Reporting how far along a long phase is, without the analysis depending on a
//! terminal.
//!
//! Every phase in this crate that can take more than a moment -- listing a
//! repository, parsing every file in it -- is a loop over files. A caller that
//! wants to show a bar needs to know how far along that loop is, and the only
//! honest source of that number is the loop itself: it knows the total and what
//! it has finished.
//!
//! So the reporting is a callback, and [`NoProgress`] is the implementation every
//! existing caller keeps using. That matters beyond tidiness: when the callback is
//! `NoProgress` the work is inlined away to nothing, so a non-interactive run
//! pays no cost at all for a progress feature it cannot display.

/// How far a phase has got.
///
/// `Sync` because every phase that reports is a `par_iter`: the callback is
/// reached from every worker thread, so a caller that cannot be shared has
/// nowhere to put its counter. [`NoProgress`] is trivially shareable, which is
/// what keeps the common case free.
///
/// Both methods take `&self` so a caller can pass a shared bar to work running
/// on several threads without wrapping it in a lock at every call site.
pub trait Progress: Sync {
    /// The number of items this phase will process in total.
    ///
    /// Called once, before any [`Progress::advance`]. A phase that only learns
    /// its total after a first pass reports it late, which is why the bar treats
    /// a missing total as "indeterminate" rather than as zero.
    fn set_total(&self, total: u64);

    /// `by` more items finished.
    fn advance(&self, by: u64);
}

/// Reports nothing, and costs nothing to call.
///
/// Not a zero-sized generic parameter or a boxed trait object on purpose: a
/// `&dyn Progress` vtable hop per file is measurable on a repository of a
/// hundred thousand files, and this type is what every caller that does not want
/// a bar is using.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn set_total(&self, _total: u64) {}

    fn advance(&self, _by: u64) {}
}

/// A counter, for the phases that want one without wanting a type.
///
/// Used where the count is already known and only needs to be summed, such as a
/// phase whose work is done by a library that does not take a callback.
#[derive(Debug, Default)]
pub struct CountingProgress {
    seen: std::sync::atomic::AtomicU64,
}

impl CountingProgress {
    #[must_use]
    pub fn seen(&self) -> u64 {
        self.seen.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Progress for CountingProgress {
    fn set_total(&self, _total: u64) {}

    fn advance(&self, by: u64) {
        self.seen
            .fetch_add(by, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        mem::size_of_val,
        sync::{Arc, Mutex},
    };

    /// Records what it was told, so a test can assert on the numbers rather than
    /// on whether a bar was drawn.
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<(u64, u64)>>,
    }

    impl Recorder {
        fn push(&self, call: (u64, u64)) {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(call);
        }

        fn calls(&self) -> Vec<(u64, u64)> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl Progress for Recorder {
        fn set_total(&self, total: u64) {
            self.push((total, 0));
        }

        fn advance(&self, by: u64) {
            self.push((0, by));
        }
    }

    #[test]
    fn a_recorder_sees_the_total_and_every_advance() {
        let recorder = Recorder::default();
        recorder.set_total(3);
        recorder.advance(1);
        recorder.advance(2);

        assert_eq!(recorder.calls(), vec![(3, 0), (0, 1), (0, 2)]);
    }

    #[test]
    fn no_progress_is_a_counter_of_nothing() {
        let no_progress = NoProgress;
        no_progress.set_total(100);
        no_progress.advance(100);

        // Nothing to assert beyond "this does not panic and holds no state". A
        // `NoProgress` that quietly grew a counter would be a tax on every
        // caller that does not want a bar, which is nearly all of them.
        assert_eq!(size_of_val(&no_progress), 0);
    }

    #[test]
    fn the_counting_progress_sums_what_it_is_told() {
        let counting = CountingProgress::default();
        counting.advance(3);
        counting.advance(4);

        assert_eq!(counting.seen(), 7);
    }

    #[test]
    fn the_counting_progress_totals_correctly_across_threads() {
        let counting = Arc::new(CountingProgress::default());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let counting = Arc::clone(&counting);
                scope.spawn(move || {
                    for _ in 0..1000 {
                        counting.advance(1);
                    }
                });
            }
        });

        // Progress is reported from inside a `par_iter`, so a plain `usize`
        // counter here would be a lost update waiting for a wide repository.
        assert_eq!(counting.seen(), 8000);
    }
}
