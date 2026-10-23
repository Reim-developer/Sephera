//! A child module file whose parent is the crate root itself.
//!
//! `use super::*;` at the top of this file names the root module, whose
//! file is `lib.rs` -- beside this file, not above it. The climb from
//! here lands on the `src` directory, and the root file sits *inside*
//! that directory under a name the ordinary candidate list (`src.rs`,
//! `src/mod.rs`) never tries. `core/child_tests.rs` cannot cover this:
//! its parent is a `mod.rs`, which that list already knows. axum's
//! compression example is this shape in the wild -- `mod tests;` in
//! `src/main.rs`, the glob at the top of `src/tests.rs`.

use super::*;

pub fn checks() {}
