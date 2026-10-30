//! Harness for the regression suite.
//!
//! Cargo compiles each file directly under `tests/` as its own
//! integration-test target and ignores bare subdirectories, so this
//! file is the single target that pulls the `regressions/` modules
//! in. Add a new area with one `#[path]` line.

#[path = "regressions/sarif.rs"]
mod sarif;
#[path = "regressions/tworef_paths.rs"]
mod tworef_paths;
