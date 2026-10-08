//! Language plugin traits and the built-in language registry.
//!
//! Import handling has two halves that fail independently, so they are two
//! traits:
//!
//! - [`ImportPlugin`] extracts import statements out of source text. This is
//!   where Tree-sitter grammars live.
//! - [`ResolverPlugin`] turns an extracted import into a project-relative file
//!   path, or reports that the import is external.
//!
//! Languages are registered in [`builtin_plugins`]. Adding a language means
//! adding one file here; no existing dispatch needs to change.

use std::collections::BTreeSet;

use crate::core::{compression::SupportedLanguage, graph::ImportKind};

mod c_cpp;
mod go;
mod java;
mod javascript;
mod python;
pub(crate) mod rust;
mod walk;

/// Re-exported so plugin files can reach the shared helpers through one path.
pub use super::path_utils as paths;

/// Every file in an analysis, as normalised relative paths.
pub type KnownFiles = BTreeSet<String>;

/// Everything one source file contributes to the graph.
///
/// Returned as a pair because the two halves come from the same parse tree, and
/// building them separately meant reading and parsing the same bytes twice.
#[derive(Debug, Default)]
pub struct ExtractedSource {
    /// The imports this file names, in source order.
    ///
    /// The graph's own [`ImportStatement`](super::types::ImportStatement),
    /// which is what the walk produces. A separate plugin-facing import type used
    /// to sit between the two, and every statement was converted into it only to
    /// be converted straight back by the caller. It existed to satisfy two
    /// separate methods, and there is now one.
    pub imports: Vec<super::types::ImportStatement>,

    /// The names this file declares, when the language needs a lookup by name.
    ///
    /// `None` for every language but Rust, and that is the normal answer rather
    /// than a gap.
    pub declared: Option<super::declarations::DeclaredNames>,
}

/// Everything a resolver needs to look at besides the import itself.
#[derive(Debug, Clone, Copy)]
pub struct ResolveContext<'a> {
    /// Normalised, `/`-separated path of the file containing the import.
    pub source_file: &'a str,
    /// Every file in the analysis, as normalised relative paths.
    pub known_files: &'a BTreeSet<String>,
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
    pub declarations: Option<&'a super::declarations::DeclarationIndex>,

    /// What the project's manifests say about it.
    ///
    /// Read for one thing: Go's module path. An import of the bare module path
    /// names the package at the project root, and no directory-name match finds
    /// it, because the files sit at the top level rather than in a directory
    /// named after the module.
    pub manifests: Option<&'a super::manifests::ManifestIndex>,

    /// Directory the analysis was rooted at.
    ///
    /// Resolvers work in project-relative paths, so reading a file one of them
    /// names -- a `package.json` for its `main` field -- needs the root to turn
    /// it into something on disk. Without it the only guess is the process
    /// working directory, which is a different directory whenever `--path` was
    /// given.
    pub base_path: &'a std::path::Path,
}

/// A context for a resolver test, with no lookup tables attached.
///
/// The two indexes are what a production resolution reads, and a test that
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
        kind: crate::core::graph::ImportKind::Dependency,
        declarations: None,
        manifests: None,
        base_path: std::path::Path::new(""),
    }
}

impl ResolveContext<'_> {
    /// Whether a candidate path names a real file in the analysis.
    #[must_use]
    pub fn contains(&self, candidate: &str) -> bool {
        self.known_files.contains(candidate)
    }

    /// First candidate in `candidates` that names a real file.
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

/// Extracts import statements from source text for one language.
///
/// `Sync` because extraction runs across a thread pool: parsing is the
/// dominant cost of a graph run and the work is independent per file. The
/// built-in plugins are stateless, so this costs nothing and buys the
/// parallelism. A plugin holding mutable state would need interior locking.
pub trait ImportPlugin: Sync {
    /// The language this plugin handles.
    fn language(&self) -> SupportedLanguage;

    /// Read whatever this node says about imports, if it says anything.
    ///
    /// One method per AST node rather than one per language statement, because
    /// the walk visits every node and the plugin decides which are the
    /// interesting ones. Required rather than defaulted: a plugin with no
    /// opinion on any node would silently contribute an empty graph, which is
    /// the hardest kind of failure to notice.
    ///
    /// Extracting text from a node is the language's job, not the walker's.
    /// Searching `import { from as origin } from './b'` for `"from "` found the
    /// binding named `from` and produced the path `as origin } from './b'`; every
    /// extractor here reads the grammar's own fields, and none of them splits a
    /// statement apart.
    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<super::types::ImportStatement>>;

    /// Everything one source contributes: the imports it names, and what it
    /// declares.
    ///
    /// One parse answers both. They used to be two methods — `extract` and
    /// `declared_names` — each building its own parser, so a Rust file was read
    /// and parsed twice to produce two halves of one answer. Two methods that
    /// must agree about parsing is a way to disagree.
    ///
    /// # Panics
    ///
    /// Never. A source that cannot be parsed yields `None`, which the caller
    /// treats as "nothing here" rather than as an error, because a file the
    /// grammar rejects is a file with no imports in it.
    fn extract_source(&self, source: &[u8]) -> Option<ExtractedSource>;

    /// How much deeper references inside this node's children sit.
    ///
    /// Rust's `mod tests { ... }` declares a module, so `super::` inside it
    /// climbs one level further — ten references in this repository were counted
    /// as unresolved before the depth was carried. No other supported language
    /// has the construct, so the answer is zero and costs nothing.
    fn child_depth_step(&self, _node: &tree_sitter::Node<'_>) -> u8 {
        0
    }

    /// Whether this reference only exists when a feature flag is on.
    ///
    /// Rust's `#[cfg(feature = "json")]` is the case. The reference is real, but
    /// a blast radius that counts it without saying so claims a dependency the
    /// default build does not have. Languages with no conditional imports leave
    /// this false.
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
    /// answer, and only Rust does, because `use crate::Router;` names a type the
    /// crate root re-exported rather than a file called `Router`.
    ///
    /// The tree is a parameter rather than something the plugin parses, because
    /// that is the whole point: one parse, two answers. The trait used to state
    /// "one extra parse per Rust file" as though that were a cost worth
    /// documenting, which is usually a sign the cost should go away instead.
    fn collect_declarations(
        &self,
        _source: &[u8],
        _tree: &tree_sitter::Tree,
    ) -> Option<super::declarations::DeclaredNames> {
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
    /// The counterpart to [`ResolverPlugin::leaves_project`], and asked of the
    /// same thing: a path the resolver could not place. A language whose absolute
    /// paths are spelled the same whether they leave the project or not has no
    /// evidence in the shape, and this is where the evidence is. Python's
    /// `collections.OrderedDict` and a broken `pkg.absent` differ only in whether
    /// a file of that name is here, and without this the second was filed as an
    /// external dependency -- a path that meant to name a project file and could
    /// not, reported as a reference to code outside it.
    ///
    /// The default says "cannot tell", which leaves every language that was not
    /// counting these as gaps continuing not to count them.
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
    /// looking exactly like a missing file. Only a resolver that has read the
    /// crate root can tell those apart, and counting them as gaps made a
    /// correctly declared dependency look broken.
    fn leaves_project(
        &self,
        _name: &str,
        _context: ResolveContext<'_>,
    ) -> bool {
        false
    }

    /// The language this plugin handles.
    fn language(&self) -> SupportedLanguage;

    /// Turn an import path into a project-relative file path.
    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String>;
}

/// Every plugin bundled with Sephera, in a stable order.
///
/// Read off the registry rather than spelled out separately. It used to be its
/// own eight-item `vec!`, which meant adding a language was two edits in this one
/// file and neither one complained if the other was missed.
#[must_use]
pub fn builtin_languages() -> Vec<SupportedLanguage> {
    BUNDLED.iter().map(|entry| entry.language).collect()
}

/// One bundled plugin, reachable through either trait.
///
/// One entry rather than two lists: the plugin behind both traits is the same
/// value, and the sixteen `static`s and two eight-arm `match`es this replaced
/// named `RustPlugin` sixteen times and could have disagreed about which plugin
/// served which language without anything noticing.
///
/// Two trait-object fields rather than one `dyn ImportPlugin + ResolverPlugin`
/// because Rust allows one non-auto trait per trait object. Nothing in the type
/// stops an entry pairing one language's extractor with another's resolver, so
/// `the_registry_pairs_each_language_with_its_own_plugin` checks it.
struct Bundled {
    /// The language these plugins serve.
    language: SupportedLanguage,
    /// The extractor for the language.
    import: &'static dyn ImportPlugin,
    /// The resolver for the same language.
    ///
    /// `Sync` because the registry is a `static`. Every bundled resolver holds no
    /// interior state, so this costs nothing.
    resolver: &'static (dyn ResolverPlugin + Sync),
}

/// One registry entry.
///
/// Both halves take the same expression, which is what keeps a language's
/// extractor from being paired with another's resolver. The macro exists only for
/// that: spelled out, each entry names its plugin twice and the table is 82 lines
/// longer than this form.
///
/// `$plugin` is an expression, not a type: `TypeScript`/`JavaScript` share one
/// struct and `C`/`Cpp` share another, and the field is what says which.
macro_rules! bundled {
    ($language:expr, $plugin:expr $(,)?) => {
        Bundled {
            language: $language,
            import: &$plugin,
            resolver: &$plugin,
        }
    };
}

/// Every bundled language, in a stable order.
///
/// `JavaScriptPlugin` and `CCppPlugin` carry a `language` field rather than
/// being unit structs, because one type serves two languages and only the field
/// says which. A unit struct per language would be two more types to write and
/// two more implementations of two traits.
static BUNDLED: &[Bundled] = &[
    bundled!(SupportedLanguage::Rust, self::rust::RustPlugin),
    bundled!(SupportedLanguage::Python, self::python::PythonPlugin),
    bundled!(
        SupportedLanguage::TypeScript,
        self::javascript::JavaScriptPlugin {
            language: SupportedLanguage::TypeScript,
        }
    ),
    bundled!(
        SupportedLanguage::JavaScript,
        self::javascript::JavaScriptPlugin {
            language: SupportedLanguage::JavaScript,
        }
    ),
    bundled!(SupportedLanguage::Go, self::go::GoPlugin),
    bundled!(SupportedLanguage::Java, self::java::JavaPlugin),
    bundled!(
        SupportedLanguage::C,
        self::c_cpp::CCppPlugin {
            language: SupportedLanguage::C,
        }
    ),
    bundled!(
        SupportedLanguage::Cpp,
        self::c_cpp::CCppPlugin {
            language: SupportedLanguage::Cpp,
        }
    ),
];

/// The entry serving a language, if one is bundled.
fn bundled_for(language: SupportedLanguage) -> Option<&'static Bundled> {
    BUNDLED.iter().find(|entry| entry.language == language)
}

/// The extraction plugin for a language, if one is bundled.
///
/// A shared reference rather than a `Box`: it removes one heap allocation per
/// lookup, and it is what lets extraction run across a thread pool without a
/// lock, since every caller wants the same value.
#[must_use]
pub fn builtin_import_plugin(
    language: SupportedLanguage,
) -> Option<&'static dyn ImportPlugin> {
    bundled_for(language).map(|entry| entry.import)
}

/// The resolution plugin for a language, if one is bundled.
///
/// The `Sync` bound is dropped on the way out: the registry needs it because its
/// contents live in a `static`, and a caller holding a `&'static` reference has
/// no thread-safety obligation left to satisfy.
#[must_use]
pub fn builtin_resolver_plugin(
    language: SupportedLanguage,
) -> Option<&'static dyn ResolverPlugin> {
    bundled_for(language).map(|entry| entry.resolver as &dyn ResolverPlugin)
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
    fn every_variant_has_both_plugins() {
        // Enumerated from the enum rather than from `builtin_languages()`, which
        // reads off the registry. Walking the registry to check the registry
        // passes whatever it contains, so a variant with no entry would leave a
        // language silently unanalysed instead of failing here.
        for variant in [
            SupportedLanguage::Rust,
            SupportedLanguage::Python,
            SupportedLanguage::TypeScript,
            SupportedLanguage::JavaScript,
            SupportedLanguage::Go,
            SupportedLanguage::Java,
            SupportedLanguage::C,
            SupportedLanguage::Cpp,
        ] {
            assert!(
                builtin_import_plugin(variant).is_some(),
                "{variant:?} is missing an import plugin"
            );
            assert!(
                builtin_resolver_plugin(variant).is_some(),
                "{variant:?} is missing a resolver plugin"
            );
        }
    }

    #[test]
    fn plugin_reports_its_own_language() {
        for language in builtin_languages() {
            let extractor = builtin_import_plugin(language).unwrap();
            assert_eq!(
                extractor.language(),
                language,
                "import plugin mismatch"
            );

            let resolver = builtin_resolver_plugin(language).unwrap();
            assert_eq!(
                resolver.language(),
                language,
                "resolver plugin mismatch"
            );
        }
    }

    #[test]
    fn the_registry_pairs_each_language_with_its_own_plugin() {
        // A mismatched pair is silent: the C entry would answer with TypeScript's
        // resolver, and the result would read as a resolver gap on a C file --
        // exactly the kind of finding that gets believed. Splitting the two
        // plugins into two registries is what this guards against, and it could
        // not be stated before they were one entry.
        for entry in BUNDLED {
            assert_eq!(entry.import.language(), entry.language, "extractor");
            assert_eq!(entry.resolver.language(), entry.language, "resolver");
        }
    }

    #[test]
    fn resolver_for_unknown_language_is_absent() {
        // A language without a bundled plugin must not panic on lookup; callers
        // treat `None` as "no resolution available".
        assert!(builtin_resolver_plugin(SupportedLanguage::Rust).is_some());
    }

    #[test]
    fn context_reports_membership() {
        let files: BTreeSet<String> =
            std::iter::once("src/a.rs".to_owned()).collect();
        let context = test_context("src/b.rs", &files);

        assert!(context.contains("src/a.rs"));
        assert!(!context.contains("src/missing.rs"));
    }

    #[test]
    fn first_existing_skips_unknown_candidates() {
        let files: BTreeSet<String> =
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
