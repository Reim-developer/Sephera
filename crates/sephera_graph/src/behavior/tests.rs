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

/// A labelled precision scenario: two trees, the declarations
/// both sides carry, and whether the scenario IS a behaviour
/// change the detector must flag.
///
/// The corpus is the detector's honest answer to "how often is
/// this right". Positives are changes that really do change
/// behaviour; negatives are changes that look like behaviour
/// changes but are not — symmetric bodies, coordinated signature
/// changes, string constants, unused constants, calls that never
/// type check. One negative is a KNOWN false positive, counted
/// honestly rather than hidden: a symmetric body the simple core
/// check cannot see. Precision is computed from the run, and the
/// bar is 0.8: below it, the failing detector is cut, not shipped
/// with a warning.
struct CorpusScenario {
    name: &'static str,
    old_files: Vec<(&'static str, &'static str)>,
    new_files: Vec<(&'static str, &'static str)>,
    /// (file, name, kind, parameters) on the old side.
    old_declarations:
        Vec<(&'static str, &'static str, DeclarationKind, &'static str)>,
    /// (file, name, kind, parameters) on the new side.
    new_declarations:
        Vec<(&'static str, &'static str, DeclarationKind, &'static str)>,
    /// (from, to, `import_path`) edges of the NEW report, for the
    /// constant usage test.
    new_edges: Vec<(&'static str, &'static str, &'static str)>,
    /// True when the scenario really changes behaviour.
    is_change: bool,
    /// True when the detector is known to flag this negative
    /// anyway. Counted as a false positive, never hidden.
    known_false_positive: bool,
}

fn corpus_index(
    declarations: &[(
        &'static str,
        &'static str,
        DeclarationKind,
        &'static str,
    )],
) -> DeclarationIndex {
    let mut index = DeclarationIndex::default();
    for (file, name, kind, parameters) in declarations {
        let mut declared = DeclaredNames::default();
        declared.declare_as(*name, *kind);
        declared.declare_signature(*name, signature(parameters));
        index.insert(file, declared);
    }
    index
}

fn corpus_facts(files: &[(&'static str, &'static str)]) -> TreeFacts {
    let mut merged = TreeFacts::default();
    for (file, text) in files {
        let facts = extract_facts(file, text);
        merged.calls.extend(facts.calls);
        merged.constants.extend(facts.constants);
        merged.functions.extend(facts.functions);
    }
    merged
}

#[allow(
    clippy::too_many_lines,
    reason = "a corpus table is data, not logic; splitting it would hide the labelled set"
)]
fn scenarios() -> Vec<CorpusScenario> {
    let function = DeclarationKind::Function;
    vec![
        // --- Swap positives: asymmetric bodies, same types. ---
        CorpusScenario {
            name: "py_divide",
            old_files: vec![
                ("lib.py", "def divide(a, b):\n    return a / b\n"),
                (
                    "user.py",
                    "from lib import divide\n\n\nprint(divide(x, y))\n",
                ),
            ],
            new_files: vec![
                ("lib.py", "def divide(a, b):\n    return a / b\n"),
                (
                    "user.py",
                    "from lib import divide\n\n\nprint(divide(y, x))\n",
                ),
            ],
            old_declarations: vec![("lib.py", "divide", function, "a, b")],
            new_declarations: vec![("lib.py", "divide", function, "a, b")],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_subtract",
            old_files: vec![
                ("lib.py", "def subtract(a, b):\n    return a - b\n"),
                ("user.py", "print(subtract(x, y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def subtract(a, b):\n    return a - b\n"),
                ("user.py", "print(subtract(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "subtract", function, "a, b")],
            new_declarations: vec![("lib.py", "subtract", function, "a, b")],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_power",
            old_files: vec![
                ("lib.py", "def power(a, b):\n    return a ** b\n"),
                ("user.py", "print(power(x, y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def power(a, b):\n    return a ** b\n"),
                ("user.py", "print(power(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "power", function, "a, b")],
            new_declarations: vec![("lib.py", "power", function, "a, b")],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_format",
            old_files: vec![
                ("lib.py", "def label(a, b):\n    return f\"{a}-{b}\"\n"),
                ("user.py", "print(label(x, y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def label(a, b):\n    return f\"{a}-{b}\"\n"),
                ("user.py", "print(label(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "label", function, "a, b")],
            new_declarations: vec![("lib.py", "label", function, "a, b")],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_subtract",
            old_files: vec![
                (
                    "lib.rs",
                    "pub fn subtract(a: i64, b: i64) -> i64 { a - b }\n",
                ),
                ("user.rs", "fn main() { let _ = subtract(x, y); }\n"),
            ],
            new_files: vec![
                (
                    "lib.rs",
                    "pub fn subtract(a: i64, b: i64) -> i64 { a - b }\n",
                ),
                ("user.rs", "fn main() { let _ = subtract(y, x); }\n"),
            ],
            old_declarations: vec![(
                "lib.rs",
                "subtract",
                function,
                "a: i64, b: i64",
            )],
            new_declarations: vec![(
                "lib.rs",
                "subtract",
                function,
                "a: i64, b: i64",
            )],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_divide",
            old_files: vec![
                ("lib.rs", "pub fn divide(a: i64, b: i64) -> i64 { a / b }\n"),
                ("user.rs", "fn main() { let _ = divide(x, y); }\n"),
            ],
            new_files: vec![
                ("lib.rs", "pub fn divide(a: i64, b: i64) -> i64 { a / b }\n"),
                ("user.rs", "fn main() { let _ = divide(y, x); }\n"),
            ],
            old_declarations: vec![(
                "lib.rs",
                "divide",
                function,
                "a: i64, b: i64",
            )],
            new_declarations: vec![(
                "lib.rs",
                "divide",
                function,
                "a: i64, b: i64",
            )],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        // --- Swap negatives: the filters refuse these. ---
        CorpusScenario {
            name: "py_add_symmetric",
            old_files: vec![
                ("lib.py", "def add(a, b):\n    return a + b\n"),
                ("user.py", "print(add(x, y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def add(a, b):\n    return a + b\n"),
                ("user.py", "print(add(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "add", function, "a, b")],
            new_declarations: vec![("lib.py", "add", function, "a, b")],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_mul_symmetric",
            old_files: vec![
                ("lib.py", "def mul(a, b):\n    return a * b\n"),
                ("user.py", "print(mul(x, y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def mul(a, b):\n    return a * b\n"),
                ("user.py", "print(mul(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "mul", function, "a, b")],
            new_declarations: vec![("lib.py", "mul", function, "a, b")],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_min_symmetric",
            old_files: vec![
                (
                    "lib.rs",
                    "pub fn min2(a: i64, b: i64) -> i64 { std::cmp::min(a, b) }\n",
                ),
                ("user.rs", "fn main() { let _ = min2(x, y); }\n"),
            ],
            new_files: vec![
                (
                    "lib.rs",
                    "pub fn min2(a: i64, b: i64) -> i64 { std::cmp::min(a, b) }\n",
                ),
                ("user.rs", "fn main() { let _ = min2(y, x); }\n"),
            ],
            old_declarations: vec![(
                "lib.rs",
                "min2",
                function,
                "a: i64, b: i64",
            )],
            new_declarations: vec![(
                "lib.rs",
                "min2",
                function,
                "a: i64, b: i64",
            )],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_signature_changed",
            old_files: vec![
                ("lib.py", "def f(a, b):\n    return a - b\n"),
                ("user.py", "print(f(x, y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def f(b, a):\n    return b - a\n"),
                ("user.py", "print(f(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "f", function, "a, b")],
            new_declarations: vec![("lib.py", "f", function, "b, a")],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_unresolvable_callee",
            old_files: vec![(
                "user.rs",
                "fn main() { let _ = helper(x, y); }\n",
            )],
            new_files: vec![(
                "user.rs",
                "fn main() { let _ = helper(y, x); }\n",
            )],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_unequal_types",
            old_files: vec![
                ("lib.rs", "pub fn f(a: i64, b: &str) -> i64 { 0 }\n"),
                ("user.rs", "fn main() { let _ = f(x, s); }\n"),
            ],
            new_files: vec![
                ("lib.rs", "pub fn f(a: i64, b: &str) -> i64 { 0 }\n"),
                ("user.rs", "fn main() { let _ = f(s, x); }\n"),
            ],
            old_declarations: vec![(
                "lib.rs",
                "f",
                function,
                "a: i64, b: &str",
            )],
            new_declarations: vec![(
                "lib.rs",
                "f",
                function,
                "a: i64, b: &str",
            )],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_kwargs_skipped",
            old_files: vec![
                ("lib.py", "def f(a, b):\n    return a - b\n"),
                ("user.py", "print(f(a=x, b=y))\n"),
            ],
            new_files: vec![
                ("lib.py", "def f(a, b):\n    return a - b\n"),
                ("user.py", "print(f(b=y, a=x))\n"),
            ],
            old_declarations: vec![("lib.py", "f", function, "a, b")],
            new_declarations: vec![("lib.py", "f", function, "a, b")],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_rotation_not_a_swap",
            old_files: vec![
                ("lib.py", "def f(a, b, c):\n    return a - b - c\n"),
                ("user.py", "print(f(x, y, z))\n"),
            ],
            new_files: vec![
                ("lib.py", "def f(a, b, c):\n    return a - b - c\n"),
                ("user.py", "print(f(y, z, x))\n"),
            ],
            old_declarations: vec![("lib.py", "f", function, "a, b, c")],
            new_declarations: vec![("lib.py", "f", function, "a, b, c")],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_abs_difference_known_fp",
            old_files: vec![
                (
                    "lib.py",
                    "def gap(a, b):\n    if a > b:\n        return a - b\n    return b - a\n",
                ),
                ("user.py", "print(gap(x, y))\n"),
            ],
            new_files: vec![
                (
                    "lib.py",
                    "def gap(a, b):\n    if a > b:\n        return a - b\n    return b - a\n",
                ),
                ("user.py", "print(gap(y, x))\n"),
            ],
            old_declarations: vec![("lib.py", "gap", function, "a, b")],
            new_declarations: vec![("lib.py", "gap", function, "a, b")],
            new_edges: vec![],
            is_change: false,
            known_false_positive: true,
        },
        // --- Constant positives: numeric, still used. ---
        CorpusScenario {
            name: "py_used_numeric_constant",
            old_files: vec![
                ("cfg.py", "LIMIT = 100\n"),
                ("user.py", "from cfg import LIMIT\n\n\nprint(LIMIT)\n"),
            ],
            new_files: vec![
                ("cfg.py", "LIMIT = 200\n"),
                ("user.py", "from cfg import LIMIT\n\n\nprint(LIMIT)\n"),
            ],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![("user.py", "cfg.py", "cfg.LIMIT")],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_same_file_numeric_constant",
            old_files: vec![("cfg.py", "LIMIT = 100\nprint(LIMIT)\n")],
            new_files: vec![("cfg.py", "LIMIT = 200\nprint(LIMIT)\n")],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_used_numeric_constant",
            old_files: vec![(
                "lib.rs",
                "const MAX: i64 = 10;\npub fn get() -> i64 { MAX }\n",
            )],
            new_files: vec![(
                "lib.rs",
                "const MAX: i64 = 20;\npub fn get() -> i64 { MAX }\n",
            )],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![],
            is_change: true,
            known_false_positive: false,
        },
        // --- Constant negatives. ---
        CorpusScenario {
            name: "py_string_constant",
            old_files: vec![
                ("cfg.py", "VERSION = \"1.0\"\n"),
                ("user.py", "from cfg import VERSION\n\n\nprint(VERSION)\n"),
            ],
            new_files: vec![
                ("cfg.py", "VERSION = \"2.0\"\n"),
                ("user.py", "from cfg import VERSION\n\n\nprint(VERSION)\n"),
            ],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![("user.py", "cfg.py", "cfg.VERSION")],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "py_unused_numeric_constant",
            old_files: vec![("cfg.py", "TIMEOUT = 30\n")],
            new_files: vec![("cfg.py", "TIMEOUT = 60\n")],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
        CorpusScenario {
            name: "rs_string_constant",
            old_files: vec![(
                "lib.rs",
                "const NAME: &str = \"a\";\npub fn name() -> &'static str { NAME }\n",
            )],
            new_files: vec![(
                "lib.rs",
                "const NAME: &str = \"b\";\npub fn name() -> &'static str { NAME }\n",
            )],
            old_declarations: vec![],
            new_declarations: vec![],
            new_edges: vec![],
            is_change: false,
            known_false_positive: false,
        },
    ]
}

#[test]
fn corpus_precision_meets_the_bar() {
    let all = scenarios();
    assert_eq!(
        all.len(),
        21,
        "the corpus grew or shrank without updating this guard"
    );
    let mut true_positives = 0;
    let mut false_positives = 0;
    let mut missed = Vec::new();
    for scenario in &all {
        let mut new_report = empty_report();
        for (from, to, path) in &scenario.new_edges {
            new_report = report_with_edge(from, to, path, &[]);
        }
        let measured = measure_behavior(
            &corpus_index(&scenario.old_declarations),
            &corpus_index(&scenario.new_declarations),
            &new_report,
            &corpus_facts(&scenario.old_files),
            &corpus_facts(&scenario.new_files),
            "old",
            "new",
        );
        let flagged = !measured.changes.is_empty();
        if scenario.is_change {
            if flagged {
                true_positives += 1;
            } else {
                missed.push(scenario.name);
            }
            assert_eq!(
                measured.changes.len(),
                1,
                "{}: a positive scenario must produce exactly its one change",
                scenario.name
            );
        } else if flagged {
            false_positives += 1;
            assert!(
                scenario.known_false_positive,
                "{}: flagged a negative the corpus did not declare as a known false positive",
                scenario.name
            );
        }
        // A negative that stays silent is correct.
    }
    assert!(
        missed.is_empty(),
        "positives the detector missed: {missed:?}"
    );
    let precision = if true_positives + false_positives == 0 {
        0.0
    } else {
        f64::from(true_positives) / f64::from(true_positives + false_positives)
    };
    println!(
        "behavior corpus: {true_positives} true positives, {false_positives} false positives, precision {precision:.3}"
    );
    assert!(
        precision >= 0.8,
        "behavior precision {precision:.3} is below the 0.8 bar: cut the failing detector, do not ship it"
    );
}
