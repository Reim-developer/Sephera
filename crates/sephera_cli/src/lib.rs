#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod args;
mod budget;
pub(crate) mod context_config;
mod output;
mod run;

#[doc(hidden)]
pub use output::{
    render_context_json, render_context_markdown, render_report_table,
};
pub use run::{main_exit_code, run};
