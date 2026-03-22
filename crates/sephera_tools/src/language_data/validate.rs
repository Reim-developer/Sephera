use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use super::model::{
    LEGACY_DOTFILE_EXACT_NAMES, LanguageRegistry, LanguageSpec,
    RawLanguageRegistry,
};

pub fn validate_registry(
    raw_registry: RawLanguageRegistry,
) -> Result<LanguageRegistry> {
    if raw_registry.comment_styles.is_empty() {
        bail!("at least one comment style is required");
    }
    if raw_registry.languages.is_empty() {
        bail!("at least one language is required");
    }

    let mut seen_language_names = BTreeSet::new();
    let mut seen_exact_names = BTreeMap::<String, String>::new();
    let mut seen_extensions = BTreeMap::<String, String>::new();
    let mut languages = Vec::with_capacity(raw_registry.languages.len());

    for raw_language in raw_registry.languages {
        let name = raw_language.name.trim();
        if name.is_empty() {
            bail!("language names must not be empty");
        }
        if !seen_language_names.insert(name.to_owned()) {
            bail!("duplicate language name `{name}`");
        }

        if !raw_registry
            .comment_styles
            .contains_key(raw_language.comment_styles.as_str())
        {
            bail!(
                "language `{name}` references unknown comment style `{}`",
                raw_language.comment_styles
            );
        }

        let mut exact_names = raw_language.exact_names;
        let mut extensions = Vec::new();

        for pattern in raw_language.patterns {
            if pattern.trim().is_empty() {
                bail!("language `{name}` contains an empty pattern");
            }

            if !pattern.starts_with('.') {
                exact_names.push(pattern);
                continue;
            }

            if LEGACY_DOTFILE_EXACT_NAMES.contains(&pattern.as_str()) {
                exact_names.push(pattern);
                continue;
            }

            extensions.push(pattern);
        }

        if exact_names.is_empty() && extensions.is_empty() {
            bail!("language `{name}` must define at least one match pattern");
        }

        exact_names.sort();
        exact_names.dedup();
        extensions.sort();
        extensions.dedup();

        for exact_name in &exact_names {
            if exact_name.trim().is_empty() {
                bail!("language `{name}` contains an empty exact filename");
            }

            if let Some(previous_language) =
                seen_exact_names.insert(exact_name.clone(), name.to_owned())
            {
                bail!(
                    "exact filename `{exact_name}` is defined by both `{previous_language}` and `{name}`"
                );
            }
        }

        for extension in &extensions {
            if !extension.starts_with('.') || extension.len() < 2 {
                bail!(
                    "language `{name}` contains invalid extension pattern `{extension}`"
                );
            }

            let extension_key = extension.trim_start_matches('.').to_owned();
            if let Some(previous_language) =
                seen_extensions.insert(extension_key.clone(), name.to_owned())
            {
                bail!(
                    "extension `{extension}` is defined by both `{previous_language}` and `{name}`"
                );
            }
        }

        languages.push(LanguageSpec {
            name: name.to_owned(),
            extensions,
            exact_names,
            comment_style_key: raw_language.comment_styles,
        });
    }

    Ok(LanguageRegistry {
        comment_styles: raw_registry.comment_styles,
        languages,
    })
}

pub fn validate_style_identifier_collisions(
    registry: &LanguageRegistry,
) -> Result<()> {
    let mut seen_identifiers = BTreeMap::<String, String>::new();

    for style_key in registry.comment_styles.keys() {
        let const_name = style_const_name(style_key)?;
        if let Some(previous_style) =
            seen_identifiers.insert(const_name.clone(), style_key.clone())
        {
            bail!(
                "comment style identifiers collide after sanitization: `{previous_style}` and `{style_key}` both map to `{const_name}`"
            );
        }
    }

    Ok(())
}

pub fn collect_exact_name_index(
    registry: &LanguageRegistry,
) -> Result<Vec<(&str, usize)>> {
    let mut entries = Vec::new();

    for (index, language) in registry.languages.iter().enumerate() {
        for exact_name in &language.exact_names {
            entries.push((exact_name.as_str(), index));
        }
    }

    entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
    validate_sorted_uniqueness(&entries, "exact filename")?;
    Ok(entries)
}

pub fn collect_extension_index(
    registry: &LanguageRegistry,
) -> Result<Vec<(&str, usize)>> {
    let mut entries = Vec::new();

    for (index, language) in registry.languages.iter().enumerate() {
        for extension in &language.extensions {
            entries.push((extension.trim_start_matches('.'), index));
        }
    }

    entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
    validate_sorted_uniqueness(&entries, "extension")?;
    Ok(entries)
}

fn validate_sorted_uniqueness(
    entries: &[(&str, usize)],
    label: &str,
) -> Result<()> {
    for pair in entries.windows(2) {
        let [(current_key, current_index), (next_key, next_index)] = pair
        else {
            continue;
        };

        if current_key == next_key && current_index != next_index {
            bail!("{label} `{current_key}` appears more than once");
        }
    }

    Ok(())
}

pub fn style_const_name(style_key: &str) -> Result<String> {
    let mut identifier = String::new();

    for character in style_key.chars() {
        if character.is_ascii_alphanumeric() {
            identifier.push(character.to_ascii_uppercase());
        } else {
            identifier.push('_');
        }
    }

    while identifier.contains("__") {
        identifier = identifier.replace("__", "_");
    }

    let identifier = identifier.trim_matches('_').to_owned();
    if identifier.is_empty() {
        bail!(
            "comment style `{style_key}` cannot be converted into a Rust identifier"
        );
    }

    if identifier
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        return Ok(format!("N_{identifier}"));
    }

    Ok(identifier)
}
