//! Telling a crate from a name.
//!
//! A path that does not resolve is either a reference to code outside the
//! project or a resolver gap, and the difference is the whole value of
//! `local_gap`. Nothing about most paths' *shape* settles it -- `serde::Serialize`
//! and `example.com/acme/store` look alike -- so the questions here are the few
//! that genuinely can be answered: is this a qualifier, is this word a crate,
//! does the file itself declare this name.
//!
//! They are separated because a guard written here was tried three times and got
//! each time broader than the evidence, removing four real self-references along
//! with the false ones. Each of those attempts is written down at the decision it
//! belongs to.

use crate::paths::{crate_root, module_path};
use sephera_core::{path_utils as shared_paths, plugins::ResolveContext};

/// Whether a word is a Rust path qualifier rather than a name.
///
/// `super`, `self` and `crate` are the three the resolver strips before walking,
/// so a path ending in one of them names a module and never a file.
///
/// This matters more than it looks. `use super::*;` records `super` where a
/// name would go, and it is a reference to the file it appears in -- 41 of axum's
/// 65 self-references are exactly that, which `AGENTS.md` lists under the
/// self-references the counting rules call real. Asking whether any file is named
/// `super.rs` rejects every one of them, and the guard that did so was reverted
/// twice before the reason was written here.
#[must_use]
pub fn is_qualifier(name: &str) -> bool {
    matches!(name, "super" | "self" | "crate")
}

/// Whether `name` is a bare identifier naming a crate from outside this project.
///
/// A bare `use` has no qualifier to read, so the evidence is that the name
/// matches no file anywhere in the analysis. `use serde::Serialize;` and
/// `use fastrand;` rest on exactly this -- and `serde` is qualified while
/// `fastrand` is not.
///
/// The declaration index cannot answer it. A private `use` is deliberately not
/// recorded as a re-export, because recording it once made `use crate::service;`
/// in `main.rs` resolve to `main.rs`, so there is nothing here to look up and
/// the module walk is the evidence there is.
#[must_use]
pub fn names_a_crate_outside(
    context: ResolveContext<'_>,
    name: &str,
    // Whether the path naming `name` carried `super::`, `self::` or `crate::`.
    //
    // It decides what a re-export in the importing file means, and the two
    // answers are opposites. `pub use sephera_core::core::graph::blast_radius::
    // BlastRadius;` in `crates/sephera_cli/src/impact.rs` followed by `use
    // super::{BlastRadius, render_markdown, ..}` in that file's test module is a
    // real self-reference: the file brings the name into its own scope on
    // purpose. `pub use typed_json;` in
    // `axum-extra/src/response/erased_json.rs` followed by a bare `use
    // typed_json;` is a crate from outside, and the re-export says nothing about
    // where the name comes from.
    //
    // So a re-export is not the signal either way, and it is not this function's
    // to read on its own: the qualifier has been stripped by the time the leaf is
    // left, and it is the only thing that separates the two.
    qualified: bool,
) -> bool {
    // A qualifier is a word, not a crate. Checked before anything else because it
    // is the case that has no evidence either way.
    if name.is_empty() || name.contains(':') || is_qualifier(name) {
        return false;
    }

    // A module of this crate has a file behind it, and the name to compare is
    // the *last segment* of that file's module path -- not the file name. `use
    // store;` in `src/leaf.rs` is a reference to `src/store.rs` or
    // `src/store/mod.rs`, and comparing against the file name alone matches
    // neither, which made every bare `use` in the project look like a crate
    // from outside. That test caught it.
    if context
        .files()
        .any(|known| shared_paths::file_name(&module_path(known)) == name)
    {
        return false;
    }

    // A name this file declares is an item of this file, and a name it re-exports
    // is in its own scope. Both are what `use super::Cli` inside a test module
    // is, and both are the line between `use fastrand;` and a bare `use` of a
    // struct written above it.
    //
    // The re-export half is gated on the qualifier, and that gate is the whole
    // of the distinction. Reading it unconditionally put `typed_json` back as a
    // self-edge -- `pub use typed_json;` says the name is reachable, not that it
    // is local -- and took `super::BlastRadius` in Sephera's own
    // `crates/sephera_cli/src/impact.rs` out, which is a real self-reference the
    // file exists to make.
    if context.declarations.is_some_and(|index| {
        index.file_declares(context.source_file, name)
            || (qualified && index.file_reaches(context.source_file, name))
    }) {
        return false;
    }

    // Without a crate root there is no `src` to anchor a bare name against, and
    // guessing would turn every unresolved single-segment path into an external
    // dependency -- which is what happened when this rule was tried without the
    // qualifier check and took 41 real self-references with it.
    !crate_root(context.source_file).is_empty()
}

/// The file that is a crate's root module, when the analysis found one.
///
/// A crate root is a directory in a path sense -- `crate::core` resolves relative
/// to `src` -- but a declaration lookup needs the file that declares the names:
/// `lib.rs` or `main.rs` inside it. Going through the same candidate walk as any
/// other module path means a crate with only `main.rs`, or one that spells its
/// root as `src/mod.rs`, is found without a second rule to keep in step with the
/// first.
#[must_use]
pub fn crate_root_file(
    context: ResolveContext<'_>,
    source_file: &str,
) -> Option<String> {
    let root = crate_root(source_file);
    if root.is_empty() {
        return None;
    }

    // The root is a directory, and a module path walk on it would only try
    // `src.rs`, so the two file names a crate root can have are named here.
    // `src/mod.rs` is included because it is a valid spelling of a root module
    // and costs one comparison.
    for name in ["lib", "main", "mod"] {
        let candidate = format!("{root}/{name}.rs");
        if context.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context over a small crate, so the tests do not depend on which files a
    /// real project happens to have.
    fn context_over<'a>(
        known: &'a std::collections::BTreeSet<String>,
        source_file: &'a str,
    ) -> ResolveContext<'a> {
        ResolveContext {
            source_file,
            known_files: known,
            module_depth: 0,
            kind: sephera_core::types::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: std::path::Path::new(""),
        }
    }

    /// The file set a one-leaf crate under `src` contains.
    fn a_crate_under_src(extra: &str) -> std::collections::BTreeSet<String> {
        ["src/leaf.rs".to_owned(), extra.to_owned()]
            .into_iter()
            .collect()
    }

    /// A context whose importing file re-exports one name from another crate.
    ///
    /// The re-export is the whole of the distinction this function draws, and it
    /// cannot be reached through `from_names`, which records declarations only.
    struct ReexportingFile {
        known: std::collections::BTreeSet<String>,
        index: sephera_core::declarations::DeclarationIndex,
    }

    impl ReexportingFile {
        fn new(reexported: &str) -> Self {
            let mut index =
                sephera_core::declarations::DeclarationIndex::default();
            index.insert(
                "src/leaf.rs",
                sephera_core::declarations::DeclaredNames::from_names_and_reexports(
                    [],
                    [reexported],
                ),
            );
            Self {
                known: a_crate_under_src(""),
                index,
            }
        }

        fn context(&self) -> ResolveContext<'_> {
            ResolveContext {
                source_file: "src/leaf.rs",
                known_files: &self.known,
                module_depth: 0,
                kind: sephera_core::types::ImportKind::Dependency,
                declarations: Some(&self.index),
                manifests: None,
                base_path: std::path::Path::new(""),
            }
        }
    }

    #[test]
    fn a_qualifier_is_never_a_crate_name() {
        assert!(is_qualifier("super"));
        assert!(is_qualifier("self"));
        assert!(is_qualifier("crate"));
        assert!(!is_qualifier("fastrand"));
        assert!(!is_qualifier("MultipartForm"));
    }

    #[test]
    fn a_bare_name_with_no_file_is_a_crate_outside() {
        // `use fastrand;` in axum names a crate from outside, and answering it
        // with the file it was written in is a self-dependency no compiler has.
        let known = a_crate_under_src("");

        assert!(names_a_crate_outside(
            context_over(&known, "src/leaf.rs"),
            "fastrand",
            false
        ));
    }

    #[test]
    fn a_qualified_name_is_never_a_bare_crate() {
        let known = a_crate_under_src("");

        assert!(!names_a_crate_outside(
            context_over(&known, "src/leaf.rs"),
            "serde::Serialize",
            true
        ));
    }

    #[test]
    fn a_file_named_after_the_word_makes_it_local() {
        // `use store;` where `src/store.rs` exists is a module of this crate.
        let known = a_crate_under_src("src/store.rs");

        assert!(!names_a_crate_outside(
            context_over(&known, "src/leaf.rs"),
            "store",
            false
        ));
    }

    #[test]
    fn a_reexport_does_not_make_a_bare_name_local() {
        // `pub use typed_json;` in `axum-extra/src/response/erased_json.rs`
        // says the name is reachable, not that it is local. Reading it as local
        // put that self-edge back.
        let file = ReexportingFile::new("typed_json");

        assert!(names_a_crate_outside(file.context(), "typed_json", false));
    }

    #[test]
    fn a_reexport_makes_a_qualified_name_local() {
        // `pub use sephera_core::core::graph::blast_radius::BlastRadius;` in
        // `crates/sephera_cli/src/impact.rs` followed by `use
        // super::{BlastRadius, render_markdown, ..}` in that file's test module
        // is a real self-reference, and the file exists to make it.
        let file = ReexportingFile::new("BlastRadius");

        assert!(!names_a_crate_outside(file.context(), "BlastRadius", true));
    }

    #[test]
    fn a_generic_struct_is_still_a_declaration() {
        // `pub struct JsonLines<S, T = AsExtractor>` in
        // `axum-extra/src/json_lines.rs` is declared by that file, and a
        // `use super::JsonLines` in its test module is a real self-reference.
        // The declaration walk has to see it through the generics.
        let mut parser = sephera_compression::new_parser(
            sephera_compression::SupportedLanguage::Rust,
        )
        .expect("rust parser");
        let source = "pub struct JsonLines<S, T = AsExtractor> {}\n";
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let names = sephera_core::declarations::collect_declared_names(
            source.as_bytes(),
            &tree,
        );

        assert!(names.declares("JsonLines"));
    }

    #[test]
    fn without_a_crate_root_a_bare_name_is_not_guessed_at() {
        // `crate_root` is empty for a file under `tests/`, and treating every
        // unresolved word there as an external crate would be a guess.
        let known: std::collections::BTreeSet<String> =
            std::iter::once("tests/leaf.rs".to_owned()).collect();

        assert!(!names_a_crate_outside(
            context_over(&known, "tests/leaf.rs"),
            "anything",
            false
        ));
    }
}
