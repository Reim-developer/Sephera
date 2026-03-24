use std::path::Path;

use sephera_core::core::language_data::{builtin_languages, language_for_path};

#[test]
fn every_builtin_language_resolves_all_declared_selectors() {
    for (language_index, language) in builtin_languages().iter().enumerate() {
        assert!(
            !language.extensions.is_empty() || !language.exact_names.is_empty(),
            "{} must expose at least one selector",
            language.name
        );

        for extension in language.extensions {
            let path = format!("fixture{extension}");
            let (resolved_index, resolved_language) =
                language_for_path(Path::new(&path)).unwrap_or_else(|| {
                    panic!(
                        "{} extension {} did not resolve",
                        language.name, extension
                    )
                });

            assert_eq!(
                resolved_index, language_index,
                "{} extension {} resolved to the wrong language index",
                language.name, extension
            );
            assert_eq!(
                resolved_language.name, language.name,
                "{} extension {} resolved to {}",
                language.name, extension, resolved_language.name
            );
        }

        for exact_name in language.exact_names {
            let (resolved_index, resolved_language) =
                language_for_path(Path::new(exact_name)).unwrap_or_else(|| {
                    panic!(
                        "{} exact name {} did not resolve",
                        language.name, exact_name
                    )
                });

            assert_eq!(
                resolved_index, language_index,
                "{} exact name {} resolved to the wrong language index",
                language.name, exact_name
            );
            assert_eq!(
                resolved_language.name, language.name,
                "{} exact name {} resolved to {}",
                language.name, exact_name, resolved_language.name
            );
        }
    }
}
