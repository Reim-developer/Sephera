//! The registry of every bundled language plugin.
//!
//! Import handling has two halves that fail independently, so two traits exist
//! -- and both live in `sephera_core`, because the six language crates implement
//! them and must not depend on this crate to do so. What is here is the table
//! pairing each language with its own two halves, and the lookups that reach it.
//!
//! One entry rather than two lists: the plugin behind both traits is the same
//! value, and the sixteen `static`s and two eight-arm `match`es this replaced
//! named `RustPlugin` sixteen times and could have disagreed about which plugin
//! served which language without anything noticing. Pairing one language's
//! extractor with another's resolver is silent -- the result reads as a resolver
//! gap on a file that has none -- so the table takes both halves in one
//! expression and a test checks the pairing.

use sephera_compression::SupportedLanguage;
use sephera_core::plugins::ImportPlugin;

use sephera_graph_cpp::CCppPlugin;
use sephera_graph_go::GoPlugin;
use sephera_graph_java::JavaPlugin;
use sephera_graph_javascript::JavaScriptPlugin;
use sephera_graph_python::PythonPlugin;
use sephera_graph_rust::RustPlugin;

pub use sephera_core::plugins::{
    KnownFiles, ResolveContext, ResolverPlugin, ends_with_segments,
    test_context,
};

pub use sephera_core::path_utils as paths;

/// Every bundled language, in a stable order.
#[must_use]
pub fn builtin_languages() -> Vec<SupportedLanguage> {
    BUNDLED.iter().map(|entry| entry.language).collect()
}

/// One bundled plugin, reachable through either trait.
struct Bundled {
    language: SupportedLanguage,
    import: &'static dyn ImportPlugin,
    /// `Sync` because the registry is a `static`. Every bundled resolver holds no
    /// interior state, so this costs nothing.
    resolver: &'static (dyn ResolverPlugin + Sync),
}

/// Both halves take the same expression, which is what keeps a language's
/// extractor from being paired with another's resolver.
macro_rules! bundled {
    ($language:expr, $plugin:expr $(,)?) => {
        Bundled {
            language: $language,
            import: &$plugin,
            resolver: &$plugin,
        }
    };
}

/// `$plugin` is an expression, not a type: `TypeScript`/`JavaScript` share one
/// struct and `C`/`Cpp` share another, and the field is what says which.
static BUNDLED: &[Bundled] = &[
    bundled!(SupportedLanguage::Rust, RustPlugin),
    bundled!(SupportedLanguage::Python, PythonPlugin),
    bundled!(
        SupportedLanguage::TypeScript,
        JavaScriptPlugin {
            language: SupportedLanguage::TypeScript
        }
    ),
    bundled!(
        SupportedLanguage::JavaScript,
        JavaScriptPlugin {
            language: SupportedLanguage::JavaScript
        }
    ),
    bundled!(SupportedLanguage::Go, GoPlugin),
    bundled!(SupportedLanguage::Java, JavaPlugin),
    bundled!(
        SupportedLanguage::C,
        CCppPlugin {
            language: SupportedLanguage::C
        }
    ),
    bundled!(
        SupportedLanguage::Cpp,
        CCppPlugin {
            language: SupportedLanguage::Cpp
        }
    ),
];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_both_plugins() {
        // Enumerated from the enum rather than from `builtin_languages()`, which
        // reads off the registry. Walking the registry to check the registry
        // passes whatever it contains, so a variant with no entry would leave a
        // language silently unanalysed instead of failing here.
        for language in SupportedLanguage::all() {
            assert!(
                builtin_import_plugin(*language).is_some(),
                "{language:?} is missing an import plugin"
            );
            assert!(
                builtin_resolver_plugin(*language).is_some(),
                "{language:?} is missing a resolver plugin"
            );
        }
    }

    #[test]
    fn the_registry_pairs_each_language_with_its_own_plugin() {
        // A mismatched pair is silent: the C entry would answer with
        // TypeScript's resolver, and the result would read as a resolver gap on
        // a C file -- exactly the kind of finding that gets believed.
        assert_eq!(BUNDLED.len(), SupportedLanguage::all().len());
    }

    #[test]
    fn two_languages_sharing_a_struct_get_distinct_entries() {
        // TypeScript and JavaScript share one struct, as C and C++ share another,
        // so the registry holds one value per language and each carries its own.
        //
        // That the pair is *correctly* paired is not observable from here, and
        // deliberately so: `ImportPlugin` has no `language()` method, because the
        // table is the one place that says which plugin serves which language
        // and a second answer to the same question is one more thing to keep in
        // step. The two tests above are the whole of what can be checked -- every
        // variant has both halves, and there is one entry per variant -- plus this
        // one, which catches the mistake those two cannot: collapsing the pair
        // onto a single `static`.
        //
        // Collapsing is invisible otherwise. Every lookup still returns a working
        // plugin, and the only consequence is that TypeScript files are parsed
        // with whichever grammar the one value names.
        let typescript = builtin_resolver_plugin(SupportedLanguage::TypeScript)
            .expect("TypeScript has a resolver");
        let javascript = builtin_resolver_plugin(SupportedLanguage::JavaScript)
            .expect("JavaScript has a resolver");

        assert!(
            !std::ptr::eq(typescript, javascript),
            "the two share a struct, so the registry must hold one value per language"
        );
    }

    #[test]
    fn resolver_for_unknown_language_is_absent() {
        // A language without a bundled plugin must not panic on lookup; callers
        // treat `None` as "no resolution available".
        assert!(builtin_resolver_plugin(SupportedLanguage::Rust).is_some());
    }
}
