//! Rust boundary harvest: the modules a file exposes to Python through
//! `PyO3`, and the names it exposes to JavaScript through `wasm_bindgen`.
//!
//! A `#[pymodule]` function is the whole boundary in one item: its name is
//! what a Python `import` names, and the names it registers are what
//! `from m import f` can reach. Neither fact is visible to either language's
//! own resolver — Rust sees an attribute it has no reason to read, Python
//! sees a module with no file — so the graph's join needs them harvested
//! into the boundary index during extraction.
//!
//! What counts as evidence, deliberately narrow:
//!
//! - a `#[pymodule]` function, or a `#[pymodule]` module item — the module;
//! - inside a module function, `wrap_pyfunction!(path)` registrations and
//!   `add_class::<T>()` calls — the exposed functions and classes;
//! - inside a module item, the `#[pyfunction]` / `#[pyclass]` items it
//!   directly holds;
//! - a `name = "…"` argument on the module attribute or on a same-file
//!   `#[pyfunction]` / `#[pyclass]` definition — the Python spelling,
//!   which is the spelling the index is keyed by.
//!
//! A `#[pyfunction]` that no module in this file registers is not exposed
//! by this file, and is not harvested: registration is the act that makes a
//! name reachable from Python, and guessing otherwise would put names in
//! the index that Python cannot import. Renames are applied from same-file
//! definitions only; a function defined in another file and registered here
//! keeps its Rust name in the exposed set, and an import of its renamed
//! spelling falls back to the module-level edge rather than fabricating an
//! item edge.

use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::{Node, Tree};

use sephera_core::boundary::{
    BoundaryKind, BoundaryModule, BoundaryRole, BoundarySymbol, FileBoundary,
};
use sephera_core::plugins::node_text;

/// Harvests the boundary evidence this source carries, if any: `PyO3`
/// modules for the join, and `wasm_bindgen` exports as implemented symbols.
///
/// The two are recorded differently because they are used differently.
/// A `PyO3` module is joined — Python imports it by name. A `wasm_bindgen`
/// export's caller is generated JavaScript glue that does not live in the
/// tree, so there is nothing to join; the record exists so analyses that
/// cannot see the caller (dead-code reachability above all) know the file
/// is reached from outside the graph.
pub fn collect_boundary(source: &[u8], tree: &Tree) -> Option<FileBoundary> {
    let renames = collect_renames(source, tree.root_node());
    let mut modules = Vec::new();
    collect_modules(source, tree.root_node(), &renames, &mut modules);
    let mut symbols = Vec::new();
    collect_wasm_exports(source, tree.root_node(), &mut symbols);
    if modules.is_empty() && symbols.is_empty() {
        None
    } else {
        Some(FileBoundary { modules, symbols })
    }
}

/// Every `#[wasm_bindgen]` item in the file, as an implemented symbol.
///
/// Items carry the attribute directly; methods carry it through their
/// `impl` block, which is where `wasm_bindgen` is written for a type's
/// whole surface. The recorded name honours a `js_name` override on the
/// item itself.
fn collect_wasm_exports(
    source: &[u8],
    node: Node<'_>,
    symbols: &mut Vec<BoundarySymbol>,
) {
    match node.kind() {
        "function_item" | "struct_item" | "enum_item"
            if has_attribute(source, &node, "wasm_bindgen") =>
        {
            if let Some(name_node) = node.child_by_field_name("name") {
                let local = node_text(source, &name_node);
                let name = attribute_text(source, &node, "wasm_bindgen")
                    .and_then(|text| name_argument(&text))
                    .unwrap_or_else(|| local.clone());
                symbols.push(BoundarySymbol {
                    name,
                    kind: BoundaryKind::WasmBindgen,
                    role: BoundaryRole::Implements,
                    local,
                });
            }
        }
        "impl_item" if has_attribute(source, &node, "wasm_bindgen") => {
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    if child.kind() == "function_item"
                        && let Some(name_node) =
                            child.child_by_field_name("name")
                    {
                        let local = node_text(source, &name_node);
                        symbols.push(BoundarySymbol {
                            name: local.clone(),
                            kind: BoundaryKind::WasmBindgen,
                            role: BoundaryRole::Implements,
                            local,
                        });
                    }
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_wasm_exports(source, child, symbols);
    }
}

/// The attribute run directly above an item: `(head, full text)` pairs, in
/// source order.
///
/// Attributes are *siblings* of the item they decorate in this grammar, so
/// the run is read backwards from the item, skipping comments — the same
/// shape as the `#[macro_export]` check in the declaration walk.
fn attribute_run(source: &[u8], node: &Node<'_>) -> Vec<(String, String)> {
    let mut run = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        match sibling.kind() {
            "attribute_item" => {
                let text = node_text(source, &sibling);
                if let Some(head) = attribute_head(&text) {
                    run.push((head, text));
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        current = sibling.prev_sibling();
    }
    run
}

/// The name an attribute is keyed by: `#[pyo3::pyfunction(…)]` answers
/// `pyfunction`, because the path is how the macro was imported, not which
/// macro it is.
fn attribute_head(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix("#[")?;
    let end = inner
        .find(|c: char| c == '(' || c == ']' || c.is_whitespace())
        .unwrap_or(inner.len());
    let path = inner[..end].trim();
    let head = path.rsplit("::").next().unwrap_or(path);
    if head.is_empty() {
        None
    } else {
        Some(head.to_owned())
    }
}

/// The `name = "…"` argument inside an attribute's text, when it carries one.
///
/// Read from the text rather than the token tree because the argument's
/// shape is fixed — a keyword, an equals sign, a string literal — and the
/// text is the whole attribute, so there is nothing else the `name` in it
/// could be naming.
fn name_argument(text: &str) -> Option<String> {
    // The first `name` in the text is the argument keyword. Slicing from it
    // rather than splitting on it matters: the value itself may contain the
    // word (`name = "renamed_ext"`), and a split would cut the string in
    // half at exactly that point.
    let start = text.find("name")? + "name".len();
    let after_equals = text[start..].trim_start().strip_prefix('=')?;
    let quoted = after_equals.trim_start().strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(quoted[..end].to_owned())
}

/// The trailing segment of a path: `crate::math::add` answers `add`, and a
/// generic tail (`Point<T>`) answers the name before the arguments.
fn trailing_segment(path: &str) -> &str {
    let tail = path.trim().rsplit("::").next().unwrap_or(path).trim();
    tail.split('<').next().unwrap_or(tail).trim()
}

/// Maps each `#[pyfunction]` / `#[pyclass]` definition in this file that
/// renames itself to its Python spelling, keyed by the Rust name.
fn collect_renames(source: &[u8], node: Node<'_>) -> BTreeMap<String, String> {
    let mut renames = BTreeMap::new();
    collect_renames_from(source, node, &mut renames);
    renames
}

fn collect_renames_from(
    source: &[u8],
    node: Node<'_>,
    renames: &mut BTreeMap<String, String>,
) {
    if matches!(
        node.kind(),
        "function_item" | "struct_item" | "enum_item" | "union_item"
    ) && let Some(name_node) = node.child_by_field_name("name")
    {
        for (head, text) in attribute_run(source, &node) {
            if (head == "pyfunction" || head == "pyclass")
                && let Some(python_name) = name_argument(&text)
            {
                renames.insert(node_text(source, &name_node), python_name);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_renames_from(source, child, renames);
    }
}

/// Finds `#[pymodule]` items anywhere in the file and builds their modules.
fn collect_modules(
    source: &[u8],
    node: Node<'_>,
    renames: &BTreeMap<String, String>,
    modules: &mut Vec<BoundaryModule>,
) {
    match node.kind() {
        "function_item" if has_attribute(source, &node, "pymodule") => {
            if let Some(module) = module_function(source, &node, renames) {
                modules.push(module);
            }
        }
        "mod_item" if has_attribute(source, &node, "pymodule") => {
            if let Some(module) = module_item(source, &node, renames) {
                modules.push(module);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_modules(source, child, renames, modules);
    }
}

fn has_attribute(source: &[u8], node: &Node<'_>, head: &str) -> bool {
    attribute_run(source, node)
        .iter()
        .any(|(found, _)| found == head)
}

/// The attribute text for one head in an item's run, when present.
fn attribute_text(
    source: &[u8],
    node: &Node<'_>,
    head: &str,
) -> Option<String> {
    attribute_run(source, node)
        .into_iter()
        .find(|(found, _)| found == head)
        .map(|(_, text)| text)
}

/// A `#[pymodule]` function: the module name, and the names its body
/// registers.
fn module_function(
    source: &[u8],
    node: &Node<'_>,
    renames: &BTreeMap<String, String>,
) -> Option<BoundaryModule> {
    let name_node = node.child_by_field_name("name")?;
    let name = attribute_text(source, node, "pymodule")
        .and_then(|text| name_argument(&text))
        .unwrap_or_else(|| node_text(source, &name_node));
    let mut exposed = BTreeSet::new();
    if let Some(body) = node.child_by_field_name("body") {
        collect_registrations(source, body, renames, &mut exposed);
    }
    Some(BoundaryModule {
        name,
        kind: BoundaryKind::PyO3,
        exposed,
    })
}

/// A `#[pymodule]` module item: the items it directly holds are its surface.
fn module_item(
    source: &[u8],
    node: &Node<'_>,
    renames: &BTreeMap<String, String>,
) -> Option<BoundaryModule> {
    let name_node = node.child_by_field_name("name")?;
    let name = attribute_text(source, node, "pymodule")
        .and_then(|text| name_argument(&text))
        .unwrap_or_else(|| node_text(source, &name_node));
    let mut exposed = BTreeSet::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if matches!(
                child.kind(),
                "function_item" | "struct_item" | "enum_item" | "union_item"
            ) && let Some(child_name) = child.child_by_field_name("name")
            {
                let run = attribute_run(source, &child);
                if run
                    .iter()
                    .any(|(head, _)| head == "pyfunction" || head == "pyclass")
                {
                    let rust_name = node_text(source, &child_name);
                    exposed.insert(
                        renames.get(&rust_name).cloned().unwrap_or(rust_name),
                    );
                }
            }
        }
    }
    Some(BoundaryModule {
        name,
        kind: BoundaryKind::PyO3,
        exposed,
    })
}

/// The names a module function registers: every `wrap_pyfunction!` path in
/// its body, and every `add_class::<T>()` type.
fn collect_registrations(
    source: &[u8],
    node: Node<'_>,
    renames: &BTreeMap<String, String>,
    exposed: &mut BTreeSet<String>,
) {
    match node.kind() {
        "macro_invocation" => {
            if let Some(macro_node) = node.child_by_field_name("macro") {
                let macro_text = node_text(source, &macro_node);
                if trailing_segment(&macro_text) == "wrap_pyfunction"
                    && let Some(tree_node) = node
                        .children(&mut node.walk())
                        .find(|child| child.kind() == "token_tree")
                {
                    let text = node_text(source, &tree_node);
                    let inner = text
                        .trim()
                        .trim_start_matches('(')
                        .trim_end_matches(')');
                    let path = inner.split(',').next().unwrap_or(inner);
                    let rust_name = trailing_segment(path);
                    if !rust_name.is_empty() {
                        exposed.insert(
                            renames
                                .get(rust_name)
                                .cloned()
                                .unwrap_or_else(|| rust_name.to_owned()),
                        );
                    }
                }
            }
        }
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                // A turbofish call wraps the callee in a `generic_function`
                // node: `m.add_class::<Point>()` has that wrapper between
                // the call and the field expression, carrying the type
                // arguments as its own child.
                let callee_node = if function.kind() == "generic_function" {
                    function.child_by_field_name("function").unwrap_or(function)
                } else {
                    function
                };
                // The callee's own name, without any generic arguments:
                // the field is asked for directly rather than read off the
                // end of the text, where the turbofish would be mistaken
                // for part of the name.
                let callee =
                    callee_node.child_by_field_name("field").map_or_else(
                        || node_text(source, &callee_node),
                        |field| node_text(source, &field),
                    );
                if trailing_segment(&callee) == "add_class" {
                    // The type rides in the call's generic arguments, which
                    // sit on the function node rather than the call arguments.
                    let mut cursor = function.walk();
                    if let Some(type_args) = function
                        .children(&mut cursor)
                        .find(|child| child.kind() == "type_arguments")
                    {
                        let text = node_text(source, &type_args);
                        let inner = text
                            .trim()
                            .trim_start_matches('<')
                            .trim_end_matches('>');
                        let rust_name = trailing_segment(
                            inner.split(',').next().unwrap_or(inner),
                        );
                        if !rust_name.is_empty() {
                            exposed.insert(
                                renames
                                    .get(rust_name)
                                    .cloned()
                                    .unwrap_or_else(|| rust_name.to_owned()),
                            );
                        }
                    }
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_registrations(source, child, renames, exposed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sephera_compression::{SupportedLanguage, with_parser};

    fn harvest(source: &str) -> Option<FileBoundary> {
        with_parser(SupportedLanguage::Rust, source.len(), |parser| {
            let tree = parser
                .parse(source.as_bytes(), None)
                .expect("the source parses");
            Ok(collect_boundary(source.as_bytes(), &tree))
        })
        .expect("a parser exists")
    }

    fn module(source: &str) -> BoundaryModule {
        harvest(source)
            .expect("a boundary is harvested")
            .modules
            .into_iter()
            .next()
            .expect("one module")
    }

    #[test]
    fn a_pymodule_registers_its_functions_and_classes() {
        let found = module(
            r"
use pyo3::prelude::*;

#[pyfunction]
fn add(a: i32, b: i32) -> i32 { a + b }

#[pyclass]
struct Point { x: f64 }

#[pymodule]
fn myext(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(add, m)?)?;
    m.add_function(wrap_pyfunction!(crate::math::multiply, m)?)?;
    m.add_class::<Point>()?;
    Ok(())
}
",
        );
        assert_eq!(found.name, "myext");
        assert_eq!(found.kind, BoundaryKind::PyO3);
        assert_eq!(
            found.exposed,
            BTreeSet::from([
                "Point".to_owned(),
                "add".to_owned(),
                "multiply".to_owned()
            ])
        );
    }

    #[test]
    fn a_pymodule_name_argument_wins_over_the_function_name() {
        let found = module(
            r#"
#[pymodule(name = "renamed_ext")]
fn myext(m: &Bound<'_, PyModule>) -> PyResult<()> { Ok(()) }
"#,
        );
        assert_eq!(found.name, "renamed_ext");
    }

    #[test]
    fn a_pyfunction_rename_is_the_exposed_spelling() {
        let found = module(
            r#"
#[pyfunction(name = "sum_all")]
fn add(a: i32, b: i32) -> i32 { a + b }

#[pymodule]
fn myext(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(add, m)?)?;
    Ok(())
}
"#,
        );
        assert_eq!(found.exposed, BTreeSet::from(["sum_all".to_owned()]));
    }

    #[test]
    fn an_unregistered_pyfunction_is_not_exposed() {
        let found = module(
            r"
#[pyfunction]
fn helper(a: i32) -> i32 { a }

#[pymodule]
fn myext(m: &Bound<'_, PyModule>) -> PyResult<()> { Ok(()) }
",
        );
        assert!(found.exposed.is_empty());
    }

    #[test]
    fn a_module_item_exposes_the_items_it_holds() {
        let found = module(
            r"
#[pymodule]
mod myext {
    #[pyfunction]
    pub fn add(a: i32, b: i32) -> i32 { a + b }

    #[pyclass]
    pub struct Point { x: f64 }

    pub fn internal(a: i32) -> i32 { a }
}
",
        );
        assert_eq!(found.name, "myext");
        assert_eq!(
            found.exposed,
            BTreeSet::from(["Point".to_owned(), "add".to_owned()])
        );
    }

    #[test]
    fn a_plain_rust_file_has_no_boundary() {
        assert!(
            harvest("pub fn add(a: i32, b: i32) -> i32 { a + b }\n").is_none()
        );
    }

    #[test]
    fn two_modules_in_one_file_are_both_harvested() {
        let boundary = harvest(
            r"
#[pymodule]
fn one(m: &Bound<'_, PyModule>) -> PyResult<()> { Ok(()) }

#[pymodule]
fn two(m: &Bound<'_, PyModule>) -> PyResult<()> { Ok(()) }
",
        )
        .expect("a boundary is harvested");
        assert_eq!(boundary.modules.len(), 2);
    }

    #[test]
    fn wasm_bindgen_items_are_implemented_symbols() {
        let boundary = harvest(
            r#"
#[wasm_bindgen]
pub fn greet(name: &str) -> String { name.to_owned() }

#[wasm_bindgen(js_name = "makePoint")]
pub struct Point { x: f64 }
"#,
        )
        .expect("a boundary is harvested");
        assert_eq!(boundary.modules.len(), 0);
        let names: Vec<(&str, &str)> = boundary
            .symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.local.as_str()))
            .collect();
        assert_eq!(names, [("greet", "greet"), ("makePoint", "Point")]);
        assert!(
            boundary
                .symbols
                .iter()
                .all(|symbol| symbol.kind == BoundaryKind::WasmBindgen
                    && symbol.role == BoundaryRole::Implements)
        );
    }

    #[test]
    fn wasm_bindgen_impl_methods_are_implemented_symbols() {
        let boundary = harvest(
            r"
pub struct Counter { value: i32 }

#[wasm_bindgen]
impl Counter {
    pub fn bump(&mut self) { self.value += 1; }
}
",
        )
        .expect("a boundary is harvested");
        assert_eq!(boundary.symbols.len(), 1);
        assert_eq!(boundary.symbols[0].name, "bump");
    }
}
