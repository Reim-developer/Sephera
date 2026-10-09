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

/// Everything one source file contributes to the graph.
///
/// Returned as a pair because the two halves come from the same parse tree, and
/// building them separately meant reading and parsing the same bytes twice.
#[derive(Debug, Default)]
pub struct ExtractedSource {
    /// The imports this file names, in source order.
    pub imports: Vec<crate::types::ImportStatement>,

    /// The names this file declares, when the language needs a lookup by name.
    ///
    /// `None` for every language but Rust, and that is the normal answer rather
    /// than a gap.
    pub declared: Option<crate::declarations::DeclaredNames>,
}

/// A context for a resolver test, with no lookup tables attached.
///
/// Public and unconditional: it is used by the test modules of six language
/// crates, and `cfg(test)` compiles per crate rather than per workspace, so a
/// tests-only helper in `sephera_core` would be unreachable from all of them.
///
/// The two lookups are what a production resolution reads, and a test that
/// populated them would be testing the fixture as much as the resolver. Tests
/// that do exercise a lookup build the index and pass it explicitly.
#[must_use]
pub fn test_context<'a>(
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

/// The parse and traversal every language shares.
///
/// The walker visits every node in the tree and asks the plugin which ones say
/// something about imports. Keeping only this in one place is what lets a new
/// language be added by writing one module file; the alternative was a `match`
/// over every language here as well as in the plugin registry, which is a second
/// place to edit and a second place to get wrong.
///
/// Depth is carried down rather than looked up, because Rust's `mod tests { ... }`
/// declares a module and `super::` inside it climbs one level further: ten
/// references in this repository were counted as unresolved before depth was
/// tracked, because a reference in a test module resolved to a sibling of the
/// file rather than to the file itself.
///
/// The parser comes from the caller rather than from `sephera_compression`, so
/// this module links no grammar crate. The caller has one -- the cache exists so
/// a run over N files sets the language up once per worker rather than N times.
use anyhow::Result;
use tree_sitter::{Node, Parser, Tree};

use crate::types::ImportStatement;

/// Parse `source` once and return both what it imports and what it declares.
///
/// The two used to be separate methods on the plugin, each building its own
/// parser, so a Rust file was parsed twice to answer two questions about the same
/// bytes.
///
/// # Errors
///
/// Returns an error when the parse fails. A source the grammar rejects is an
/// error here rather than an empty result: the caller knows which file it was.
pub fn walk_with_declarations(
    source: &[u8],
    parser: &mut Parser,
    extractor: &dyn ImportPlugin,
) -> Result<ExtractedSource> {
    let tree: Tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Tree-sitter returned no parse tree"))?;

    let mut imports = Vec::new();
    descend(source, &tree.root_node(), extractor, 0, &mut imports);

    Ok(ExtractedSource {
        imports,
        declared: extractor.collect_declarations(source, &tree),
    })
}

/// Walk a node's children, carrying the depth a reference inside them sits at.
fn descend(
    source: &[u8],
    node: &Node<'_>,
    extractor: &dyn ImportPlugin,
    depth: u8,
    imports: &mut Vec<ImportStatement>,
) {
    if let Some(mut extracted) = extractor.extract_from_node(source, node) {
        let gated = extractor.is_cfg_gated(source, node);
        for statement in &mut extracted {
            statement.module_depth = depth;
            statement.cfg_gated = gated;
        }
        imports.extend(extracted);
    }

    let child_depth = depth.saturating_add(extractor.child_depth_step(node));

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        descend(source, &child, extractor, child_depth, imports);
    }
}

/// The text of a node, as written.
///
/// Every extractor reads paths out of the grammar rather than slicing
/// statements apart, and this is how it gets at a field's text. Trailing
/// whitespace is trimmed because a field's span can include it, and a path with
/// a trailing space matches no file.
#[must_use]
pub fn node_text(source: &[u8], node: &Node<'_>) -> String {
    let start = node.start_byte();
    let end = node.end_byte().min(source.len());
    if start >= source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[start..end])
        .trim_end()
        .to_owned()
}

/// The 1-based line a node sits on.
///
/// Tree-sitter rows are 0-based and every report here is 1-based, so the
/// conversion is not optional, and doing it in one place is what stops two
/// extractors disagreeing about the same import's line.
///
/// Returns `None` rather than clamping: a row that cannot be converted means the
/// position is not a line number, and an extractor reporting a guess would put a
/// reader on the wrong line with no way to tell.
#[must_use]
pub fn line_of(node: &Node<'_>) -> Option<u64> {
    u64::try_from(node.start_position().row + 1).ok()
}

/// The 1-based line a node sits on, falling back when the row is unusable.
///
/// For the extractors that report a line *per import* rather than per statement:
/// a grouped `import (...)` is one node for the whole block, so the statement's
/// line is the right answer for any import in it whose own line cannot be read.
#[must_use]
pub fn line_of_or(node: &Node<'_>, fallback: u64) -> u64 {
    line_of(node).unwrap_or(fallback)
}

/// The value a string literal holds, without its quotes.
///
/// Prefers the grammar's own `string_fragment`, which is the unescaped content,
/// and falls back to trimming quote characters for grammars that expose no
/// fragment. An empty result means the node was not a usable path, so a caller
/// treats it as "no import here" rather than as an empty path.
#[must_use]
pub fn string_value(source: &[u8], node: &Node<'_>) -> String {
    if let Some(fragment) = node.named_child(0) {
        let text = node_text(source, &fragment);
        if !text.is_empty() {
            return text;
        }
    }
    node_text(source, node)
        .trim_matches(['\'', '"', '`', ';'])
        .trim()
        .to_owned()
}

/// Extracts import statements from source text for one language.
///
/// `Sync` because extraction runs across a thread pool: parsing is the
/// dominant cost of a graph run and the work is independent per file. The
/// built-in plugins are stateless, so this costs nothing and buys the
/// parallelism. A plugin holding mutable state would need interior locking.
pub mod walk {
    pub use super::{
        line_of, line_of_or, node_text, string_value, walk_with_declarations,
    };
}

/// Every import a plugin finds in one source, for an extractor's own tests.
///
/// This is [`walk_with_declarations`] under a name that says what a test wants.
/// It exists because that traversal was reimplemented in each extractor's test
/// module -- six copies of the same recursion, each one reaching for the
/// extractor's free `extract_from_node` because it had no plugin to hand. That
/// was the extractor tested and not the walk: a statement the walker reached at
/// the wrong depth, or marked `cfg_gated` when the extractor cannot see an
/// attribute, passed in the copy and failed in production.
///
/// Taking the plugin rather than a function pointer is what closes that. The
/// plugin is the thing that knows the language, and going through it is what
/// makes the test agree with the walk the graph actually uses.
///
/// Takes a parser rather than a language, for the reason
/// [`walk_with_declarations`] does: this crate may not depend on the one holding
/// the eight grammars, and six extractor test modules need this name.
#[must_use]
pub fn imports_found_by(
    source: &[u8],
    parser: &mut tree_sitter::Parser,
    plugin: &dyn ImportPlugin,
) -> Vec<ImportStatement> {
    walk_with_declarations(source, parser, plugin)
        .map(|extracted| extracted.imports)
        .unwrap_or_default()
}

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
        let files: KnownFiles =
            std::iter::once("src/a.rs".to_owned()).collect();
        let context = test_context("src/b.rs", &files);

        assert!(context.contains("src/a.rs"));
        assert!(!context.contains("src/missing.rs"));
    }

    #[test]
    fn first_existing_skips_unknown_candidates() {
        let files: KnownFiles =
            std::iter::once("src/b.rs".to_owned()).collect();
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
