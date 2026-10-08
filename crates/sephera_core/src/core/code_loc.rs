#![allow(clippy::module_name_repetitions)]

mod analyzer;
#[cfg(test)]
mod differential_tests;
mod reader;
mod scanner;
#[cfg(test)]
mod tests;
mod types;

pub use crate::core::ignore::IgnoreMatcher;
pub use analyzer::CodeLoc;
pub use scanner::scan_content;
pub use types::{CodeLocReport, LanguageLoc, LocMetrics};
