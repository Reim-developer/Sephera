use std::{
    io::{IsTerminal, stderr},
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use indicatif::{ProgressBar, ProgressStyle};

use crate::args::ProgressMode;

const TICK_STRINGS: [&str; 6] =
    ["[   ]", "[=  ]", "[== ]", "[===]", "[ ==]", "[  =]"];

/// Set once at startup from `--progress`.
///
/// A process-wide setting rather than a field on every command struct: it is
/// decided before any command runs and cannot change afterwards, so threading it
/// through `run_loc`, `run_graph`, `run_impact` and the rest would be eight
/// parameters that are all the same value.
static MODE: AtomicU8 = AtomicU8::new(MODE_AUTO);

const MODE_AUTO: u8 = 0;
const MODE_ALWAYS: u8 = 1;
const MODE_NEVER: u8 = 2;

/// Apply `--progress` for the rest of the process.
pub fn set_mode(mode: ProgressMode) {
    MODE.store(
        match mode {
            ProgressMode::Auto => MODE_AUTO,
            ProgressMode::Always => MODE_ALWAYS,
            ProgressMode::Never => MODE_NEVER,
        },
        Ordering::Relaxed,
    );
}

/// What a task is doing, and how far along it is.
///
/// Two things this deliberately is not.
///
/// It does not draw when stderr is not a terminal. Every machine-readable
/// format -- `--format json`, `--format markdown`, anything piped to a file --
/// goes to stdout, so a bar on stderr would not corrupt it, but it would leave
/// the user of a script with a line of escape codes interleaved into their
/// captured output and no obvious way to turn it off. The bar is for a person
/// watching, and a person watching has a terminal.
///
/// It does not claim a percentage it does not have. Phases that learn their
/// total part-way through stay indeterminate until they know it, and say so
/// rather than showing a bar that fills up towards a number nobody has counted
/// yet.
pub struct CliProgress {
    progress_bar: Option<ProgressBar>,
}

impl CliProgress {
    /// A bar with no total yet: a spinner with a message.
    ///
    /// The right choice for a phase whose size is unknown before it runs, such
    /// as fetching a repository that has not been cloned yet, and for the walk
    /// that has to happen before a file count exists to divide by.
    #[must_use]
    pub fn start(message: impl Into<String>) -> Self {
        let Some(progress_bar) = Self::spinner() else {
            return Self::disabled();
        };
        progress_bar.set_style(spinner_style());
        progress_bar.set_message(message.into());
        Self {
            progress_bar: Some(progress_bar),
        }
    }

    /// Adopt a total learned part-way through, after showing an indeterminate
    /// bar until now.
    ///
    /// Switching an `indicatif` bar from a spinner to a fixed length is what
    /// turns `Scanning files...` into `Reading 1,234/8,901 (14%)`, and it is why
    /// a total can be reported late without the bar having lied in the meantime.
    ///
    /// The position lives in the `indicatif` bar, not here. Keeping a second copy
    /// would be two counters that agree until one of them is updated twice.
    pub fn set_total(&self, total: u64) {
        if let Some(progress_bar) = &self.progress_bar {
            progress_bar.set_style(bar_style());
            progress_bar.set_length(total);
        }
    }

    pub fn advance(&self, by: u64) {
        if let Some(progress_bar) = &self.progress_bar {
            progress_bar.inc(by);
        }
    }

    pub fn set_message(&self, message: impl Into<String>) {
        if let Some(progress_bar) = &self.progress_bar {
            progress_bar.set_message(message.into());
        }
    }

    pub fn finish(mut self) {
        if let Some(progress_bar) = self.progress_bar.take() {
            progress_bar.finish_and_clear();
        }
    }

    fn spinner() -> Option<ProgressBar> {
        let wanted = MODE.load(Ordering::Relaxed);
        if wanted == MODE_NEVER {
            return None;
        }
        // `always` exists so the bar can be recorded into a log or a screen
        // capture that has no terminal, and so the rendering path is reachable
        // from a test. Both are reasons to want output where `is_terminal` is
        // false, which is why this is a mode rather than a hard-coded check.
        if wanted == MODE_AUTO && !stderr().is_terminal() {
            return None;
        }

        let progress_bar = ProgressBar::new_spinner();
        if wanted == MODE_ALWAYS {
            // `ProgressBar::new_spinner` targets `indicatif`'s stderr draw
            // target, and that target is *hidden* when stderr is not a terminal
            // -- which is precisely the case `always` is asking for. A
            // `console::Term` writes regardless of what it is attached to, so
            // pointing the bar at one is what makes the mode mean anything.
            progress_bar.set_draw_target(
                indicatif::ProgressDrawTarget::term_like(Box::new(
                    console::Term::buffered_stderr(),
                )),
            );
        }
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        Some(progress_bar)
    }

    const fn disabled() -> Self {
        Self { progress_bar: None }
    }
}

impl Drop for CliProgress {
    fn drop(&mut self) {
        if let Some(progress_bar) = self.progress_bar.take() {
            progress_bar.finish_and_clear();
        }
    }
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner} {msg}")
        .expect("spinner template must be valid")
        .tick_strings(&TICK_STRINGS)
}

/// `{wide_bar}` and `{percent}` both come from `indicatif`, which sizes the bar to
/// the terminal it is drawing into. Without `wide_bar` a determinate bar would
/// print the same twelve characters on a phone and on a wide display.
fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg} {wide_bar} {pos}/{len} ({percent})")
        .expect("progress bar template must be valid")
        .progress_chars("#>-")
}

/// Hands the bar to core as the trait core reports through.
///
/// The dependency runs CLI to core, never the other way round, so this is the
/// only place the two meet on progress. A newtype rather than implementing
/// `Progress` for `CliProgress` directly, because `indicatif` is a
/// presentation detail of this crate and the trait is not.
pub struct CliReporter<'a>(pub &'a CliProgress);

impl sephera_core::progress::Progress for CliReporter<'_> {
    fn set_total(&self, total: u64) {
        self.0.set_total(total);
    }

    fn advance(&self, by: u64) {
        self.0.advance(by);
    }
}
