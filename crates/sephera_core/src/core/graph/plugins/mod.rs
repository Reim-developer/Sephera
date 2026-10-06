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

pub(crate) mod c_cpp_plugin;
mod go_plugin;
mod java_plugin;
mod javascript_plugin;
mod python_plugin;
pub(crate) mod rust_plugin;

/// Re-exported so plugin files can reach the shared helpers through one path.
pub use super::path_utils as paths;

/// Every file in an analysis, as normalised relative paths.
pub type KnownFiles = BTreeSet<String>;

/// Convert the Tree-sitter walker's output into the plugin-facing type.
///
/// Lives here so the six `extract` implementations differ only in which
/// grammar they pass, rather than repeating the same conversion.
pub(super) fn to_extracted(
    statements: Vec<super::types::ImportStatement>,
) -> Vec<ExtractedImport> {
    statements
        .into_iter()
        .map(|statement| ExtractedImport {
            raw_path: statement.raw_path,
            line: usize::try_from(statement.line).unwrap_or(1),
            kind: statement.kind,
            module_depth: statement.module_depth,
            cfg_gated: statement.cfg_gated,
        })
        .collect()
}

/// An import statement as it appears in source text.
///
/// `line` is 1-based so it can be reported to a user without adjustment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedImport {
    /// The raw import target exactly as written, such as `crate::core::graph`.
    pub raw_path: String,
    /// 1-based line number where the import appears.
    pub line: usize,
    /// What this reference says about the file it names.
    ///
    /// Carried through the plugin boundary because the resolver needs it to keep
    /// declarations out of cycle detection and to apply the edge filters. See
    /// [`ImportKind`](crate::core::graph::ImportKind).
    pub kind: ImportKind,
    /// How many inline mod name { ... } blocks the reference sits inside.
    pub module_depth: u8,
    /// Whether a `#[cfg(...)]` attribute gated the reference.
    pub cfg_gated: bool,
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

    /// Extracts imports from `source`, or `None` if the text cannot be parsed.
    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>>;

    /// Names this source declares, when the language needs a lookup by name.
    ///
    /// Returning `None` is the normal answer and costs nothing: most languages
    /// resolve an import to a file without knowing what the file declares. Only
    /// a plugin whose imports can name something other than a module has to
    /// answer, and only Rust does, because `use crate::Router;` names a type the
    /// crate root re-exported rather than a file called `Router`.
    ///
    /// This parses again rather than sharing a tree with [`Self::extract`]. The
    /// cost is one extra parse per Rust file and buys a trait that stays a
    /// trait: a language with no name-based imports adds nothing here, and one
    /// that needs them overrides one method instead of editing the extractor.
    fn declared_names(
        &self,
        _source: &[u8],
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
/// Used by [`builtin_import_plugin`] and [`builtin_resolver_plugin`].
#[must_use]
pub fn builtin_languages() -> Vec<SupportedLanguage> {
    vec![
        SupportedLanguage::Rust,
        SupportedLanguage::Python,
        SupportedLanguage::TypeScript,
        SupportedLanguage::JavaScript,
        SupportedLanguage::Go,
        SupportedLanguage::Java,
        SupportedLanguage::C,
        SupportedLanguage::Cpp,
    ]
}

/// Every bundled extraction plugin, as a static.
///
/// A plugin holds no state beyond which language it handles, so there is nothing
/// to build per call. Handing back a shared reference instead of a
/// `Box` removes one heap allocation per file per lookup — `extract_one_file`
/// asks twice, once for imports and once for declared names — and it is what
/// lets extraction run across a thread pool without a lock.
static RUST_IMPORT: self::rust_plugin::RustPlugin =
    self::rust_plugin::RustPlugin;
static PYTHON_IMPORT: self::python_plugin::PythonPlugin =
    self::python_plugin::PythonPlugin;
static TYPESCRIPT_IMPORT: self::javascript_plugin::JavaScriptPlugin =
    self::javascript_plugin::JavaScriptPlugin {
        language: SupportedLanguage::TypeScript,
    };
static JAVASCRIPT_IMPORT: self::javascript_plugin::JavaScriptPlugin =
    self::javascript_plugin::JavaScriptPlugin {
        language: SupportedLanguage::JavaScript,
    };
static GO_IMPORT: self::go_plugin::GoPlugin = self::go_plugin::GoPlugin;
static JAVA_IMPORT: self::java_plugin::JavaPlugin =
    self::java_plugin::JavaPlugin;
static C_IMPORT: self::c_cpp_plugin::CCppPlugin =
    self::c_cpp_plugin::CCppPlugin {
        language: SupportedLanguage::C,
    };
static CPP_IMPORT: self::c_cpp_plugin::CCppPlugin =
    self::c_cpp_plugin::CCppPlugin {
        language: SupportedLanguage::Cpp,
    };

/// The extraction plugin for a language, if one is bundled.
#[must_use]
pub fn builtin_import_plugin(
    language: SupportedLanguage,
) -> Option<&'static dyn ImportPlugin> {
    match language {
        SupportedLanguage::Rust => Some(&RUST_IMPORT),
        SupportedLanguage::Python => Some(&PYTHON_IMPORT),
        SupportedLanguage::TypeScript => Some(&TYPESCRIPT_IMPORT),
        SupportedLanguage::JavaScript => Some(&JAVASCRIPT_IMPORT),
        SupportedLanguage::Go => Some(&GO_IMPORT),
        SupportedLanguage::Java => Some(&JAVA_IMPORT),
        SupportedLanguage::C => Some(&C_IMPORT),
        SupportedLanguage::Cpp => Some(&CPP_IMPORT),
    }
}

/// Every bundled resolver plugin, as a static.
///
/// Same reasoning as [`RUST_IMPORT`] and the rest: no state to build, no
/// allocation per call, and shareable across threads.
static RUST_RESOLVER: self::rust_plugin::RustPlugin =
    self::rust_plugin::RustPlugin;
static PYTHON_RESOLVER: self::python_plugin::PythonPlugin =
    self::python_plugin::PythonPlugin;
static TYPESCRIPT_RESOLVER: self::javascript_plugin::JavaScriptPlugin =
    self::javascript_plugin::JavaScriptPlugin {
        language: SupportedLanguage::TypeScript,
    };
static JAVASCRIPT_RESOLVER: self::javascript_plugin::JavaScriptPlugin =
    self::javascript_plugin::JavaScriptPlugin {
        language: SupportedLanguage::JavaScript,
    };
static GO_RESOLVER: self::go_plugin::GoPlugin = self::go_plugin::GoPlugin;
static JAVA_RESOLVER: self::java_plugin::JavaPlugin =
    self::java_plugin::JavaPlugin;
static C_RESOLVER: self::c_cpp_plugin::CCppPlugin =
    self::c_cpp_plugin::CCppPlugin {
        language: SupportedLanguage::C,
    };
static CPP_RESOLVER: self::c_cpp_plugin::CCppPlugin =
    self::c_cpp_plugin::CCppPlugin {
        language: SupportedLanguage::Cpp,
    };

/// The resolution plugin for a language, if one is bundled.
#[must_use]
pub fn builtin_resolver_plugin(
    language: SupportedLanguage,
) -> Option<&'static dyn ResolverPlugin> {
    match language {
        SupportedLanguage::Rust => Some(&RUST_RESOLVER),
        SupportedLanguage::Python => Some(&PYTHON_RESOLVER),
        SupportedLanguage::TypeScript => Some(&TYPESCRIPT_RESOLVER),
        SupportedLanguage::JavaScript => Some(&JAVASCRIPT_RESOLVER),
        SupportedLanguage::Go => Some(&GO_RESOLVER),
        SupportedLanguage::Java => Some(&JAVA_RESOLVER),
        SupportedLanguage::C => Some(&C_RESOLVER),
        SupportedLanguage::Cpp => Some(&CPP_RESOLVER),
    }
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
    fn every_builtin_language_has_both_plugins() {
        for language in builtin_languages() {
            assert!(
                builtin_import_plugin(language).is_some(),
                "{language:?} is missing an import plugin"
            );
            assert!(
                builtin_resolver_plugin(language).is_some(),
                "{language:?} is missing a resolver plugin"
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
