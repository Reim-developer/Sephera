//! Java module resolution.
//!
//! Java imports are fully-qualified package paths while a project usually stores
//! sources under a build prefix such as `src/main/java`, so the full path is
//! tried first and then progressively fewer leading segments.
//!
//! Two forms put a name after the file, and both are handled by dropping exactly
//! one trailing segment. A static import says so with a keyword; an on-demand
//! import of a nested type has no keyword, so it is recognised by the naming
//! convention that a type starts with an upper-case letter.

mod extract;

use sephera_compression::{SupportedLanguage};
use sephera_graph::{ImportKind};

use super::{
    ExtractedSource, ImportPlugin, ResolveContext, ResolverPlugin,
    ends_with_segments, paths, walk::walk_with_declarations,
};
/// Java import extraction and resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct JavaPlugin;

/// Longest suffix tried before giving up, to bound the search on wide imports.
const MAX_SUFFIX_SEGMENTS: usize = 12;

impl ImportPlugin for JavaPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Java
    }

    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<sephera_graph::types::ImportStatement>> {
        extract::extract_from_node(source, node)
    }

    fn extract_source(&self, source: &[u8]) -> Option<ExtractedSource> {
        walk_with_declarations(source, ImportPlugin::language(self), self).ok()
    }
}

impl ResolverPlugin for JavaPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Java
    }

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

    let mut candidates = Vec::new();
    if let Some(shortened) = shortened_candidate(&parts, &context) {
        candidates.push(shortened);
    }
    candidates.extend(leading_drops(&parts));

    for candidate in &candidates {
        if let Some(found) = first_existing(&context, candidate) {
            return Some(found);
        }
    }

    None
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
    let candidate = format!("{path}.java");
    if context.contains(&candidate) {
        return Some(candidate);
    }

    context
        .files()
        .find(|known| ends_with_segments(known, &candidate))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

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
        let mut context = super::super::test_context("main.java", &known);
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
}
