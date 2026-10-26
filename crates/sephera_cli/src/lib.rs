#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod affected_tests;
mod archaeology;
mod args;
mod behavior;
mod breaking;
mod budget;
mod change_impact;
pub(crate) mod configure;
pub(crate) mod context_config;
mod coupling;
mod dead;
mod diff;
mod gate;
mod guard;
mod impact;
mod merge;
mod output;
mod progress;
mod review;
mod risk;
mod run;
mod sarif;
mod schema_impact;
mod simulate;
mod skill;
mod test_priority;
mod untested;
mod watch;
mod why;

#[doc(hidden)]
pub use output::{
    render_context_json, render_context_markdown, render_report_table,
};
pub use run::{main_exit_code, run};
