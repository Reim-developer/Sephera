use std::{
    io::{IsTerminal, stderr},
    time::Duration,
};

use indicatif::{ProgressBar, ProgressStyle};

const TICK_STRINGS: [&str; 6] =
    ["[   ]", "[=  ]", "[== ]", "[===]", "[ ==]", "[  =]"];

pub struct CliProgress {
    progress_bar: Option<ProgressBar>,
}

impl CliProgress {
    #[must_use]
    pub fn start(message: impl Into<String>) -> Self {
        if !stderr().is_terminal() {
            return Self { progress_bar: None };
        }

        let progress_bar = ProgressBar::new_spinner();
        progress_bar.set_style(progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        progress_bar.set_message(message.into());

        Self {
            progress_bar: Some(progress_bar),
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
}

impl Drop for CliProgress {
    fn drop(&mut self) {
        if let Some(progress_bar) = self.progress_bar.take() {
            progress_bar.finish_and_clear();
        }
    }
}

fn progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner} {msg}")
        .expect("spinner template must be valid")
        .tick_strings(&TICK_STRINGS)
}
