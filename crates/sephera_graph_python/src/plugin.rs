//! Python module resolution.
//!
//! Leading dots encode relative depth: `.mod` is the current package, `..mod`
//! its parent. Counting them is what distinguishes a relative import from an
//! absolute one, because everything after the dots is a plain module path in
//! both cases.

use sephera_core::{
    path_utils as paths,
    plugins::{ImportPlugin, ResolveContext, ResolverPlugin},
};
/// Python import extraction and resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonPlugin;

impl ImportPlugin for PythonPlugin {
    /// Python's imports can name something other than a module.
    ///
    /// `from .app import Flask` names a class, and `from .globals import request`
    /// names a module-level assignment. Neither has a file to resolve to, and
    /// without this the resolver reported every one of them as a path that meant
    /// to name a project file and could not -- 162 on flask, against 185 edges it
    /// had resolved. The dependency was already recorded by the sibling import
    /// (`.app`), so these were pure false alarms in the one metric the corpus
    /// says should only ever go down.
    fn collect_declarations(
        &self,
        source: &[u8],
        tree: &tree_sitter::Tree,
    ) -> Option<sephera_core::declarations::DeclaredNames> {
        Some(crate::extract::declared_names(source, tree))
    }

    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<sephera_core::types::ImportStatement>> {
        crate::extract::extract_from_node(source, node)
    }
}

impl ResolverPlugin for PythonPlugin {
    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        let levels =
            import_path.bytes().take_while(|byte| *byte == b'.').count();
        let module_path = paths::replace_separator(&import_path[levels..], '.');

        if levels > 0 {
            let package =
                ascend(&paths::parent(context.source_file), levels - 1);

            // A path made only of dots names the package and nothing else:
            // `from . import x` in `pkg/sub/deep.py` names `pkg/sub`.
            //
            // `first_existing` cannot answer that, and its last resort is the
            // reason. It matches any directory with files under it, and the
            // directory an import has just climbed to is one by construction --
            // so `from ... import x` could never come back empty, and answered
            // with whichever file happened to sort first underneath it. In the
            // end-to-end fixture that was `python/orphan.py`, for a statement
            // naming a directory two levels above its own package.
            //
            // Whether something is a package is a question about `__init__.py`,
            // and that is the only evidence there is for it.
            if module_path.is_empty() {
                let package_root = paths::join(&package, &["__init__.py"]);
                return context.contains(&package_root).then_some(package_root);
            }

            let relative = paths::join(&package, &[&module_path]);
            // A relative import that does not resolve is a local gap, not an
            // external dependency: the import names something that should exist
            // in this project's package structure and does not.
            return first_existing(context, &relative);
        }

        // Absolute import. Two roots, because Python has two: the analysis base,
        // and the directory a module in this package would have on `sys.path`.
        //
        // Only the first was tried, so `from pkg.sub.value import VALUE` inside
        // `pkg/sub/value.py`'s own package produced no edge at all when `pkg`
        // sat below the base rather than at it. The relative forms walked up
        // correctly the whole time, which is why the walk worked in one direction
        // only.
        first_existing(context, &module_path).or_else(|| {
            let rooted = paths::join(&import_root(context), &[&module_path]);
            first_existing(context, &rooted)
        })
    }

    /// Whether `name` names a module this analysis can see.
    ///
    /// A Python absolute import is spelled identically whether it leaves the
    /// project or not: `collections.OrderedDict` and a broken `pkg.absent` are
    /// both a dotted path with no leading dot, and nothing in the shape says
    /// which. So the question cannot be answered from the path and is answered
    /// here instead -- by whether a file of that name is in the analysis, under
    /// either root the language would search.
    ///
    /// The default in the trait says "cannot tell", so the languages that were
    /// not counting their bare specifiers as gaps go on not counting them.
    fn names_a_known_module(
        &self,
        name: &str,
        context: ResolveContext<'_>,
    ) -> bool {
        if name.is_empty() {
            return false;
        }

        first_existing(context, name).is_some()
            || first_existing(
                context,
                &paths::join(&import_root(context), &[name]),
            )
            .is_some()
    }
}

/// Climb `levels` package directories.
fn ascend(base: &str, levels: usize) -> String {
    let mut result = base.to_owned();

    for _ in 0..levels {
        let parent = paths::parent(&result);
        if parent == result {
            break;
        }
        result = parent;
    }

    result
}

/// The directory Python would search for an absolute import made in a file.
///
/// Python puts a package's *parent* on `sys.path`, not the package itself, so
/// `from pkg.sub.value import VALUE` written inside `pkg/sub/deep.py` resolves
/// against the directory holding `pkg` -- which is usually not the analysis
/// base, since a repository rarely puts its packages at the top.
///
/// That directory is the parent of the **topmost** package on the way to the
/// file, not the first one that stops looking like a package. PEP 420 makes a
/// directory without an `__init__.py` importable as a namespace package, and
/// those are common inside real packages: flask's `src/flask/sansio/` has no
/// `__init__.py` of its own and still belongs to `flask`. Stopping at the first
/// directory that lacks one puts the root *inside* the package, and every
/// absolute import written from beneath it stops resolving.
///
/// A file outside any package has no topmost package, and returns the analysis
/// base -- which makes the second attempt identical to the first. Nothing is
/// lost by asking, and a bare module beside the source stays unresolved rather
/// than becoming a dependency Python has no package structure to justify.
fn import_root(context: ResolveContext<'_>) -> String {
    let mut directory = paths::parent(context.source_file);
    let mut topmost_package: Option<String> = None;

    loop {
        let parent = paths::parent(&directory);
        if parent == directory {
            break;
        }

        if context.contains(&paths::join(&directory, &["__init__.py"])) {
            topmost_package = Some(directory.clone());
        }

        directory = parent;
    }

    topmost_package.map_or(directory, |package| paths::parent(&package))
}

/// Try `path.py`, then `path/__init__.py`.
fn first_existing(context: ResolveContext<'_>, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let module = format!("{path}.py");
    if context.contains(&module) {
        return Some(module);
    }

    let package = paths::join(path, &["__init__.py"]);
    if context.contains(&package) {
        return Some(package);
    }

    // A directory named after the module, with its own package root. This is
    // what a namespace package looks like, and it has no `__init__.py` at all.
    let directory = format!("{path}/");
    context
        .files()
        .find(|known| known.starts_with(&directory))
        .cloned()
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
        let context = sephera_core::plugins::test_context(source_file, &known);
        PythonPlugin.resolve(import_path, context)
    }

    #[test]
    fn resolves_absolute_module() {
        let files = ["pkg/util.py", "pkg/__init__.py", "main.py"];

        assert_eq!(
            resolve("pkg.util", "main.py", &files),
            Some("pkg/util.py".to_owned())
        );
    }

    #[test]
    fn resolves_package_init() {
        let files = ["pkg/__init__.py", "main.py"];

        assert_eq!(
            resolve("pkg", "main.py", &files),
            Some("pkg/__init__.py".to_owned())
        );
    }

    #[test]
    fn resolves_single_dot_relative_import() {
        let files = ["pkg/util.py", "pkg/main.py"];

        assert_eq!(
            resolve(".util", "pkg/main.py", &files),
            Some("pkg/util.py".to_owned())
        );
    }

    #[test]
    fn resolves_double_dot_relative_import() {
        let files = ["pkg/util.py", "pkg/sub/deep.py"];

        assert_eq!(
            resolve("..util", "pkg/sub/deep.py", &files),
            Some("pkg/util.py".to_owned())
        );
    }

    #[test]
    fn relative_import_falls_back_to_an_absolute_sibling() {
        // A relative import that names something outside the package is still a
        // reference to a real module when one sits at the top level.
        let files = ["util.py", "pkg/main.py"];

        assert_eq!(
            resolve("..util", "pkg/main.py", &files),
            Some("util.py".to_owned())
        );
    }

    #[test]
    fn relative_import_that_matches_nothing_stays_unresolved() {
        let files = ["pkg/main.py"];

        assert_eq!(resolve("..nowhere", "pkg/main.py", &files), None);
    }

    #[test]
    fn external_package_is_not_resolved() {
        let files = ["main.py"];

        assert_eq!(resolve("os", "main.py", &files), None);
        assert_eq!(resolve("os.path", "main.py", &files), None);
    }

    #[test]
    fn ascend_stops_at_the_root() {
        assert_eq!(ascend("a/b/c", 0), "a/b/c");
        assert_eq!(ascend("a/b/c", 1), "a/b");
        assert_eq!(ascend("a/b/c", 2), "a");
        // Climbing past the top must not loop or panic.
        assert_eq!(ascend("a", 5), "");
    }

    #[test]
    fn a_dots_only_path_resolves_the_package_it_names() {
        // `from . import x` in `pkg/sub/deep.py` names `pkg/sub` and nothing
        // else, so the answer is `pkg/sub/__init__.py` or nothing at all.
        let files = ["pkg/sub/__init__.py", "pkg/sub/deep.py"];

        assert_eq!(
            resolve(".", "pkg/sub/deep.py", &files),
            Some("pkg/sub/__init__.py".to_owned())
        );
        assert_eq!(
            resolve("..", "pkg/sub/deep.py", &files),
            None,
            "`pkg` has no `__init__.py` here, so two dots name no package"
        );
    }

    #[test]
    fn a_dots_only_path_does_not_land_on_a_bare_directory() {
        // The defect this pins. `...` from `python/pkg/sub/deep.py` climbs to
        // `python`, and `first_existing`'s last resort matches any directory
        // with files under it -- which the directory an import has just climbed
        // to always is, so the answer was whichever file happened to sort first
        // underneath: `python/orphan.py`, for a statement naming a directory two
        // levels above its own package.
        //
        // The source path matters to whether this test can fail at all. With the
        // package at the analysis base the climb lands on an empty string, and
        // `first_existing` bails on that before the directory rule is reached --
        // so a shallower fixture would pass against the broken resolver, which is
        // what the first version of this test did.
        let files = [
            "python/pkg/__init__.py",
            "python/pkg/sub/deep.py",
            "python/orphan.py",
        ];

        assert_eq!(
            resolve("...", "python/pkg/sub/deep.py", &files),
            None,
            "a directory is not a package because it has files in it"
        );
        // And the statement's other half agrees with it, which is the point of
        // `the_two_halves_of_one_climbing_import_disagree_on_gap`.
        assert_eq!(
            resolve("...outside", "python/pkg/sub/deep.py", &files),
            None
        );
    }

    #[test]
    fn an_absolute_import_finds_the_directory_python_would_search() {
        // Python puts a package's *parent* on `sys.path`. A repository rarely
        // puts its packages at the top, so an absolute import from inside one
        // used to find nothing at all and the walk worked in one direction only.
        let files = [
            "app/pkg/__init__.py",
            "app/pkg/sub/__init__.py",
            "app/pkg/sub/deep.py",
            "app/pkg/sub/value.py",
        ];

        assert_eq!(
            resolve("pkg.sub.value", "app/pkg/sub/deep.py", &files),
            Some("app/pkg/sub/value.py".to_owned())
        );
        assert_eq!(
            resolve("pkg", "app/pkg/sub/deep.py", &files),
            Some("app/pkg/__init__.py".to_owned())
        );
    }

    #[test]
    fn an_absolute_import_works_from_inside_a_namespace_subpackage() {
        // flask's `src/flask/sansio/` has no `__init__.py` and still belongs to
        // `flask`. Treating the first directory that lacks one as the root puts
        // the root *inside* the package, and every absolute import written from
        // beneath it stops resolving -- which is how a correct-looking rule
        // becomes a wrong one.
        let files = [
            "src/flask/__init__.py",
            "src/flask/app.py",
            "src/flask/sansio/app.py",
        ];

        assert_eq!(
            resolve("flask.app", "src/flask/sansio/app.py", &files),
            Some("src/flask/app.py".to_owned()),
            "`sansio` has no `__init__.py` and is still inside `flask`"
        );
        assert_eq!(
            resolve("flask", "src/flask/sansio/app.py", &files),
            Some("src/flask/__init__.py".to_owned())
        );
    }

    #[test]
    fn an_absolute_import_still_tries_the_analysis_base_first() {
        // Two roots, not one: a module beside the source is imported without
        // knowing which package it belongs to, and it is found either way.
        let files = ["pkg/util.py", "main.py"];

        assert_eq!(
            resolve("pkg.util", "main.py", &files),
            Some("pkg/util.py".to_owned())
        );
        assert_eq!(
            resolve("util", "pkg/main.py", &["util.py", "pkg/main.py"]),
            Some("util.py".to_owned()),
            "a file outside any package makes both attempts identical"
        );
    }

    #[test]
    fn an_absolute_import_outside_a_package_stays_unresolved() {
        // The judgement call this pins, in the direction of not inventing a
        // dependency. flask's `tests/test_apps/helloworld/wsgi.py` says
        // `from hello import app`, and `hello.py` sits beside it -- but
        // `helloworld` has no `__init__.py` and neither does `test_apps`, so
        // there is no package whose parent would be on `sys.path`. The import
        // works because flask's test setup puts the directory there itself, and
        // nothing in the repository says so.
        //
        // The rule that keeps it unresolved is the one above finding the topmost
        // package: with none, the root is the analysis base, and `hello` is not
        // there. `blueprintapp` beside it has an `__init__.py` and does resolve,
        // which is what makes this a distinction rather than a blanket refusal.
        let files = [
            "tests/test_apps/helloworld/wsgi.py",
            "tests/test_apps/helloworld/hello.py",
        ];

        assert_eq!(
            resolve("hello", "tests/test_apps/helloworld/wsgi.py", &files),
            None,
            "a bare module in a directory that is not a package is not \
             importable by convention"
        );
    }

    #[test]
    fn a_name_the_analysis_holds_is_told_from_the_standard_library() {
        // Nothing in the shape of a Python absolute import says whether it
        // leaves the project: `collections.OrderedDict` and a broken
        // `pkg.absent` are the same spelling. This is the only evidence there
        // is, and without it the second was filed as an external dependency.
        let files = ["app/pkg/__init__.py", "app/pkg/sub/deep.py"];
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let plugin = PythonPlugin;

        let inside =
            sephera_core::plugins::test_context("app/pkg/sub/deep.py", &known);
        assert!(
            plugin.names_a_known_module("pkg", inside),
            "`pkg` is a package in this analysis"
        );

        let outside =
            sephera_core::plugins::test_context("app/pkg/sub/deep.py", &known);
        assert!(
            !plugin.names_a_known_module("collections", outside),
            "the standard library is not a local gap"
        );
        assert!(
            !plugin.names_a_known_module("", outside),
            "an empty name is a malformed path, not a module"
        );
    }

    #[test]
    fn resolve_missing_absolute_submodule_is_local_gap() {
        // `from pkg import absent` where `absent` doesn't exist.
        // The import `pkg.absent` should be a local gap (unresolved but local).
        let files = ["pkg/__init__.py", "main.py"];
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = sephera_core::plugins::test_context("main.py", &known);
        let plugin = PythonPlugin;

        // The module `pkg` resolves, but `pkg.absent` should not.
        // The resolver is called with the full path `pkg.absent`.
        let resolved = plugin.resolve("pkg.absent", context);
        assert_eq!(resolved, None, "missing submodule should not resolve");
    }

    #[test]
    fn resolve_relative_import_with_too_many_dots_is_local_gap() {
        // `from ... import outside` from `pkg/sub/deep.py` climbs past the root.
        let files = ["pkg/sub/deep.py", "pkg/__init__.py", "main.py"];
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context =
            sephera_core::plugins::test_context("pkg/sub/deep.py", &known);
        let plugin = PythonPlugin;

        let resolved = plugin.resolve("...outside", context);
        assert_eq!(resolved, None, "climbing past root should not resolve");
    }
}
