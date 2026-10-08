//! Dependency graph builder and analyzer.
//!
//! Given a project directory, this module collects all source files, extracts
//! their imports using Tree-sitter, resolves internal file references, and
//! builds a complete dependency graph with metrics.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;

use crate::core::{
    compression::SupportedLanguage,
    ignore::IgnoreMatcher,
    progress::{NoProgress, Progress},
    project_files::{ProjectFile, collect_project_files},
};

use super::{declarations, manifests, plugins};

use super::types::{
    FileMetric, GraphEdge, GraphMetrics, GraphNode, GraphQuery, GraphReport,
    ImportKind, ImportStatement, NodeMap,
};

/// Maximum file size in bytes to analyze for imports.
const MAX_IMPORT_FILE_BYTES: u64 = 512 * 1024;

/// How many unresolved local paths to list in the report.
///
/// Enough to see a pattern; few enough that a badly misparsed file cannot
/// flood the output.
const MAX_UNRESOLVED_LOCAL_SAMPLES: usize = 20;

/// Builds a dependency graph for the given project.
///
/// # Arguments
///
/// * `base_path` — root directory of the project.
/// * `ignore` — compiled ignore patterns.
/// * `focus_paths` — optional sub-paths to restrict analysis to.
/// * `depth` — maximum depth for transitive dependency resolution
///   (0 = direct only, `None` = unlimited).
///
/// # Errors
///
/// Import forms to leave out of the graph.
///
/// Some imports describe a name rather than a runtime dependency. A Rust
/// `use foo::Bar as Baz` alias or a wildcard import such as `use foo::*`
/// introduces a local name; neither says which file must be recompiled when
/// the target changes, so these edges add noise to a blast-radius answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EdgeFilters {
    /// Drop aliasing and wildcard imports.
    pub exclude_type_aliases: bool,
}

impl EdgeFilters {
    /// Whether an import should be dropped.
    ///
    /// Only these forms are filtered, and only when explicitly enabled: a
    /// plain `use path::to::thing` is a real dependency and is always kept.
    ///
    /// The alias and glob forms are recognised from the parse rather than by
    /// searching the path text, because the path no longer carries the `as`
    /// clause and a substring test would miss it.
    #[must_use]
    pub const fn rejects(&self, statement: &ImportStatement) -> bool {
        if !self.exclude_type_aliases {
            return false;
        }

        // `use foo::Bar as Baz`, `use foo::*` and `use foo::* as f` introduce a
        // local name without saying which file must be recompiled.
        statement.kind.is_renaming()
    }
}

/// Builds a dependency graph for a project.
///
/// # Errors
///
/// Returns an error when project traversal or import extraction fails.
pub fn build_graph(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    focus_paths: &[PathBuf],
    depth: Option<u32>,
    query: Option<GraphQuery>,
) -> Result<GraphReport> {
    build_graph_with(
        base_path,
        ignore,
        focus_paths,
        depth,
        query,
        EdgeFilters::default(),
    )
}

/// Build a graph, applying edge filters.
///
/// [`build_graph`] remains the plain entry point; this exists so callers can
/// drop imports that describe types rather than runtime coupling.
///
/// # Errors
///
/// Returns an error when project traversal or import extraction fails.
pub fn build_graph_with(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    focus_paths: &[PathBuf],
    depth: Option<u32>,
    query: Option<GraphQuery>,
    filters: EdgeFilters,
) -> Result<GraphReport> {
    build_graph_with_progress(
        base_path,
        ignore,
        focus_paths,
        depth,
        query,
        filters,
        &NoProgress,
    )
}

/// As [`build_graph_with`], reporting progress through the parse phase.
///
/// # Errors
///
/// Returns an error when project traversal or import extraction fails.
pub fn build_graph_with_progress(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    focus_paths: &[PathBuf],
    depth: Option<u32>,
    query: Option<GraphQuery>,
    filters: EdgeFilters,
    progress: &dyn Progress,
) -> Result<GraphReport> {
    let project_files = collect_project_files(base_path, ignore)?;
    let focus_set = build_focus_set(base_path, focus_paths);
    let query = query
        .map(|query| normalize_graph_query(base_path, query))
        .transpose()?;

    // Phase 1: Extract imports from all supported files, and what each declares.
    let (mut all_file_imports, declarations) =
        extract_all_imports(&project_files, progress)?;
    if filters != EdgeFilters::default() {
        for file in &mut all_file_imports {
            file.imports.retain(|statement| !filters.rejects(statement));
        }
    }

    // Phase 2: Build a lookup set of all known file paths for resolution.
    let known_files: BTreeSet<String> = project_files
        .iter()
        .map(|f| f.normalized_relative_path.clone())
        .collect();

    // Phase 3: Build the full graph once, then apply selection/filtering.
    //
    // The manifests are read once here and used twice: resolution needs Go's
    // module path, and the metrics attribute each edge to a package.
    let manifests = manifests::ManifestIndex::discover(base_path);
    let project = ResolutionInputs {
        base_path: base_path.to_path_buf(),
        known_files,
        declarations,
        manifests,
    };
    let (edges, node_map) = build_edges_and_nodes(&all_file_imports, &project);
    let selection = select_graph(&node_map, &focus_set, depth, query)?;
    let filtered_node_map = filter_node_map(&node_map, &selection.node_paths);
    let filtered_edges = filter_edges(&edges, &selection.node_paths);

    // Phase 4: Compute metrics.
    let metrics = compute_metrics(
        &project.manifests,
        &filtered_node_map,
        &filtered_edges,
    );

    // Phase 5: Build final nodes list.
    let nodes = build_node_list(&filtered_node_map);

    Ok(GraphReport {
        base_path: base_path.to_path_buf(),
        focus_paths: focus_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        depth: selection.applied_depth,
        query: selection.query,
        nodes,
        edges: filtered_edges,
        metrics,
    })
}

/// Collects import information from a single file.
struct FileImportData {
    file_path: String,
    language: Option<&'static str>,
    ts_language: SupportedLanguage,
    imports: Vec<ImportStatement>,
}

/// Extracts imports from all project files that have a supported language.
///
/// Also builds the declaration index, which resolution needs for paths that name
/// a declaration rather than a module. Both come from one read per file.
///
/// Across a thread pool, because the cost is one Tree-sitter parse per file and
/// the files are independent. Two things keep the result identical to the
/// sequential version it replaced: `collect` preserves input order, and the
/// declaration index is folded afterwards rather than being mutated from
/// several workers, so nothing depends on which thread finished first.
/// Extract every file's imports and declarations, reporting progress.
///
/// Progress is advanced over the whole collection rather than per file.
/// `par_iter` gives no guarantee about which file any given worker reaches
/// first, so advancing from inside the parallel map would report a count that is
/// correct and a set of files that is arbitrary -- fine for a percentage, and
/// wrong for anything that wants to name what is being worked on. Counting what
/// came back keeps the number honest on both counts.
fn extract_all_imports(
    project_files: &[ProjectFile],
    progress: &dyn Progress,
) -> Result<(Vec<FileImportData>, declarations::DeclarationIndex)> {
    progress.set_total(project_files.len() as u64);

    let extracted: Vec<ExtractedFile> = project_files
        .par_iter()
        .map(extract_one_file)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    progress.advance(project_files.len() as u64);

    let mut results = Vec::with_capacity(extracted.len());
    let mut declarations = declarations::DeclarationIndex::default();

    for file in extracted {
        if let Some((path, declared)) = file.declared {
            declarations.insert(&path, declared);
        }
        results.push(file.data);
    }

    Ok((results, declarations))
}

/// One file's imports, plus whatever it declares.
struct ExtractedFile {
    data: FileImportData,
    /// The file's path and the names it declares, kept beside the imports so
    /// the declaration index can be built in one ordered pass.
    declared: Option<(String, declarations::DeclaredNames)>,
}

/// Extracts from one file, or reports that it has nothing to extract.
///
/// `Ok(None)` covers every reason a file is skipped — unsupported language,
/// empty, too large — and is not an error. A read failure is, because it means
/// a file that should have been analysed was silently dropped otherwise.
fn extract_one_file(
    project_file: &ProjectFile,
) -> Result<Option<ExtractedFile>> {
    // Only an oversized file is skipped. A zero-byte file is not.
    //
    // It was skipped, and that dropped it from the graph entirely rather than
    // merely leaving it without edges, so an empty source was invisible in a way
    // that depended on whether some other file happened to declare it: a Rust
    // `mod empty;` kept an empty file as a node while an empty `.js`, `.h` or
    // `__init__.py` vanished. Four languages disagreed about what an empty file is,
    // and `total_files` therefore depended on the language of the file it was
    // counting.
    //
    // It matters most for Python, where an empty `__init__.py` is the thing that
    // makes a directory a package -- and this resolver already treats such a file as
    // a resolution target for `from . import X`. Counting it as a node and
    // resolving `from . import X` onto it were two answers to the same question and
    // they disagreed. On flask that is three files: every empty `__init__.py`
    // under `tests/`.
    if project_file.size_bytes > MAX_IMPORT_FILE_BYTES {
        return Ok(None);
    }

    let Some((_, language)) = project_file.language_match else {
        return Ok(None);
    };

    let Some(ts_language) =
        SupportedLanguage::from_language_name(language.name)
    else {
        return Ok(None);
    };

    let source =
        std::fs::read(&project_file.absolute_path).with_context(|| {
            format!(
                "failed to read `{}` for import extraction",
                project_file.absolute_path.display()
            )
        })?;

    // Bytes that are not valid UTF-8 yield no edges at all.
    //
    // Tree-sitter recovers rather than failing, so a file with a broken encoding
    // produced *something* -- and what it produced was a fragment. `use crate::`
    // followed by bytes that are not a name became a `crate` path, which the
    // module walk then resolved to the file it was written in: a self-dependency
    // out of bytes that are not a source file.
    //
    // Refusing to read the file is the honest answer. Every extractor here reads
    // node text as a string, so a reference recovered from undecodable bytes is
    // not a weaker claim than one from decodable bytes -- it is not a claim at
    // all. The file stays a node, which is what it is; it just contributes
    // nothing.
    if std::str::from_utf8(&source).is_err() {
        return Ok(Some(ExtractedFile {
            data: FileImportData {
                file_path: project_file.normalized_relative_path.clone(),
                language: Some(language.name),
                ts_language,
                imports: Vec::new(),
            },
            declared: None,
        }));
    }

    // Extraction goes through the language's plugin so that dispatch is uniform:
    // adding a language means adding one plugin file, not editing the extractor
    // and the resolver separately.
    //
    // One call, one parse. Asking the plugin for imports and then again for
    // declared names meant reading and parsing every Rust file twice, which is
    // what the declaration index cost before the plugin returned both halves
    // together.
    let mut extracted = plugins::builtin_import_plugin(ts_language)
        .and_then(|plugin| plugin.extract_source(&source))
        .unwrap_or_default();

    // Taken rather than cloned: the declaration map is the larger of the two
    // halves by far, and there is no reason to copy it to get it out of the
    // struct that is about to be dropped.
    let declared = extracted
        .declared
        .take()
        .map(|names| (project_file.normalized_relative_path.clone(), names));

    Ok(Some(ExtractedFile {
        data: FileImportData {
            file_path: project_file.normalized_relative_path.clone(),
            language: Some(language.name),
            ts_language,
            imports: extracted.imports,
        },
        declared,
    }))
}

/// Builds the set of focused normalized paths for filtering.
/// Normalise focus paths into the `/`-separated, base-relative spelling the
/// graph uses.
///
/// Public so a caller that filters the graph *after* it is built -- rather than
/// passing focus paths into graph selection -- produces the same set of scoped
/// paths.
///
/// Three rules, and the order matters:
///
/// * A **relative** focus path is spelled relative to the *analysis base*, so it
///   is never resolved against the working directory -- that would break
///   `--path crates/x --focus src/main.rs` outright. It only loses `.` components
///   and `x/..` pairs, neither of which the graph ever spells. A `..` that would
///   escape the front is left alone rather than guessed at.
/// * An **absolute** focus path is stripped of the absolute base. `--path .`
///   leaves the base relative, so an absolute `--focus /repo/crates/x` could not
///   otherwise be stripped at all, and the comparison silently matched nothing:
///   a scoped blast radius of zero with no explanation. Zero is worse than an
///   error here, because zero is a plausible answer.
/// * A path that normalises away to nothing -- `.`, `./`, `src/..` -- **drops
///   out of the set**, which both callers read as "no scope", which is what
///   scoping to the whole analysis base means.
///
/// The last rule is the one that was missing. `--focus .` used to scope to the
/// literal string `"."`, which matches no node, so `graph --path . --focus .`
/// reported a repository of zero files and `--focus ./crates/x` reported zero
/// dependents. Both answers were wrong and neither said so.
///
/// Scopes are a union, so one scope naming the whole base makes the whole union
/// the whole base. `--focus . --focus src/core` means "the base, or `src/core`",
/// and the base contains `src/core` -- answering with `src/core` alone would
/// undercount dependents and could turn a failing `--fail-on` into a passing one.
///
/// # What is and is not normalised
///
/// Two branches, and they make different promises.
///
/// A **relative** scope is rewritten when it has no root or drive prefix and no
/// `..` that escapes the front. Those are collapsed, `.` is dropped, and the result
/// is the spelling a node could have. Anything else -- a rooted but drive-less
/// `/src/core` on Windows, a `..` that escapes, an `N:` that reads as a drive
/// prefix -- is returned **exactly as typed**, spelling included. That is
/// deliberate: an unrecognised scope should look unrecognised rather than be
/// quietly turned into a different one.
///
/// An **absolute** scope has the base stripped lexically, and *that* has no check
/// for `..`. `<base>/../outside` becomes `../outside` rather than being refused,
/// which is the one case where the output is neither canonical nor the caller's
/// own spelling. It matches no node either way, so the answer is the same -- but
/// a caller reading the set should not assume the output never starts with `..`.
///
/// Either way a scope the resolver cannot place narrows the answer to nothing,
/// which is the silent-wrong-number shape this function exists to avoid. What the
/// pass-through buys is that the string is recognisable as the caller's own, so
/// the miss is legible.
///
/// This is stated here because it is the part a caller cannot infer from the
/// signature, and it took a fuzzer to pin down: six wrong assumptions about which
/// inputs get normalised, every one of them mine.
#[must_use]
pub fn build_focus_set(
    base_path: &Path,
    focus_paths: &[PathBuf],
) -> BTreeSet<String> {
    // Absolutised only for the absolute branch below: a relative path never
    // needs it, and computing it unconditionally would make the working
    // directory part of the answer.
    let base = std::path::absolute(base_path)
        .unwrap_or_else(|_| base_path.to_path_buf());

    let mut scopes = BTreeSet::new();

    for focus in focus_paths {
        let resolved = if focus.is_absolute() {
            focus
                .strip_prefix(&base)
                .unwrap_or(focus.as_path())
                .to_path_buf()
        } else {
            strip_relative_noise(focus).unwrap_or_else(|| focus.clone())
        };

        let spelled =
            super::path_utils::forward_slashes(&resolved.to_string_lossy());

        // `""` is how a scope that means "the whole base" spells itself, and
        // both callers already read an empty set that way.
        if spelled.is_empty() {
            return BTreeSet::new();
        }
        scopes.insert(spelled);
    }

    scopes
}

/// Drop `.` components and resolve `..` within a relative path, keeping it
/// relative.
///
/// Returns `None` for anything this cannot clean without guessing: a `..` that
/// would escape the front, or a root or drive prefix. Those are left as written,
/// because a plausible-looking wrong scope is harder to notice than one the
/// caller can recognise as not understood.
fn strip_relative_noise(path: &Path) -> Option<PathBuf> {
    let mut cleaned = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => cleaned.push(part),
            // The guard is the point: `pop` returns false with nothing to pop,
            // and `../x` landing on `x` would silently move the scope.
            Component::ParentDir if cleaned.pop() => {}
            _ => return None,
        }
    }

    Some(cleaned)
}

#[derive(Debug)]
struct GraphSelection {
    node_paths: BTreeSet<String>,
    applied_depth: Option<u32>,
    query: Option<GraphQuery>,
}

#[derive(Debug, Clone, Copy)]
enum TraversalDirection {
    Imports,
    ImportedBy,
}

fn normalize_graph_query(
    base_path: &Path,
    query: GraphQuery,
) -> Result<GraphQuery> {
    match query {
        GraphQuery::DependsOn(path) => Ok(GraphQuery::DependsOn(
            normalize_query_target(base_path, &path)?,
        )),
    }
}

fn normalize_query_target(base_path: &Path, raw_path: &str) -> Result<String> {
    let raw_path = Path::new(raw_path);
    let relative_path = if raw_path.is_absolute() {
        let canonical_base = base_path.canonicalize().with_context(|| {
            format!("failed to resolve base path `{}`", base_path.display())
        })?;
        let absolute_target = raw_path
            .canonicalize()
            .unwrap_or_else(|_| raw_path.to_path_buf());
        absolute_target
            .strip_prefix(&canonical_base)
            .with_context(|| {
                format!(
                    "path `{}` must resolve inside `{}`",
                    raw_path.display(),
                    base_path.display()
                )
            })?
            .to_path_buf()
    } else {
        raw_path.to_path_buf()
    };

    normalize_user_relative_path(&relative_path)
}

fn normalize_user_relative_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            Component::ParentDir => {
                if parts.pop().is_none() {
                    bail!(
                        "path `{}` must resolve inside the base path",
                        path.display()
                    );
                }
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }

    Ok(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}
/// Attempts to resolve an import path to a known file in the project.
///
/// Resolution is delegated to the language's [`ResolverPlugin`]. An import that
/// leaves the project, or a language with no bundled plugin, yields
/// [`Resolution::External`]; a path that should be local but cannot be placed
/// yields [`Resolution::Unresolved`]. Both are recorded as edges rather than
/// errors, because a graph of first-party dependencies is useful on its own, but
/// the report tells them apart because only one is worth fixing.
fn resolve_import_with(
    import_path: &str,
    source_file: &str,
    module_depth: u8,
    kind: ImportKind,
    language: SupportedLanguage,
    project: &ResolutionInputs,
) -> Resolution {
    let Some(plugin) = plugins::builtin_resolver_plugin(language) else {
        return Resolution::External;
    };

    let context = plugins::ResolveContext {
        source_file,
        known_files: &project.known_files,
        module_depth,
        kind,
        declarations: Some(&project.declarations),
        manifests: Some(&project.manifests),
        base_path: &project.base_path,
    };

    match plugin.resolve(import_path, context) {
        Some(file) => Resolution::File(file),
        // A path the resolver proved leaves the project is not a gap, however
        // local its shape looks. `crate::http::Request` is the case that
        // mattered: `pub use http;` re-exports a crate from outside, so the
        // prefix says "this project" and the code says otherwise.
        None if leaves_project(plugin, import_path, context) => {
            Resolution::External
        }
        None => Resolution::Unresolved,
    }
}

/// Ask a resolver whether an unresolved path was meant to name an outside crate.
///
/// The qualifier is stripped first: `crate::http::Request` has to be judged on
/// `http`, the name the crate root re-exports, not on the whole path.
fn leaves_project(
    plugin: &dyn plugins::ResolverPlugin,
    import_path: &str,
    context: plugins::ResolveContext<'_>,
) -> bool {
    const QUALIFIERS: [&str; 3] = ["crate::", "self::", "super::"];
    let Some(rest) = QUALIFIERS
        .iter()
        .find_map(|prefix| import_path.strip_prefix(prefix))
    else {
        return false;
    };
    let name = rest.split("::").next().unwrap_or(rest);
    !name.is_empty() && plugin.leaves_project(name, context)
}

/// What an import path turned out to name.
///
/// `Option<String>` could not express the third case. A path that fails to
/// resolve is either a gap in this resolver or a reference to code outside the
/// project, and the report draws a line between them: one is worth fixing, the
/// other is the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A file in the analysis.
    File(String),
    /// Meant for this project; the resolver could not place it.
    Unresolved,
    /// A crate, package, or standard library outside the analysis.
    External,
}

impl Resolution {
    /// The file this resolved to, if any.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        match self {
            Self::File(file) => Some(file),
            Self::Unresolved | Self::External => None,
        }
    }

    /// Whether a file was found.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::File(_))
    }
}

/// Test shim naming the language first, so the assertions read left to right.
#[cfg(test)]
fn resolve_import_lang(
    language: SupportedLanguage,
    import_path: &str,
    source_file: &str,
    known_files: &plugins::KnownFiles,
) -> Option<String> {
    resolve_plain(language, import_path, source_file, known_files)
        .file()
        .map(ToOwned::to_owned)
}

/// Resolve with no project indexes attached, which is what most tests want.
#[cfg(test)]
fn resolve_plain(
    language: SupportedLanguage,
    import_path: &str,
    source_file: &str,
    known_files: &plugins::KnownFiles,
) -> Resolution {
    resolve_import_with(
        import_path,
        source_file,
        0,
        ImportKind::Dependency,
        language,
        &ResolutionInputs {
            base_path: std::path::PathBuf::new(),
            known_files: known_files.clone(),
            declarations: declarations::DeclarationIndex::default(),
            manifests: manifests::ManifestIndex::default(),
        },
    )
}

/// The project-wide facts every resolution reads.
///
/// Grouped because the argument list outgrew what a reader can hold: the eighth
/// parameter was a second index, and a caller that passed them in the wrong
/// order would compile and resolve against the wrong table.
struct ResolutionInputs {
    base_path: PathBuf,
    known_files: plugins::KnownFiles,
    declarations: declarations::DeclarationIndex,
    manifests: manifests::ManifestIndex,
}

/// Builds edges and populates the node map from extracted imports.
fn build_edges_and_nodes(
    all_imports: &[FileImportData],
    project: &ResolutionInputs,
) -> (Vec<GraphEdge>, NodeMap) {
    let mut edges = Vec::new();
    let mut node_map: NodeMap = BTreeMap::new();

    // Initialize all source files as nodes.
    for file_data in all_imports {
        node_map
            .entry(file_data.file_path.clone())
            .or_default()
            .language = file_data.language;
    }

    for file_data in all_imports {
        for statement in &file_data.imports {
            let resolved = resolve_import_with(
                &statement.raw_path,
                &file_data.file_path,
                statement.module_depth,
                statement.kind,
                file_data.ts_language,
                project,
            );

            let is_resolved = resolved.is_resolved();
            let target = resolved.file().map(ToOwned::to_owned);
            // Only a path the resolver could not place, that still looks local,
            // counts as a gap. One it proved leaves the project is an external
            // dependency whatever its prefix says.
            let local_gap = matches!(resolved, Resolution::Unresolved)
                && looks_local(&statement.raw_path, file_data.ts_language)
                // A namespace import binds a name rather than naming a module,
                // so one that does not resolve is not a gap in the resolver.
                // `from . import Flask` in flask's `cli.py` names a class the
                // package re-exports, and there is no `Flask.py` for it to find.
                // A renaming import gets no such leniency: it names one path.
                //
                // A wildcard is the exception, and it is the opposite case: a
                // wildcard names a *package*, which is the broadest dependency a
                // language has, and there is no file for it to bind to.
                && (statement.kind.is_namespace()
                    == statement.raw_path.ends_with('*'))
                && !names_a_declared_symbol(statement, file_data, project);

            // Every resolved edge goes in the adjacency, including a parent naming its own
            // child with `mod child;`.
            //
            // Filtering module declarations out here was a real bug, and the
            // comment above this condition used to argue against the very filter
            // it sat next to. Blast radius wants the edge: editing `ignore.rs`
            // forces `core.rs` to rebuild, so "changing this breaks that" is
            // true. Excluding it meant a reverse query could not reach a parent
            // through its own `mod` declaration -- and because `core.rs` is the
            // parent of most of this repository, one missing edge hid 17 of
            // `ignore.rs`'s 34 real dependents.
            //
            // Cycle detection does not need the edge excluded here: `detect_cycles`
            // walks `structural_adjacency`, which drops structural edges by path
            // shape, and doing it in one place rather than two is what lets both
            // answers be right at once.
            if let Some(ref target) = target
                && *target != file_data.file_path
            {
                // Update imported_by for the target
                node_map
                    .entry(target.clone())
                    .or_default()
                    .imported_by
                    .push(file_data.file_path.clone());

                // Update imports for the source
                let source_entry =
                    node_map.entry(file_data.file_path.clone()).or_default();
                source_entry.imports.push(target.clone());

                // A `mod child;` is recorded by kind on the *source* entry, so
                // cycle detection can drop exactly this edge while the blast
                // radius keeps it. Recorded per source because that is the entry
                // whose `imports` list the declaration appears in.
                if statement.kind == ImportKind::ModuleDeclaration {
                    source_entry.declarations.insert(target.clone());
                }
            }

            edges.push(GraphEdge {
                from: file_data.file_path.clone(),
                to: target,
                import_path: statement.raw_path.clone(),
                resolved: is_resolved,
                kind: statement.kind,
                cfg_gated: statement.cfg_gated,
                // Whether this edge was meant for the project and could not be
                // placed. Carried on the edge so the metrics read a fact rather
                // than re-guessing from the path's shape.
                local_gap,
            });
        }
    }

    (edges, node_map)
}

fn select_graph(
    node_map: &NodeMap,
    focus_set: &BTreeSet<String>,
    depth: Option<u32>,
    query: Option<GraphQuery>,
) -> Result<GraphSelection> {
    if focus_set.is_empty() && query.is_none() {
        return Ok(GraphSelection {
            node_paths: node_map.keys().cloned().collect(),
            applied_depth: None,
            query: None,
        });
    }

    let mut selected = BTreeSet::new();

    if let Some(ref query_mode) = query {
        let query_roots = roots_for_query(node_map, query_mode)?;
        let mut reachable = traverse_graph(
            node_map,
            &query_roots,
            depth,
            TraversalDirection::ImportedBy,
        );

        // A reverse query is a question about one file, so `--focus` narrows the
        // answer rather than adding to it. It used to be unioned in, which meant
        // `--focus crates/sephera_core --what-depends-on <a file in it>` answered
        // a different question than the one asked: the whole focused subtree plus
        // the dependents, so 72 files instead of 18. The reading a caller wants
        // is "among the files in this scope, which depend on this one".
        if !focus_set.is_empty() {
            let focus_roots = collect_focus_roots(node_map, focus_set);
            reachable.retain(|path| focus_roots.contains(path));

            // The target stays in the report even when the scope excludes it, so
            // the answer reads "nothing in this scope depends on it" rather than
            // "nothing at all". A report with no node for the file being asked
            // about cannot tell those two apart, and they are opposites.
            for root in &query_roots {
                reachable.insert(root.clone());
            }
        }

        selected.extend(reachable);
    } else if !focus_set.is_empty() {
        // With no query, `--focus` means "start here and follow imports", which
        // is what a dependency report needs: the subtree plus what it pulls in.
        let focus_roots = collect_focus_roots(node_map, focus_set);
        selected.extend(traverse_graph(
            node_map,
            &focus_roots,
            depth,
            TraversalDirection::Imports,
        ));
    }

    Ok(GraphSelection {
        node_paths: selected,
        applied_depth: depth,
        query,
    })
}

fn collect_focus_roots(
    node_map: &NodeMap,
    focus_set: &BTreeSet<String>,
) -> BTreeSet<String> {
    node_map
        .keys()
        .filter(|path| {
            focus_set
                .iter()
                .any(|focus| path_matches_focus(path, focus))
        })
        .cloned()
        .collect()
}

/// Whether `path` lies inside the scope named by `focus`.
///
/// Public because scoping has to mean one thing across the tool. `graph
/// --focus` and `impact --focus` both answer "within this scope, which files
/// depend on this one", and two implementations of "inside the scope" would let
/// them disagree on the boundary -- one saying a file is in scope and the other
/// silently dropping it from a blast radius.
///
/// `focus` matches a path exactly or as a directory prefix at a `/` boundary, so
/// `crates/sephera_core` covers `crates/sephera_core/src/lib.rs` but not
/// `crates/sephera_core_extra/src/lib.rs`.
#[must_use]
pub fn path_matches_focus(path: &str, focus: &str) -> bool {
    path == focus
        || path
            .strip_prefix(focus)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn roots_for_query(
    node_map: &NodeMap,
    query: &GraphQuery,
) -> Result<BTreeSet<String>> {
    match query {
        GraphQuery::DependsOn(path) => {
            if node_map.contains_key(path) {
                Ok(BTreeSet::from([path.clone()]))
            } else {
                bail!(
                    "path `{path}` did not resolve to an analyzed graph node"
                );
            }
        }
    }
}

fn traverse_graph(
    node_map: &NodeMap,
    roots: &BTreeSet<String>,
    depth: Option<u32>,
    direction: TraversalDirection,
) -> BTreeSet<String> {
    // The requested depth *is* the number of hops. Roots sit at distance 0 and are
    // always included, so `--depth 0` selects the roots and nothing else.
    //
    // This used to add one to whatever was asked for, so the flag counted graph
    // *levels* while `impact --depth` counted hops, and the two commands answered
    // the same question with the same flag name and different numbers: on a chain
    // c -> b -> a -> target, `impact --depth 1` reported one dependent and
    // `graph --what-depends-on target --depth 1` reported two. A reader who learned
    // "1 means direct importers" from one command got two hops from the other,
    // silently.
    //
    // One convention across both commands is worth the breaking change. The old
    // `--depth 0` on a forward query kept the roots and what they reach, which is
    // spelled `--depth 1` now.
    let max_distance = depth;
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<(String, u32)> =
        roots.iter().cloned().map(|path| (path, 0)).collect();

    while let Some((path, distance)) = queue.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }

        if max_distance.is_some_and(|limit| distance >= limit) {
            continue;
        }

        let neighbors =
            node_map.get(path.as_str()).map_or(
                &[][..],
                |entry| match direction {
                    TraversalDirection::Imports => entry.imports.as_slice(),
                    TraversalDirection::ImportedBy => {
                        entry.imported_by.as_slice()
                    }
                },
            );

        for neighbor in neighbors {
            if !visited.contains(neighbor) {
                queue.push_back((neighbor.clone(), distance + 1));
            }
        }
    }

    visited
}

fn filter_node_map(
    node_map: &NodeMap,
    selected_paths: &BTreeSet<String>,
) -> NodeMap {
    selected_paths
        .iter()
        .filter_map(|path| {
            node_map.get(path).map(|entry| {
                (
                    path.clone(),
                    super::types::NodeEntry {
                        language: entry.language,
                        imports: entry
                            .imports
                            .iter()
                            .filter(|neighbor| {
                                selected_paths.contains(*neighbor)
                            })
                            .cloned()
                            .collect(),
                        imported_by: entry
                            .imported_by
                            .iter()
                            .filter(|neighbor| {
                                selected_paths.contains(*neighbor)
                            })
                            .cloned()
                            .collect(),
                        // Kept so a filtered report can still tell a
                        // declaration edge from a `use`, which is what stops
                        // cycle detection from inventing cycles out of the
                        // Rust module tree.
                        declarations: entry
                            .declarations
                            .iter()
                            .filter(|neighbor| {
                                selected_paths.contains(*neighbor)
                            })
                            .cloned()
                            .collect(),
                    },
                )
            })
        })
        .collect()
}

fn filter_edges(
    edges: &[GraphEdge],
    selected_paths: &BTreeSet<String>,
) -> Vec<GraphEdge> {
    edges
        .iter()
        .filter(|edge| {
            selected_paths.contains(&edge.from)
                && (if edge.resolved {
                    edge.to
                        .as_ref()
                        .is_some_and(|target| selected_paths.contains(target))
                } else {
                    true
                })
        })
        .cloned()
        .collect()
}

/// Builds the final node list from the node map.
fn build_node_list(node_map: &NodeMap) -> Vec<GraphNode> {
    node_map
        .iter()
        .map(|(file_path, entry)| GraphNode {
            file_path: file_path.clone(),
            language: entry.language,
            imports_count: u64::try_from(entry.imports.len())
                .unwrap_or(u64::MAX),
            imported_by_count: u64::try_from(entry.imported_by.len())
                .unwrap_or(u64::MAX),
        })
        .collect()
}

/// Computes graph metrics including cycle detection.
fn compute_metrics(
    index: &manifests::ManifestIndex,
    node_map: &NodeMap,
    edges: &[GraphEdge],
) -> GraphMetrics {
    let total_files = u64::try_from(node_map.len()).unwrap_or(u64::MAX);
    // A `use super::*;` inside a test module resolves to the file it is written
    // in. That is a real reference but says nothing about how files depend on
    // each other, so it is counted apart from the internal edges.
    let self_references = u64::try_from(
        edges
            .iter()
            .filter(|edge| edge.resolved && is_self_edge(edge))
            .count(),
    )
    .unwrap_or(u64::MAX);
    let total_internal_edges = u64::try_from(
        edges
            .iter()
            .filter(|edge| edge.resolved && !is_self_edge(edge))
            .count(),
    )
    .unwrap_or(u64::MAX);

    let cfg_gated_edges =
        u64::try_from(edges.iter().filter(|edge| edge.cfg_gated).count())
            .unwrap_or(u64::MAX);
    let unresolved_local: Vec<&GraphEdge> =
        edges.iter().filter(|edge| edge.local_gap).collect();
    let external_count = edges.iter().filter(|e| !e.resolved).count();
    let total_external_edges =
        u64::try_from(external_count - unresolved_local.len())
            .unwrap_or(u64::MAX);
    let unresolved_local_edges =
        u64::try_from(unresolved_local.len()).unwrap_or(u64::MAX);
    let unresolved_local_samples = unresolved_local
        .iter()
        .take(MAX_UNRESOLVED_LOCAL_SAMPLES)
        .map(|edge| format!("{}: {}", edge.from, edge.import_path))
        .collect();

    // Attribute what is left to actual packages, so the report can answer which
    // dependency an edge refers to rather than only how many there are.
    let dependencies = index.summarise(edges.iter().filter_map(|edge| {
        // The ecosystem follows from the file the import was written in, which
        // is what decides whether a bare name is a standard library module.
        let extension = std::path::Path::new(&edge.from)
            .extension()
            .and_then(std::ffi::OsStr::to_str)?;
        let language = SupportedLanguage::from_extension(extension)?;
        (!edge.resolved).then_some((
            edge.import_path.as_str(),
            manifests::Ecosystem::of(language),
        ))
    }));
    let edges_of_kind = |kind: manifests::DependencyKind| {
        dependencies
            .iter()
            .filter(|dependency| dependency.kind == kind)
            .map(|dependency| dependency.edge_count)
            .sum::<u64>()
    };
    let declared_dependency_edges =
        edges_of_kind(manifests::DependencyKind::Declared);
    let local_crate_edges = edges_of_kind(manifests::DependencyKind::Local);
    let builtin_edges = edges_of_kind(manifests::DependencyKind::Builtin);

    let mut most_importing: Vec<FileMetric> = node_map
        .iter()
        .map(|(path, entry)| FileMetric {
            file_path: path.clone(),
            count: u64::try_from(entry.imports.len()).unwrap_or(u64::MAX),
        })
        .filter(|m| m.count > 0)
        .collect();
    most_importing.sort_by_key(|m| std::cmp::Reverse(m.count));
    most_importing.truncate(10);

    let mut most_imported: Vec<FileMetric> = node_map
        .iter()
        .map(|(path, entry)| FileMetric {
            file_path: path.clone(),
            count: u64::try_from(entry.imported_by.len()).unwrap_or(u64::MAX),
        })
        .filter(|m| m.count > 0)
        .collect();
    most_imported.sort_by_key(|m| std::cmp::Reverse(m.count));
    most_imported.truncate(10);

    let cycles = detect_cycles(node_map);
    let circular_dependencies = u64::try_from(cycles.len()).unwrap_or(u64::MAX);

    GraphMetrics {
        total_files,
        total_internal_edges,
        self_references,
        total_external_edges,
        unresolved_local_edges,
        unresolved_local_samples,
        cfg_gated_edges,
        dependencies,
        declared_dependency_edges,
        local_crate_edges,
        builtin_edges,
        circular_dependencies,
        most_importing,
        most_imported,
        cycles,
    }
}

/// Whether an edge between a module and its own hierarchy is structural rather
/// than a dependency.
///
/// Rust's module tree makes these relationships unavoidable in both directions.
/// A parent names its children with `mod child;` or `pub use child::Thing`, and a
/// child names anything its ancestors declare with `super::` or `crate::`. So the
/// pair always points at each other, and neither edge is something an edit can
/// remove: there is no way to make `error.rs` stop using the `BoxError` alias
/// `lib.rs` declares without deleting one of them.
///
/// Reporting them costs the blast radius its meaning. Sephera's own tree went
/// from 0 cycles to 4 when only the parent's declaration was recognised, and
/// axum from 21 to 37 once a child could reach a name its ancestor declares.
/// The edges stay in the report, because "changing `BoxError` affects `error.rs`"
/// is worth knowing; they just do not close a loop.
fn is_structural_module_edge(source: &str, target: &str) -> bool {
    is_own_child_module(source, target)
        || is_own_child_module(target, source)
        || is_ancestor_module(target, source)
}

/// Whether `target` is a direct child module of the file named by `source`.
fn is_own_child_module(source: &str, target: &str) -> bool {
    let children = plugins::rust::module_children_dir(source);
    match target.rsplit_once('/') {
        Some((directory, _)) => directory == children,
        None => false,
    }
}

/// Whether `ancestor` encloses `descendant` in the module tree.
///
/// Catches the multi-level case a direct parent check misses: a reference from
/// `src/a/b/c.rs` to `src/lib.rs` is as unavoidable as one to `src/a/mod.rs`, and
/// a cycle that walked only immediate neighbours would miss it.
fn is_ancestor_module(ancestor: &str, descendant: &str) -> bool {
    let enclosing = plugins::rust::module_children_dir(ancestor);
    let inner = plugins::rust::module_children_dir(descendant);
    inner.starts_with(&enclosing)
        && inner.len() > enclosing.len()
        && inner.as_bytes().get(enclosing.len()) == Some(&b'/')
}
/// Whether an edge points from a file back to itself.
fn is_self_edge(edge: &GraphEdge) -> bool {
    edge.to.as_deref() == Some(edge.from.as_str())
}

/// Whether an unresolved path still looks like it names something in the project.
///
/// This decides which bucket a failed resolution is counted in, and it used to
/// recognise only Rust's qualifiers and a leading dot. So a Java import of a
/// package that does not exist, a C `#include "missing.h"` and a Go package path
/// that finds no directory were all filed as *external* -- counted with the
/// standard library rather than with the problems. `local_gap` existed, and for
/// four of the eight bundled languages it could never be true.
/// Whether an unresolved path still looks like it names something in the project.
///
/// This decides which bucket a failed resolution is counted in, and it used to
/// recognise only Rust's qualifiers and a leading dot -- so for four of the
/// eight bundled languages `local_gap` could never be true. A Java import of a
/// package that does not exist, a C `#include "missing.h"`, a Go package path
/// with no matching directory and a JavaScript `require('./gone')` were all
/// filed as *external*: counted with the standard library rather than with the
/// problems.
///
/// The three-way [`Resolution`] enum already draws this line. A plugin's
/// `leaves_project` is what decides whether an unresolved path provably leaves
/// the project, and it answered "no" for every one of those. Asking a Rust
/// qualifier list a second time therefore re-decided something the resolver had
/// already settled, using rules from one language.
/// Whether an unresolved path ends in a name the project already declares.
///
/// `from .app import Flask` produces two edges. `.app` resolves to `app.py` and
/// carries the dependency. `.app.Flask` resolves to nothing, because `Flask` is
/// a class inside `app.py` and not a module beside it -- so it was reported as a
/// path that meant to name a project file and could not, which is the one thing
/// `unresolved_local` is documented to mean. On flask that was 162 of them,
/// against 185 edges the resolver had actually placed.
///
/// The dependency was never at risk, so this removes a false alarm rather than
/// fixing a missed edge. That distinction is the whole reason the check is
/// here and not in the extractor: an extractor that dropped the name would lose
/// the ability to report `from pkg import absent`, where nothing declares the
/// name and the import really is broken.
///
/// Split on the last separator, then ask the file the parent names. A path with
/// no parent -- `from . import Flask`, which binds the name straight against the
/// package -- has nothing to split and stays a gap.
fn names_a_declared_symbol(
    statement: &ImportStatement,
    file_data: &FileImportData,
    project: &ResolutionInputs,
) -> bool {
    let Some((parent, name)) = split_trailing_name(&statement.raw_path) else {
        return false;
    };

    let Resolution::File(parent_file) = resolve_import_with(
        &parent,
        &file_data.file_path,
        statement.module_depth,
        statement.kind,
        file_data.ts_language,
        project,
    ) else {
        return false;
    };

    project.declarations.file_reaches(&parent_file, &name)
}

/// Split a dotted path into everything before the last separator and the last
/// segment, keeping the leading dots on the parent.
///
/// The separator has to be the last one rather than the first: `.sansio.app.App`
/// names the `App` class in the module `.sansio.app`, so the name is what is
/// missing and the parent is what resolves.
///
/// A trailing `*` needs no handling here. A wildcard names a package, which the
/// gap rule already counts on its own account, and no declaration index holds a
/// name spelled `*` -- every one of them is read out of a grammar's `name`
/// field, so the lookup finds nothing and the answer was false anyway.
fn split_trailing_name(import_path: &str) -> Option<(String, String)> {
    let separator = import_path.rfind(['.', '/', ':'])?;
    let name = &import_path[separator + 1..];
    let parent = &import_path[..separator];

    if name.is_empty() || parent.is_empty() || parent.ends_with(['.', '/', ':'])
    {
        return None;
    }

    Some((parent.to_owned(), name.to_owned()))
}

fn looks_local(import_path: &str, language: SupportedLanguage) -> bool {
    if import_path.ends_with('*') {
        // A wildcard names a package rather than a module, and there is no file
        // to point at: `import com.example.util.*;` depends on every type in
        // that package. `ResolverPlugin::resolve` returns one file, so the
        // honest answer cannot be expressed -- and choosing one file would
        // present a fraction of the dependency as though it were all of it.
        //
        // Counting it as external hides a dependency on a whole package
        // underneath the standard library's shape. Counting it as a gap puts it
        // in the number a reader can act on.
        return true;
    }

    match language {
        // Rust is the language that has to guess. An unqualified `serde::Serialize`
        // names a different crate and no path in the source says so; the
        // qualifier list is the only evidence available, and a path carrying none
        // of them is left to the resolver's own reasoning.
        SupportedLanguage::Rust => {
            const QUALIFIERS: [&str; 4] = ["crate::", "self::", "super::", "."];

            QUALIFIERS
                .iter()
                .any(|qualifier| import_path.starts_with(qualifier))
        }

        // A relative import is local by construction. An absolute one that does
        // not resolve names a package this analysis can see but does not
        // contain, and the standard library is already separated from it by
        // `leaves_project`.
        //
        // Not the other four languages, though. `leaves_project` is implemented
        // for Rust and C/C++ only; a bare `react`, a `fmt` and a `com.example.Foo`
        // all reach it as "did not leave the project" simply because nothing
        // asked, and treating that as evidence of locality turns every external
        // dependency into a resolver gap -- 472 on flask and 241 on express.
        // Nothing about a bare specifier's shape says it is external, so the
        // honest answer for those languages is still "cannot tell", and the
        // count stays where it was.
        SupportedLanguage::Python => import_path.starts_with('.'),

        // The two include forms are the distinction the language exists for: a
        // quoted include names a project file and an angled one names the
        // toolchain's. A quoted include that resolves to nothing is exactly the
        // case worth reporting, and filing it alongside `<stdio.h>` hid it.
        SupportedLanguage::C | SupportedLanguage::Cpp => {
            !import_path.starts_with('<')
        }

        // A relative specifier names a path inside this project -- Node resolves
        // `./x` against the importing file and there is nowhere else it could
        // go. So `./gone` failing to resolve is a gap by construction, and no
        // evidence is needed.
        //
        // A *bare* specifier is the opposite: `react` and `express` are
        // packages, and their shape says nothing either way. Only a plugin that
        // has read a manifest can tell, and none of these has one threaded
        // through yet, so they stay as they were rather than becoming every
        // external dependency's worth of gaps.
        SupportedLanguage::JavaScript | SupportedLanguage::TypeScript => {
            import_path.starts_with("./") || import_path.starts_with("../")
        }

        // Go package paths and Java dotted paths carry the same problem without
        // the one shape that settles it. `fmt` and `example.com/acme/store` look
        // alike; only a directory match or a manifest tells them apart, and the
        // resolver has already had its say by the time this is asked.
        SupportedLanguage::Go | SupportedLanguage::Java => false,
    }
}

/// Canonical identity of a cycle, used to collapse duplicates.
///
/// A cycle is reported once per edge by the traversal, so the same ring can be
/// discovered from several of its members and in either direction. Keying on
/// the rotation-normalised ring makes those discoveries compare equal, which is
/// what keeps the reported cycle count an actual count of distinct rings rather
/// than a count of traversal artefacts.
fn cycle_key(cycle: &[String]) -> String {
    // `cycle` repeats its entry node at the end, so the repeat is dropped
    // before keying: otherwise a self-import and a two-node ring would not
    // compare equal to themselves across representations.
    let mut ring: Vec<String> = cycle
        .iter()
        .take(cycle.len().saturating_sub(1))
        .cloned()
        .collect();
    ring.sort();
    ring.join("|")
}

/// A copy of the adjacency with structural module edges removed.
///
/// Rust's module tree makes these edges unavoidable in both directions, so
/// keeping them in the cycle walk reported relationships no edit can remove: a
/// parent names its children with `mod child;`, and a child names anything its
/// ancestors declare with `super::` or `crate::`. On axum, including them turned
/// 18 real cycles into 37, and on this repository 0 into 4 -- every one of them
/// `lib.rs` and `error.rs` pointing at each other over an alias.
///
/// Filtering here rather than when the graph is built keeps the two purposes
/// separate. Blast radius wants the edge: editing `service.rs` really does affect
/// `main.rs`. Only a cycle claim is something an edit cannot resolve.
fn structural_adjacency(node_map: &NodeMap) -> NodeMap {
    let mut filtered: NodeMap = node_map.to_owned();
    for (file, entry) in &mut filtered {
        entry
            .imports
            .retain(|target| !is_structural_module_edge(file, target));
    }
    filtered
}

/// Drop exactly the edges that are `mod child;` declarations.
///
/// A second filter, applied after `structural_adjacency`, and for a different
/// reason. `structural_adjacency` guesses structural edges from the shape of two
/// paths, which is how a child pointing back at its ancestor through `super::` or
/// `crate::` gets caught. It cannot tell a declaration from a `use`: both appear
/// in `imports` as bare paths. Leaving them in put axum's real cycle count at 23
/// instead of 18, so the declaration edge is recorded by kind and removed here.
fn without_declaration_edges(node_map: &NodeMap) -> NodeMap {
    let mut filtered: NodeMap = node_map.to_owned();
    for entry in filtered.values_mut() {
        entry
            .imports
            .retain(|target| !entry.declarations.contains(target));
    }
    filtered
}

/// Detect cycles in the dependency graph using iterative DFS.
///
/// Each distinct ring is reported once. Cycles are returned in a deterministic
/// order so that repeated runs over an unchanged tree produce identical output.
fn detect_cycles(node_map: &NodeMap) -> Vec<Vec<String>> {
    let adjacency = without_declaration_edges(&structural_adjacency(node_map));
    let node_map = &adjacency;
    let mut cycles: Vec<Vec<String>> = Vec::new();

    // These three sets deliberately live outside the start-node loop. Marking a
    // node visited once, globally, is what confines each connected component to
    // a single traversal: a ring is therefore discovered from one of its members
    // and every later member is skipped, rather than the ring being rediscovered
    // once per member.
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut in_stack: BTreeSet<String> = BTreeSet::new();
    let mut seen_cycle_keys: BTreeSet<String> = BTreeSet::new();

    for start_node in node_map.keys() {
        if visited.contains(start_node) {
            continue;
        }

        // DFS using explicit stack: (node, child_index)
        let mut stack: Vec<(String, usize)> = vec![(start_node.clone(), 0)];
        in_stack.insert(start_node.clone());

        while let Some((node, child_idx)) = stack.last_mut() {
            let children = node_map
                .get(node.as_str())
                .map_or(&[][..], |e| e.imports.as_slice());

            if *child_idx >= children.len() {
                // Backtrack
                let node = stack.pop().unwrap().0;
                in_stack.remove(&node);
                visited.insert(node);
                continue;
            }

            let child = children[*child_idx].clone();
            *child_idx += 1;

            if in_stack.contains(&child) {
                // Found a cycle — extract it
                let cycle_start_idx =
                    stack.iter().position(|(n, _)| *n == child).unwrap_or(0);
                let mut cycle: Vec<String> = stack[cycle_start_idx..]
                    .iter()
                    .map(|(n, _)| n.clone())
                    .collect();
                cycle.push(child.clone());

                if seen_cycle_keys.insert(cycle_key(&cycle)) {
                    cycles.push(cycle);
                }
            } else if !visited.contains(&child) {
                in_stack.insert(child.clone());
                stack.push((child, 0));
            }
        }
    }

    cycles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::ImportKind;
    use std::fs;
    use tempfile::tempdir;

    fn known_files(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    /// Build a statement of the given kind for filter tests.
    fn statement(raw_path: &str, kind: ImportKind) -> ImportStatement {
        ImportStatement {
            raw_path: raw_path.to_owned(),
            line: 1,
            kind,
            module_depth: 0,
            cfg_gated: false,
        }
    }

    #[test]
    fn a_relative_focus_path_is_left_exactly_as_written() {
        // The graph is spelled relative to the analysis base, not the working
        // directory. Resolving a relative scope against the working directory
        // turns `--path crates/x --focus src/main.rs` into a path under the
        // repository root that names a file nothing else refers to, so the scope
        // silently matches nothing.
        let focus = vec![PathBuf::from("src/main.rs")];

        assert_eq!(
            build_focus_set(Path::new("/anywhere"), &focus),
            known_files(&["src/main.rs"])
        );
        assert_eq!(
            build_focus_set(Path::new("."), &focus),
            known_files(&["src/main.rs"]),
            "a relative base must not change how a relative scope is read"
        );
        assert_eq!(
            build_focus_set(&std::env::temp_dir().join("repo"), &focus),
            known_files(&["src/main.rs"]),
            "the working directory must not appear in the answer"
        );
    }

    #[test]
    fn an_absolute_focus_path_is_made_relative_to_an_absolute_base() {
        let base = std::env::temp_dir().join("repo");
        let focus = vec![base.join("crates").join("one")];

        assert_eq!(
            build_focus_set(&base, &focus),
            known_files(&["crates/one"]),
            "an absolute scope under the base is base-relative in the graph"
        );
    }

    #[test]
    fn an_absolute_scope_measures_the_same_from_a_relative_base() {
        // `--path .` is what makes the base relative in practice, and it is why
        // an absolute `--focus` used to match nothing and report a blast radius
        // of zero without saying so.
        //
        // Built under the working directory rather than the temp directory,
        // because "inside the base" is the entire claim. A scope pointing
        // somewhere else is out of scope whichever way the base is spelled, and
        // pretending otherwise would be testing the wrong thing.
        let base = std::env::current_dir().expect("a working directory");
        let focus = vec![base.join("crates").join("one")];

        assert_eq!(
            build_focus_set(Path::new("."), &focus),
            known_files(&["crates/one"]),
            "an absolute scope under `.` must resolve as a relative one does"
        );
        assert_eq!(
            build_focus_set(Path::new("."), &focus),
            build_focus_set(&base, &focus),
            "the base's own relativity must not decide whether a scope matches"
        );
    }

    #[test]
    fn an_absolute_scope_outside_the_base_is_left_alone() {
        // Mangling it into a relative-looking path would hide the mismatch, which
        // is the one case where saying "I do not recognise this scope" would be
        // more useful than silently matching nothing.
        let base = std::env::temp_dir().join("repo");
        let elsewhere = std::env::temp_dir().join("elsewhere");

        let normalised =
            build_focus_set(&base, std::slice::from_ref(&elsewhere));

        assert!(
            normalised.iter().any(|path| path.contains("elsewhere")),
            "expected the outside path to survive, got {normalised:?}"
        );
    }

    #[test]
    fn a_scope_that_spells_the_whole_base_becomes_no_scope_at_all() {
        // `.`, `./` and `x/../..` all name the analysis base. Scoping to the base
        // is not a narrower question, and both callers read an empty scope as "no
        // restriction". Used to be scoped to the literal string `"."`, which
        // matched no node -- so `graph --path . --focus .` reported a repository of
        // zero files and `--focus ./crates/x` reported zero dependents. Both
        // wrong, both silent.
        let base = std::env::temp_dir().join("repo");
        // Relative spellings only: an absolute path is stripped against the base
        // rather than normalised through it, and that is a separate rule.
        let mut spellings = vec![
            PathBuf::from("."),
            PathBuf::from("./"),
            PathBuf::from("crates/x/../.."),
        ];
        spellings.extend(windows_paths(&[".\\"]));

        for spelling in &spellings {
            assert_eq!(
                build_focus_set(&base, std::slice::from_ref(spelling)),
                BTreeSet::new(),
                "`{}` names the whole base, so it should not restrict anything",
                spelling.display()
            );
        }
    }

    #[test]
    fn dot_segments_inside_a_scope_are_resolved_away() {
        // The graph never spells a `.` or a `..`, so leaving one in the scope makes
        // it match nothing -- quietly, since an empty answer looks like a finding.
        for (spelling, resolves_to) in [
            ("crates/cli/./src", "crates/cli/src"),
            ("./crates/cli/src", "crates/cli/src"),
            // One level up, so this resolves to the parent rather than to itself.
            ("crates/cli/src/..", "crates/cli"),
            ("crates/cli/./src/../..", "crates"),
        ] {
            assert_eq!(
                build_focus_set(
                    std::env::temp_dir().join("repo").as_path(),
                    &[PathBuf::from(spelling)],
                ),
                known_files(&[resolves_to]),
                "`{spelling}` should resolve to {resolves_to}"
            );
        }

        // Backslash-separated input only means anything where the separator is a
        // backslash. On Unix `.\\crates` is an ordinary file name with a backslash
        // in it, and asserting it resolves to a directory would be asserting that
        // a legal file name is silently rewritten.
        for (spelling, resolves_to) in
            windows_spellings(&[(".\\crates\\cli\\src", "crates/cli/src")])
        {
            assert_eq!(
                build_focus_set(
                    std::env::temp_dir().join("repo").as_path(),
                    &[PathBuf::from(spelling)],
                ),
                known_files(&[resolves_to]),
                "`{spelling}` should resolve to {resolves_to}"
            );
        }
    }

    /// Windows-only spellings, as paths.
    fn windows_paths(raw: &[&str]) -> Vec<PathBuf> {
        if cfg!(windows) {
            raw.iter().map(PathBuf::from).collect()
        } else {
            Vec::new()
        }
    }

    /// Windows-only spellings, as (spelling, expected) pairs.
    ///
    /// Guards two mistakes at once: a Unix run failing on a Windows-only input,
    /// and a Windows run quietly skipping coverage it thought it had.
    fn windows_spellings<'a>(
        pairs: &[(&'a str, &'a str)],
    ) -> Vec<(&'a str, &'a str)> {
        if cfg!(windows) {
            pairs.to_vec()
        } else {
            Vec::new()
        }
    }

    #[test]
    fn a_backslash_is_a_separator_only_where_it_is_one() {
        // On Unix a backslash is an ordinary character in a file name, so
        // rewriting it would silently widen a scope from one file to a directory
        // tree. Found because a Windows-only test input failed on Linux, where it
        // is a legal file name rather than a spelling mistake.
        let spelled =
            crate::core::graph::path_utils::forward_slashes("crates/cli\\src");

        if cfg!(windows) {
            assert_eq!(spelled, "crates/cli/src");
        } else {
            assert_eq!(
                spelled, "crates/cli\\src",
                "a backslash in a Unix file name is part of the name"
            );
        }
    }

    #[test]
    fn one_scope_naming_the_whole_base_overrides_the_others() {
        // Scopes are a union. `--focus . --focus src/core` asks for the base *or*
        // `src/core`, and the base contains `src/core` -- so the answer is the whole
        // base. Answering with `src/core` alone undercounts dependents, which is how a
        // `--fail-on` limit stops failing without anyone touching the rule.
        let base = std::env::temp_dir().join("repo");
        let scopes = vec![PathBuf::from("."), PathBuf::from("src/core")];

        assert_eq!(
            build_focus_set(&base, &scopes),
            BTreeSet::new(),
            "a scope that is the whole base makes the whole union the whole base"
        );

        // Order must not matter, or the same flags mean two different things.
        let reversed = vec![PathBuf::from("src/core"), PathBuf::from("./")];
        assert_eq!(build_focus_set(&base, &reversed), BTreeSet::new());
    }

    #[test]
    fn an_absolute_scope_can_still_produce_a_leading_parent() {
        // `strip_prefix` is lexical, so `<base>/../outside` strips to `../outside`
        // without anything noticing that it now points above the base. Worth pinning
        // because the obvious reading of "an absolute scope is made relative" is
        // wrong, and the difference is only visible in the output.
        let base = std::env::temp_dir().join("repo");
        let escaping = base.join("..").join("outside");

        let scope = build_focus_set(&base, &[escaping]);

        assert_eq!(
            scope,
            known_files(&["../outside"]),
            "the base is stripped lexically, with no check for `..` in what is left"
        );
    }

    #[test]
    fn a_parent_segment_that_escapes_the_front_is_left_alone() {
        // `../x` cannot be resolved without knowing what the base is relative to,
        // so it is passed through rather than collapsed onto `x` -- which would
        // silently move the scope somewhere the caller never asked about.
        let scope = build_focus_set(
            std::env::temp_dir().join("repo").as_path(),
            &[PathBuf::from("../outside")],
        );

        assert!(
            scope.iter().any(|path| path.contains("..")),
            "expected the escaping path to survive, got {scope:?}"
        );
    }

    #[test]
    fn depth_counts_hops_from_the_traversal_root() {
        // One convention across `graph` and `impact`: the number asked for is the
        // number of hops. It used to be levels, so `--depth 1` was one hop wider
        // here than there and `--depth 0` kept the roots plus their neighbours
        // rather than the roots alone.
        //
        // A chain c -> b -> a -> target, so `a` is one hop from the target and `c`
        // is three. Pinned on the walk itself because no test compared the two
        // commands' arithmetic, which is how the extra hop survived: every test
        // using the flag still passed while it was wrong.
        let reached = |depth: Option<u32>| {
            let (node_map, roots) = chain_fixture();
            traverse_graph(
                &node_map,
                &roots,
                depth,
                TraversalDirection::ImportedBy,
            )
            .len()
        };

        assert_eq!(reached(None), 4, "unbounded reaches the whole chain");
        assert_eq!(reached(Some(0)), 1, "depth 0 is the root and nothing else");
        assert_eq!(reached(Some(1)), 2, "depth 1 is the root plus one hop");
        assert_eq!(reached(Some(2)), 3, "depth 2 adds the second hop");
        assert_eq!(reached(Some(3)), 4, "depth 3 reaches the end of the chain");
        assert_eq!(reached(Some(4)), 4, "already everything; no further hop");
    }

    /// A node map for `c -> b -> a -> target`, each imported by the one above it.
    ///
    /// Paths are spelled the way the graph spells them, so a failure reads as a path.
    fn chain_fixture() -> (NodeMap, BTreeSet<String>) {
        let mut node_map: NodeMap = BTreeMap::new();

        let link =
            |node_map: &mut NodeMap, path: &str, imported_by: &[&str]| {
                node_map.insert(
                    path.to_owned(),
                    crate::core::graph::types::NodeEntry {
                        language: Some("Rust"),
                        imports: Vec::new(),
                        imported_by: imported_by
                            .iter()
                            .map(|s| (*s).to_owned())
                            .collect(),
                        declarations: BTreeSet::new(),
                    },
                );
            };

        link(&mut node_map, "target", &["a"]);
        link(&mut node_map, "a", &["b"]);
        link(&mut node_map, "b", &["c"]);
        link(&mut node_map, "c", &[]);

        (node_map, BTreeSet::from(["target".to_owned()]))
    }

    #[test]
    fn type_alias_imports_are_kept_by_default() {
        // Dropping edges silently would be worse than showing them, so the
        // filter is opt-in and the default graph is unchanged.
        let filters = EdgeFilters::default();

        assert!(
            !filters
                .rejects(&statement("crate::foo::Bar", ImportKind::TypeAlias))
        );
        assert!(
            !filters.rejects(&statement("crate::foo", ImportKind::Namespace))
        );
    }

    #[test]
    fn type_alias_imports_are_dropped_when_enabled() {
        let filters = EdgeFilters {
            exclude_type_aliases: true,
        };

        assert!(
            filters
                .rejects(&statement("crate::foo::Bar", ImportKind::TypeAlias))
        );
        assert!(
            filters.rejects(&statement("crate::foo", ImportKind::Namespace))
        );
    }

    #[test]
    fn a_module_declaration_survives_the_filter() {
        // `--exclude-types` is about import forms, not about declarations. A
        // declaration is kept in the edge list either way; cycle detection is
        // what ignores it.
        let filters = EdgeFilters {
            exclude_type_aliases: true,
        };

        assert!(
            !filters.rejects(&statement(
                "self::types",
                ImportKind::ModuleDeclaration
            ))
        );
    }

    #[test]
    fn ordinary_imports_survive_the_filter() {
        let filters = EdgeFilters {
            exclude_type_aliases: true,
        };

        for import in [
            "crate::core::graph",
            "crate::foo::Bar",
            "std::collections::HashMap",
            // A name that merely starts with `*` is an ordinary item, not a
            // namespace import.
            "crate::a::b::*inner",
        ] {
            assert!(
                !filters.rejects(&statement(import, ImportKind::Dependency)),
                "{import} is a real dependency and must be kept"
            );
        }
    }

    #[test]
    fn excluding_type_aliases_removes_only_that_edge() {
        let temp_dir = tempdir().unwrap();
        for (relative, contents) in [
            (
                "src/main.rs",
                "mod util;\nuse crate::alias::Thing as Other;\nuse crate::util::u;\n",
            ),
            ("src/util.rs", "pub fn u() {}\n"),
            ("src/alias.rs", "pub struct Thing;\n"),
        ] {
            let absolute = temp_dir.path().join(relative);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, contents).unwrap();
        }

        let baseline = build_graph(
            temp_dir.path(),
            &IgnoreMatcher::empty(),
            &[],
            None,
            None,
        )
        .expect("baseline graph");

        let filtered = build_graph_with(
            temp_dir.path(),
            &IgnoreMatcher::empty(),
            &[],
            None,
            None,
            EdgeFilters {
                exclude_type_aliases: true,
            },
        )
        .expect("filtered graph");

        // Three edges in the baseline: the mod declaration plus two uses. The
        // alias use is the only one dropped.
        assert_eq!(baseline.metrics.total_internal_edges, 3);
        assert_eq!(filtered.metrics.total_internal_edges, 2);

        let kept: Vec<&str> = filtered
            .edges
            .iter()
            .filter(|edge| edge.resolved)
            .filter_map(|edge| edge.to.as_deref())
            .collect();
        assert!(
            kept.contains(&"src/util.rs"),
            "a real import must survive: {kept:?}"
        );
    }

    #[test]
    fn an_unresolved_local_path_is_not_counted_as_external() {
        // `std::io` and a `crate::` path the resolver failed to place were both
        // counted in `total_external_edges`, so the number could not be read: it
        // did not say which crates the project uses and which of its own files
        // are missing from the analysis.
        let report = graph_for(&[
            (
                "src/main.rs",
                "use std::io;\nuse crate::nowhere::Thing;\nfn main() {}\n",
            ),
            ("src/lib.rs", "pub fn f() {}\n"),
        ]);

        assert_eq!(
            report.metrics.total_external_edges, 1,
            "only std::io leaves the project"
        );
        assert_eq!(report.metrics.unresolved_local_edges, 1);
        assert_eq!(
            report.metrics.unresolved_local_samples,
            vec!["src/main.rs: crate::nowhere::Thing".to_owned()]
        );
    }

    #[test]
    fn a_resolvable_project_reports_no_unresolved_local_paths() {
        let report = graph_for(&[("src/main.rs", "fn main() {}\n")]);

        assert_eq!(report.metrics.unresolved_local_edges, 0);
        assert!(
            report.metrics.unresolved_local_samples.is_empty(),
            "nothing should be listed when nothing is unresolved"
        );
    }

    /// The four cases below are one behaviour seen from both sides, and the two
    /// halves are what keep it honest. `from .app import Flask` leaves two edges:
    /// `.app`, which resolves to `app.py` and carries the dependency, and
    /// `.app.Flask`, which resolves to nothing because `Flask` is a class inside
    /// `app.py` rather than a module beside it. Counting the second as a gap is
    /// what reported 162 of them on flask against 185 edges the resolver had
    /// actually placed -- a false alarm in the one metric the corpus says should
    /// only ever go down.
    ///
    /// Each fixture is a Python package because the shape is Python's: the
    /// extractor emits a submodule edge for every name in a `from X import` list
    /// so that `from pkg import absent` can still be reported.
    fn flask_package(extra_imports: &str) -> GraphReport {
        graph_for(&[
            (
                "pkg/__init__.py",
                &format!("from .app import Flask{extra_imports}"),
            ),
            (
                "pkg/app.py",
                "class Flask:\n    pass\n\n\ndef helper() -> int:\n    return 1\n",
            ),
        ])
    }

    #[test]
    fn a_name_the_module_declares_is_not_an_unresolved_local_path() {
        let report = flask_package("");

        assert_eq!(
            report.metrics.unresolved_local_edges, 0,
            "a class `app.py` declares is a name, not a missing module: {:?}",
            report.metrics.unresolved_local_samples
        );
        assert_eq!(
            imports_of(&report, "pkg/__init__.py"),
            vec!["pkg/app.py".to_owned()],
            "the dependency itself must still be there -- only the false alarm \
             is removed, not the edge"
        );
    }

    #[test]
    fn a_module_level_assignment_is_a_name_the_module_declares() {
        // Half the names on flask are this shape: `request = LocalProxy(...)` in
        // `globals.py`, `message_flashed = signals.signal(...)` in `signals.py`.
        // A rule reading only `def` and `class` would have left all 47 of them
        // counted as missing modules.
        let report = graph_for(&[
            ("pkg/__init__.py", "from .globals import request, missing\n"),
            ("pkg/globals.py", "request = object()\n"),
        ]);

        assert_eq!(
            report.metrics.unresolved_local_edges, 1,
            "only `missing`, which nothing declares: {:?}",
            report.metrics.unresolved_local_samples
        );
        assert_eq!(
            imports_of(&report, "pkg/__init__.py"),
            vec!["pkg/globals.py".to_owned()]
        );
    }

    #[test]
    fn a_name_nothing_declares_is_still_an_unresolved_local_path() {
        // The other direction, and the one that matters most. A fix that stopped
        // counting every unresolved submodule path would pass the two tests above
        // and fail this one, and `unresolved_local` would then mean "nothing",
        // which is worse than the false positives were.
        let report = flask_package(", Absent");

        assert_eq!(
            report.metrics.unresolved_local_edges, 1,
            "`app.py` declares `Flask` and `helper`, and no `Absent`"
        );
        assert_eq!(
            report.metrics.unresolved_local_samples,
            vec!["pkg/__init__.py: .app.Absent".to_owned()]
        );
    }

    #[test]
    fn a_local_variable_is_not_a_name_the_module_declares() {
        // `from module import name` reaches module scope and nothing below it, so
        // collecting assignment targets at any depth -- rather than at module
        // level only -- would hide a broken import behind a local variable.
        let report = graph_for(&[
            ("pkg/__init__.py", "from .app import config\n"),
            (
                "pkg/app.py",
                "def build() -> int:\n    config = 1\n    return config\n",
            ),
        ]);

        assert_eq!(
            report.metrics.unresolved_local_edges, 1,
            "`config` is assigned inside a function, so the import cannot succeed"
        );
    }

    #[test]
    fn a_decorated_definition_is_still_a_declaration() {
        // The grammar wraps `@cache` around a definition in a
        // `decorated_definition`, so flask's `_split_blueprint_path` is a
        // function two levels down rather than one. A collector that counted
        // definitions at module level only would miss it.
        let report = graph_for(&[
            ("pkg/__init__.py", "from .app import cached\n"),
            (
                "pkg/app.py",
                "from functools import cache\n\n\n@cache\ndef cached() -> int:\n    return 1\n",
            ),
        ]);

        assert_eq!(
            report.metrics.unresolved_local_edges, 0,
            "{:?}",
            report.metrics.unresolved_local_samples
        );
    }

    #[test]
    fn a_climbing_name_the_module_declares_is_not_an_unresolved_local_path() {
        // Flask's own shape: `from ..helpers import _split_blueprint_path` in
        // `pkg/sansio/app.py` reaches up two levels, so the parent of the name is
        // `..helpers` rather than `.helpers`. Splitting on the wrong separator
        // would leave the parent holding a dangling dot and resolve nothing, and
        // every one of these would go back to being a gap.
        let report = graph_for(&[
            ("pkg/sansio/app.py", "from ..helpers import split_path\n"),
            (
                "pkg/helpers.py",
                "def split_path(name: str) -> list[str]:\n    return [name]\n",
            ),
        ]);

        assert_eq!(
            report.metrics.unresolved_local_edges, 0,
            "{:?}",
            report.metrics.unresolved_local_samples
        );
        assert_eq!(
            imports_of(&report, "pkg/sansio/app.py"),
            vec!["pkg/helpers.py".to_owned()],
            "climbing up two levels must still land on the module"
        );
    }

    #[test]
    fn a_name_against_the_package_itself_is_still_an_unresolved_local_path() {
        // `from . import Flask` binds the name straight against the package, so
        // the path has no parent to split and nothing to ask. It stays a gap,
        // which is the two edges flask still reports after this fix.
        let report = graph_for(&[
            ("pkg/__init__.py", "class Flask:\n    pass\n"),
            ("pkg/cli.py", "from . import Flask\n"),
        ]);

        assert_eq!(report.metrics.unresolved_local_edges, 1);
    }

    #[test]
    fn unresolved_local_samples_are_capped() {
        // One badly misparsed file must not flood the report, so the sample list
        // is bounded while the count stays exact.
        let files: Vec<(String, String)> = (0..MAX_UNRESOLVED_LOCAL_SAMPLES
            + 5)
            .map(|index| {
                (
                    format!("src/f{index}.rs"),
                    format!("use crate::absent{index}::Thing;\n"),
                )
            })
            .collect();
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect();

        let report = graph_for(&borrowed);

        assert_eq!(
            report.metrics.unresolved_local_edges,
            u64::try_from(MAX_UNRESOLVED_LOCAL_SAMPLES + 5).unwrap_or(u64::MAX),
            "the count must stay exact even when the list is capped"
        );
        assert_eq!(
            report.metrics.unresolved_local_samples.len(),
            MAX_UNRESOLVED_LOCAL_SAMPLES,
            "the sample list must be capped"
        );
    }

    /// A parent module that declares a child, and a child that refers back to
    /// its parent with `super::`. This is ordinary Rust, not a dependency loop.
    fn nested_module_tree() -> Vec<(&'static str, &'static str)> {
        vec![
            ("src/main.rs", "mod util;\nfn main() {}\n"),
            ("src/util.rs", "mod helper;\npub fn u() {}\n"),
            ("src/util/helper.rs", "use super::u;\npub fn h() {}\n"),
        ]
    }

    #[test]
    fn a_parent_and_child_module_are_not_reported_as_a_cycle() {
        // Every nested Rust module with a `super::` reference produced a
        // phantom cycle, because the `mod` declaration closed the loop. Sephera's
        // own repository reported 38 cycles for this reason and 0 real ones.
        let report = graph_for(&nested_module_tree());

        assert_eq!(
            report.metrics.circular_dependencies, 0,
            "module structure is not a dependency cycle: {:?}",
            report.metrics.cycles
        );
    }

    #[test]
    fn a_real_import_cycle_is_still_reported() {
        // The point of excluding declarations is precision, not silence: a cycle
        // between two `use` statements must still be found.
        let report = graph_for(&[
            ("src/main.rs", "mod a;\nmod b;\nfn main() {}\n"),
            ("src/a.rs", "use crate::b::Thing;\npub struct A;\n"),
            ("src/b.rs", "use crate::a::A;\npub struct Thing;\n"),
        ]);

        assert_eq!(
            report.metrics.circular_dependencies, 1,
            "a genuine import cycle must be reported: {:?}",
            report.metrics.cycles
        );
    }

    #[test]
    fn a_module_declaration_is_still_reported_as_an_edge() {
        // Excluding declarations from cycle detection must not remove them from
        // the graph: they are the structure a reader wants to see.
        let report = graph_for(&nested_module_tree());

        assert!(
            report
                .edges
                .iter()
                .any(|edge| edge.kind == ImportKind::ModuleDeclaration
                    && edge.to.as_deref() == Some("src/util.rs")),
            "the `mod util;` edge must still be present: {:?}",
            report.edges
        );
    }

    #[test]
    fn no_edge_points_at_its_own_file() {
        // A resolver fallback used to answer an unresolved module with the
        // declaring file, which reported a resolved self-edge.
        let report = graph_for(&[
            ("src/main.rs", "mod missing;\nfn main() {}\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
        ]);

        let self_loops: Vec<&str> = report
            .edges
            .iter()
            .filter(|edge| {
                edge.from == *edge.to.as_ref().unwrap_or(&String::new())
            })
            .map(|edge| edge.import_path.as_str())
            .collect();
        assert_eq!(
            self_loops,
            Vec::<&str>::new(),
            "a file never depends on itself"
        );
    }

    /// Build a graph over an in-memory tree and return the report.
    fn graph_for(files: &[(&str, &str)]) -> GraphReport {
        let temp_dir = tempdir().unwrap();
        for (relative, contents) in files {
            let absolute = temp_dir.path().join(relative);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, contents).unwrap();
        }

        build_graph(temp_dir.path(), &IgnoreMatcher::empty(), &[], None, None)
            .expect("graph build must succeed")
    }

    /// Paths that a file imports, according to the resolved edge list.
    fn imports_of(report: &GraphReport, source: &str) -> Vec<String> {
        report
            .edges
            .iter()
            .filter(|edge| edge.from == source && edge.resolved)
            .filter_map(|edge| edge.to.clone())
            .collect()
    }

    #[test]
    fn a_reverse_query_reaches_the_whole_transitive_closure() {
        // `mid` imports `leaf`, `top` imports `mid`, so `leaf`'s blast radius is
        // both `mid` and `top`. A reverse query that returned only the direct
        // importer would answer half the question and look complete doing it.
        let temp_dir = tempdir().unwrap();
        for (relative, contents) in [
            ("src/leaf.rs", "pub fn leaf() {}\n"),
            ("src/mid.rs", "use crate::leaf;\npub fn mid() {}\n"),
            ("src/top.rs", "use crate::mid;\npub fn top() {}\n"),
            ("src/lib.rs", "pub mod leaf;\npub mod mid;\npub mod top;\n"),
        ] {
            let absolute = temp_dir.path().join(relative);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, contents).unwrap();
        }

        let report = build_graph(
            temp_dir.path(),
            &IgnoreMatcher::empty(),
            &[],
            None,
            Some(GraphQuery::DependsOn("src/leaf.rs".to_owned())),
        )
        .expect("graph build must succeed");

        let mut paths: Vec<String> = report
            .nodes
            .iter()
            .map(|node| node.file_path.clone())
            .collect();
        paths.sort();

        // `src/lib.rs` is in the answer because `pub mod leaf;` is a real edge:
        // deleting `leaf.rs` breaks the crate root as surely as it breaks `mid`.
        // It was missing before module declarations were admitted to the
        // adjacency, which is how `ignore.rs` lost 17 of its 34 real dependents
        // on this repository.
        assert_eq!(
            paths,
            vec![
                "src/leaf.rs".to_owned(),
                "src/lib.rs".to_owned(),
                "src/mid.rs".to_owned(),
                "src/top.rs".to_owned(),
            ],
            "a reverse query must include transitive dependents and the \
             parent that declares them, not only direct importers"
        );
    }

    #[test]
    fn a_module_declaration_does_not_close_a_cycle() {
        // `lib.rs` declares `mod service;` and `service.rs` refers back to its
        // parent with `crate::`. Both edges are real and the blast radius wants
        // both, but no edit can break the pair apart, so calling it a cycle would
        // be a claim the report cannot act on.
        let temp_dir = tempdir().unwrap();
        for (relative, contents) in [
            ("src/lib.rs", "pub mod service;\n"),
            ("src/service.rs", "use crate::Config;\npub fn run() {}\n"),
        ] {
            let absolute = temp_dir.path().join(relative);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, contents).unwrap();
        }

        let report = build_graph(
            temp_dir.path(),
            &IgnoreMatcher::empty(),
            &[],
            None,
            None,
        )
        .expect("graph build must succeed");

        assert_eq!(
            report.metrics.circular_dependencies, 0,
            "a forced module relationship is not a cycle an edit can fix"
        );
    }

    #[test]
    fn mod_declarations_create_internal_edges() {
        // `mod util;` is a compile-time dependency: editing util.rs forces a
        // rebuild of the module that declares it, so it must appear as an edge.
        // It previously produced none, which made a `mod`-only crate look
        // completely disconnected.
        let report = graph_for(&[
            ("src/main.rs", "mod util;\nfn main() {}\n"),
            ("src/util.rs", "pub fn helper() {}\n"),
        ]);

        let imports = imports_of(&report, "src/main.rs");

        assert_eq!(
            imports,
            vec!["src/util.rs".to_owned()],
            "a mod declaration must resolve to its file"
        );
    }

    #[test]
    fn a_file_that_is_not_utf8_is_a_node_and_contributes_nothing() {
        // `use crate::` followed by bytes that are not a name is not an import.
        // Tree-sitter recovers rather than failing, so the fragment used to reach
        // the module walk as a bare `crate` and resolve to the file it was
        // written in -- a self-dependency manufactured from bytes that are not a
        // source file.
        //
        // The file stays a node, because a file the tool cannot read is still a
        // file in the tree, and it must not take the rest of the graph with it.
        let temp_dir = tempdir().unwrap();
        for (relative, contents) in [
            ("src/main.rs", "mod broken;\npub struct Config;\n"),
            ("src/leaf.rs", "use crate::Config;\npub fn leaf() {}\n"),
        ] {
            let absolute = temp_dir.path().join(relative);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, contents).unwrap();
        }
        // `broken.rs` opens with a real `use` and then stops being text.
        fs::write(
            temp_dir.path().join("src/broken.rs"),
            b"use crate::\xff\xfe;\n",
        )
        .unwrap();

        let report = build_graph(
            temp_dir.path(),
            &IgnoreMatcher::empty(),
            &[],
            None,
            None,
        )
        .expect("graph build must succeed");

        assert!(
            report
                .nodes
                .iter()
                .any(|node| node.file_path == "src/broken.rs"),
            "a file whose bytes cannot be decoded is still a node: {:?}",
            report
                .nodes
                .iter()
                .map(|n| &n.file_path)
                .collect::<Vec<_>>()
        );
        assert!(
            imports_of(&report, "src/broken.rs").is_empty(),
            "no edge can be claimed from bytes that are not a source file: {:?}",
            report
                .edges
                .iter()
                .filter(|edge| edge.from == "src/broken.rs")
                .collect::<Vec<_>>()
        );
        assert_eq!(
            imports_of(&report, "src/leaf.rs"),
            vec!["src/main.rs".to_owned()],
            "one unreadable file must not cost the graph its other edges"
        );
    }

    #[test]
    fn a_mod_only_crate_is_fully_connected() {
        let report = graph_for(&[
            ("src/main.rs", "mod a;\nmod b;\nfn main() {}\n"),
            ("src/a.rs", "pub fn a() {}\n"),
            ("src/b.rs", "pub fn b() {}\n"),
        ]);

        assert_eq!(report.metrics.total_internal_edges, 2);
        assert_eq!(report.metrics.circular_dependencies, 0);
    }

    #[test]
    fn inline_modules_are_not_edges() {
        // An inline module declares no file, so there is nothing to point at.
        let report = graph_for(&[(
            "src/main.rs",
            "mod inner {\n    pub fn f() {}\n}\nfn main() {}\n",
        )]);

        assert_eq!(
            report.metrics.total_internal_edges, 0,
            "an inline module has no file to import"
        );
    }

    #[test]
    fn a_path_attribute_module_is_skipped() {
        // With `#[path = "..."]` the declared name no longer matches the file,
        // so reporting `crate::util` would invent an edge that cannot resolve.
        let report = graph_for(&[
            (
                "src/main.rs",
                "#[path = \"other/renamed.rs\"]\nmod util;\nfn main() {}\n",
            ),
            ("src/other/renamed.rs", "pub fn r() {}\n"),
        ]);

        assert_eq!(
            report.metrics.total_internal_edges, 0,
            "a renamed module must not resolve against the declared name"
        );
    }

    #[test]
    fn mod_and_use_together_both_resolve() {
        let report = graph_for(&[
            (
                "src/main.rs",
                "mod util;\nuse crate::helper::h;\nfn main() {}\n",
            ),
            ("src/util.rs", "pub fn u() {}\n"),
            ("src/helper.rs", "pub fn h() {}\n"),
        ]);

        let mut imports = imports_of(&report, "src/main.rs");
        imports.sort();

        assert_eq!(
            imports,
            vec!["src/helper.rs".to_owned(), "src/util.rs".to_owned()]
        );
    }

    #[test]
    fn mod_resolves_through_mod_rs() {
        let report = graph_for(&[
            ("src/lib.rs", "mod graph;\n"),
            ("src/graph/mod.rs", "pub mod types;\n"),
            ("src/graph/types.rs", "pub struct T;\n"),
        ]);

        assert_eq!(
            imports_of(&report, "src/lib.rs"),
            vec!["src/graph/mod.rs".to_owned()]
        );
        assert_eq!(
            imports_of(&report, "src/graph/mod.rs"),
            vec!["src/graph/types.rs".to_owned()],
            "a nested mod must also resolve"
        );
    }

    fn write_file(base_dir: &Path, relative_path: &str, contents: &str) {
        let absolute_path = base_dir.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute_path, contents).unwrap();
    }

    fn node_map_from_edges(edges: &[(&str, &[&str])]) -> NodeMap {
        let mut map: NodeMap = BTreeMap::new();
        for (node, imports) in edges {
            let entry = map.entry((*node).to_owned()).or_default();
            for import in *imports {
                entry.imports.push((*import).to_owned());
            }
        }
        map
    }

    #[test]
    fn cycle_key_ignores_rotation_and_direction() {
        let a = vec![
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "a".to_owned(),
        ];
        let rotated = vec![
            "b".to_owned(),
            "c".to_owned(),
            "a".to_owned(),
            "b".to_owned(),
        ];

        assert_eq!(cycle_key(&a), cycle_key(&rotated));
    }

    #[test]
    fn cycle_key_distinguishes_different_node_sets() {
        let a = vec!["a".to_owned(), "b".to_owned(), "a".to_owned()];
        let b = vec!["a".to_owned(), "c".to_owned(), "a".to_owned()];

        assert_ne!(cycle_key(&a), cycle_key(&b));
    }

    #[test]
    fn detects_a_two_node_cycle() {
        let map =
            node_map_from_edges(&[("a.rs", &["b.rs"]), ("b.rs", &["a.rs"])]);

        let cycles = detect_cycles(&map);

        assert_eq!(cycles.len(), 1, "got {cycles:?}");
    }

    #[test]
    fn a_ring_is_reported_once_not_once_per_member() {
        // Three files in a ring. Every member can start the traversal and reach
        // the same ring, which previously produced duplicate entries.
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs"]),
            ("b.rs", &["c.rs"]),
            ("c.rs", &["a.rs"]),
        ]);

        let cycles = detect_cycles(&map);

        assert_eq!(
            cycles.len(),
            1,
            "a 3-node ring must report once: {cycles:?}"
        );
    }

    #[test]
    fn a_self_importing_file_is_one_cycle() {
        let map = node_map_from_edges(&[("a.rs", &["a.rs"])]);

        assert_eq!(detect_cycles(&map).len(), 1);
    }

    #[test]
    fn distinct_rings_are_reported_separately() {
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs"]),
            ("b.rs", &["a.rs"]),
            ("x.rs", &["y.rs"]),
            ("y.rs", &["x.rs"]),
        ]);

        assert_eq!(detect_cycles(&map).len(), 2);
    }

    #[test]
    fn acyclic_graph_reports_no_cycles() {
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs"]),
            ("b.rs", &["c.rs"]),
            ("c.rs", &[]),
        ]);

        assert_eq!(detect_cycles(&map), Vec::<Vec<String>>::new());
    }

    #[test]
    fn detection_is_deterministic_across_repeated_runs() {
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs", "c.rs"]),
            ("b.rs", &["c.rs", "a.rs"]),
            ("c.rs", &["a.rs", "b.rs"]),
        ]);

        let first = detect_cycles(&map);
        let second = detect_cycles(&map);

        assert_eq!(first, second);
    }

    #[test]
    fn resolves_rust_imports_for_crate_self_and_super_modules() {
        let files = known_files(&[
            "src/main.rs",
            "src/core/graph.rs",
            "src/core/graph/mod.rs",
            "src/core/graph/parser.rs",
            "src/core/graph/types.rs",
        ]);

        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "crate::core::graph",
                "src/main.rs",
                &files
            ),
            Some("src/core/graph.rs".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "self::types",
                "src/core/graph/mod.rs",
                &files
            ),
            Some("src/core/graph/types.rs".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "super::types",
                "src/core/graph/parser.rs",
                &files
            ),
            Some("src/core/graph/types.rs".to_owned())
        );
    }

    #[test]
    fn an_unqualified_rust_path_resolves_to_the_local_module() {
        // Rust 2018 uniform paths: `pub use types::{A};` inside `code_loc.rs`
        // names `code_loc/types.rs`, not a crate called `types`. Treating it as
        // external was worth 105 wrong edges on this repository and put junk like
        // `types` at the top of the dependency table.
        let files = known_files(&[
            "src/main.rs",
            "src/core/code_loc.rs",
            "src/core/code_loc/analyzer.rs",
            "src/core/code_loc/types.rs",
            "src/types.rs",
        ]);

        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "types::CodeLocReport",
                "src/core/code_loc.rs",
                &files,
            ),
            Some("src/core/code_loc/types.rs".to_owned()),
            "the module beside the importing file wins over the crate root"
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "analyzer::CodeLoc",
                "src/core/code_loc.rs",
                &files,
            ),
            Some("src/core/code_loc/analyzer.rs".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "serde::Serialize",
                "src/core/code_loc.rs",
                &files,
            ),
            None,
            "with no local module of that name the path is an external crate"
        );
    }

    #[test]
    fn a_parent_referring_to_its_own_child_is_not_a_cycle() {
        // Rust compiles a child module as part of its parent, so `mod context;`
        // and `pub use context::{...}` say the same thing. Counting only the
        // first form left the second looking like a dependency, which closed a
        // loop with the child's own `super::` reference.
        assert!(
            is_own_child_module(
                "src/core/runtime.rs",
                "src/core/runtime/context.rs"
            ),
            "a parent to its declared child is structural"
        );
        assert!(
            is_own_child_module(
                "src/core/code_loc.rs",
                "src/core/code_loc/analyzer.rs"
            ),
            "however the child is named"
        );
        assert!(
            !is_own_child_module("src/core/graph.rs", "src/core/types.rs"),
            "a sibling module is a real dependency"
        );
        assert!(
            !is_own_child_module("src/core/context.rs", "src/core.rs"),
            "a child naming its parent is a real reference, which is why the \\
             declaration edge is the one excluded"
        );
        assert!(
            is_own_child_module("src/core.rs", "src/core/context.rs"),
            "a module file owns the directory beside it"
        );
        assert!(
            is_own_child_module("src/core/mod.rs", "src/core/graph.rs"),
            "and so does mod.rs"
        );
    }

    #[test]
    fn a_path_naming_a_reachability_resolves_to_the_declaring_file() {
        // `use crate::Router;` names a type, not a file called `Router`. The
        // crate root re-exports it, so the crate root is where the reference
        // lands. These were 66 unresolved edges on axum before the lookup.
        let files = known_files(&[
            "axum/src/lib.rs",
            "axum/src/boxed.rs",
            "axum/src/routing/mod.rs",
            "axum-core/src/lib.rs",
            "axum-core/src/body.rs",
        ]);

        let mut index = declarations::DeclarationIndex::default();
        index.insert(
            "axum/src/lib.rs",
            // `pub use self::routing::Router;`
            declarations::DeclaredNames::from_names(["Router"]),
        );
        index.insert(
            "axum/src/routing/mod.rs",
            declarations::DeclaredNames::from_names(["Router"]),
        );
        // `pub type BoxError = ...` sits directly in the root, with no re-export.
        index.insert(
            "axum-core/src/lib.rs",
            declarations::DeclaredNames::from_names(["BoxError"]),
        );

        assert_eq!(
            resolve_with_index(
                "crate::Router",
                "axum/src/boxed.rs",
                &files,
                &index,
            ),
            Some("axum/src/lib.rs".to_owned()),
            "a name the crate root re-exports points at the crate root"
        );
        assert_eq!(
            resolve_with_index(
                "crate::BoxError",
                "axum-core/src/body.rs",
                &files,
                &index,
            ),
            Some("axum-core/src/lib.rs".to_owned()),
            "a name the crate root declares directly points there too"
        );
    }

    #[test]
    fn a_super_path_naming_a_name_this_file_declares_resolves_to_it() {
        // `use super::JsonLines;` in the `mod tests` of
        // `axum-extra/src/json_lines.rs`, where the struct is declared at line 61
        // of that file and used at line 177. The reference is real, so the
        // fallback has to accept it: a name this file declares is an item of
        // this file, and `names_a_crate_outside` is what keeps it from being
        // filed as a crate from outside.
        //
        // The index is built by hand rather than parsed, because the struct sits
        // inside a `pin_project! { }` invocation and tree-sitter records no
        // item inside a `token_tree` --
        // `a_struct_inside_a_macro_invocation_is_not_declared` in
        // `declarations.rs` asserts that limitation rather than a fix. This test
        // is about what resolution does once the index knows the name, which is
        // the half that is the resolver's to answer.
        let files = known_files(&["src/json_lines.rs", "src/lib.rs"]);

        let mut index = declarations::DeclarationIndex::default();
        index.insert(
            "src/json_lines.rs",
            declarations::DeclaredNames::from_names(["JsonLines"]),
        );

        let resolved = resolve_import_with(
            "super::JsonLines",
            "src/json_lines.rs",
            1,
            ImportKind::Dependency,
            SupportedLanguage::Rust,
            &ResolutionInputs {
                base_path: std::path::PathBuf::new(),
                known_files: files,
                declarations: index,
                manifests: manifests::ManifestIndex::default(),
            },
        );

        assert_eq!(
            resolved.file().map(ToOwned::to_owned),
            Some("src/json_lines.rs".to_owned()),
            "a `super::` path naming a struct in the same file is a self-reference"
        );
    }

    #[test]
    fn a_private_use_does_not_make_a_name_reachable_from_the_crate_root() {
        // `use crate::service;` in `main.rs` binds `service` inside `main.rs`.
        // Treating every `use` as a re-export resolved `crate::service` to
        // `main.rs` itself -- a file depending on itself -- and a CLI test with
        // three files caught it.
        let files = known_files(&["src/main.rs", "src/service.rs"]);
        let mut index = declarations::DeclarationIndex::default();
        index.insert("src/main.rs", declarations::DeclaredNames::default());
        index.insert(
            "src/service.rs",
            declarations::DeclaredNames::from_names(["run"]),
        );

        assert_eq!(
            resolve_with_index("crate::service", "src/main.rs", &files, &index),
            Some("src/service.rs".to_owned()),
            "the module walk answers this one; no name lookup is needed"
        );
    }

    #[test]
    fn an_unqualified_path_never_resolves_through_a_name_lookup() {
        // `use axum::Router;` in an example's `main.rs` names a *different*
        // crate. Without this guard the crate-root check found `Router` in the
        // example's own imports and resolved every example's first `use` to
        // itself: 934 invented self-edges on axum.
        let files =
            known_files(&["examples/demo/src/main.rs", "src/routing/mod.rs"]);
        let mut index = declarations::DeclarationIndex::default();
        index.insert(
            "examples/demo/src/main.rs",
            declarations::DeclaredNames::from_names(["Router"]),
        );
        index.insert(
            "src/routing/mod.rs",
            declarations::DeclaredNames::from_names(["Router"]),
        );

        assert_eq!(
            resolve_with_index(
                "axum::Router",
                "examples/demo/src/main.rs",
                &files,
                &index,
            ),
            None,
            "an unqualified path names a crate, not this file's own module"
        );
    }

    #[test]
    fn a_structural_edge_stays_in_the_graph_and_leaves_the_cycle_walk() {
        // The two uses of the adjacency conflict. Blast radius wants the edge:
        // editing `inner.rs` does affect `lib.rs`. A cycle claim through the
        // module tree is not something an edit can resolve, so the walk drops it.
        // Filtering when the graph was built made `--what-depends-on` answer
        // "nothing" for a real dependency, which two CLI tests caught.
        let mut node_map: NodeMap = BTreeMap::new();
        // `lib.rs` owns both of these, so neither edge can close a loop.
        node_map.entry("src/lib.rs".to_owned()).or_default().imports =
            vec!["src/inner.rs".to_owned(), "src/routing.rs".to_owned()];
        node_map
            .entry("src/inner.rs".to_owned())
            .or_default()
            .imports = vec!["src/lib.rs".to_owned()];
        node_map
            .entry("src/routing.rs".to_owned())
            .or_default()
            .imports = vec!["src/lib.rs".to_owned()];
        // Siblings: no parent, no ancestor, so this one is a real dependency.
        node_map
            .entry("src/routing.rs".to_owned())
            .or_default()
            .imports
            .push("src/inner.rs".to_owned());

        let walk = structural_adjacency(&node_map);

        assert_eq!(
            node_map["src/lib.rs"].imports.len(),
            2,
            "both dependencies are reported, so a blast-radius query can use them"
        );
        assert_eq!(
            walk["src/lib.rs"].imports,
            Vec::<String>::new(),
            "a parent naming its children is structural in both directions"
        );
        assert_eq!(
            walk["src/inner.rs"].imports,
            Vec::<String>::new(),
            "a child naming its ancestor is structural the other way"
        );
        assert_eq!(
            walk["src/routing.rs"].imports,
            vec!["src/inner.rs".to_owned()],
            "between siblings the edge is a dependency and must survive"
        );
    }

    fn resolve_with_index(
        import_path: &str,
        source_file: &str,
        files: &plugins::KnownFiles,
        index: &declarations::DeclarationIndex,
    ) -> Option<String> {
        resolve_import_with(
            import_path,
            source_file,
            0,
            ImportKind::Dependency,
            SupportedLanguage::Rust,
            &ResolutionInputs {
                base_path: std::path::PathBuf::new(),
                known_files: files.clone(),
                declarations: index.clone(),
                manifests: manifests::ManifestIndex::default(),
            },
        )
        .file()
        .map(ToOwned::to_owned)
    }

    #[test]
    fn resolves_python_imports_for_absolute_and_relative_modules() {
        let files = known_files(&[
            "pkg/local.py",
            "pkg/sub/module.py",
            "pkg/shared/util.py",
            "pkg/shared/__init__.py",
        ]);

        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                "pkg.local",
                "pkg/sub/module.py",
                &files
            ),
            Some("pkg/local.py".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                ".local",
                "pkg/module.py",
                &files
            ),
            Some("pkg/local.py".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                "..shared.util",
                "pkg/sub/module.py",
                &files
            ),
            Some("pkg/shared/util.py".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                "requests",
                "pkg/sub/module.py",
                &files
            ),
            None
        );
    }

    #[test]
    fn resolves_js_ts_imports_with_relative_paths_and_index_files() {
        let files = known_files(&[
            "src/main.ts",
            "src/utils.ts",
            "src/lib/index.ts",
            "src/components/button.jsx",
        ]);

        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::TypeScript,
                "./utils",
                "src/main.ts",
                &files
            ),
            Some("src/utils.ts".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::TypeScript,
                "../lib",
                "src/features/item.ts",
                &files
            ),
            Some("src/lib/index.ts".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::TypeScript,
                "react",
                "src/main.ts",
                &files
            ),
            None
        );
    }

    #[test]
    fn resolves_go_java_and_c_family_imports() {
        let go_files =
            known_files(&["internal/app/main.go", "internal/pkg/service.go"]);
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Go,
                "github.com/demo/pkg",
                "internal/app/main.go",
                &go_files
            ),
            Some("internal/pkg/service.go".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Go,
                "fmt",
                "internal/app/main.go",
                &go_files
            ),
            None
        );

        let java_files = known_files(&[
            "src/com/example/utils/Helper.java",
            "utils/Helper.java",
        ]);
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Java,
                "com.example.utils.Helper",
                "src/com/example/Main.java",
                &java_files
            ),
            Some("src/com/example/utils/Helper.java".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Java,
                "utils.Helper",
                "src/com/example/Main.java",
                &java_files
            ),
            Some("utils/Helper.java".to_owned())
        );

        let c_files = known_files(&["src/main.c", "include/util.h", "util.h"]);
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::C,
                "util.h",
                "src/main.c",
                &c_files
            ),
            Some("util.h".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::C,
                "<stdio.h>",
                "src/main.c",
                &c_files
            ),
            None
        );
    }

    #[test]
    fn builds_graph_for_rust_project() {
        let temp_dir = tempdir().unwrap();
        write_file(
            temp_dir.path(),
            "src/main.rs",
            "use crate::utils;\n\nfn main() {}\n",
        );
        write_file(
            temp_dir.path(),
            "src/utils.rs",
            "pub fn helper() -> bool { true }\n",
        );

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.nodes.len() >= 2);
        assert_ne!(
            report.edges,
            Vec::<GraphEdge>::new(),
            "expected at least one edge"
        );
    }

    #[test]
    fn detects_circular_dependency() {
        let temp_dir = tempdir().unwrap();

        write_file(temp_dir.path(), "a.rs", "use crate::b;\n\npub fn a() {}\n");
        write_file(temp_dir.path(), "b.rs", "use crate::a;\n\npub fn b() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(
            report.metrics.circular_dependencies > 0,
            "should detect circular dependency between a.rs and b.rs"
        );
    }

    #[test]
    fn nested_rust_super_imports_resolve_to_sibling_module() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/lib.rs", "mod outer;\n");
        write_file(
            temp_dir.path(),
            "src/outer/mod.rs",
            "mod parser;\nmod types;\n",
        );
        write_file(
            temp_dir.path(),
            "src/outer/parser.rs",
            "use super::types::Token;\n\npub fn parse(_: Token) {}\n",
        );
        write_file(
            temp_dir.path(),
            "src/outer/types.rs",
            "pub struct Token;\n",
        );

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.edges.iter().any(|edge| {
            edge.from == "src/outer/parser.rs"
                && edge.to.as_deref() == Some("src/outer/types.rs")
                && edge.resolved
        }));
    }

    #[test]
    fn parent_relative_python_imports_resolve_across_packages() {
        let temp_dir = tempdir().unwrap();
        write_file(
            temp_dir.path(),
            "pkg/app/main.py",
            "from ..shared.util import helper\n",
        );
        write_file(
            temp_dir.path(),
            "pkg/shared/util.py",
            "def helper() -> None:\n    pass\n",
        );

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.edges.iter().any(|edge| {
            edge.from == "pkg/app/main.py"
                && edge.to.as_deref() == Some("pkg/shared/util.py")
                && edge.resolved
        }));
    }

    #[test]
    fn depth_zero_keeps_the_traversal_root_and_nothing_else() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "use crate::middle;\n");
        write_file(temp_dir.path(), "src/middle.rs", "use crate::leaf;\n");
        write_file(temp_dir.path(), "src/leaf.rs", "pub fn leaf() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report = build_graph(
            temp_dir.path(),
            &ignore,
            &[PathBuf::from("src/main.rs")],
            Some(0),
            None,
        )
        .unwrap();

        let node_paths: BTreeSet<_> = report
            .nodes
            .iter()
            .map(|node| node.file_path.as_str())
            .collect();
        assert_eq!(node_paths, BTreeSet::from(["src/main.rs"]));
        assert_eq!(report.metrics.total_files, 1);
    }

    #[test]
    fn depth_one_keeps_the_root_and_what_it_reaches_directly() {
        // What `--depth 0` used to mean on a forward query, and what a reader
        // reaching for "the roots and their immediate dependencies" writes now.
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "use crate::middle;\n");
        write_file(temp_dir.path(), "src/middle.rs", "use crate::leaf;\n");
        write_file(temp_dir.path(), "src/leaf.rs", "pub fn leaf() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report = build_graph(
            temp_dir.path(),
            &ignore,
            &[PathBuf::from("src/main.rs")],
            Some(1),
            None,
        )
        .unwrap();

        let node_paths: BTreeSet<_> = report
            .nodes
            .iter()
            .map(|node| node.file_path.as_str())
            .collect();
        assert_eq!(
            node_paths,
            BTreeSet::from(["src/main.rs", "src/middle.rs"])
        );
        assert_eq!(report.metrics.total_files, 2);
        assert_eq!(report.metrics.total_internal_edges, 1);
        assert_eq!(report.metrics.most_imported[0].file_path, "src/middle.rs");
    }

    #[test]
    fn depends_on_query_at_depth_one_returns_the_direct_importers() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "use crate::service;\n");
        write_file(temp_dir.path(), "src/service.rs", "use crate::util;\n");
        write_file(temp_dir.path(), "src/util.rs", "pub fn util() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report = build_graph(
            temp_dir.path(),
            &ignore,
            &[],
            Some(1),
            Some(GraphQuery::DependsOn("src/util.rs".to_owned())),
        )
        .unwrap();

        let node_paths: BTreeSet<_> = report
            .nodes
            .iter()
            .map(|node| node.file_path.as_str())
            .collect();

        // `main.rs` reaches `util.rs` in two hops, so it is outside depth 1. It
        // used to be inside: `traverse_graph` added one to the requested depth,
        // which made `graph --depth 1` a wider answer than `impact --depth 1`
        // for the same question. The target itself stays in the report even
        // though nothing reaches it in one hop, so the answer reads as "nothing
        // directly imports this" rather than as "this does not exist".
        assert_eq!(
            node_paths,
            BTreeSet::from(["src/service.rs", "src/util.rs"])
        );
        assert_eq!(
            report.query,
            Some(GraphQuery::DependsOn("src/util.rs".to_owned()))
        );
        assert_eq!(report.depth, Some(1));
    }

    #[test]
    fn depends_on_query_requires_existing_node() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "fn main() {}\n");

        let ignore = IgnoreMatcher::empty();
        let error = build_graph(
            temp_dir.path(),
            &ignore,
            &[],
            None,
            Some(GraphQuery::DependsOn("src/missing.rs".to_owned())),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("did not resolve to an analyzed graph node")
        );
    }

    #[test]
    fn simplify_relative_path_works() {
        assert_eq!(
            plugins::paths::resolve_relative("src/core", "../utils"),
            "src/utils"
        );
        assert_eq!(
            plugins::paths::resolve_relative("src", "./helpers"),
            "src/helpers"
        );
        assert_eq!(plugins::paths::resolve_relative("", "./foo"), "foo");
    }

    #[test]
    fn empty_directory_produces_empty_graph() {
        let temp_dir = tempdir().unwrap();
        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert_eq!(report.nodes, Vec::<GraphNode>::new());
        assert_eq!(report.edges, Vec::<GraphEdge>::new());
    }
}
