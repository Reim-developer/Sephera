//! The traits every language plugin implements, and the context it resolves
//! against.
//!
//! These live in `sephera_core` rather than beside the graph resolver because of
//! the dependency direction the split imposed. A language crate needs the trait
//! to implement a plugin; the graph needs the registry of every plugin to resolve
//! an import. Put the trait in `sephera_graph` and the language crate depends on
//! it, which puts the registry in a crate the language crate depends on -- a
//! cycle. Here, the language crates depend on `sephera_core` and on nothing that
//! knows they exist, and `sephera_graph` is free to depend on all of them.
//!
//! Two lookups reach through this boundary, and both are traits rather than the
//! concrete indexes: `DeclarationLookup` is answered by
//! `sephera_graph::declarations::DeclarationIndex`, and `ModuleManifestLookup` by
//! `sephera_graph::manifests::ManifestIndex`. A plugin asks a question -- "does
//! this file declare that name" -- and does not need to know that the answer is
//! indexed, cached, and rebuilt on every analysis.

use std::collections::BTreeSet;

use crate::types::ImportKind;

/// Every file in an analysis, as normalised relative paths.
pub type KnownFiles = BTreeSet<String>;

/// What a resolver can ask about what each file declares.
///
/// Deliberately two questions rather than the whole index. `file_declares` is
/// "the name is in this file", and `file_reaches` is "the name is in this file
/// or in something it re-exports" -- the difference is a `pub use` line, and it
/// is what separates a path that resolves to a file from one that resolves to a
/// name a file happens to contain.
pub trait DeclarationLookup {
    /// Whether `file` declares `name` itself.
    fn file_declares(&self, file: &str, name: &str) -> bool;

    /// Whether `file` declares `name` or re-exports something that does.
    fn file_reaches(&self, file: &str, name: &str) -> bool;
}

/// What a resolver can ask about the project's manifests.
///
/// Two questions, both about Go's module path. Every other ecosystem is read
/// from the import path's shape -- `from flask import x` names a package, and a
/// file called `flask` is a file called `flask` -- so Go is the only one whose
/// resolution needs the manifest, and the only one that needs it twice.
pub trait ModuleManifestLookup {
    /// The `replace` directives from `go.mod`, in declaration order.
    fn go_replaces(&self) -> &std::collections::BTreeMap<String, String>;

    /// Whether `import_path` is the module path declared by this project's
    /// `go.mod`, rather than a package inside it.
    fn is_go_module_root(&self, import_path: &str) -> bool;
}

/// Everything a resolver needs to look at besides the import itself.
///
/// Hand-written `Debug` rather than derived: the two lookups are trait objects,
/// and a derived impl would require every implementor to be `Debug` to describe a
/// context that only needs to say *whether* a lookup is attached.
#[derive(Clone, Copy)]
pub struct ResolveContext<'a> {
    /// Normalised, `/`-separated path of the file containing the import.
    pub source_file: &'a str,

    /// Every file in the analysis, as normalised relative paths.
    pub known_files: &'a KnownFiles,

    /// How many inline `mod name { ... }` blocks the reference sits inside.
    ///
    /// Only the Rust resolver reads this; the qualifier forms in other languages
    /// have no inline-module equivalent.
    pub module_depth: u8,

    /// What this reference says about the file it names.
    ///
    /// A declaration and an import resolve differently when the only remaining
    /// candidate is the source file itself: `use super::Cli` inside a test
    /// module legitimately names an item in that file, while `mod missing;`
    /// that resolves to the declaring file would be an invented edge.
    pub kind: ImportKind,

    /// What each file declares and re-exports.
    ///
    /// A path whose last segment is a name rather than a module can only be
    /// resolved by looking up what a file declares. Absent, every such path is
    /// reported as external, which is how `use crate::Router;` came to be
    /// counted as a missing dependency rather than a reference to a re-export.
    pub declarations: Option<&'a dyn DeclarationLookup>,

    /// What the project's manifests say about it.
    pub manifests: Option<&'a dyn ModuleManifestLookup>,

    /// Directory the analysis was rooted at.
    ///
    /// Resolvers work in project-relative paths, so reading a file one of them
    /// names -- a `package.json` for its `main` field -- needs the root to turn
    /// it into something on disk. Without it the only guess is the process
    /// working directory, which is a different directory whenever `--path` was
    /// given.
    pub base_path: &'a std::path::Path,
}

impl std::fmt::Debug for ResolveContext<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolveContext")
            .field("source_file", &self.source_file)
            .field("known_files", &self.known_files.len())
            .field("module_depth", &self.module_depth)
            .field("kind", &self.kind)
            .field("declarations", &self.declarations.is_some())
            .field("manifests", &self.manifests.is_some())
            .field("base_path", &self.base_path)
            .finish()
    }
}

impl ResolveContext<'_> {
    /// Whether a candidate path names a real file in the analysis.
    #[must_use]
    pub fn contains(&self, candidate: &str) -> bool {
        self.known_files.contains(candidate)
    }

    /// First candidate in `candidates` that names a real file.
    #[must_use]
    pub fn first_existing<'c>(
        &self,
        candidates: impl IntoIterator<Item = &'c str>,
    ) -> Option<String> {
        candidates
            .into_iter()
            .find(|candidate| self.contains(candidate))
            .map(ToOwned::to_owned)
    }

    /// Iterates the known files in a stable order.
    pub fn files(&self) -> impl Iterator<Item = &String> {
        self.known_files.iter()
    }
}

/// A context for a resolver test, with no lookup tables attached.
///
/// The two lookups are what a production resolution reads, and a test that
/// populated them would be testing the fixture as much as the resolver. Tests
/// that do exercise a lookup build the index and pass it explicitly.
#[cfg(test)]
pub(crate) fn test_context<'a>(
    source_file: &'a str,
    known_files: &'a KnownFiles,
) -> ResolveContext<'a> {
    ResolveContext {
        source_file,
        known_files,
        module_depth: 0,
        kind: ImportKind::Dependency,
        declarations: None,
        manifests: None,
        base_path: std::path::Path::new(""),
    }
}

/// Extracts import statements from source text for one language.
///
/// `Sync` because extraction runs across a thread pool: parsing is the
/// dominant cost of a graph run and the work is independent per file. The
/// built-in plugins are stateless, so this costs nothing and buys the
/// parallelism. A plugin holding mutable state would need interior locking.
pub trait ImportPlugin: Sync {
    /// Read whatever this node says about imports, if it says anything.
    ///
    /// One method per AST node rather than one per language statement, because
    /// the walk visits every node and the plugin decides which are the
    /// interesting ones. Required rather than defaulted: a plugin with no
    /// opinion on any node would silently contribute an empty graph, which is
    /// the hardest kind of failure to notice.
    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<crate::types::ImportStatement>>;

    /// How much deeper references inside this node's children sit.
    ///
    /// Rust's `mod tests { ... }` declares a module, so `super::` inside it
    /// climbs one level further. No other supported language has the construct,
    /// so the answer is zero and costs nothing.
    fn child_depth_step(&self, _node: &tree_sitter::Node<'_>) -> u8 {
        0
    }

    /// Whether this reference only exists when a feature flag is on.
    ///
    /// Rust's `#[cfg(feature = "json")]` is the case. The reference is real, but
    /// a blast radius that counts it without saying so claims a dependency the
    /// default build does not have.
    fn is_cfg_gated(
        &self,
        _source: &[u8],
        _node: &tree_sitter::Node<'_>,
    ) -> bool {
        false
    }

    /// Names this source declares, when the language needs a lookup by name.
    ///
    /// Returning `None` is the normal answer and costs nothing: most languages
    /// resolve an import to a file without knowing what the file declares. Only
    /// a plugin whose imports can name something other than a module has to
    /// answer, and only Rust does.
    fn collect_declarations(
        &self,
        _source: &[u8],
        _tree: &tree_sitter::Tree,
    ) -> Option<crate::declarations::DeclaredNames> {
        None
    }
}

/// Maps an import path onto a file inside the analysis.
///
/// Implementations return `None` for imports that leave the project, which is
/// the normal case for external crates and standard library modules. Returning
/// `None` means "external", not "failed", so a language with no local imports
/// still produces a usable graph.
pub trait ResolverPlugin {
    /// Whether `name` names a module this analysis can see.
    ///
    /// The counterpart to [`Self::leaves_project`], and asked of the same thing:
    /// a path the resolver could not place. A language whose absolute paths are
    /// spelled the same whether they leave the project or not has no evidence in
    /// the shape, and this is where the evidence is. Python's
    /// `collections.OrderedDict` and a broken `pkg.absent` differ only in whether
    /// a file of that name is here.
    fn names_a_known_module(
        &self,
        _name: &str,
        _context: ResolveContext<'_>,
    ) -> bool {
        false
    }

    /// Whether a path that names `name` provably leaves the project.
    ///
    /// Most languages can answer this from the path's shape, so the default
    /// says "no" and a local-looking path stays a local-looking path. Rust is
    /// the exception: `pub use http;` in a crate root re-exports an external
    /// crate, so `crate::http::Request` names a dependency on `http` while
    /// looking exactly like a missing file.
    fn leaves_project(
        &self,
        _name: &str,
        _context: ResolveContext<'_>,
    ) -> bool {
        false
    }

    /// Turn an import path into a project-relative file path.
    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String>;
}

/// Compares a known file's trailing segments against a candidate suffix.
///
/// Shared by the Java resolver, which must match `com.example.Foo` against a
/// project that stores sources at `src/main/java/com/example/Foo.java`.
#[must_use]
pub fn ends_with_segments(path: &str, suffix: &str) -> bool {
    if suffix.is_empty() {
        return true;
    }
    path == suffix
        || path
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_reports_membership() {
        let files: KnownFiles = std::iter::once("src/a.rs".to_owned()).collect();
        let context = test_context("src/b.rs", &files);

        assert!(context.contains("src/a.rs"));
        assert!(!context.contains("src/missing.rs"));
    }

    #[test]
    fn first_existing_skips_unknown_candidates() {
        let files: KnownFiles = std::iter::once("src/b.rs".to_owned()).collect();
        let context = test_context("src/a.rs", &files);

        assert_eq!(
            context.first_existing(["src/x.rs", "src/b.rs", "src/y.rs"]),
            Some("src/b.rs".to_owned())
        );
        assert_eq!(context.first_existing(["src/x.rs"]), None);
    }

    #[test]
    fn segment_suffix_match_requires_a_directory_boundary() {
        assert!(ends_with_segments("a/b/Foo.java", "Foo.java"));
        assert!(ends_with_segments("a/b/com/x/Foo.java", "com/x/Foo.java"));
        assert!(ends_with_segments("Foo.java", "Foo.java"));
        assert!(!ends_with_segments("a/b/NotFoo.java", "Foo.java"));
        // A partial segment must not match, or `Helper` would match `MyHelper`.
        assert!(!ends_with_segments("a/b/XHelper.java", "Helper.java"));
    }
}