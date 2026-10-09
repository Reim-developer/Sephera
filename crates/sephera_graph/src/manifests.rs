//! of 2615 unresolved edges were workspace members.
//!
//! This module answers three questions about one import path: is the first
//! segment a crate in this project, is it something the standard library
//! provides, or is it a package from elsewhere. Where a manifest records a
//! version, that version is carried through, because "which dependency do I need
//! to bump" is the question a blast radius exists to answer.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// One manifest reader, so the readers can be listed as a table.
type ReadManifest = fn(&mut ManifestIndex, &str);

/// Which language's standard library applies to an import.
///
/// Needed because the languages share names. `http` is a Node builtin module and
/// a Rust crate of the same name; calling it a builtin everywhere reported
/// axum's 191 references to the `http` crate as standard library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ecosystem {
    Rust,
    Python,
    Node,
    Go,
    /// A language with no builtin list here, such as Java or C.
    Other,
}

impl Ecosystem {
    /// The ecosystem an import written in `language` belongs to.
    #[must_use]
    pub const fn of(
        language: sephera_compression::SupportedLanguage,
    ) -> Self {
        use sephera_compression::SupportedLanguage as Language;
        match language {
            Language::Rust => Self::Rust,
            Language::Python => Self::Python,
            Language::JavaScript | Language::TypeScript => Self::Node,
            Language::Go => Self::Go,
            Language::Java | Language::C | Language::Cpp => Self::Other,
        }
    }
}

/// What an unresolved import path turned out to name.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// A path the project defines itself: `crate::`, `self::`, `super::`, or a
    /// relative path. Its absence from the graph is a resolver gap.
    Local,

    /// Provided by the language's standard library, with no version to manage.
    Builtin,

    /// A package named by a manifest, so a version is known.
    Declared,

    /// Not named by any manifest this reader understands. The name is still
    /// reported; only the version is missing.
    Undeclared,
}

/// A package an unresolved import refers to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Dependency {
    /// Package or crate name, taken from the first path segment.
    pub name: String,
    /// What kind of reference this is.
    pub kind: DependencyKind,
    /// Version from a manifest or lockfile, when one was found.
    pub version: Option<String>,
    /// How many import paths name this package.
    pub edge_count: u64,
}

/// Everything the manifests of one project say about its dependencies.
#[derive(Debug, Clone, Default)]
pub struct ManifestIndex {
    /// Directory the manifests were read from.
    ///
    /// Held here so workspace globs such as `axum-*` can be expanded against the
    /// repository rather than the process working directory.
    base_path: std::path::PathBuf,
    /// Crates and packages defined inside this repository.
    local_names: BTreeSet<String>,
    /// Declared third-party packages, mapped to a version when known.
    declared: BTreeMap<String, Option<String>>,
    /// Names provided without a manifest, which only the standard library has.
    ///
    /// Keyed by ecosystem because the lists collide: `http`, `url`, `os` and
    /// `path` are all Node builtins and all common crate names.
    builtin_names: BTreeMap<Ecosystem, BTreeSet<String>>,
    /// Go's module path from `go.mod`, which maps to the project root directory.
    module_path: Option<String>,
    /// Go `replace` directives from `go.mod`, mapping a module path to its
    /// replacement path (relative to the go.mod directory). Used by the Go
    /// resolver to redirect imports to local paths.
    go_replaces: BTreeMap<String, String>,
}

impl ManifestIndex {
    /// Read whatever manifests exist under `base_path`.
    ///
    /// Missing or unreadable manifests are not errors. A project with no
    /// lockfile still gets local crate names from its workspace declaration, and
    /// a project with no manifests at all still gets standard library handling.
    #[must_use]
    pub fn discover(base_path: &Path) -> Self {
        let mut index = Self {
            base_path: base_path.to_path_buf(),
            ..Self::default()
        };

        for (file, read) in [
            ("Cargo.toml", Self::read_cargo_toml as ReadManifest),
            ("package.json", Self::read_package_json),
            ("go.mod", Self::read_go_mod),
            ("requirements.txt", Self::read_requirements_txt),
        ] {
            let path = base_path.join(file);
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            read(&mut index, &contents);
        }

        index.read_cargo_lock(base_path);
        index.add_builtins();
        index
    }

    /// Build an index directly, for tests and for callers that already have the
    /// file contents.
    #[must_use]
    pub fn from_parts(
        local_names: impl IntoIterator<Item = String>,
        declared: impl IntoIterator<Item = (String, Option<String>)>,
    ) -> Self {
        let mut index = Self {
            local_names: local_names
                .into_iter()
                .map(|name| normalise(&name))
                .collect(),
            declared: declared
                .into_iter()
                .map(|(name, version)| (normalise(&name), version))
                .collect(),
            builtin_names: BTreeMap::new(),
            module_path: None,
            go_replaces: BTreeMap::new(),
            base_path: std::path::PathBuf::new(),
        };
        index.add_builtins();
        index
    }

    /// Whether `name` is this ecosystem's standard library.
    ///
    /// Scoped to the ecosystem on purpose. `http` is a Node builtin module and
    /// the most widely used Rust web crate, and one shared list reported axum's
    /// 191 references to the crate as standard library.
    #[must_use]
    pub fn is_builtin(&self, name: &str, ecosystem: Ecosystem) -> bool {
        self.builtin_names
            .get(&ecosystem)
            .is_some_and(|names| names.contains(name))
    }

    /// Classify one import path and return the package name it names.
    ///
    /// Returns `None` for a local path, since there is no package to report.
    #[must_use]
    pub fn attribute(
        &self,
        import_path: &str,
        ecosystem: Ecosystem,
    ) -> Option<(String, DependencyKind, Option<String>)> {
        if is_local_path(import_path) {
            return None;
        }

        // A path cannot be split into a package name without knowing the
        // ecosystem: Python separates with `.` and Go with `/`, and a Go module
        // path contains `.` in its domain. So every plausible prefix is tried and
        // the index decides which one names a real package.
        for candidate in package_candidates(import_path) {
            if self.is_builtin(&candidate, ecosystem) {
                return Some((candidate, DependencyKind::Builtin, None));
            }
            if self.local_names.contains(&candidate) {
                // A workspace member reached by name rather than by path:
                // internal, but the graph has no file for it, which is a
                // resolution limit rather than a dependency.
                return Some((candidate, DependencyKind::Local, None));
            }
            if let Some(version) = self.declared.get(&candidate) {
                return Some((
                    candidate,
                    DependencyKind::Declared,
                    version.clone(),
                ));
            }
        }

        // Nothing matched, so report the shortest plausible name. It is a guess
        // and `Undeclared` says so.
        let fallback = shortest_candidate(import_path);
        if fallback.is_empty() {
            return None;
        }
        Some((fallback, DependencyKind::Undeclared, None))
    }

    /// Fold a set of import paths into per-package counts, most used first.
    #[must_use]
    pub fn summarise<'a>(
        &self,
        import_paths: impl IntoIterator<Item = (&'a str, Ecosystem)>,
    ) -> Vec<Dependency> {
        let mut counts: BTreeMap<
            (String, DependencyKind),
            (Option<String>, u64),
        > = BTreeMap::new();

        for (path, ecosystem) in import_paths {
            let Some((name, kind, version)) = self.attribute(path, ecosystem)
            else {
                continue;
            };
            let entry = counts.entry((name, kind)).or_insert((version, 0));
            entry.1 = entry.1.saturating_add(1);
        }

        let mut dependencies: Vec<Dependency> = counts
            .into_iter()
            .map(|((name, kind), (version, edge_count))| Dependency {
                name,
                kind,
                version,
                edge_count,
            })
            .collect();

        // Most used first, then by name so the report is stable.
        dependencies.sort_by(|left, right| {
            right
                .edge_count
                .cmp(&left.edge_count)
                .then_with(|| left.name.cmp(&right.name))
        });
        dependencies
    }

    /// Crates and packages defined inside this repository.
    #[must_use]
    pub const fn local_names(&self) -> &BTreeSet<String> {
        &self.local_names
    }

    /// Go's module path, when a `go.mod` declared one.
    #[must_use]
    pub fn module_path(&self) -> Option<&str> {
        self.module_path.as_deref()
    }

    /// Record a Go module path directly, for tests and for callers that already
    /// have one.
    pub fn set_go_module_path(&mut self, module_path: &str) {
        self.module_path = Some(module_path.to_owned());
    }

    /// Whether an import names this project's own Go module root package.
    ///
    /// Go maps the module path to the directory holding `go.mod`, and the root
    /// package's files sit directly in it. An import of the bare module path is
    /// therefore a reference to that directory, which no directory-name match
    /// can find: `example.com/app` names package `app`, and there is no
    /// directory called `app`.
    ///
    /// Returns a flag rather than a path because the graph's file list holds
    /// paths relative to the project root, so the directory it names is the root
    /// itself.
    #[must_use]
    pub fn is_go_module_root(&self, import_path: &str) -> bool {
        self.module_path.as_deref() == Some(import_path)
    }

    /// Go `replace` directives from `go.mod`.
    ///
    /// Maps a module path prefix to its replacement path (relative to the
    /// go.mod directory). Used by the Go resolver to redirect imports to local
    /// paths.
    #[must_use]
    pub const fn go_replaces(&self) -> &BTreeMap<String, String> {
        &self.go_replaces
    }

    /// Mutable access to Go `replace` directives, for tests.
    #[allow(clippy::missing_const_for_fn)]
    pub fn go_replaces_mut(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.go_replaces
    }

    fn read_cargo_toml(&mut self, contents: &str) {
        // `toml::from_str` rather than `str::parse`: in toml 0.9 the latter
        // rejects a document, so every Cargo.toml read failed silently and no
        // workspace member was ever recognised as local.
        let Ok(value) = toml::from_str::<toml::Value>(contents) else {
            return;
        };

        // Workspace members, including globs such as `axum-*`.
        if let Some(members) = value
            .get("workspace")
            .and_then(|workspace| workspace.get("members"))
            .and_then(toml::Value::as_array)
        {
            for member in members.iter().filter_map(toml::Value::as_str) {
                self.add_workspace_member(member);
            }
        }

        // This package's own name, so a single-crate repository is recognised.
        if let Some(name) = value
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
        {
            self.local_names.insert(normalise(name));
        }

        for section in
            ["dependencies", "dev-dependencies", "build-dependencies"]
        {
            if let Some(table) =
                value.get(section).and_then(toml::Value::as_table)
            {
                for (name, _) in table {
                    self.declared.entry(normalise(name)).or_insert(None);
                }
            }
        }
    }

    /// Add one `members` entry, expanding a glob against what is on disk.
    ///
    /// `axum` declares `["axum", "axum-*"]`, so without this the sibling crates
    /// look like third-party packages -- which is exactly the confusion that made
    /// axum report 1018 workspace edges as external.
    fn add_workspace_member(&mut self, member: &str) {
        if !member.contains('*') {
            let name = Path::new(member).file_name().map_or_else(
                || member.to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            );
            self.local_names.insert(normalise(&name));
            return;
        }

        // `axum-*` means directories under the workspace root matching the glob.
        let Some((prefix, suffix)) = member.split_once('*') else {
            return;
        };
        let (prefix, suffix) = (prefix.trim_end_matches('/'), suffix);

        let Ok(entries) = std::fs::read_dir(self.root_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(prefix)
                && name.ends_with(suffix)
                && entry.path().join("Cargo.toml").is_file()
            {
                self.local_names.insert(normalise(&name));
            }
        }
    }

    /// Directory the manifests were read from, for glob expansion.
    ///
    /// Set by [`ManifestIndex::discover`]; empty otherwise, which makes glob
    /// expansion a no-op rather than a scan of the process working directory.
    fn root_dir(&self) -> &Path {
        &self.base_path
    }

    fn read_cargo_lock(&mut self, base_path: &Path) {
        let Ok(contents) =
            std::fs::read_to_string(base_path.join("Cargo.lock"))
        else {
            return;
        };
        let Ok(value) = toml::from_str::<toml::Value>(&contents) else {
            return;
        };
        let Some(packages) =
            value.get("package").and_then(toml::Value::as_array)
        else {
            return;
        };

        for package in packages {
            let Some(name) = package.get("name").and_then(toml::Value::as_str)
            else {
                continue;
            };
            let version = package
                .get("version")
                .and_then(toml::Value::as_str)
                .map(ToOwned::to_owned);
            let key = normalise(name);
            // A workspace member also appears in the lock file; keep it local.
            if self.local_names.contains(&key) {
                continue;
            }
            self.declared.insert(key, version);
        }
    }

    fn read_package_json(&mut self, contents: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(contents)
        else {
            return;
        };
        if let Some(name) =
            value.get("name").and_then(serde_json::Value::as_str)
        {
            self.local_names.insert(normalise(name));
        }
        for section in ["dependencies", "devDependencies", "peerDependencies"] {
            let Some(table) =
                value.get(section).and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            for (name, spec) in table {
                let key = normalise(name);
                let version = spec.as_str().map(|range| {
                    // Ranges are constraints, not versions; keep the raw text
                    // rather than inventing a resolved number.
                    range.to_owned()
                });
                self.declared.entry(key).or_insert(version);
            }
        }
    }

    fn read_go_mod(&mut self, contents: &str) {
        let mut in_require = false;
        let mut in_replace = false;
        for line in contents.lines() {
            let line = line.trim();
            // `module example.com/app` -- the path that maps to the project
            // root directory. Without it `import "example.com/app"` cannot
            // resolve, because Go names the root package by the module path
            // while the files sit at the top level rather than in a directory
            // called `app`.
            if let Some(rest) = line.strip_prefix("module ") {
                let name = rest.split_whitespace().next().unwrap_or("");
                if !name.is_empty() {
                    self.module_path = Some(name.to_owned());
                }
                continue;
            }
            if line.starts_with("require (") {
                in_require = true;
                continue;
            }
            if in_require && line == ")" {
                in_require = false;
                continue;
            }
            if line.starts_with("replace (") {
                in_replace = true;
                continue;
            }
            if in_replace && line == ")" {
                in_replace = false;
                continue;
            }
            // `replace example.com/foo => ./local/foo` or
            // `replace example.com/bar v1.2.3 => ./local/bar`
            if in_replace || line.starts_with("replace ") {
                let candidate = if in_replace {
                    line
                } else if let Some(rest) = line.strip_prefix("replace ") {
                    rest
                } else {
                    continue;
                };
                // Split on `=>` to get module and replacement
                if let Some((module, replacement)) = candidate.split_once("=>")
                {
                    let module = module.trim();
                    let replacement = replacement.trim();
                    // Module may have a version: `example.com/foo v1.2.3`
                    let module_name =
                        module.split_whitespace().next().unwrap_or("");
                    if !module_name.is_empty() && !replacement.is_empty() {
                        self.go_replaces
                            .entry(normalise(module_name))
                            .or_insert_with(|| replacement.to_owned());
                    }
                }
                continue;
            }
            let candidate = if in_require {
                line
            } else if let Some(rest) = line.strip_prefix("require ") {
                rest
            } else {
                continue;
            };
            let Some(name) = candidate.split_whitespace().next() else {
                continue;
            };
            let version =
                candidate.split_whitespace().nth(1).map(ToOwned::to_owned);
            self.declared.entry(normalise(name)).or_insert(version);
        }
    }

    fn read_requirements_txt(&mut self, contents: &str) {
        for line in contents.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('-') {
                continue;
            }
            let name = line
                .split(['=', '>', '<', '!', '[', ';'])
                .next()
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }
            self.declared.entry(normalise(name)).or_insert(None);
        }
    }

    /// Names the standard library provides, per ecosystem.
    ///
    /// Only lists that are short and stable are kept. Python's standard library
    /// is deliberately absent: it has over three hundred names and no single
    /// authoritative list to copy, and guessing would turn "undeclared" into a
    /// wrong answer for every undeclared third-party package too. A Python
    /// standard library module is therefore reported as undeclared, which is
    /// imprecise but not wrong.
    ///
    /// The lists are per ecosystem rather than global because the names collide.
    /// `http` is a Node builtin and also the most common Rust web crate, and one
    /// shared list reported axum's 191 references to the `http` crate as
    /// standard library.
    fn add_builtins(&mut self) {
        const RUST: &[&str] = &["std", "core", "alloc"];
        // Node's own modules, from `module.builtinModules`. An explicit
        // `node:` prefix needs no list because it is unambiguous.
        const NODE: &[&str] = &[
            "assert",
            "buffer",
            "child_process",
            "cluster",
            "console",
            "constants",
            "crypto",
            "dgram",
            "diagnostics_channel",
            "dns",
            "domain",
            "events",
            "fs",
            "http",
            "http2",
            "https",
            "module",
            "net",
            "os",
            "path",
            "perf_hooks",
            "process",
            "punycode",
            "querystring",
            "readline",
            "repl",
            "stream",
            "string_decoder",
            "sys",
            "timers",
            "tls",
            "trace_events",
            "tty",
            "url",
            "util",
        ];

        self.builtin_names
            .entry(Ecosystem::Rust)
            .or_default()
            .extend(RUST.iter().map(|name| (*name).to_owned()));
        self.builtin_names
            .entry(Ecosystem::Node)
            .or_default()
            .extend(NODE.iter().map(|name| (*name).to_owned()));
    }
}

/// A qualifier that names this project rather than a package.
const LOCAL_QUALIFIERS: [&str; 3] = ["crate", "self", "super"];

/// A path that names a file in this project rather than a package.
///
/// The bare qualifiers count too: `use super::*;` yields the path `super`, since
/// the star is a namespace rather than a segment.
fn is_local_path(import_path: &str) -> bool {
    // A leading `.` or `/` is a relative path.
    if import_path.starts_with('.') || import_path.starts_with('/') {
        return true;
    }

    // A qualifier, alone or followed by `::`. Comparing the first `::`-separated
    // segment rather than the whole prefix keeps `superstruct` a crate name
    // instead of reading it as the `super` keyword.
    let first = import_path
        .split_once("::")
        .map_or(import_path, |(head, _)| head);
    LOCAL_QUALIFIERS.contains(&first)
}

/// Every package name an import path could plausibly begin with, longest first.
///
/// A path cannot be split without knowing the ecosystem: Python separates with
/// `.`, Go with `/`, and a Go module path contains `.` in its domain. So each
/// prefix is offered in both spellings and the index picks whichever it
/// recognises, longest first so `github.com/x/y` wins over `github.com`.
fn package_candidates(import_path: &str) -> Vec<String> {
    // An explicit `node:` prefix names the whole module, builtins included, so
    // splitting on the colon would report every one of them as `node`.
    if let Some(module) = import_path.strip_prefix("node:") {
        return vec![normalise(module)];
    }

    let trimmed = import_path.trim_start_matches(['.', '/']);

    // Two families, because the separator that matters differs. Splitting on both
    // `.` and `/` at once would tear `github.com` into two segments and rebuild it
    // as `github/com`, which matches nothing. So one family keeps `.` inside a
    // name -- Python's dotted modules and a Go module's domain -- and the other
    // keeps `/` inside one -- a Go module path and an npm scope.
    let dotted: Vec<&str> =
        trimmed.split([':', '/']).filter(non_empty).collect();
    let slashed: Vec<&str> =
        trimmed.split([':', '.']).filter(non_empty).collect();

    let longest = dotted.len().max(slashed.len());
    let mut candidates = Vec::with_capacity(longest * 2);
    for take in (1..=longest).rev() {
        if take <= dotted.len() {
            push_candidate(&mut candidates, &dotted[..take].join("/"));
        }
        if take <= slashed.len() {
            push_candidate(&mut candidates, &slashed[..take].join("."));
        }
    }
    candidates
}

const fn non_empty(segment: &&str) -> bool {
    !segment.is_empty()
}

fn push_candidate(candidates: &mut Vec<String>, joined: &str) {
    let normalised = normalise(joined);
    if !normalised.is_empty() && !candidates.contains(&normalised) {
        candidates.push(normalised);
    }
}

/// The shortest plausible package name, used when the index knows none of them.
fn shortest_candidate(import_path: &str) -> String {
    package_candidates(import_path).pop().unwrap_or_default()
}

/// Normalise a package or module name for comparison.
///
/// Case and separators are folded away, since `serde_json`, `serde-json` and
/// `serdeJson` name the same thing. A scope or host prefix is kept rather than
/// dropped: `@scope/pkg` and `github.com/x/y` are only unique with it.
fn normalise(name: &str) -> String {
    name.trim().to_lowercase().replace(['-', '_', ':'], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_member_is_not_a_third_party_package() {
        let index = ManifestIndex::from_parts(
            ["axum", "axum-core"].iter().map(ToString::to_string),
            [("serde".to_owned(), Some("1.0".to_owned()))],
        );

        assert_eq!(
            index.attribute("axum_core::body::Body", Ecosystem::Rust),
            Some(("axumcore".to_owned(), DependencyKind::Local, None))
        );
    }

    #[test]
    fn a_declared_package_carries_its_version() {
        let index = ManifestIndex::from_parts(
            Vec::<String>::new(),
            [("anyhow".to_owned(), Some("1.0.102".to_owned()))],
        );

        assert_eq!(
            index.attribute("anyhow::Result", Ecosystem::Rust),
            Some((
                "anyhow".to_owned(),
                DependencyKind::Declared,
                Some("1.0.102".to_owned())
            ))
        );
    }

    #[test]
    fn an_undeclared_package_is_named_without_a_version() {
        let index = ManifestIndex::from_parts(Vec::<String>::new(), []);

        assert_eq!(
            index.attribute("werkzeug.utils", Ecosystem::Rust),
            Some(("werkzeug".to_owned(), DependencyKind::Undeclared, None))
        );
    }

    #[test]
    fn the_standard_library_is_recognised_without_a_manifest() {
        let index = ManifestIndex::from_parts(Vec::<String>::new(), []);

        assert_eq!(
            index.attribute("std::collections::BTreeMap", Ecosystem::Rust),
            Some(("std".to_owned(), DependencyKind::Builtin, None))
        );
        assert_eq!(
            index.attribute("core::fmt", Ecosystem::Rust),
            Some(("core".to_owned(), DependencyKind::Builtin, None))
        );
    }

    #[test]
    fn node_builtin_modules_are_recognised() {
        let index = ManifestIndex::from_parts(Vec::<String>::new(), []);

        assert_eq!(
            index.attribute("node:fs", Ecosystem::Node),
            index.attribute("fs", Ecosystem::Node),
            "the `node:` prefix is a spelling, so both name the same package"
        );
        assert_eq!(
            index.attribute("path", Ecosystem::Node),
            Some(("path".to_owned(), DependencyKind::Builtin, None))
        );
    }

    #[test]
    fn a_builtin_name_in_another_ecosystem_is_not_a_builtin() {
        // `http`, `url`, `os` and `path` are Node builtins and all common crate
        // names. One shared list reported axum's 191 references to the `http`
        // crate as standard library, which is the opposite of the truth.
        let index = ManifestIndex::from_parts(
            Vec::<String>::new(),
            [("http".to_owned(), Some("1.0".to_owned()))],
        );

        assert_eq!(
            index.attribute("http::Request", Ecosystem::Rust),
            Some((
                "http".to_owned(),
                DependencyKind::Declared,
                Some("1.0".to_owned())
            )),
            "a Rust file importing the http crate is importing the crate"
        );
        assert_eq!(
            index.attribute("path", Ecosystem::Rust),
            Some(("path".to_owned(), DependencyKind::Undeclared, None)),
            "`path` is a Node builtin, not a Rust one"
        );
    }

    #[test]
    fn a_local_path_names_no_package() {
        let index = ManifestIndex::from_parts(Vec::<String>::new(), []);

        for path in [
            "sephera_core::graph",
            "self::types",
            "super::x",
            "./util",
            "../..",
            // A bare qualifier comes from `use super::*;`, where the star is a
            // namespace rather than a path segment.
            "super",
            "crate",
            "self",
        ] {
            assert_eq!(
                index.attribute(path, Ecosystem::Rust),
                None,
                "{path} is this project's own code, not a package"
            );
        }
    }

    #[test]
    fn a_package_whose_name_starts_like_a_keyword_is_not_local() {
        let index = ManifestIndex::from_parts(Vec::<String>::new(), []);

        assert_eq!(
            index.attribute("superstruct::Thing", Ecosystem::Rust),
            Some(("superstruct".to_owned(), DependencyKind::Undeclared, None)),
            "`superstruct` is a crate, not the `super` keyword"
        );
        assert_eq!(
            index.attribute("selfie", Ecosystem::Rust),
            Some(("selfie".to_owned(), DependencyKind::Undeclared, None))
        );
    }

    #[test]
    fn hyphenated_and_underscored_spellings_match_one_package() {
        let index = ManifestIndex::from_parts(
            Vec::<String>::new(),
            [("serde_json".to_owned(), Some("1.0".to_owned()))],
        );

        assert_eq!(
            index.attribute("serde_json::Value", Ecosystem::Rust),
            index.attribute("serde-json/Value", Ecosystem::Rust),
            "separator style must not create two packages"
        );
    }

    #[test]
    fn cargo_workspace_members_and_globs_are_local() {
        let contents = r#"
[workspace]
members = ["crates/core", "crates/tools-*"]

[package]
name = "demo"

[dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
"#;
        let mut index = ManifestIndex::default();
        ManifestIndex::read_cargo_toml(&mut index, contents);

        assert!(index.local_names().contains("core"));
        assert!(index.local_names().contains("demo"));
        assert!(index.declared.contains_key("anyhow"));
        assert!(index.declared.contains_key("serde"));
        assert!(
            !index.declared.contains_key("core"),
            "a member must not also be recorded as a dependency"
        );
    }

    #[test]
    fn a_workspace_member_in_the_lock_file_stays_local() {
        let mut index = ManifestIndex::from_parts(["demo".to_owned()], []);
        index.read_cargo_lock(Path::new("."));
        // No lock file in the test working directory, so this must be a no-op
        // rather than a panic.
        assert!(index.local_names().contains("demo"));
    }

    #[test]
    fn go_mod_requires_are_read_in_both_forms() {
        let contents = "module example.com/app\n\nrequire (\n\tgithub.com/x/y v1.2.3\n\tgithub.com/a/b v0.1.0 // indirect\n)\n\nrequire github.com/single/s v2.0.0\n";
        let mut index = ManifestIndex::default();
        ManifestIndex::read_go_mod(&mut index, contents);

        assert_eq!(
            index.attribute("github.com/x/y/pkg", Ecosystem::Rust),
            Some((
                "github.com/x/y".to_owned(),
                DependencyKind::Declared,
                Some("v1.2.3".to_owned())
            )),
            "the declared module path must win over its shorter prefixes"
        );
        assert_eq!(
            index.attribute("github.com/single/s", Ecosystem::Rust),
            Some((
                "github.com/single/s".to_owned(),
                DependencyKind::Declared,
                Some("v2.0.0".to_owned())
            ))
        );
    }

    #[test]
    fn go_mod_replaces_are_parsed() {
        let contents = "module example.com/app\n\nreplace (\n\tgithub.com/x/y => ./local/x\n\tgithub.com/a/b v1.2.3 => ./local/a\n)\n\nreplace github.com/single/s => ./local/s\n";
        let mut index = ManifestIndex::default();
        ManifestIndex::read_go_mod(&mut index, contents);

        assert_eq!(
            index.go_replaces().get("github.com/x/y"),
            Some(&"./local/x".to_owned())
        );
        assert_eq!(
            index.go_replaces().get("github.com/a/b"),
            Some(&"./local/a".to_owned())
        );
        assert_eq!(
            index.go_replaces().get("github.com/single/s"),
            Some(&"./local/s".to_owned())
        );
        assert_eq!(index.go_replaces().len(), 3);
    }

    #[test]
    fn package_json_dependencies_are_read() {
        let contents = r#"{"name":"demo","dependencies":{"left-pad":"^1.3.0"},"devDependencies":{"vitest":"~1.0"}}"#;
        let mut index = ManifestIndex::default();
        ManifestIndex::read_package_json(&mut index, contents);

        assert!(index.local_names().contains("demo"));
        assert_eq!(
            index.attribute("left-pad", Ecosystem::Rust),
            Some((
                "leftpad".to_owned(),
                DependencyKind::Declared,
                Some("^1.3.0".to_owned())
            ))
        );
    }

    #[test]
    fn requirements_txt_ignores_comments_and_pins() {
        let contents =
            "# comment\n\nflask==3.0.0\nrequests>=2.31\n-r other.txt\n-e .\n";
        let mut index = ManifestIndex::default();
        ManifestIndex::read_requirements_txt(&mut index, contents);

        assert_eq!(
            index.attribute("flask", Ecosystem::Rust),
            Some(("flask".to_owned(), DependencyKind::Declared, None))
        );
        assert!(index.declared.contains_key("requests"));
        assert!(
            !index.declared.contains_key("other.txt"),
            "an include line names no package"
        );
    }

    #[test]
    fn summarise_counts_paths_per_package_most_used_first() {
        let index = ManifestIndex::from_parts(
            Vec::<String>::new(),
            [("anyhow".to_owned(), Some("1".to_owned()))],
        );

        let summary = index.summarise([
            ("std::io", Ecosystem::Rust),
            ("std::fs", Ecosystem::Rust),
            ("anyhow::Result", Ecosystem::Rust),
            ("anyhow::Context", Ecosystem::Rust),
            ("anyhow::anyhow", Ecosystem::Rust),
            ("sephera_core::graph", Ecosystem::Rust),
        ]);

        assert_eq!(
            summary[0].name, "anyhow",
            "the most used package comes first"
        );
        assert_eq!(summary[0].edge_count, 3);
        assert_eq!(summary[1].name, "std");
        assert_eq!(summary[1].edge_count, 2);
        assert_eq!(summary.len(), 2, "the local path names no package");
    }

    #[test]
    fn a_malformed_manifest_leaves_the_index_usable() {
        let mut index = ManifestIndex::default();
        ManifestIndex::read_cargo_toml(&mut index, "this is not toml = = =");
        ManifestIndex::read_package_json(&mut index, "{not json");
        ManifestIndex::read_go_mod(&mut index, "");
        ManifestIndex::read_requirements_txt(&mut index, "");

        assert!(index.local_names().is_empty());
        assert!(index.declared.is_empty());
    }
}
