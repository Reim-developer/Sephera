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
//!
//! A bare specifier is a package name -- except the ones the project's
//! tsconfig maps into the tree with `paths`. Monorepos import themselves by
//! name (`@vue/shared` is `packages/shared/src`), and those are local
//! couplings in a package's clothes: vue's tree carries 425 of them, and
//! filing them as external dependencies erased the graph that holds the
//! monorepo together.

use sephera_compression::SupportedLanguage;

use sephera_core::{
    path_utils as paths,
    plugins::{ImportPlugin, ResolveContext, ResolverPlugin},
};

/// Suffixes tried in order when a specifier names a file without an extension.
///
/// An empty suffix comes first so an explicit extension in the specifier wins
/// over a guess. The order after it is TypeScript's own: a `.ts` source beats
/// its `.d.ts` shadow, which beats the compiled `.js` beside both -- and a
/// declaration file is a real target, because a `./utils` that only exists as
/// `utils.d.ts` is an import of a file this project holds, not a gap.
const EXTENSION_CANDIDATES: &[&str] = &[
    "",
    ".ts",
    ".tsx",
    ".d.ts",
    ".mts",
    ".cts",
    ".js",
    ".jsx",
    ".mjs",
    ".cjs",
    ".vue",
    "/index.ts",
    "/index.tsx",
    "/index.d.ts",
    "/index.js",
    "/index.jsx",
    "/index.mjs",
    "/index.cjs",
];

/// JavaScript or TypeScript import extraction and resolution.
#[derive(Debug, Clone, Copy)]
pub struct JavaScriptPlugin {
    /// Selects the grammar used during extraction.
    pub language: SupportedLanguage,
}

impl ImportPlugin for JavaScriptPlugin {
    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<sephera_core::types::ImportStatement>> {
        crate::extract::extract_from_node(source, node)
    }
}

impl ResolverPlugin for JavaScriptPlugin {
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
    // A bundler query (`./data?raw`, `./worker?worker`) is an instruction to
    // the loader, not part of the path; the file it names is.
    let import_path = import_path
        .find(['?', '#'])
        .map_or(import_path, |index| &import_path[..index]);

    // Bare specifiers (`react`, `lodash/fp`) name packages, not project
    // files -- except the ones the project's own tsconfig maps into the
    // tree, which are local imports wearing a package's clothes.
    if !import_path.starts_with('.') {
        return resolve_alias(import_path, &context);
    }

    let base = paths::parent(context.source_file);
    let escaped = paths::resolve_relative(&base, import_path);
    let resolved = clamp_to_root(&escaped);

    resolve_path_base(&context, &resolved)
}

/// Try a resolved path as a file, then as a directory with an entry point.
fn resolve_path_base(
    context: &ResolveContext<'_>,
    resolved: &str,
) -> Option<String> {
    if let Some(entry) = package_entry_point(context, resolved) {
        return Some(entry);
    }

    EXTENSION_CANDIDATES
        .iter()
        .map(|suffix| join_candidate(resolved, suffix))
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
    // common case and the read is what discovers it. The read goes through
    // the analysis' parsed cache: a tree asks this question per directory import,
    // and the manifest's answer cannot change mid-analysis.
    let manifest = join_candidate(directory, "/package.json");
    let contents = context.read_json_project_file(&manifest)?;
    let main = contents.get("main")?.as_str()?.trim();
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

/// Resolve a bare specifier through the project's tsconfig `paths`.
///
/// A specifier no mapping claims stays external: `react` is still not a
/// project file, and this returns `None` for it exactly as before.
fn resolve_alias(
    import_path: &str,
    context: &ResolveContext<'_>,
) -> Option<String> {
    TsconfigPaths::load(context)?.resolve(import_path, context)
}

/// The `paths` mappings of the tsconfig that governs a source file.
///
/// Selected the way TypeScript selects a project: the nearest
/// `tsconfig.json` at or above the source file, then its `extends` chain,
/// with the first `paths` block found along the way. Targets hang from the
/// directory of the config that declares them, shifted by the chain's
/// `baseUrl` when one is declared -- which is where TypeScript resolves
/// them from.
/// One config's `paths` block: `(pattern, targets)` pairs, where a
/// pattern carries at most one `*`.
type PathMappings = Vec<(String, Vec<String>)>;

struct TsconfigPaths {
    /// Directory the targets hang from, relative to the analysis root.
    origin: String,
    /// The mappings of the config that declared them.
    mappings: PathMappings,
}

impl TsconfigPaths {
    fn load(context: &ResolveContext<'_>) -> Option<Self> {
        let mut dir = paths::parent(context.source_file);
        loop {
            let config = join_candidate(&dir, "/tsconfig.json");
            if let Some(found) = Self::load_chain(context, &config) {
                return Some(found);
            }
            if dir.is_empty() {
                return None;
            }
            dir = paths::parent(&dir);
        }
    }

    /// Read one config and its `extends` chain, nearest first.
    fn load_chain(context: &ResolveContext<'_>, config: &str) -> Option<Self> {
        let mut chain: Vec<(String, std::sync::Arc<serde_json::Value>)> =
            Vec::new();
        let mut current = Some(config.to_owned());
        for _ in 0..8 {
            let Some(relative) = current.take() else {
                break;
            };
            // Through the analysis' parsed cache, as every project-file
            // read: a monorepo's bare imports all climb to the same
            // few tsconfigs, and re-parsing them per import was the
            // cost the PR #41 review flagged.
            let Some(value) = context.read_jsonc_project_file(&relative) else {
                break;
            };
            let dir = paths::parent(&relative);
            current = extends_target(&value, &dir);
            chain.push((dir, value));
        }
        if chain.is_empty() {
            return None;
        }

        let mut declared: Option<(&String, PathMappings)> = None;
        for (dir, value) in &chain {
            let Some(paths_object) = value
                .get("compilerOptions")
                .and_then(|options| options.get("paths"))
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            let mappings: Vec<(String, Vec<String>)> = paths_object
                .iter()
                .map(|(pattern, targets)| {
                    let targets: Vec<String> = targets
                        .as_array()
                        .map(|entries| {
                            entries
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    (pattern.clone(), targets)
                })
                .filter(|(_, targets)| !targets.is_empty())
                .collect();
            if !mappings.is_empty() {
                declared = Some((dir, mappings));
                break;
            }
        }
        let (declared_dir, mappings) = declared?;

        // `baseUrl`, wherever along the chain it is declared, is what the
        // targets are relative to; without one they hang from the config
        // that declared the paths themselves.
        let mut origin = declared_dir.clone();
        for (dir, value) in &chain {
            let Some(base_url) = value
                .get("compilerOptions")
                .and_then(|options| options.get("baseUrl"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            origin = clamp_to_root(&paths::resolve_relative(dir, base_url));
            break;
        }

        Some(Self { origin, mappings })
    }

    fn resolve(
        &self,
        import_path: &str,
        context: &ResolveContext<'_>,
    ) -> Option<String> {
        let mut targets: Vec<String> = Vec::new();

        // An exact pattern names one specifier outright and beats every
        // wildcard that would also match it: `@vue/compat` is not
        // `@vue/*` with `compat` captured, however the files are laid out.
        for (pattern, mapped) in &self.mappings {
            if !pattern.contains('*') && pattern == import_path {
                targets.extend(mapped.iter().cloned());
            }
        }

        // Among wildcards the longest prefix wins, which is TypeScript's
        // own rule for the same reason: it is the most specific claim.
        let mut wildcards: Vec<&(String, Vec<String>)> = self
            .mappings
            .iter()
            .filter(|(pattern, _)| pattern.contains('*'))
            .collect();
        wildcards.sort_by_key(|(pattern, _)| {
            std::cmp::Reverse(pattern.split('*').next().map_or(0, str::len))
        });
        for (pattern, mapped) in wildcards {
            let Some(captured) = match_pattern(pattern, import_path) else {
                continue;
            };
            targets.extend(
                mapped.iter().map(|target| target.replace('*', &captured)),
            );
        }

        targets.iter().find_map(|target| {
            let joined =
                clamp_to_root(&paths::resolve_relative(&self.origin, target));
            resolve_path_base(context, &joined)
        })
    }
}

/// The text a wildcard pattern captures from a specifier, if it matches.
///
/// A pattern carries at most one `*`; what stands between its prefix and
/// its suffix is the capture.
fn match_pattern(pattern: &str, specifier: &str) -> Option<String> {
    let (prefix, suffix) = pattern.split_once('*')?;
    if specifier.len() < prefix.len() + suffix.len()
        || !specifier.starts_with(prefix)
        || !specifier.ends_with(suffix)
    {
        return None;
    }
    Some(specifier[prefix.len()..specifier.len() - suffix.len()].to_owned())
}

/// The next config a tsconfig's `extends` names, as a path from the root.
///
/// Only relative extends are followed: a package name (`@vue/tsconfig`)
/// names a config outside the tree this analysis can see. A missing
/// `.json` suffix is supplied, as authors write both spellings.
fn extends_target(value: &serde_json::Value, dir: &str) -> Option<String> {
    let extends = value.get("extends")?;
    let raw = match extends {
        serde_json::Value::String(name) => name.clone(),
        serde_json::Value::Array(entries) => entries
            .iter()
            .find_map(serde_json::Value::as_str)
            .map(str::to_owned)?,
        _ => return None,
    };
    if !raw.starts_with('.') {
        return None;
    }
    let joined = clamp_to_root(&paths::resolve_relative(dir, &raw));
    Some(if joined.to_ascii_lowercase().ends_with(".json") {
        joined
    } else {
        format!("{joined}.json")
    })
}

/// Parse a tsconfig, which is JSON with opinions.
///
/// Real tsconfig files carry `//` and `/* */` comments and trailing
/// commas, and `serde_json` wants neither. Both are removed here, with
/// string contents left alone -- a `//` inside a quoted path is text,
/// not a comment.
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
        let context = sephera_core::plugins::test_context(source_file, &known);
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
            kind: sephera_core::types::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: directory.path(),
            file_text: None,
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
            kind: sephera_core::types::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: empty.path(),
            file_text: None,
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
            kind: sephera_core::types::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: empty.path(),
            file_text: None,
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

    #[test]
    fn a_declaration_file_is_a_target() {
        // dts-test suites import `./utils` where only `utils.d.ts`
        // exists. A declaration file is a file this project holds; the
        // candidate list never tried its suffix, so every such import
        // was a resolver gap.
        let files = ["src/a.ts", "src/utils.d.ts"];

        assert_eq!(
            resolve("./utils", "src/a.ts", &files),
            Some("src/utils.d.ts".to_owned())
        );
    }

    #[test]
    fn a_bundler_query_is_not_part_of_the_path() {
        let files = ["src/a.ts", "src/data.ts"];

        assert_eq!(
            resolve("./data?raw", "src/a.ts", &files),
            Some("src/data.ts".to_owned())
        );
    }

    /// Resolve with a real directory behind the context, as an analysis
    /// has.
    ///
    /// A tsconfig is read from disk, so a fixture of bare names -- what
    /// `test_context` builds -- can never present one. These write the
    /// config into a temporary root instead and point the context at it.
    fn resolve_in(
        directory: &std::path::Path,
        import_path: &str,
        source_file: &str,
        files: &[&str],
    ) -> Option<String> {
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = ResolveContext {
            source_file,
            known_files: &known,
            module_depth: 0,
            kind: sephera_core::types::ImportKind::Dependency,
            declarations: None,
            manifests: None,
            base_path: directory,
            file_text: None,
        };
        JavaScriptPlugin {
            language: SupportedLanguage::TypeScript,
        }
        .resolve(import_path, context)
    }

    fn write_file(directory: &std::path::Path, path: &str, contents: &str) {
        let full = directory.join(path);
        std::fs::create_dir_all(full.parent().expect("a parent"))
            .expect("the tree is written");
        std::fs::write(full, contents).expect("the file is written");
    }

    #[test]
    fn a_tsconfig_wildcard_alias_resolves_into_the_package_it_names() {
        // Vue's own shape: `@vue/*` is `./packages/*/src`, a directory
        // whose index is the package. Read as a package name, every such
        // import was an external dependency and the couplings between
        // the monorepo's own packages left the graph -- 425 of them on
        // vue's tree.
        let directory = tempfile::tempdir().expect("a writable directory");
        write_file(
            directory.path(),
            "tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": {
                        "@vue/*": ["./packages/*/src"],
                        "vue": ["./packages/vue/src"]
                    }
                }
            }"#,
        );
        let files = [
            "packages/shared/src/index.ts",
            "packages/vue/src/index.ts",
            "packages/runtime-core/src/index.ts",
        ];
        let source = "packages/runtime-core/src/index.ts";

        assert_eq!(
            resolve_in(directory.path(), "@vue/shared", source, &files),
            Some("packages/shared/src/index.ts".to_owned())
        );
        assert_eq!(
            resolve_in(directory.path(), "vue", source, &files),
            Some("packages/vue/src/index.ts".to_owned())
        );
        assert_eq!(
            resolve_in(directory.path(), "react", source, &files),
            None,
            "a specifier no mapping claims is still a package"
        );
    }

    #[test]
    fn an_exact_alias_beats_the_wildcard_that_would_also_match() {
        // `@vue/compat` lives in `packages/vue-compat`; the wildcard
        // alone would name `packages/compat`, which is not a place.
        let directory = tempfile::tempdir().expect("a writable directory");
        write_file(
            directory.path(),
            "tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": {
                        "@vue/compat": ["./packages/vue-compat/src"],
                        "@vue/*": ["./packages/*/src"]
                    }
                }
            }"#,
        );
        let files = [
            "packages/vue-compat/src/index.ts",
            "packages/runtime-core/src/index.ts",
        ];

        assert_eq!(
            resolve_in(
                directory.path(),
                "@vue/compat",
                "packages/runtime-core/src/index.ts",
                &files,
            ),
            Some("packages/vue-compat/src/index.ts".to_owned())
        );
    }

    #[test]
    fn alias_paths_are_found_through_an_extends_chain() {
        // A package config extends the root one, and the paths live at
        // the root. The targets are the root's, so they hang from the
        // root's directory, not the package's.
        let directory = tempfile::tempdir().expect("a writable directory");
        write_file(
            directory.path(),
            "tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": { "@lib/*": ["./src/lib/*"] }
                }
            }"#,
        );
        write_file(
            directory.path(),
            "packages/app/tsconfig.json",
            r#"{ "extends": "../../tsconfig.json" }"#,
        );
        let files = ["src/lib/util.ts", "packages/app/src/main.ts"];

        assert_eq!(
            resolve_in(
                directory.path(),
                "@lib/util",
                "packages/app/src/main.ts",
                &files,
            ),
            Some("src/lib/util.ts".to_owned())
        );
    }

    #[test]
    fn a_tsconfig_with_comments_and_trailing_commas_still_loads() {
        // Tsconfig is JSONC in the wild; a strict parser reads this as
        // an error and the mappings silently never happen.
        let directory = tempfile::tempdir().expect("a writable directory");
        write_file(
            directory.path(),
            "tsconfig.json",
            r#"{
                // where the shared code lives
                "compilerOptions": {
                    "paths": {
                        "@/*": ["./src/*"],
                    },
                },
            }"#,
        );
        let files = ["src/util.ts", "src/main.ts"];

        assert_eq!(
            resolve_in(directory.path(), "@/util", "src/main.ts", &files),
            Some("src/util.ts".to_owned())
        );
    }

    #[test]
    fn a_base_url_shifts_where_alias_targets_hang_from() {
        let directory = tempfile::tempdir().expect("a writable directory");
        write_file(
            directory.path(),
            "tsconfig.json",
            r#"{
                "compilerOptions": {
                    "baseUrl": "src",
                    "paths": { "@/*": ["./*"] }
                }
            }"#,
        );
        let files = ["src/lib/util.ts", "src/app/main.ts"];

        assert_eq!(
            resolve_in(
                directory.path(),
                "@/lib/util",
                "src/app/main.ts",
                &files,
            ),
            Some("src/lib/util.ts".to_owned())
        );
    }
}
