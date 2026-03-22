#![allow(clippy::module_name_repetitions)]

mod analyzer;
mod ignore;
mod reader;
mod scanner;
#[cfg(test)]
mod tests;
mod types;

pub use analyzer::CodeLoc;
pub use ignore::IgnoreMatcher;
pub use scanner::scan_content;
pub use types::{CodeLocReport, LanguageLoc, LocMetrics};
