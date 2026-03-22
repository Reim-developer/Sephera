#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod args;
mod output;
mod run;

pub use run::{main_exit_code, run};
