//! Imports that name a member or sit on the include path must stay in the graph.
//!
//! Every case here was a project-local reference reported as a third-party
//! package. That failure is worse than an unresolved path, because the path did
//! not look local, so it was not counted as a resolver gap either — it simply
//! left the graph, while the report still showed plausible totals.
//!
//! The fixtures are inline rather than borrowed from the environment, so a
//! repository that happens to be laid out differently cannot make these pass or
//! fail. They are also end-to-end: each one goes through the real extractor and
//! resolver, because the bug was never in resolution alone — the Java extractor
//! recorded every import as a plain dependency, including one that names no
//! module at all.

use std::path::Path;

use sephera_graph::resolver::build_graph;
use tempfile::TempDir;

/// A small project: write each file and return its root.
fn project(files: &[(&str, &str)]) -> TempDir {
    let temp_dir = tempfile::tempdir().expect("a temporary directory");
    for (relative, contents) in files {
        let path = temp_dir.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent"))
            .expect("creating the directory");
        std::fs::write(path, contents).expect("writing the file");
    }
    temp_dir
}

/// The project files that `to` points at, for every edge in the report.
fn resolved_targets(root: &Path) -> Vec<String> {
    let report = build_graph(
        root,
        &sephera_scan::IgnoreMatcher::empty(),
        &[],
        None,
        None,
    )
    .expect("the graph should build");

    report
        .edges
        .iter()
        .filter_map(|edge| edge.to.clone())
        .collect()
}

#[test]
fn a_header_under_include_is_reached_through_the_include_path() {
    // The layout nearly every C project uses: `-Iinclude` from the build system,
    // and nothing in the source saying where the header lives.
    let temp_dir = project(&[
        ("include/shared.h", "int shared_add(int a, int b);\n"),
        (
            "src/main.c",
            "#include \"shared.h\"\nint main(void){return 0;}\n",
        ),
    ]);

    let targets = resolved_targets(temp_dir.path());

    assert_eq!(
        targets,
        vec!["include/shared.h".to_owned()],
        "the header is the project's own source and must be in the graph"
    );
}

#[test]
fn a_system_header_is_reported_as_external_not_as_project_source() {
    let temp_dir = project(&[(
        "src/main.c",
        "#include <vector>\nint main(void){return 0;}\n",
    )]);

    let targets = resolved_targets(temp_dir.path());

    assert!(
        targets.is_empty(),
        "a system header is not a project file, got {targets:?}"
    );
}

#[test]
fn a_macro_include_is_skipped_rather_than_guessed() {
    // `#include SOME_MACRO` says nothing about a path. Inventing one would be a
    // fabricated edge.
    let temp_dir = project(&[(
        "src/main.c",
        "#include MISSING_MACRO\nint main(void){return 0;}\n",
    )]);

    let targets = resolved_targets(temp_dir.path());

    assert!(targets.is_empty(), "got {targets:?}");
}

#[test]
fn a_static_java_import_points_at_the_declaring_type() {
    let temp_dir = project(&[
        (
            "org/junit/Assert.java",
            "package org.junit;\npublic class Assert {}\n",
        ),
        (
            "app/Uses.java",
            "package app;\nimport static org.junit.Assert.assertEquals;\n\
             public class Uses {}\n",
        ),
    ]);

    let targets = resolved_targets(temp_dir.path());

    assert_eq!(
        targets,
        vec!["org/junit/Assert.java".to_owned()],
        "a static import names a member of Assert, so Assert is the file"
    );
}

#[test]
fn an_on_demand_java_import_points_at_the_outer_type() {
    let temp_dir = project(&[
        (
            "com/example/Foo.java",
            "package com.example;\npublic class Foo { public static class Inner {} }\n",
        ),
        (
            "com/example/app/Uses.java",
            "package com.example.app;\nimport com.example.Foo.Inner;\n\
             public class Uses {}\n",
        ),
    ]);

    let targets = resolved_targets(temp_dir.path());

    assert_eq!(
        targets,
        vec!["com/example/Foo.java".to_owned()],
        "`import a.b.C.D;` names a nested type of C"
    );
}

#[test]
fn a_plain_java_import_still_resolves_to_its_own_file() {
    // The guard on the two fixes above. Shortening a path must never take
    // precedence over matching it whole.
    let temp_dir = project(&[
        (
            "com/example/Foo.java",
            "package com.example;\npublic class Foo {}\n",
        ),
        (
            "com/example/app/Uses.java",
            "package com.example.app;\nimport com.example.Foo;\n\
             public class Uses {}\n",
        ),
    ]);

    let targets = resolved_targets(temp_dir.path());

    assert_eq!(targets, vec!["com/example/Foo.java".to_owned()]);
}
