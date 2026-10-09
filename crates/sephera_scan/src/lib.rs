#![allow(clippy::module_name_repetitions)]

mod analyzer;
#[cfg(test)]
mod differential_tests;
pub mod project_files;
mod reader;
mod scanner;
#[cfg(test)]
mod tests;
mod types;

pub use analyzer::CodeLoc;
pub use scanner::scan_content;
pub use sephera_ignore::IgnoreMatcher;
pub use types::{CodeLocReport, LanguageLoc, LocMetrics};
