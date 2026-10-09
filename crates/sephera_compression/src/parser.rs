//! Tree-sitter parser management.
//!
//! This module provides a unified interface for obtaining a configured
//! [`tree_sitter::Parser`] for any language that Sephera supports.  Grammars
//! are compiled into the binary via the `tree-sitter-*` crates so no external
//! grammar files are required at runtime.

use std::{cell::RefCell, collections::HashMap};

use tree_sitter::{Language, Parser};

/// A language for which Sephera can perform Tree-sitter parsing.
///
/// Each variant maps directly to a `tree-sitter-*` crate linked at compile
/// time.  The set intentionally covers the most popular languages to maximise
/// out-of-the-box usefulness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    /// Rust source files (`.rs`).
    Rust,
    /// Python source files (`.py`, `.pyi`).
    Python,
    /// TypeScript source files (`.ts`, `.tsx`).
    TypeScript,
    /// JavaScript source files (`.js`, `.jsx`, `.mjs`, `.cjs`).
    JavaScript,
    /// Go source files (`.go`).
    Go,
    /// Java source files (`.java`).
    Java,
    /// C++ source files (`.cpp`, `.cxx`, `.cc`, `.hpp`, `.hxx`, `.h`
    /// when accompanied by C++ indicators).
    Cpp,
    /// C source files (`.c`, `.h`).
    C,
}

impl SupportedLanguage {
    /// Attempts to identify the Tree-sitter language from a Sephera language
    /// name (the same name used in `LanguageConfig::name`).  Returns [`None`]
    /// for languages that do not have a Tree-sitter grammar linked into this
    /// build.
    ///
    /// # Examples
    ///
    /// ```
    /// use sephera_core::core::compression::SupportedLanguage;
    ///
    /// assert_eq!(
    ///     SupportedLanguage::from_language_name("Rust"),
    ///     Some(SupportedLanguage::Rust),
    /// );
    /// assert_eq!(SupportedLanguage::from_language_name("TOML"), None);
    /// ```
    #[must_use]
    pub fn from_language_name(name: &str) -> Option<Self> {
        match name {
            "Rust" => Some(Self::Rust),
            "Python" => Some(Self::Python),
            "TypeScript" | "TSX" => Some(Self::TypeScript),
            "JavaScript" | "JSX" => Some(Self::JavaScript),
            // `config/languages.yml` names this language `Golang`, while
            // `SupportedLanguage::Go` is the enum spelling. Accepting both
            // keeps the scanner's naming and the enum from drifting apart,
            // which silently dropped every Go file from graph analysis.
            "Go" | "Golang" => Some(Self::Go),
            "Java" => Some(Self::Java),
            "C++" | "C++ Header File" => Some(Self::Cpp),
            // The registry lists headers as their own language so that
            // `languages.yml` can give them a comment style, and those names are
            // what `language_for_path` hands back. Mapping them to the C and
            // C++ grammars is what makes a header produce edges: a header names
            // other headers, so without this the graph stopped at the top of
            // every header tree and described a C project as if its headers
            // were leaves.
            "C" | "C Header" | "C Header File" => Some(Self::C),
            _ => None,
        }
    }

    /// The language a file extension names.
    ///
    /// A graph edge carries the file it came from but not the language, and the
    /// extension is what the scanner itself keys on, so this is the same decision
    /// made in one more place rather than a second rule to keep in step.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" => Some(Self::Cpp),
            "c" | "h" => Some(Self::C),
            _ => None,
        }
    }

    /// Returns the [`tree_sitter::Language`] grammar for this variant.
    #[must_use]
    fn tree_sitter_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::TypeScript => {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
        }
    }

    /// Returns all supported language variants.
    ///
    /// # Examples
    ///
    /// ```
    /// use sephera_core::core::compression::SupportedLanguage;
    ///
    /// assert!(SupportedLanguage::all().len() >= 8);
    /// ```
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Rust,
            Self::Python,
            Self::TypeScript,
            Self::JavaScript,
            Self::Go,
            Self::Java,
            Self::Cpp,
            Self::C,
        ]
    }
}

/// Creates a new [`Parser`] configured for the given language.
///
/// # Errors
///
/// Returns an error if Tree-sitter fails to set the language (should
/// not happen with compiled-in grammars).
///
/// # Examples
///
/// ```
/// use sephera_core::core::compression::{SupportedLanguage, new_parser};
///
/// let mut parser = new_parser(SupportedLanguage::Rust).unwrap();
/// let tree = parser.parse("fn main() {}", None).unwrap();
/// assert!(!tree.root_node().has_error());
/// ```
pub fn new_parser(language: SupportedLanguage) -> anyhow::Result<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.tree_sitter_language())
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to set Tree-sitter language for {language:?}: {error}",
            )
        })?;
    Ok(parser)
}

/// The largest source a cached parser is kept for after parsing it.
///
/// A parser holds the scratch space its last parse needed, so a cache that
/// reuses parsers is also a cache that remembers the largest file each thread
/// saw. One vendored 400 MB generated header would otherwise leave every worker
/// holding hundreds of megabytes until the process ended, and sixteen workers
/// would turn that into an out-of-memory kill on a machine that had analysed the
/// same tree happily before.
///
/// So a parse over this size still uses the cached parser -- dropping it first
/// would mean the allocation cost is paid anyway -- but the parser is released
/// afterwards instead of going back in the cache. The cost is one parser per
/// oversized file, and oversized files are rare enough that it does not show.
const CACHE_AFTER_BYTES: usize = 2 * 1024 * 1024;

/// How many parsers `with_parser` has built **on this thread**.
///
/// Test-only, and the reason the reuse test below has teeth: counting what the
/// cache *holds* cannot tell a parser that was reused from one that was rebuilt
/// and put straight back, and that is exactly the difference the cache claims to
/// make.
///
/// Per thread rather than global because the cache is per thread, and the test
/// runner runs tests concurrently -- a global count picks up every other test's
/// parsers and fails for reasons that have nothing to do with this one.
///
/// In its own module so this comment attaches to a `mod` rather than to a
/// `thread_local!` item, which the compiler treats as a macro invocation and
/// would leave undocumented.
#[cfg(test)]
mod built {
    thread_local! {
        static PARSERS: std::cell::Cell<usize> =
            const { std::cell::Cell::new(0) };
    }

    pub(super) fn count() -> usize {
        PARSERS.with(std::cell::Cell::get)
    }

    pub(super) fn reset() {
        PARSERS.with(|count| count.set(0));
    }

    pub(super) fn record_one_built() {
        PARSERS.with(|count| count.set(count.get() + 1));
    }
}

#[cfg(test)]
fn parsers_built() -> usize {
    built::count()
}

#[cfg(test)]
fn reset_parsers_built() {
    built::reset();
}

thread_local! {
    /// One parser per language, per thread.
    ///
    /// Tree-sitter's own guidance is to keep a parser and hand it each source
    /// rather than build one per file, and this is where that guidance had not
    /// been taken: `symbols` and `graph` each called [`new_parser`] once per
    /// file, inside a `par_iter`, so a repository of 630 files built 630
    /// parsers and threw away every buffer they had grown.
    ///
    /// Thread-local rather than a shared pool because `Parser` is `!Sync` and the
    /// work already runs on a rayon pool, so the thread *is* the natural owner.
    /// Keyed by language because `set_language` is the expensive part and a
    /// parser is only worth keeping for the language it was set to.
    static PARSERS: RefCell<HashMap<SupportedLanguage, Parser>> =
        RefCell::new(HashMap::new());
}

/// Runs `use_parser` with a [`Parser`] already configured for `language`.
///
/// The parser comes from this thread's cache when there is one, so a run that
/// walks a repository pays for parser setup once per language per thread rather
/// than once per file. `source_bytes` is the size of the source about to be
/// parsed, and only affects whether the parser is *kept* afterwards -- see
/// [`CACHE_AFTER_BYTES`].
///
/// The parser is borrowed rather than returned, so a caller cannot keep it past
/// the closure. That is the price of the cache: a parser handed out would have to
/// be handed back, and a caller that forgot would leave the cache borrowed.
///
/// # Errors
///
/// Returns an error if Tree-sitter fails to set the language, or whatever
/// `use_parser` returns.
pub fn with_parser<R>(
    language: SupportedLanguage,
    source_bytes: usize,
    use_parser: impl FnOnce(&mut Parser) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let setup_failure = std::cell::Cell::new(None);
    let parsed = PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        let mut parser = if let Some(parser) = parsers.remove(&language) {
            parser
        } else {
            // Built rather than reused: this is the branch the cache exists to
            // avoid, once per language per thread.
            #[cfg(test)]
            built::record_one_built();

            match new_parser(language) {
                Ok(parser) => parser,
                Err(problem) => {
                    setup_failure.set(Some(problem));
                    return None;
                }
            }
        };

        let outcome = use_parser(&mut parser);

        // An oversized source is parsed but not remembered, so the memory one
        // huge file needed is released instead of held until the process ends.
        if source_bytes <= CACHE_AFTER_BYTES {
            parsers.insert(language, parser);
        }

        outcome.ok()
    });

    parsed.ok_or_else(|| {
        setup_failure
            .into_inner()
            .unwrap_or_else(|| anyhow::anyhow!("parsing {language:?} failed"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many parsers this thread is currently holding.
    ///
    /// Read-only and test-only, because the cache's memory behaviour is the one
    /// thing about it that is not visible in what a command prints: a parser that
    /// silently keeps a 400 MB file's worth of scratch space looks exactly like a
    /// working tool from the outside.
    fn cached_parser_count() -> usize {
        PARSERS.with(|parsers| parsers.borrow().len())
    }

    #[test]
    fn a_reused_parser_answers_the_same_as_a_fresh_one() {
        // The whole claim of the cache is that a second parse with a borrowed
        // parser is identical to one with a new parser. If reusing changed a
        // tree, every count this repository publishes would depend on parse
        // order, and the corpus pins would stop meaning anything.
        let source = b"use std::io;\n\npub fn run() -> u32 { 7 }\n";

        let first =
            with_parser(SupportedLanguage::Rust, source.len(), |parser| {
                Ok(parser.parse(source.as_slice(), None))
            })
            .expect("first parse")
            .expect("a tree");

        let second =
            with_parser(SupportedLanguage::Rust, source.len(), |parser| {
                Ok(parser.parse(source.as_slice(), None))
            })
            .expect("second parse")
            .expect("a tree");

        assert_eq!(first.root_node().to_sexp(), second.root_node().to_sexp());
    }

    #[test]
    fn a_parser_is_kept_between_calls_and_not_for_every_language() {
        let source = b"fn main() {}\n";
        reset_parsers_built();

        for _ in 0..5 {
            with_parser(SupportedLanguage::Rust, source.len(), |parser| {
                Ok(parser.parse(source.as_slice(), None))
            })
            .expect("rust parse")
            .expect("a tree");
        }

        assert_eq!(
            parsers_built(),
            1,
            "five parses of one language must build one parser, not five"
        );
        assert_eq!(cached_parser_count(), 1, "the parser was not kept");

        with_parser(SupportedLanguage::Python, source.len(), |parser| {
            Ok(parser.parse(source.as_slice(), None))
        })
        .expect("python parse")
        .expect("a tree");
        assert_eq!(
            cached_parser_count(),
            2,
            "a second language gets its own parser, because `set_language` is \
             the expensive part and a parser is only worth keeping for the \
             language it was configured with"
        );
    }

    #[test]
    fn an_oversized_source_is_parsed_but_not_kept() {
        // The memory bound. A parser holds the scratch space its last parse
        // needed, so keeping one after a very large file would leave every
        // worker holding that much until the process ended.
        let source = b"fn main() {}\n";

        with_parser(SupportedLanguage::Go, source.len(), |parser| {
            Ok(parser.parse(source.as_slice(), None))
        })
        .expect("first parse")
        .expect("a tree");
        assert_eq!(cached_parser_count(), 1);

        with_parser(SupportedLanguage::Go, CACHE_AFTER_BYTES + 1, |parser| {
            Ok(parser.parse(source.as_slice(), None))
        })
        .expect("the oversized parse still succeeds")
        .expect("a tree");
        assert_eq!(
            cached_parser_count(),
            0,
            "a parser that parsed something this large must be released rather \
             than kept"
        );
    }

    #[test]
    fn a_parse_that_fails_leaves_the_cache_usable() {
        // The closure's error must not take the cached parser with it: one file
        // the grammar rejects should not cost the next one its parser.
        let source = b"fn main() {}\n";

        let failed = with_parser(SupportedLanguage::Java, source.len(), |_| {
            Err::<tree_sitter::Tree, _>(anyhow::anyhow!("deliberate failure"))
        });
        assert!(failed.is_err(), "the error must reach the caller");

        let recovered =
            with_parser(SupportedLanguage::Java, source.len(), |parser| {
                Ok(parser.parse(source.as_slice(), None))
            })
            .expect("a later parse still works")
            .expect("a tree");
        assert!(!recovered.root_node().has_error());
    }

    #[test]
    fn creates_parser_for_all_languages() {
        for language in SupportedLanguage::all() {
            let parser = new_parser(*language);
            assert!(parser.is_ok(), "failed to create parser for {language:?}");
        }
    }

    #[test]
    fn resolves_language_names() {
        assert_eq!(
            SupportedLanguage::from_language_name("Rust"),
            Some(SupportedLanguage::Rust)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("Python"),
            Some(SupportedLanguage::Python)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("TypeScript"),
            Some(SupportedLanguage::TypeScript)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("JavaScript"),
            Some(SupportedLanguage::JavaScript)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("Go"),
            Some(SupportedLanguage::Go)
        );
        // The scanner reports this language as `Golang`, so that spelling must
        // resolve as well; it did not, which dropped every Go file from graph
        // analysis while `loc` still counted its lines.
        assert_eq!(
            SupportedLanguage::from_language_name("Golang"),
            Some(SupportedLanguage::Go)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("Java"),
            Some(SupportedLanguage::Java)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("C++"),
            Some(SupportedLanguage::Cpp)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("C"),
            Some(SupportedLanguage::C)
        );
    }

    #[test]
    fn returns_none_for_unsupported_languages() {
        assert_eq!(SupportedLanguage::from_language_name("TOML"), None);
        assert_eq!(SupportedLanguage::from_language_name("YAML"), None);
        assert_eq!(SupportedLanguage::from_language_name("Markdown"), None);
    }

    #[test]
    fn parses_simple_rust_source() {
        let mut parser = new_parser(SupportedLanguage::Rust).unwrap();
        let tree = parser.parse("fn main() {}", None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parses_simple_python_source() {
        let mut parser = new_parser(SupportedLanguage::Python).unwrap();
        let tree = parser
            .parse("def greet(name):\n    print(name)\n", None)
            .unwrap();
        assert!(!tree.root_node().has_error());
    }
}
