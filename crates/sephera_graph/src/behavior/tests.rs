//! Behaviour-detector rules: what is flagged, and what the
//! filters refuse to flag.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames, Signature,
};

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::{
    BehaviorChange, CallSite, ConstantDef, FunctionDef, TreeFacts,
    extract_facts, measure_behavior,
};

fn signature(parameters: &str) -> Signature {
    Signature {
        parameters: parameters.to_owned(),
        returns: None,
    }
}

fn index_with(
    file: &str,
    name: &str,
    kind: DeclarationKind,
    parameters: &str,
) -> DeclarationIndex {
    let mut index = DeclarationIndex::default();
    let mut declared = DeclaredNames::default();
    declared.declare_as(name, kind);
    declared.declare_signature(name, signature(parameters));
    index.insert(file, declared);
    index
}

fn empty_index() -> DeclarationIndex {
    DeclarationIndex::default()
}

fn empty_metrics() -> GraphMetrics {
    GraphMetrics {
        total_files: 0,
        total_internal_edges: 0,
        self_references: 0,
        total_external_edges: 0,
        unresolved_local_edges: 0,
        unresolved_local_samples: Vec::new(),
        cfg_gated_edges: 0,
        dependencies: Vec::new(),
        declared_dependency_edges: 0,
        local_crate_edges: 0,
        builtin_edges: 0,
        circular_dependencies: 0,
        most_importing: vec![],
        most_imported: vec![],
        cycles: vec![],
    }
}

fn empty_report() -> GraphReport {
    GraphReport {
        base_path: PathBuf::from("."),
        focus_paths: vec![],
        depth: None,
        query: None,
        nodes: vec![],
        edges: vec![],
        boundary_files: Vec::new(),
        metrics: empty_metrics(),
    }
}

fn report_with_edge(
    from: &str,
    to: &str,
    path: &str,
    items: &[&str],
) -> GraphReport {
    let mut report = empty_report();
    report.nodes.push(GraphNode {
        file_path: from.to_owned(),
        language: None,
        imports_count: 1,
        imported_by_count: 0,
    });
    report.nodes.push(GraphNode {
        file_path: to.to_owned(),
        language: None,
        imports_count: 0,
        imported_by_count: 1,
    });
    report.edges.push(GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: items.iter().map(|item| (*item).to_owned()).collect(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    });
    report
}

fn call(file: &str, callee: &str, args: &[&str]) -> CallSite {
    CallSite {
        file: file.to_owned(),
        callee: callee.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
    }
}

fn function(file: &str, name: &str, body: &str) -> FunctionDef {
    FunctionDef {
        file: file.to_owned(),
        name: name.to_owned(),
        body: body.to_owned(),
    }
}

fn constant(
    file: &str,
    name: &str,
    value: &str,
    numeric: bool,
    referenced: bool,
) -> ConstantDef {
    ConstantDef {
        file: file.to_owned(),
        name: name.to_owned(),
        value: value.to_owned(),
        numeric,
        line: 0,
        referenced_elsewhere: referenced,
    }
}

fn facts(
    calls: Vec<CallSite>,
    constants: Vec<ConstantDef>,
    functions: Vec<FunctionDef>,
) -> TreeFacts {
    TreeFacts {
        calls,
        constants,
        functions,
    }
}

#[test]
fn rust_extraction_skips_methods_and_keeps_path_calls() {
    let facts = extract_facts(
        "src/main.rs",
        "fn main() {\n    let a = divide(x, y);\n    let b = obj.method(x, y);\n    let c = foo::bar(x, y);\n}\n",
    );
    let callees: Vec<&str> = facts
        .calls
        .iter()
        .map(|call| call.callee.as_str())
        .collect();
    assert_eq!(callees, vec!["divide", "bar"]);
    assert_eq!(facts.calls[0].args, vec!["x", "y"]);
}

#[test]
fn python_extraction_skips_kwargs_splats_and_methods() {
    let facts = extract_facts(
        "app.py",
        "divide(a, b)\nkeep(a, b=1)\nspread(*args)\nobj.method(a, b)\n",
    );
    assert_eq!(facts.calls.len(), 1);
    assert_eq!(facts.calls[0].callee, "divide");
}

#[test]
fn constant_extraction_marks_numeric_and_same_file_use() {
    let facts = extract_facts(
        "cfg.py",
        "TIMEOUT = 30\nVERSION = \"1.0\"\n\n\nprint(TIMEOUT)\n",
    );
    assert_eq!(facts.constants.len(), 2);
    let timeout = &facts.constants[0];
    assert_eq!(timeout.name, "TIMEOUT");
    assert!(timeout.numeric);
    assert!(timeout.referenced_elsewhere);
    let version = &facts.constants[1];
    assert!(!version.numeric);
    assert!(!version.referenced_elsewhere);
}

#[test]
fn rust_swap_with_equal_types_is_flagged() {
    let old_index = index_with(
        "src/a.rs",
        "divide",
        DeclarationKind::Function,
        "a: i32, b: i32",
    );
    let new_index = old_index.clone();
    let old = facts(
        vec![call("src/b.rs", "divide", &["x", "y"])],
        vec![],
        vec![function("src/a.rs", "divide", "{ a - b }")],
    );
    let new = facts(
        vec![call("src/b.rs", "divide", &["y", "x"])],
        vec![],
        vec![function("src/a.rs", "divide", "{ a - b }")],
    );
    let report = measure_behavior(
        &old_index,
        &new_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(report.changes.len(), 1);
    assert!(matches!(
        &report.changes[0],
        BehaviorChange::ArgOrderSwap { callee, .. } if callee == "divide"
    ));
}

#[test]
fn rust_swap_with_unequal_types_is_not_silent() {
    let old_index = index_with(
        "src/a.rs",
        "pair",
        DeclarationKind::Function,
        "a: i32, b: String",
    );
    let new_index = old_index.clone();
    let old =
        facts(vec![call("src/b.rs", "pair", &["x", "s"])], vec![], vec![]);
    let new =
        facts(vec![call("src/b.rs", "pair", &["s", "x"])], vec![], vec![]);
    let report = measure_behavior(
        &old_index,
        &new_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(report.changes.len(), 0);
}

#[test]
fn symmetric_body_is_not_a_behaviour_change() {
    let old_index = index_with(
        "src/a.rs",
        "add",
        DeclarationKind::Function,
        "a: i32, b: i32",
    );
    let new_index = old_index.clone();
    let old = facts(
        vec![call("src/b.rs", "add", &["x", "y"])],
        vec![],
        vec![function("src/a.rs", "add", "{ a + b }")],
    );
    let new = facts(
        vec![call("src/b.rs", "add", &["y", "x"])],
        vec![],
        vec![function("src/a.rs", "add", "{ a + b }")],
    );
    let report = measure_behavior(
        &old_index,
        &new_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(report.changes.len(), 0);
}

#[test]
fn signature_change_moves_the_call_to_breaking_territory() {
    let old_index = index_with(
        "src/a.rs",
        "divide",
        DeclarationKind::Function,
        "a: i32, b: i32",
    );
    let new_index = index_with(
        "src/a.rs",
        "divide",
        DeclarationKind::Function,
        "b: i32, a: i32",
    );
    let old = facts(
        vec![call("src/b.rs", "divide", &["x", "y"])],
        vec![],
        vec![],
    );
    let new = facts(
        vec![call("src/b.rs", "divide", &["y", "x"])],
        vec![],
        vec![],
    );
    let report = measure_behavior(
        &old_index,
        &new_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(report.changes.len(), 0);
}

#[test]
fn python_swap_is_flagged_and_param_rename_skips_it() {
    let old_index =
        index_with("lib.py", "divide", DeclarationKind::Function, "a, b");
    let new_index = old_index.clone();
    let old =
        facts(vec![call("user.py", "divide", &["x", "y"])], vec![], vec![]);
    let new =
        facts(vec![call("user.py", "divide", &["y", "x"])], vec![], vec![]);
    let flagged = measure_behavior(
        &old_index,
        &new_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(flagged.changes.len(), 1);

    let renamed_index =
        index_with("lib.py", "divide", DeclarationKind::Function, "num, den");
    let skipped = measure_behavior(
        &old_index,
        &renamed_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(skipped.changes.len(), 0);
}

#[test]
fn numeric_constant_rules() {
    let old_index = empty_index();
    let new_index = empty_index();
    let old = facts(
        vec![],
        vec![
            constant("cfg.py", "TIMEOUT", "30", true, true),
            constant("cfg.py", "VERSION", "\"1.0\"", false, true),
            constant("cfg.py", "UNUSED", "5", true, false),
        ],
        vec![],
    );
    let new = facts(
        vec![],
        vec![
            constant("cfg.py", "TIMEOUT", "60", true, true),
            constant("cfg.py", "VERSION", "\"2.0\"", false, true),
            constant("cfg.py", "UNUSED", "9", true, false),
        ],
        vec![],
    );
    let report = measure_behavior(
        &old_index,
        &new_index,
        &empty_report(),
        &old,
        &new,
        "v1",
        "v2",
    );
    assert_eq!(report.changes.len(), 1);
    assert!(matches!(
        &report.changes[0],
        BehaviorChange::ConstantChanged { name, .. } if name == "TIMEOUT"
    ));
}

#[test]
fn cross_file_import_counts_as_use_for_a_constant() {
    let old_index = empty_index();
    let new_index = empty_index();
    let old = facts(
        vec![],
        vec![constant("lib.py", "LIMIT", "100", true, false)],
        vec![],
    );
    let new = facts(
        vec![],
        vec![constant("lib.py", "LIMIT", "200", true, false)],
        vec![],
    );
    let report = report_with_edge("user.py", "lib.py", "lib.LIMIT", &[]);
    let measured = measure_behavior(
        &old_index, &new_index, &report, &old, &new, "v1", "v2",
    );
    assert_eq!(measured.changes.len(), 1);
}
