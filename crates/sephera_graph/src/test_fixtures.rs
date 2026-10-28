//! Shared builders for graph measurement tests.
//!
//! Every measurement module used to carry its own copy of the same
//! builders: a one-count Rust node, a resolved edge (plain, with
//! items, or namespace), an unresolved edge, an all-zero metrics
//! value, a report assembled from them (from edges or from
//! `(from, to, path)` triples), and a declaration index from
//! `(file, [(name, kind)])` pairs, with or without call shapes.
//! The copies were byte-identical, which meant a fixture-shape fix
//! had to land in ten places at once or not at all. There is one
//! copy now, here.
//!
//! The builders spell the convention the old copies shared, so
//! migrating a module is a deletion, not a rewrite: nodes are Rust
//! files with one import and one importer, resolved edges are plain
//! dependencies, and reports sit at base `"."` with no focus, depth,
//! or query. A test that needs a different shape still builds its
//! own values; this module is the common case, not a constraint.
//!
//! Test-only: the module is compiled only under `cfg(test)`, so it
//! adds nothing to the shipped binary.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames, Signature,
};

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

/// One file node, in the shared convention: a Rust file with one
/// import and one importer.
#[must_use]
pub fn node(path: &str) -> GraphNode {
    GraphNode {
        file_path: path.to_owned(),
        language: Some("Rust"),
        imports_count: 1,
        imported_by_count: 1,
    }
}

/// One resolved dependency edge spelling `path` from `from` to `to`.
#[must_use]
pub fn edge(from: &str, to: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    }
}

/// One resolved dependency edge spelling `path` from `from` to `to`,
/// naming the imported `items`.
#[must_use]
pub fn edge_items(
    from: &str,
    to: &str,
    path: &str,
    items: &[&str],
) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: items.iter().map(|item| (*item).to_owned()).collect(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    }
}

/// One resolved namespace edge: the wildcard-import shape, where the
/// importer names the module rather than any symbol in it.
#[must_use]
pub fn namespace_edge(from: &str, to: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: true,
        kind: ImportKind::Namespace,
        local_gap: false,
        cfg_gated: false,
    }
}

/// One unresolved edge: `from` names `path`, and nothing answers for
/// it. `local_gap` marks whether the path looks local (a resolver
/// gap) rather than third-party.
#[must_use]
pub fn unresolved_edge(from: &str, path: &str, local_gap: bool) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: None,
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: false,
        kind: ImportKind::Dependency,
        local_gap,
        cfg_gated: false,
    }
}

/// An all-zero metrics value: [`GraphMetrics::default`], under the
/// name the old per-module copies used.
#[must_use]
pub fn empty_metrics() -> GraphMetrics {
    GraphMetrics::default()
}

/// One report over `nodes` with `edges`, at base `"."` and no focus,
/// depth, or query.
#[must_use]
pub fn report(nodes: &[&str], edges: Vec<GraphEdge>) -> GraphReport {
    GraphReport {
        base_path: PathBuf::from("."),
        focus_paths: vec![],
        depth: None,
        query: None,
        nodes: nodes.iter().copied().map(node).collect(),
        edges,
        boundary_files: Vec::new(),
        metrics: empty_metrics(),
    }
}

/// One report over `nodes` with triple edges, for the modules
/// whose fixtures spell edges as `(from, to, path)`.
#[must_use]
pub fn report_from_triples(
    nodes: &[&str],
    edges: &[(&str, &str, &str)],
) -> GraphReport {
    report(
        nodes,
        edges
            .iter()
            .map(|(from, to, path)| edge(from, to, path))
            .collect(),
    )
}

/// A declared name with its optional call shape, for the tests
/// that compare signatures: `(name, kind, (parameters, returns))`.
pub type SignedName<'a> =
    (&'a str, DeclarationKind, Option<(&'a str, Option<&'a str>)>);

/// Like [`index`], but each name may also carry a call shape, for
/// the tests that compare signatures.
#[must_use]
pub fn index_with_signatures(
    files: &[(&str, &[SignedName<'_>])],
) -> DeclarationIndex {
    let mut index = DeclarationIndex::default();
    for (file, names) in files {
        let mut declared = DeclaredNames::default();
        for (name, kind, signature) in *names {
            declared.declare_as(*name, *kind);
            if let Some((parameters, returns)) = signature {
                declared.declare_signature(
                    *name,
                    Signature {
                        parameters: (*parameters).to_owned(),
                        returns: returns.map(str::to_owned),
                    },
                );
            }
        }
        index.insert(file, declared);
    }
    index
}

/// One declaration index from `(file, [(name, kind)])` pairs.
/// Names for one file are accumulated before the insert, so a second
/// name in the same file adds to the first rather than overwriting
/// it.
#[must_use]
pub fn index(files: &[(&str, &[(&str, DeclarationKind)])]) -> DeclarationIndex {
    let mut index = DeclarationIndex::default();
    for (file, names) in files {
        let mut declared = DeclaredNames::default();
        for (name, kind) in *names {
            declared.declare_as(*name, *kind);
        }
        index.insert(file, declared);
    }
    index
}
