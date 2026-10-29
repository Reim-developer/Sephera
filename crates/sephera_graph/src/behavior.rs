//! Changes that still compile but change behaviour.
//!
//! Two detectors, both deliberately narrow, because the general
//! question — "did this change alter what the program does" — is
//! undecidable, and a detector that answers it by guessing trains
//! a team to ignore the detector.
//!
//! - **Argument-order swaps.** A call site whose arguments are the
//!   same multiset in a different order, where the callee's
//!   signature did not change. In Rust the swap must also type
//!   check: the types at the swapped positions must be equal, or
//!   the swap would not compile and so is not silent. In Python
//!   there is no compile gate to violate, so no type condition
//!   applies. Two filters keep the detector honest: a callee whose
//!   body is a single symmetric expression over the swapped
//!   parameters (`a + b`, `min(a, b)`) does not change behaviour
//!   when they swap; and a callee whose signature changed is
//!   `breaking`'s territory, not this command's. Only a single
//!   transposition is flagged — a rotation is a real silent change
//!   too, but it is rarer, and v1 does not guess at permutations
//!   it cannot name.
//! - **Numeric constant changes.** A module-level constant whose
//!   numeric value changed and which is actually used — referenced
//!   elsewhere in its own file, or imported by another file through
//!   the graph. String constants are never flagged: a version bump
//!   and a rewritten query are indistinguishable statically, and a
//!   detector that cannot tell them apart should not pretend it
//!   can.
//!
//! The precision bar is enforced by the corpus test, not by this
//! module's optimism: if the constructed corpus measures below
//! 0.8, the detector that missed is cut.

use serde::Serialize;

use std::collections::BTreeMap;
use std::path::Path;

use sephera_core::declarations::{DeclarationIndex, Signature};
use sephera_scan::IgnoreMatcher;

use crate::types::GraphReport;

/// One call site, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The file containing the call.
    pub file: String,
    /// The called name: the last segment of a path call, so
    /// `crate::a::add` and `add` are the same callee.
    pub callee: String,
    /// Argument texts, whitespace-normalised, in written order.
    pub args: Vec<String>,
}

/// One module-level constant, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantDef {
    /// The file declaring it.
    pub file: String,
    /// The constant's name.
    pub name: String,
    /// The value text, as written.
    pub value: String,
    /// Whether the value is a numeric literal.
    pub numeric: bool,
    /// The line the definition starts on, for the same-file
    /// reference test: a name on its own line is not a use.
    pub line: usize,
    /// Whether the name is referenced on another line of the same
    /// file: the same-file half of the usage test.
    pub referenced_elsewhere: bool,
}

/// One function definition, kept for the symmetry test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    /// The file defining it.
    pub file: String,
    /// The function's name.
    pub name: String,
    /// The body text, as written.
    pub body: String,
}

/// Everything one tree's texts say about calls and constants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeFacts {
    /// Call sites, in file order.
    pub calls: Vec<CallSite>,
    /// Module-level constants.
    pub constants: Vec<ConstantDef>,
    /// Function definitions, for the symmetry test.
    pub functions: Vec<FunctionDef>,
}

/// One silent behaviour change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BehaviorChange {
    /// A call site's arguments swapped order, same multiset.
    ArgOrderSwap {
        /// The file containing the call.
        file: String,
        /// The called name.
        callee: String,
        /// Arguments as the older tree wrote them.
        old_args: Vec<String>,
        /// Arguments as the newer tree writes them.
        new_args: Vec<String>,
    },
    /// A numeric constant changed value while still used.
    ConstantChanged {
        /// The file declaring it.
        file: String,
        /// The constant's name.
        name: String,
        /// The older value text.
        old_value: String,
        /// The newer value text.
        new_value: String,
    },
}

/// The silent behaviour changes between two refs.
///
/// Field order is the JSON order, and the ref fields serialize
/// under the `from`/`to` keys the CLI and MCP surfaces already
/// use: the CLI and the MCP tool serialize this struct directly,
/// so the declaration order is the emitted order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BehaviorReport {
    /// The older ref, as the caller spelled it.
    #[serde(rename = "from")]
    pub from_ref: String,
    /// The newer ref, as the caller spelled it, or "working tree".
    #[serde(rename = "to")]
    pub to_ref: String,
    /// The changes, file then callee or name.
    pub changes: Vec<BehaviorChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Python,
}

fn language_of(file: &str) -> Option<Language> {
    let extension = std::path::Path::new(file).extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("rs") {
        Some(Language::Rust)
    } else if extension.eq_ignore_ascii_case("py") {
        Some(Language::Python)
    } else {
        None
    }
}

/// Whether a file is Rust, by extension: the swap filters that
/// only apply to a compiled language need the same answer as the
/// extractor.
fn is_rust(file: &str) -> bool {
    language_of(file) == Some(Language::Rust)
}

/// Extract one file's calls, constants, and function bodies.
///
/// A file that does not parse yields no facts, not an error: a
/// behaviour scan over a tree with one broken file should still
/// report on the rest, and the parse failure is the parser's
/// complaint, not this scan's.
#[must_use]
pub fn extract_facts(file: &str, text: &str) -> TreeFacts {
    let Some(language) = language_of(file) else {
        return TreeFacts::default();
    };
    let mut parser = tree_sitter::Parser::new();
    let language_fn = match language {
        Language::Rust => tree_sitter_rust::LANGUAGE,
        Language::Python => tree_sitter_python::LANGUAGE,
    };
    if parser.set_language(&language_fn.into()).is_err() {
        return TreeFacts::default();
    }
    let Some(tree) = parser.parse(text, None) else {
        return TreeFacts::default();
    };
    let mut facts = TreeFacts::default();
    walk(tree.root_node(), text, file, language, &mut facts);
    // Same-file references, computed after the walk so every
    // constant knows whether anything else in its file names it.
    // The definition's own line never counts as a use.
    let lines: Vec<&str> = text.lines().collect();
    for constant in &mut facts.constants {
        constant.referenced_elsewhere =
            lines.iter().enumerate().any(|(index, line)| {
                index != constant.line
                    && line_tokens(line).any(|token| token == constant.name)
            });
    }
    facts
}

/// Word tokens of a line: identifiers and numbers, split on
/// everything else.
fn line_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
}

/// Whether a Python node sits at module level: its ancestor
/// chain, skipping the `expression_statement` wrapper the grammar
/// puts around top-level statements, ends at the module.
fn is_top_level(mut node: tree_sitter::Node<'_>) -> bool {
    loop {
        let Some(parent) = node.parent() else {
            return false;
        };
        match parent.kind() {
            "module" => return true,
            "expression_statement" => node = parent,
            _ => return false,
        }
    }
}

fn text_of<'a>(node: tree_sitter::Node<'a>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// Local name for the shared whitespace rule,
/// [`sephera_core::declarations::collapse_whitespace`], kept so the nine
/// call sites below read as signature comparison rather than text munging.
fn normalized(text: &str) -> String {
    sephera_core::declarations::collapse_whitespace(text)
}

fn walk(
    node: tree_sitter::Node<'_>,
    source: &str,
    file: &str,
    language: Language,
    facts: &mut TreeFacts,
) {
    match (language, node.kind()) {
        (Language::Rust, "call_expression") => {
            if let Some(call) = rust_call(node, source, file) {
                facts.calls.push(call);
            }
        }
        (Language::Rust, "function_item")
        | (Language::Python, "function_definition") => {
            if let (Some(name), Some(body)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("body"),
            ) {
                facts.functions.push(FunctionDef {
                    file: file.to_owned(),
                    name: normalized(text_of(name, source)),
                    body: text_of(body, source).to_owned(),
                });
            }
        }
        (Language::Rust, "const_item") => {
            if let (Some(name), Some(value)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) {
                let value_text = normalized(text_of(value, source));
                facts.constants.push(ConstantDef {
                    file: file.to_owned(),
                    name: normalized(text_of(name, source)),
                    numeric: is_rust_numeric(&value_text),
                    value: value_text,
                    line: node.start_position().row,
                    referenced_elsewhere: false,
                });
            }
        }
        (Language::Python, "call") => {
            if let Some(call) = python_call(node, source, file) {
                facts.calls.push(call);
            }
        }
        (Language::Python, "assignment") => {
            if is_top_level(node)
                && let (Some(left), Some(right)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("right"),
                )
                && left.kind() == "identifier"
            {
                let value_text = normalized(text_of(right, source));
                facts.constants.push(ConstantDef {
                    file: file.to_owned(),
                    name: normalized(text_of(left, source)),
                    numeric: is_python_numeric(&value_text),
                    value: value_text,
                    line: node.start_position().row,
                    referenced_elsewhere: false,
                });
            }
        }
        _ => {}
    }
    for index in 0..node.child_count() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        if let Some(child) = node.child(index) {
            walk(child, source, file, language, facts);
        }
    }
}

fn rust_call(
    node: tree_sitter::Node<'_>,
    source: &str,
    file: &str,
) -> Option<CallSite> {
    let function = node.child_by_field_name("function")?;
    let text = text_of(function, source);
    // Method calls name a receiver, not a function: their
    // behaviour depends on the receiver's type, which this scan
    // cannot resolve, so they are out of scope.
    if text.contains('.') {
        return None;
    }
    let callee = text.rsplit("::").next()?.to_owned();
    if callee.is_empty()
        || !callee
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let mut args = Vec::new();
    for index in 0..arguments.named_child_count() {
        let child = arguments.named_child(u32::try_from(index).ok()?)?;
        args.push(normalized(text_of(child, source)));
    }
    Some(CallSite {
        file: file.to_owned(),
        callee,
        args,
    })
}

fn python_call(
    node: tree_sitter::Node<'_>,
    source: &str,
    file: &str,
) -> Option<CallSite> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "identifier" {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let mut args = Vec::new();
    for index in 0..arguments.named_child_count() {
        let child = arguments.named_child(u32::try_from(index).ok()?)?;
        // Keyword arguments are order-independent by name, and
        // splats hide the argument list: a call with either is
        // not a call this detector can reason about.
        if child.kind() == "keyword_argument"
            || text_of(child, source).starts_with('*')
        {
            return None;
        }
        args.push(normalized(text_of(child, source)));
    }
    Some(CallSite {
        file: file.to_owned(),
        callee: normalized(text_of(function, source)),
        args,
    })
}

const RUST_SUFFIXES: [&str; 14] = [
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64",
];

fn is_rust_numeric(value: &str) -> bool {
    let stripped: String = value.chars().filter(|c| *c != '_').collect();
    if stripped.parse::<f64>().is_ok() {
        return true;
    }
    RUST_SUFFIXES.iter().any(|suffix| {
        stripped.strip_suffix(suffix).is_some_and(|prefix| {
            !prefix.is_empty() && prefix.parse::<f64>().is_ok()
        })
    })
}

fn is_python_numeric(value: &str) -> bool {
    let stripped: String = value.chars().filter(|c| *c != '_').collect();
    stripped.parse::<f64>().is_ok()
}

/// Split a parameter list on top-level commas: a default value
/// may itself contain commas inside brackets.
fn split_top_level(parameters: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for c in parameters.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }
    parts
}

/// Parameter names of a signature, in order: the text before any
/// `:` annotation or `=` default, with `mut` and `*` stripped.
fn param_names(signature: &Signature) -> Vec<String> {
    split_top_level(&signature.parameters)
        .iter()
        .map(|param| {
            let before = param
                .split([':', '='])
                .next()
                .unwrap_or("")
                .trim()
                .trim_start_matches("mut ")
                .trim_start_matches(['*', '&'])
                .trim();
            before.to_owned()
        })
        .collect()
}

/// Parameter types of a Rust signature, in order: the text after
/// `:`, or `self` for the receiver, which is always first.
fn rust_param_types(signature: &Signature) -> Vec<String> {
    split_top_level(&signature.parameters)
        .iter()
        .map(|param| {
            let trimmed = param.trim().trim_start_matches("mut ");
            if trimmed == "self"
                || trimmed.starts_with("self:")
                || trimmed.starts_with("&self")
            {
                "self".to_owned()
            } else if let Some((_, ty)) = trimmed.split_once(':') {
                ty.trim().to_owned()
            } else {
                String::new()
            }
        })
        .collect()
}

/// Whether a body is symmetric over two parameters: it contains
/// one commutative core over exactly those two names. Whitespace
/// is removed first, so formatting cannot hide the pattern — and
/// a body that merely contains such a core counts, because the
/// check errs toward skipping: a false skip costs recall, a false
/// flag costs the precision this command is measured by.
/// The operator table for [`body_is_symmetric`]'s commutative cores.
/// Hand-writing both orders per operator meant adding an operator
/// touched two mirrored lines and could leave one order present and
/// the other missing; the check set is unchanged, only its
/// construction is table-driven now.
const SYMMETRIC_OPS: [&str; 6] = ["+", "*", "==", "!=", "&&", "||"];
const SYMMETRIC_NAMES: [&str; 2] = ["min", "max"];

fn body_is_symmetric(body: &str, first: &str, second: &str) -> bool {
    let flat: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let binary = SYMMETRIC_OPS.iter().flat_map(|op| {
        [
            format!("{first}{op}{second}"),
            format!("{second}{op}{first}"),
        ]
    });
    let named = SYMMETRIC_NAMES.iter().flat_map(|name| {
        [
            format!("{name}({first},{second})"),
            format!("{name}({second},{first})"),
        ]
    });
    binary.chain(named).any(|core| flat.contains(&core))
}

/// The two positions that differ between two argument lists, when
/// the lists are a single transposition of each other.
fn transposition(
    old_args: &[String],
    new_args: &[String],
) -> Option<(usize, usize)> {
    if old_args.len() != new_args.len() {
        return None;
    }
    let differing: Vec<usize> = old_args
        .iter()
        .zip(new_args.iter())
        .enumerate()
        .filter_map(|(index, (old, new))| (old != new).then_some(index))
        .collect();
    if differing.len() != 2 {
        return None;
    }
    let (i, j) = (differing[0], differing[1]);
    if old_args[i] == new_args[j] && old_args[j] == new_args[i] {
        Some((i, j))
    } else {
        None
    }
}

fn same_multiset(old_args: &[String], new_args: &[String]) -> bool {
    if old_args.len() != new_args.len() {
        return false;
    }
    let mut old_sorted = old_args.to_vec();
    let mut new_sorted = new_args.to_vec();
    old_sorted.sort();
    new_sorted.sort();
    old_sorted == new_sorted
}

/// Whether a constant is imported by another file in the new tree:
/// any edge from elsewhere whose import path names it, or whose
/// items list it.
fn imported_elsewhere(report: &GraphReport, file: &str, name: &str) -> bool {
    report.edges.iter().any(|edge| {
        edge.from != file
            && (edge.items.iter().any(|item| item == name)
                || edge
                    .import_path
                    .rsplit(['.', '/', ':'])
                    .next()
                    .is_some_and(|last| last == name))
    })
}

/// Collect every Rust and Python file under `base`, as
/// (relative path, text) pairs.
///
/// Files the ignore matcher excludes are skipped, as is anything
/// no text scan should enter. Relative paths use the graph's
/// spelling, so the facts line up with the declaration index and
/// the report edges.
#[must_use]
pub fn collect_texts(
    base: &Path,
    ignore: &IgnoreMatcher,
) -> Vec<(String, String)> {
    let mut texts = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(base) else {
                continue;
            };
            if ignore.is_ignored(relative) {
                continue;
            }
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                {
                    continue;
                }
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "rs" || ext == "py")
            {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    texts.push((
                        relative.to_string_lossy().replace('\\', "/"),
                        text,
                    ));
                }
            }
        }
    }
    texts.sort_by(|a, b| a.0.cmp(&b.0));
    texts
}

/// Merge one side's files into a single set of facts.
#[must_use]
pub fn collect_facts(base: &Path, ignore: &IgnoreMatcher) -> TreeFacts {
    let mut merged = TreeFacts::default();
    for (file, text) in collect_texts(base, ignore) {
        let facts = extract_facts(&file, &text);
        merged.calls.extend(facts.calls);
        merged.constants.extend(facts.constants);
        merged.functions.extend(facts.functions);
    }
    merged
}

/// Render the report as Markdown, one renderer for both
/// surfaces: the CLI and the MCP tool print this text, so a
/// wording fix lands in one place.
///
/// The count line comes first, because a behaviour scan is read
/// for its size before its entries, and an empty scan says so
/// plainly rather than printing a bare heading: no silent change
/// is the answer, not an absence of one.
#[must_use]
pub fn render_markdown(report: &BehaviorReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    writeln!(
        output,
        "# Silent behaviour changes: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    writeln!(output, "{} change(s) found.", report.changes.len())
        .expect("writing to a String must succeed");

    if report.changes.is_empty() {
        writeln!(output, "\nNo silent behaviour change was found.")
            .expect("writing to a String must succeed");
        return output;
    }

    for change in &report.changes {
        let line = match change {
            BehaviorChange::ArgOrderSwap {
                file,
                callee,
                old_args,
                new_args,
            } => {
                format!(
                    "Swapped arguments at a call to `{callee}` in `{file}`: ({}) → ({}).",
                    old_args.join(", "),
                    new_args.join(", ")
                )
            }
            BehaviorChange::ConstantChanged {
                file,
                name,
                old_value,
                new_value,
            } => {
                format!(
                    "Changed `{name}` in `{file}`: {old_value} → {new_value}."
                )
            }
        };
        writeln!(output, "\n- {line}")
            .expect("writing to a String must succeed");
    }
    output
}

/// Compare two trees' facts and report the silent changes.
///
/// `old_index` and `new_index` carry the callee signatures the
/// filters need; `new_report` carries the cross-file uses the
/// constant filter needs.
#[must_use]
pub fn measure_behavior(
    old_index: &DeclarationIndex,
    new_index: &DeclarationIndex,
    new_report: &GraphReport,
    old_facts: &TreeFacts,
    new_facts: &TreeFacts,
    from_ref: &str,
    to_ref: &str,
) -> BehaviorReport {
    let mut changes = Vec::new();

    // Call sites, grouped by file and callee so one file's calls
    // to one function pair up without consulting other files.
    let mut old_calls: BTreeMap<(&str, &str), Vec<&CallSite>> = BTreeMap::new();
    for call in &old_facts.calls {
        old_calls
            .entry((call.file.as_str(), call.callee.as_str()))
            .or_default()
            .push(call);
    }
    let mut new_calls: BTreeMap<(&str, &str), Vec<&CallSite>> = BTreeMap::new();
    for call in &new_facts.calls {
        new_calls
            .entry((call.file.as_str(), call.callee.as_str()))
            .or_default()
            .push(call);
    }
    let bodies: BTreeMap<(&str, &str), &str> = new_facts
        .functions
        .iter()
        .map(|function| {
            (
                (function.file.as_str(), function.name.as_str()),
                function.body.as_str(),
            )
        })
        .collect();

    for ((file, callee), olds) in &old_calls {
        let Some(news) = new_calls.get(&(*file, *callee)) else {
            continue;
        };
        let mut used = vec![false; news.len()];
        for old in olds {
            let mut found = None;
            for (index, candidate) in news.iter().enumerate() {
                if !used[index]
                    && same_multiset(&old.args, &candidate.args)
                    && old.args != candidate.args
                {
                    found = Some(index);
                    break;
                }
            }
            let Some(new_index_in_group) = found else {
                continue;
            };
            used[new_index_in_group] = true;
            let new = news[new_index_in_group];
            let Some((i, j)) = transposition(&old.args, &new.args) else {
                continue;
            };
            if let Some(change) = check_swap(
                file, callee, old, new, i, j, old_index, new_index, &bodies,
            ) {
                changes.push(change);
            }
        }
    }

    changes.extend(detect_constant_changes(new_report, old_facts, new_facts));

    changes.sort_by(|a, b| {
        let key = |change: &BehaviorChange| match change {
            BehaviorChange::ArgOrderSwap { file, callee, .. } => {
                (file.clone(), callee.clone())
            }
            BehaviorChange::ConstantChanged { file, name, .. } => {
                (file.clone(), name.clone())
            }
        };
        key(a).cmp(&key(b))
    });
    BehaviorReport {
        from_ref: from_ref.to_owned(),
        to_ref: to_ref.to_owned(),
        changes,
    }
}

/// Whether one swapped call site is a silent behaviour change, or
/// one of the shapes the filters refuse: a coordinated signature
/// change, a Rust swap that would not type check, an unresolvable
/// Rust callee that cannot prove it would, or a symmetric body.
#[allow(clippy::too_many_arguments, reason = "the filters share one call site")]
fn check_swap(
    file: &str,
    callee: &str,
    old: &CallSite,
    new: &CallSite,
    i: usize,
    j: usize,
    old_index: &DeclarationIndex,
    new_index: &DeclarationIndex,
    bodies: &BTreeMap<(&str, &str), &str>,
) -> Option<BehaviorChange> {
    // The callee's signature, when it can be found exactly once:
    // a changed signature means the call change is coordinated,
    // and a Rust swap that would not type check is a compile
    // error, not a silent change.
    let declaring = new_index.files_declaring(callee);
    let callee_file = if declaring.len() == 1 {
        Some(declaring[0])
    } else {
        None
    };
    if let Some(callee_file) = callee_file {
        let old_sig = old_index.signature_in(callee_file, callee);
        let new_sig = new_index.signature_in(callee_file, callee);
        match (old_sig, new_sig) {
            (Some(old_sig), Some(new_sig)) => {
                if is_rust(file) {
                    let old_returns =
                        old_sig.returns.as_deref().map(normalized);
                    let new_returns =
                        new_sig.returns.as_deref().map(normalized);
                    if normalized(&old_sig.parameters)
                        != normalized(&new_sig.parameters)
                        || old_returns != new_returns
                    {
                        return None;
                    }
                    let types = rust_param_types(new_sig);
                    if types.get(i).is_none_or(String::is_empty)
                        || types.get(j).is_none_or(String::is_empty)
                        || types[i] != types[j]
                    {
                        return None;
                    }
                } else if param_names(old_sig) != param_names(new_sig) {
                    return None;
                }
            }
            // A callee that appears or vanishes is not a swap: its
            // calls are additions or removals.
            _ => return None,
        }
    } else if is_rust(file) {
        // An unresolvable Rust callee cannot prove its swap type
        // checks, so it is not flagged.
        return None;
    }
    // A symmetric body does not change behaviour when its
    // parameters swap.
    if let Some(callee_file) = callee_file
        && let Some(body) = bodies.get(&(callee_file, callee))
        && let Some(new_sig) = new_index.signature_in(callee_file, callee)
    {
        let new_names = param_names(new_sig);
        if let (Some(first), Some(second)) =
            (new_names.get(i), new_names.get(j))
            && body_is_symmetric(body, first, second)
        {
            return None;
        }
    }
    Some(BehaviorChange::ArgOrderSwap {
        file: file.to_owned(),
        callee: callee.to_owned(),
        old_args: old.args.clone(),
        new_args: new.args.clone(),
    })
}

/// Numeric constants whose value changed while still used, by
/// file and name.
fn detect_constant_changes(
    new_report: &GraphReport,
    old_facts: &TreeFacts,
    new_facts: &TreeFacts,
) -> Vec<BehaviorChange> {
    let mut old_consts: BTreeMap<(&str, &str), &ConstantDef> = BTreeMap::new();
    for constant in &old_facts.constants {
        old_consts
            .insert((constant.file.as_str(), constant.name.as_str()), constant);
    }
    let mut changes = Vec::new();
    for new in &new_facts.constants {
        let Some(old) = old_consts.get(&(new.file.as_str(), new.name.as_str()))
        else {
            continue;
        };
        if old.value == new.value || !new.numeric {
            continue;
        }
        if !new.referenced_elsewhere
            && !imported_elsewhere(new_report, &new.file, &new.name)
        {
            continue;
        }
        changes.push(BehaviorChange::ConstantChanged {
            file: new.file.clone(),
            name: new.name.clone(),
            old_value: old.value.clone(),
            new_value: new.value.clone(),
        });
    }
    changes
}

#[cfg(test)]
mod tests;
