//! TypeScript and JavaScript module resolution.
//!
//! One plugin serves both languages: only the grammar differs, and resolution is
//! identical because a specifier means the same thing in each.
//!
//! Node resolves a directory through its `package.json` `main` field before
//! falling back to `index`, and a relative specifier is resolved against the
//! package root rather than the filesystem — so `require('../..')` from
//! `examples/auth/index.js` lands on the root, not above it. Left unclamped that
//! produced `..` and then `/index.js`, a path no project has, and 46 `../`, 22
//! `..` and 12 `../..` imports on express's own examples were reported
//! unresolved.

mod extract;

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedSource, ImportPlugin, ResolveContext, ResolverPlugin, paths,
    walk::walk_with_declarations,
};

/// Suffixes tried in order when a specifier names a file without an extension.
///
/// An empty suffix comes first so an explicit extension in the specifier wins
/// over a guess.
const EXTENSION_CANDIDATES: &[&str] =
    &["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.js"];

/// JavaScript or TypeScript import extraction and resolution.
#[derive(Debug, Clone, Copy)]
pub struct JavaScriptPlugin {
    /// Selects the grammar used during extraction.
    pub language: SupportedLanguage,
}

impl ImportPlugin for JavaScriptPlugin {
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

    fn extract_source(&self, source: &[u8]) -> Option<ExtractedSource> {
        walk_with_declarations(source, ImportPlugin::language(self), self).ok()
    }
}

impl ResolverPlugin for JavaScriptPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        resolve_import(import_path, context)
    }
}

/// Resolve one specifier to a file in the analysis.
fn resolve_import(
    import_path: &str,
    context: ResolveContext<'_>,
) -> Option<String> {
    // Bare specifiers (`react`, `lodash/fp`) name packages, not project files.
    // Only `./` and `../` can be resolved locally.
    if !import_path.starts_with('.') {
        return None;
    }

    let base = paths::parent(context.source_file);
    let escaped = paths::resolve_relative(&base, import_path);
    let resolved = clamp_to_root(&escaped);

    if let Some(entry) = package_entry_point(&context, &resolved) {
        return Some(entry);
    }

    EXTENSION_CANDIDATES
        .iter()
        .map(|suffix| join_candidate(&resolved, suffix))
        .find(|candidate| context.contains(candidate))
}

/// The file a directory's `package.json` points at, if it declares one.
///
/// `main` is relative to the directory holding the manifest. A missing manifest,
/// an unreadable one, or a `main` that names nothing in the analysis all mean
/// "no entry point to add" rather than an error: Node falls back to `index.js`,
/// and so does the candidate walk above.
fn package_entry_point(
    context: &ResolveContext<'_>,
    directory: &str,
) -> Option<String> {
    // Read the manifest rather than checking the known-files set first: that set
    // holds source files, and a manifest is not one. A missing file is the
    // common case and the read is what discovers it.
    let contents = std::fs::read_to_string(
        context.base_path.join(directory).join("package.json"),
    )
    .ok()?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let main = value.get("main")?.as_str()?.trim();
    if main.is_empty() {
        return None;
    }

    let entry = join_candidate(directory, &format!("/{main}"));
    context.contains(&entry).then_some(entry)
}

/// Clamp a relative path that would leave the project to the project root.
///
/// `resolve_relative` keeps leading `..` segments so a caller can notice an
/// escape attempt, which is right for a user-supplied path and wrong here.
fn clamp_to_root(resolved: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in resolved.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Join a resolved directory and a candidate suffix into one path.
///
/// The empty directory is the case that matters. A specifier like
/// `require('../..')` from `examples/auth/index.js` resolves to the project
/// root, which as a relative path is the empty string, and appending `/index.js`
/// to it produced `/index.js` — a path no project has.
fn join_candidate(directory: &str, suffix: &str) -> String {
    if directory.is_empty() {
        suffix.trim_start_matches('/').to_owned()
    } else {
        format!("{directory}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn resolve(
        import_path: &str,
        source_file: &str,
        files: &[&str],
    ) -> Option<String> {
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context =
            crate::core::graph::plugins::test_context(source_file, &known);
        JavaScriptPlugin {
            language: SupportedLanguage::TypeScript,
        }
        .resolve(import_path, context)
    }

    #[test]
    fn resolves_sibling_with_explicit_extension() {
        let files = ["src/a.ts", "src/b.ts"];

        assert_eq!(
            resolve("./b", "src/a.ts", &files),
            Some("src/b.ts".to_owned())
        );
    }

    #[test]
    fn resolves_extensionless_shorthand() {
        let files = ["src/a.ts", "src/b.tsx"];

        assert_eq!(
            resolve("./b", "src/a.ts", &files),
            Some("src/b.tsx".to_owned())
        );
    }

    #[test]
    fn resolves_parent_directory_import() {
        let files = ["src/lib/util.ts", "src/app/main.ts"];

        assert_eq!(
            resolve("../lib/util", "src/app/main.ts", &files),
            Some("src/lib/util.ts".to_owned())
        );
    }

    #[test]
    fn resolves_directory_index() {
        let files = ["src/a.ts", "src/widget/index.ts"];

        assert_eq!(
            resolve("./widget", "src/a.ts", &files),
            Some("src/widget/index.ts".to_owned())
        );
    }

    #[test]
    fn a_package_root_import_resolves_to_its_entry_point() {
        // `require('../..')` from `examples/auth/index.js` lands on the project
        // root, which as a relative path is the empty string. Appending
        // `/index.js` to that produced `/index.js`, a path no project has, and
        // left every package-root import unresolved -- 46 `../`, 22 `..` and 12
        // `../..` on express's own examples.
        let files = ["index.js", "lib/express.js", "examples/auth/index.js"];

        assert_eq!(
            resolve("../..", "examples/auth/index.js", &files),
            Some("index.js".to_owned())
        );
        assert_eq!(
            resolve("../../../../..", "examples/auth/index.js", &files),
            Some("index.js".to_owned()),
            "Node clamps at the package root rather than escaping it"
        );
    }

    #[test]
    fn a_package_json_main_field_wins_over_the_index_fallback() {
        // Node reads `main` before falling back to `index.js`. A package whose
        // entry point has another name is the only way two packages sharing a
        // directory can be told apart.
        let directory = tempfile::tempdir().expect("a writable directory");
        std::fs::write(
            directory.path().join("package.json"),
            r#"{"name":"demo","main":"lib/entry.js"}"#,
        )
        .expect("the manifest is written");

        let known: BTreeSet<String> = ["lib/entry.js", "index.js"]
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
        let context = ResolveContext {
            source_file: "app/main.js",
            known_files: &known,
            module_depth: 0,
            kind: crate::core::graph::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: directory.path(),
        };

        let plugin = JavaScriptPlugin {
            language: SupportedLanguage::JavaScript,
        };
        assert_eq!(
            plugin.resolve("../..", context),
            Some("lib/entry.js".to_owned())
        );
    }

    #[test]
    fn a_missing_or_broken_manifest_falls_back_to_the_index() {
        // A missing manifest, invalid JSON, or a `main` naming nothing are all
        // "no entry point to add", not errors: Node falls back to `index.js`.
        let known: BTreeSet<String> =
            std::iter::once("index.js".to_owned()).collect();
        let empty = tempfile::tempdir().expect("a writable directory");
        let context = ResolveContext {
            source_file: "app/main.js",
            known_files: &known,
            module_depth: 0,
            kind: crate::core::graph::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: empty.path(),
        };
        let plugin = JavaScriptPlugin {
            language: SupportedLanguage::JavaScript,
        };
        assert_eq!(
            plugin.resolve("../..", context),
            Some("index.js".to_owned())
        );

        std::fs::write(empty.path().join("package.json"), "{not json")
            .expect("the broken manifest is written");
        let broken: BTreeSet<String> =
            std::iter::once("index.js".to_owned()).collect();
        let context = ResolveContext {
            source_file: "app/main.js",
            known_files: &broken,
            module_depth: 0,
            kind: crate::core::graph::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: empty.path(),
        };
        assert_eq!(
            plugin.resolve("../..", context),
            Some("index.js".to_owned()),
            "an unreadable manifest must not stop the fallback"
        );
    }

    #[test]
    fn bare_specifier_is_external() {
        let files = ["src/a.ts", "node_modules/react/index.js"];

        assert_eq!(resolve("react", "src/a.ts", &files), None);
        assert_eq!(resolve("lodash/fp", "src/a.ts", &files), None);
    }

    #[test]
    fn explicit_relative_path_with_extension_wins() {
        let files = ["src/a.ts", "src/b.ts", "src/b.js"];

        assert_eq!(
            resolve("./b.ts", "src/a.ts", &files),
            Some("src/b.ts".to_owned())
        );
    }
}
