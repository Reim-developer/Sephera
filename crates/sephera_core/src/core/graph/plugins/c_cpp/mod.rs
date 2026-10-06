//! C and C++ include resolution.
//!
//! One plugin serves both languages; only the grammar differs, and resolution is
//! identical because `#include` means the same thing in each.

mod extract;

use std::collections::BTreeSet;

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin, paths,
    walk::walk_imports,
};

/// C or C++ import extraction and resolution.
#[derive(Debug, Clone, Copy)]
pub struct CCppPlugin {
    /// Selects the grammar used during extraction.
    pub language: SupportedLanguage,
}

impl ImportPlugin for CCppPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<crate::core::graph::types::ImportStatement>> {
        extract::extract_from_node(source, node)
    }

    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>> {
        walk_imports(source, ImportPlugin::language(self), self)
            .ok()
            .map(super::to_extracted)
    }
}

impl ResolverPlugin for CCppPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        // `<stdio.h>` is a system header, not a project file.
        if import_path.starts_with('<') {
            return None;
        }

        // A quoted include may walk up with `../`, so the join is resolved rather
        // than concatenated; plain concatenation would leave `src/../util.h`,
        // which never matches a normalised path.
        let parent = paths::parent(context.source_file);
        let relative = paths::resolve_relative(&parent, import_path);
        if context.contains(&relative) {
            return Some(relative);
        }

        // A header may be on the include path rather than beside the importer.
        if context.contains(import_path) {
            return Some(import_path.to_owned());
        }

        include_path_match(import_path, &context)
    }
}

/// The single project file whose path ends with this include.
///
/// Compilers are given `-Iinclude` and the source says `#include "shared.h"`,
/// with nothing in the source saying where `shared.h` lives. Without this every
/// such header is reported as a third-party package named `shared` and, because
/// the path does not look project-local, is not even counted as a resolver gap.
/// It simply disappears from the graph, which is the worst failure available:
/// plausible, silent, and total.
///
/// The match is on path segments rather than characters, so `shared.h` cannot
/// match `preshared.h`. And it must be unique. Two files ending in `util.h`
/// leave the answer genuinely undetermined from the source alone, and a graph
/// that picks one of them is worse than a graph that admits it does not know.
fn include_path_match(
    import_path: &str,
    context: &ResolveContext<'_>,
) -> Option<String> {
    let wanted = import_path.strip_prefix("./").unwrap_or(import_path);

    // A path that walks up or is absolute has already had its turn above, and its
    // tail is not a reliable thing to search for.
    if wanted.starts_with("..") || wanted.starts_with('/') {
        return None;
    }

    let suffix = format!("/{wanted}");
    let known: &BTreeSet<String> = context.known_files;
    let mut found: Option<&str> = None;

    for candidate in known {
        if !candidate.ends_with(&suffix) {
            continue;
        }
        // A second match makes this ambiguous, and an ambiguity is not a match.
        if found.is_some() {
            return None;
        }
        found = Some(candidate.as_str());
    }

    found.map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(
        import_path: &str,
        source_file: &str,
        files: &[&str],
    ) -> Option<String> {
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = super::super::test_context(source_file, &known);
        CCppPlugin {
            language: SupportedLanguage::C,
        }
        .resolve(import_path, context)
    }

    #[test]
    fn resolves_header_beside_the_importer() {
        let files = ["src/main.c", "src/util.h"];

        assert_eq!(
            resolve("util.h", "src/main.c", &files),
            Some("src/util.h".to_owned())
        );
    }

    #[test]
    fn resolves_a_header_found_on_the_include_path() {
        // The layout almost every C project uses: headers in `include/`, sources
        // in `src/`, and `-Iinclude` supplied by the build system. Nothing in the
        // source says where the header lives.
        //
        // This reported the header as a third-party package named `shared`, and
        // because the path does not look project-local it was not counted as a
        // resolver gap either. It simply left the graph.
        let files = ["src/main.c", "include/shared.h", "src/local.h"];

        assert_eq!(
            resolve("shared.h", "src/main.c", &files),
            Some("include/shared.h".to_owned())
        );
    }

    #[test]
    fn resolves_a_nested_include_path() {
        let files = ["src/main.c", "third_party/lua/lua.h"];

        assert_eq!(
            resolve("lua/lua.h", "src/main.c", &files),
            Some("third_party/lua/lua.h".to_owned())
        );
    }

    #[test]
    fn refuses_to_guess_when_two_headers_share_a_tail() {
        // The answer is genuinely undetermined from the source alone. Picking
        // either one would put a real but wrong edge into a blast radius, which
        // is worse than admitting the file was not found.
        let files = ["src/main.c", "a/util.h", "b/util.h"];

        assert_eq!(resolve("util.h", "src/main.c", &files), None);
    }

    #[test]
    fn a_tail_match_respects_path_segments() {
        // `shared.h` must not match `preshared.h`, which a character-suffix test
        // would happily accept.
        let files = ["src/main.c", "include/preshared.h"];

        assert_eq!(resolve("shared.h", "src/main.c", &files), None);
    }

    #[test]
    fn a_system_header_is_never_matched_locally() {
        let files = ["src/main.c", "include/stdio.h"];

        assert_eq!(resolve("<stdio.h>", "src/main.c", &files), None);
    }

    #[test]
    fn resolves_header_from_an_include_path() {
        // This used to assert `None`, on the reasoning that no include path is
        // configured so a header under `include/` is unreachable. That reasoning
        // described a tool that could not see the most common C layout at all:
        // `include/util.h` was reported as a third-party package named `util`,
        // and since the path does not look project-local it was not counted as a
        // resolver gap either, so it left the graph without appearing anywhere.
        //
        // Requiring configuration is the right instinct — guessing between two
        // candidates invents an edge — but the guess here only happens when
        // exactly one file in the project ends with that path, which no
        // configuration would have to disambiguate either.
        let files = ["src/main.c", "include/util.h"];

        assert_eq!(
            resolve("util.h", "src/main.c", &files),
            Some("include/util.h".to_owned())
        );
    }

    #[test]
    fn resolves_header_when_quoted_path_is_relative_to_the_importer() {
        let files = ["src/main.c", "include/util.h"];

        assert_eq!(
            resolve("../include/util.h", "src/main.c", &files),
            Some("include/util.h".to_owned())
        );
    }

    #[test]
    fn missing_header_is_not_resolved() {
        let files = ["src/main.c"];

        assert_eq!(resolve("nothing.h", "src/main.c", &files), None);
    }

    #[test]
    fn system_include_is_not_resolved() {
        let files = ["src/main.c"];

        assert_eq!(resolve("<stdio.h>", "src/main.c", &files), None);
    }

    #[test]
    fn resolves_nested_relative_header() {
        let files = ["src/deep/main.c", "src/util.h"];

        assert_eq!(
            resolve("../util.h", "src/deep/main.c", &files),
            Some("src/util.h".to_owned())
        );
    }
}
