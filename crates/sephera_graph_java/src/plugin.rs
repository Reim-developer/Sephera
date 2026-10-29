//! Java module resolution.
//!
//! Java imports are fully-qualified package paths while a project usually stores
//! sources under a build prefix such as `src/main/java`, so the full path is
//! tried first and then progressively fewer leading segments. A dropped form is
//! a guess, and it is only taken on the word of the file it lands on: that
//! file's `package` statement has to declare the type the import names.
//!
//! Two forms put a name after the file, and both are handled by dropping exactly
//! one trailing segment. A static import says so with a keyword; an on-demand
//! import of a nested type has no keyword, so it is recognised by the naming
//! convention that a type starts with an upper-case letter.

use sephera_core::types::ImportKind;

use sephera_core::{
    path_utils as paths,
    plugins::{
        ImportPlugin, ResolveContext, ResolverPlugin, ends_with_segments,
    },
};
/// Java import extraction and resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct JavaPlugin;

/// Longest suffix tried before giving up, to bound the search on wide imports.
const MAX_SUFFIX_SEGMENTS: usize = 12;

impl ImportPlugin for JavaPlugin {
    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<sephera_core::types::ImportStatement>> {
        crate::extract::extract_from_node(source, node)
    }

    /// The JNI symbols this file declares through `native` methods, so a
    /// C implementation of one can be joined to the class that promises it.
    fn collect_boundary(
        &self,
        source: &[u8],
        tree: &tree_sitter::Tree,
    ) -> Option<sephera_core::boundary::FileBoundary> {
        crate::boundary::collect_boundary(source, tree)
    }
}

impl ResolverPlugin for JavaPlugin {
    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        resolve_import(import_path, context)
    }
}

/// Resolve one Java import path to a file in the analysis.
fn resolve_import(
    import_path: &str,
    context: ResolveContext<'_>,
) -> Option<String> {
    let file_path = paths::replace_separator(import_path, '.');

    if let Some(found) = first_existing(&context, &file_path) {
        return Some(found);
    }

    let parts = paths::segments(&file_path);
    if parts.len() > MAX_SUFFIX_SEGMENTS {
        return None;
    }

    if let Some(shortened) = shortened_candidate(&parts, &context) {
        if let Some(found) = first_existing(&context, &shortened) {
            return Some(found);
        }
    }

    // Dropping leading segments is a guess: it supposes the import carries
    // a prefix the source tree does not. The guess is sometimes right, and
    // unverified it also adopts external classes whose package tail a local
    // class happens to share -- so a dropped candidate is only taken when
    // the file it lands on declares the type the import names. Every file
    // the candidate names is checked, not just the first: two source roots
    // can hold the same tail, and the first one's package may be the wrong
    // one while a later file's is the import's (CodeRabbit on PR #41).
    leading_drops(&parts).into_iter().find_map(|candidate| {
        all_existing(&context, &candidate)
            .into_iter()
            .find(|found| names_the_imported_type(&context, found, import_path))
    })
}

/// Whether `file` declares the type `import_path` names.
///
/// The evidence is the file's own `package` statement plus its name, which
/// together are the fully-qualified name of its public type. The import
/// names that type when it is the name itself, names a nested type or a
/// member inside it (the name is a prefix), or ends with it once a guessed
/// leading prefix comes off. Anything else is a different type that
/// shares a tail, and a blast radius built on it would claim a coupling
/// no compiler agrees with: gson's corpus run bound
/// `com.fasterxml.jackson.core.JsonToken` and
/// `com.google.common.reflect.TypeToken` to gson's own `JsonToken` and
/// `TypeToken` this way, inventing dependencies on Jackson and Guava.
///
/// A file that cannot be read, or declares no package, verifies nothing,
/// and the guess is refused rather than taken on faith.
fn names_the_imported_type(
    context: &ResolveContext<'_>,
    file: &str,
    import_path: &str,
) -> bool {
    // Through the context's text lookup, not a fresh disk read: during a
    // graph build the lookup is the analysis's pinned cache, so a file
    // that changed mid-analysis cannot make resolution disagree with
    // extraction -- and a file the cache already holds is not re-read
    // once per candidate. Without a cache this falls back to the same
    // disk read as before, so the answer is unchanged there.
    let Some(contents) = context.read_project_file(file) else {
        return false;
    };
    let Some(package) = declared_package(&contents) else {
        return false;
    };

    let name = paths::file_name(file);
    let stem = name.strip_suffix(".java").unwrap_or(name);
    let qualified = format!("{package}.{stem}");

    import_path == qualified
        || import_path.starts_with(&format!("{qualified}."))
        || import_path.ends_with(&format!(".{qualified}"))
}

/// The package a Java source file declares, from its `package` statement.
fn declared_package(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("package ")?;
        // The name ends at the semicolon. Trimming semicolons off the
        // end of the line instead left a trailing comment in the name
        // -- `package example.util; // note` read as a package called
        // `example.util; // note` -- and verification rejected the
        // file for it (CodeRabbit on PR #41).
        let (name, _) = rest.split_once(';')?;
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_owned())
    })
}

/// The path without its last segment, when that segment names a member or a
/// nested type rather than the file.
///
/// `None` in every other case, so the shortening only ever runs where the
/// language says a name follows the file.
fn shortened_candidate(
    parts: &[&str],
    context: &ResolveContext<'_>,
) -> Option<String> {
    let last = *parts.last()?;
    if parts.len() < 2 || last == "*" {
        return None;
    }

    let is_member = matches!(context.kind, ImportKind::TypeAlias);
    let looks_like_type = last.starts_with(|c: char| c.is_ascii_uppercase());

    if !is_member && !looks_like_type {
        return None;
    }

    Some(parts[..parts.len() - 1].join("/"))
}

/// The path with progressively fewer leading segments.
///
/// `com.example.Foo` yields `example.Foo` then `Foo`, because a project usually
/// stores sources under a build prefix such as `src/main/java` and the import
/// says nothing about it.
fn leading_drops(parts: &[&str]) -> Vec<String> {
    (1..parts.len())
        .map(|start| parts[start..].join("/"))
        .collect()
}

/// Try `path.java` exactly, then as a directory suffix of a known file.
fn first_existing(context: &ResolveContext<'_>, path: &str) -> Option<String> {
    all_existing(context, path).into_iter().next()
}

/// Every file a path could name, in the order `first_existing` prefers
/// them: the path itself, then every known file whose tail it is.
fn all_existing(context: &ResolveContext<'_>, path: &str) -> Vec<String> {
    let candidate = format!("{path}.java");
    let mut found = Vec::new();
    if context.contains(&candidate) {
        found.push(candidate.clone());
    }
    found.extend(
        context
            .files()
            .filter(|known| ends_with_segments(known, &candidate))
            .cloned(),
    );
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use sephera_core::plugins::FileTextLookup;

    fn resolve(import_path: &str, files: &[&str]) -> Option<String> {
        resolve_with_kind(import_path, files, ImportKind::Dependency)
    }

    /// Resolve with the kind the extractor would have recorded.
    ///
    /// A static import is not the same kind of reference as an ordinary one, and
    /// the extractor is what knows which it is. Passing it explicitly is what
    /// lets a test exercise the static case without parsing Java.
    fn resolve_with_kind(
        import_path: &str,
        files: &[&str],
        kind: ImportKind,
    ) -> Option<String> {
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let mut context =
            sephera_core::plugins::test_context("main.java", &known);
        context.kind = kind;
        JavaPlugin.resolve(import_path, context)
    }

    #[test]
    fn resolves_exact_path() {
        let files = ["com/example/util/Helper.java"];

        assert_eq!(
            resolve("com.example.util.Helper", &files),
            Some("com/example/util/Helper.java".to_owned())
        );
    }

    #[test]
    fn resolves_a_static_import_to_the_declaring_type() {
        // `import static org.junit.Assert.assertEquals;` names a member of
        // `Assert`. The resolver looked for `org/junit/Assert/assertEquals.java`,
        // found nothing, and reported the reference as a third-party package.
        let files = ["org/junit/Assert.java"];

        assert_eq!(
            resolve_with_kind(
                "org.junit.Assert.assertEquals",
                &files,
                ImportKind::TypeAlias,
            ),
            Some("org/junit/Assert.java".to_owned())
        );
    }

    #[test]
    fn resolves_an_on_demand_import_to_the_outer_type() {
        // `import com.example.Foo.Inner;` names a nested type of `Foo`, which is
        // what the four-segment form means in Java. This was reported as a
        // third-party package.
        let files = ["com/example/Foo.java"];

        assert_eq!(
            resolve("com.example.Foo.Inner", &files),
            Some("com/example/Foo.java".to_owned())
        );
    }

    #[test]
    fn a_lower_case_tail_is_never_shortened() {
        // The on-demand case is decided by the capitalisation convention, so a
        // lower-case tail must be left alone rather than guessed at. Getting
        // this wrong would invent an edge into whatever file happens to share the
        // prefix.
        let files = ["com/example/Foo.java"];

        assert_eq!(resolve("com.example.Foo.value", &files), None);
    }

    #[test]
    fn a_top_level_import_is_preferred_over_the_outer_type() {
        // Both candidates can exist. Java resolves the full path first, so the
        // nested file must win.
        let files = ["com/example/Foo.java", "com/example/Foo/Inner.java"];

        assert_eq!(
            resolve("com.example.Foo.Inner", &files),
            Some("com/example/Foo/Inner.java".to_owned())
        );
    }

    #[test]
    fn a_wildcard_import_names_a_package_not_a_file() {
        // Still unresolved, and deliberately so: `import com.example.*;` names
        // every file in the package, and this graph's edges name one file each.
        // Expanding it would need the resolver to return several targets, which
        // is not a change to make inside a fix for static imports.
        //
        // The import is tagged `Namespace` instead, which keeps it out of cycle
        // detection and lets `--exclude-types` drop it. So the gap is named
        // rather than silently counted as a dependency on a package called `*`.
        let files = ["com/example/Foo.java", "com/example/Bar.java"];

        assert_eq!(resolve("com.example.*", &files), None);
    }

    #[test]
    fn resolves_path_under_a_source_root() {
        let files = ["src/main/java/com/example/Helper.java"];

        assert_eq!(
            resolve("com.example.Helper", &files),
            Some("src/main/java/com/example/Helper.java".to_owned())
        );
    }

    #[test]
    fn resolves_after_dropping_package_prefix() {
        let files = ["com/example/util/Helper.java"];

        assert_eq!(
            resolve("com.example.util.Helper", &files),
            Some("com/example/util/Helper.java".to_owned())
        );
    }

    #[test]
    fn external_class_is_not_resolved() {
        let files = ["src/Main.java"];

        assert_eq!(resolve("java.util.List", &files), None);
    }

    #[test]
    fn suffix_match_requires_a_segment_boundary() {
        // `NotHelper` must not satisfy an import of `Helper`.
        let files = ["com/example/util/NotHelper.java"];

        assert_eq!(resolve("com.example.util.Helper", &files), None);
    }

    /// Resolve against files that exist on disk, as a real analysis sees
    /// them.
    ///
    /// A dropped-prefix match is verified against the file it lands on,
    /// so a fixture of bare names -- what `test_context` builds -- can
    /// only ever refuse one. These tests write the files instead, under a
    /// temporary root that plays the part of the project.
    fn resolve_on_disk(
        import_path: &str,
        files: &[(&str, &str)],
    ) -> Option<String> {
        let directory = tempfile::tempdir().expect("a writable directory");
        let mut known = BTreeSet::new();
        for (path, contents) in files {
            let full = directory.path().join(path);
            std::fs::create_dir_all(
                full.parent().expect("a file has a parent"),
            )
            .expect("the tree is written");
            std::fs::write(&full, contents).expect("the file is written");
            known.insert((*path).to_owned());
        }
        let context = ResolveContext {
            source_file: "src/Main.java",
            known_files: &known,
            module_depth: 0,
            kind: ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: directory.path(),
            file_text: None,
        };
        JavaPlugin.resolve(import_path, context)
    }

    #[test]
    fn a_dropped_prefix_does_not_adopt_an_external_class_sharing_a_tail() {
        // The two edges gson's corpus run invented. Both imports name
        // classes from other libraries whose package tail a gson class
        // shares, and dropping leading segments until something matched
        // bound them to gson's own files -- a coupling no compiler agrees
        // with, reported as fact.
        let guava = [(
            "gson/src/main/java/com/google/gson/reflect/TypeToken.java",
            "package com.google.gson.reflect;\n\npublic class TypeToken<T> {}\n",
        )];
        assert_eq!(
            resolve_on_disk("com.google.common.reflect.TypeToken", &guava),
            None,
            "Guava's TypeToken is not gson's, however the tails rhyme"
        );

        let jackson = [(
            "gson/src/main/java/com/google/gson/stream/JsonToken.java",
            "package com.google.gson.stream;\n\npublic enum JsonToken {}\n",
        )];
        assert_eq!(
            resolve_on_disk("com.fasterxml.jackson.core.JsonToken", &jackson),
            None,
            "Jackson's JsonToken is not gson's either"
        );
    }

    #[test]
    fn a_dropped_prefix_resolves_when_the_file_declares_the_imported_type() {
        // The case dropping exists for: the import carries a leading
        // segment the tree does not -- a module prefix in the path that
        // is not part of the package the file declares.
        let files = [(
            "example/util/Helper.java",
            "package example.util;\n\npublic class Helper {}\n",
        )];

        assert_eq!(
            resolve_on_disk("com.example.util.Helper", &files),
            Some("example/util/Helper.java".to_owned())
        );
    }

    #[test]
    fn a_dropped_prefix_checks_every_file_sharing_the_tail() {
        // Two source roots hold the same tail. The first declares a
        // different package; the second declares the import's. Taking
        // only the first match and rejecting it on its package left
        // the import unresolved beside a verifiable match
        // (CodeRabbit on PR #41).
        let files = [
            (
                "a/example/util/Helper.java",
                "package other.util;\n\npublic class Helper {}\n",
            ),
            (
                "b/example/util/Helper.java",
                "package example.util;\n\npublic class Helper {}\n",
            ),
        ];

        assert_eq!(
            resolve_on_disk("vendor.example.util.Helper", &files),
            Some("b/example/util/Helper.java".to_owned())
        );
    }

    #[test]
    fn a_package_statement_with_a_trailing_comment_still_verifies() {
        // `package example.util; // note` is valid Java. Reading the
        // package name as everything up to a trailing semicolon left
        // the comment in the name, and verification rejected the
        // file for a package it does declare (CodeRabbit on PR #41).
        let files = [(
            "example/util/Helper.java",
            "package example.util; // the helpers\n\npublic class Helper {}\n",
        )];

        assert_eq!(
            resolve_on_disk("com.example.util.Helper", &files),
            Some("example/util/Helper.java".to_owned())
        );
    }

    #[test]
    fn a_dropped_prefix_is_refused_when_the_match_cannot_be_verified() {
        // `test_context` puts nothing on disk, so the dropped match here
        // cannot be checked against a `package` statement. An unchecked
        // guess is how the external classes above were adopted, so it is
        // refused rather than taken on faith.
        let files = ["com/example/util/Helper.java"];

        assert_eq!(resolve("vendor.com.example.util.Helper", &files), None);
    }

    /// A text lookup that answers from canned text, standing in for the
    /// analysis's pinned cache: no file on disk backs it.
    struct CannedText {
        path: &'static str,
        text: &'static str,
    }

    impl FileTextLookup for CannedText {
        fn read_text(&self, relative_path: &str) -> Option<String> {
            (relative_path == self.path).then(|| self.text.to_owned())
        }
    }

    #[test]
    fn a_dropped_prefix_verifies_against_the_cached_text_not_the_disk() {
        // The case the cache route exists for: the candidate file is
        // known to the analysis, but nothing on disk backs it in this
        // test -- the old direct disk read refused the guess, while the
        // pinned text verifies it. The cached package is what the
        // import names, so the edge is real even though the disk here
        // is empty.
        let canned = CannedText {
            path: "lib/Widget.java",
            text: "package org.example;\n\npublic class Widget {}\n",
        };
        let known: BTreeSet<String> =
            std::iter::once("lib/Widget.java".to_owned()).collect();
        let directory = tempfile::tempdir().expect("a writable directory");
        let context = ResolveContext {
            source_file: "src/Main.java",
            known_files: &known,
            module_depth: 0,
            kind: ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: directory.path(),
            file_text: Some(&canned),
        };

        assert_eq!(
            JavaPlugin.resolve("org.example.Widget", context),
            Some("lib/Widget.java".to_owned())
        );
    }
}
