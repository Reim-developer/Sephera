//! Types and traits shared by every other crate.
//!
//! Nothing here does analysis. What lives in this crate is the vocabulary the
//! rest of the workspace argues in: the graph's own types, the language table,
//! the declaration index, the two lookup traits a plugin asks questions through,
//! and the walk every language shares.
//!
//! # Why the vocabulary is here and not in the crate that uses it
//!
//! [`plugins::ImportPlugin`] is implemented by six language crates, and
//! [`plugins::ResolverPlugin`] by the same six. Neither can depend on
//! `sephera_graph`, because `sephera_graph` holds the registry that names all six
//! -- so a plugin naming an index, a path helper or a report type would close
//! that circle. This crate is the one both sides can name, which is what makes it
//! the only place those items can live.
//!
//! That is the whole design rule: **if the six language crates need it, it is
//! here; if only the graph resolver needs it, it is in `sephera_graph`.**
//!
//! # What is deliberately absent
//!
//! A language-neutral extraction trait is *not* here. The earlier shape of this
//! file declared `ExtractionRules`, taking a parsed tree and a file path and
//! returning statements; nothing implemented it, nothing called it, and the
//! per-node walk it was shaped around turned out to need to ask the plugin three
//! separate questions per node. The trait that replaced it --
//! [`plugins::ImportPlugin`] -- takes one node at a time instead, and a plugin
//! that has no opinion returns `None`. `AGENTS.md` calls this shape a dead-code
//! trap, and this was one: 40 lines of authoritative-looking trait that no `mod`
//! declaration and no caller ever reached.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod config;
pub mod declarations;
pub mod language_data;
pub mod line_slices;
pub mod path_utils;
pub mod paths;
pub mod plugins;
pub mod progress;
pub mod types;
