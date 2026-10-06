//! A demo project: one shared module, four callers.
//!
//! Four files reach `parser.rs`. Changing `TokenKind` means all four change.
//! That is the question `sephera graph` answers, and the reason this exists as
//! four tiny files rather than a screenshot.

pub mod parser;
pub mod handler;
pub mod lint;
pub mod highlight;