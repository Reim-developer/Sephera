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

use crate::core::compression::SupportedLanguage;

pub(crate) mod c_cpp_plugin;
mod go_plugin;
mod java_plugin;
mod javascript_plugin;
mod python_plugin;
mod rust_plugin;

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
}

/// Everything a resolver needs to look at besides the import itself.
#[derive(Debug, Clone, Copy)]
pub struct ResolveContext<'a> {
    /// Normalised, `/`-separated path of the file containing the import.
    pub source_file: &'a str,
    /// Every file in the analysis, as normalised relative paths.
    pub known_files: &'a BTreeSet<String>,
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
pub trait ImportPlugin {
    /// The language this plugin handles.
    fn language(&self) -> SupportedLanguage;

    /// Extracts imports from `source`, or `None` if the text cannot be parsed.
    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>>;
}

/// Maps an import path onto a file inside the analysis.
///
/// Implementations return `None` for imports that leave the project, which is
/// the normal case for external crates and standard library modules. Returning
/// `None` means "external", not "failed", so a language with no local imports
/// still produces a usable graph.
pub trait ResolverPlugin {
    /// The language this plugin handles.
    fn language(&self) -> SupportedLanguage;

    /// Resolves an import to a project file, or `None` when it is external.
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

/// The extraction plugin for a language, if one is bundled.
#[must_use]
pub fn builtin_import_plugin(
    language: SupportedLanguage,
) -> Option<Box<dyn ImportPlugin>> {
    match language {
        SupportedLanguage::Rust => {
            Some(Box::new(self::rust_plugin::RustPlugin))
        }
        SupportedLanguage::Python => {
            Some(Box::new(self::python_plugin::PythonPlugin))
        }
        SupportedLanguage::TypeScript => {
            Some(Box::new(self::javascript_plugin::JavaScriptPlugin {
                language: SupportedLanguage::TypeScript,
            }))
        }
        SupportedLanguage::JavaScript => {
            Some(Box::new(self::javascript_plugin::JavaScriptPlugin {
                language: SupportedLanguage::JavaScript,
            }))
        }
        SupportedLanguage::Go => Some(Box::new(self::go_plugin::GoPlugin)),
        SupportedLanguage::Java => {
            Some(Box::new(self::java_plugin::JavaPlugin))
        }
        SupportedLanguage::C => {
            Some(Box::new(self::c_cpp_plugin::CCppPlugin {
                language: SupportedLanguage::C,
            }))
        }
        SupportedLanguage::Cpp => {
            Some(Box::new(self::c_cpp_plugin::CCppPlugin {
                language: SupportedLanguage::Cpp,
            }))
        }
    }
}

/// The resolution plugin for a language, if one is bundled.
#[must_use]
pub fn builtin_resolver_plugin(
    language: SupportedLanguage,
) -> Option<Box<dyn ResolverPlugin>> {
    match language {
        SupportedLanguage::Rust => {
            Some(Box::new(self::rust_plugin::RustPlugin))
        }
        SupportedLanguage::Python => {
            Some(Box::new(self::python_plugin::PythonPlugin))
        }
        SupportedLanguage::TypeScript => {
            Some(Box::new(self::javascript_plugin::JavaScriptPlugin {
                language: SupportedLanguage::TypeScript,
            }))
        }
        SupportedLanguage::JavaScript => {
            Some(Box::new(self::javascript_plugin::JavaScriptPlugin {
                language: SupportedLanguage::JavaScript,
            }))
        }
        SupportedLanguage::Go => Some(Box::new(self::go_plugin::GoPlugin)),
        SupportedLanguage::Java => {
            Some(Box::new(self::java_plugin::JavaPlugin))
        }
        SupportedLanguage::C => {
            Some(Box::new(self::c_cpp_plugin::CCppPlugin {
                language: SupportedLanguage::C,
            }))
        }
        SupportedLanguage::Cpp => {
            Some(Box::new(self::c_cpp_plugin::CCppPlugin {
                language: SupportedLanguage::Cpp,
            }))
        }
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
        let context = ResolveContext {
            source_file: "src/b.rs",
            known_files: &files,
        };

        assert!(context.contains("src/a.rs"));
        assert!(!context.contains("src/missing.rs"));
    }

    #[test]
    fn first_existing_skips_unknown_candidates() {
        let files: BTreeSet<String> =
            std::iter::once("src/b.rs".to_owned()).collect();
        let context = ResolveContext {
            source_file: "src/a.rs",
            known_files: &files,
        };

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
