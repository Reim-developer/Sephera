#![allow(clippy::module_name_repetitions)]

mod analyzer;
mod reader;
mod scanner;
#[cfg(test)]
mod tests;
mod types;

pub use crate::core::ignore::IgnoreMatcher;
pub use analyzer::CodeLoc;
pub use scanner::scan_content;
pub use types::{CodeLocReport, LanguageLoc, LocMetrics};
