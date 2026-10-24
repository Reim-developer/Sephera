//! JNI boundary harvest: the `native` methods a Java file declares.
//!
//! A `native` method is a promise the language makes to a boundary: the
//! body is not Java, and the runtime will be handed a C function whose
//! name is this method's, mangled by a fixed rule — `Java_`, the package
//! with its dots as underscores, the class, the method, each part escaped.
//! That mangled name is the only thread between the two files, so it is
//! what the harvest records: this file *declares* the symbol, and whichever
//! C file defines a function of that name implements it.
//!
//! Escaping follows the JNI rule for the shapes source actually contains:
//! `_` becomes `_1`, `.` and `/` become `_`, `;` becomes `_2`, `[` becomes
//! `_3`, and any other non-alphanumeric character becomes `_0xxxx`. Class
//! nesting is part of the name (`Outer_Inner`), because that is how the
//! runtime spells it.

use std::fmt::Write as _;

use tree_sitter::{Node, Tree};

use sephera_core::boundary::{
    BoundaryKind, BoundaryRole, BoundarySymbol, FileBoundary,
};
use sephera_core::plugins::node_text;

/// Harvests the JNI symbols this source declares, if any.
pub fn collect_boundary(source: &[u8], tree: &Tree) -> Option<FileBoundary> {
    let root = tree.root_node();
    let package = package_name(source, &root);
    let mut symbols = Vec::new();
    collect_from(source, &root, &package, &mut Vec::new(), &mut symbols);
    if symbols.is_empty() {
        None
    } else {
        Some(FileBoundary {
            modules: Vec::new(),
            symbols,
        })
    }
}

/// The package this compilation unit declares, or the empty string for the
/// default package.
fn package_name(source: &[u8], root: &Node<'_>) -> String {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_declaration" {
            let mut inner = child.walk();
            for part in child.children(&mut inner) {
                if part.kind() == "scoped_identifier"
                    || part.kind() == "identifier"
                {
                    return node_text(source, &part);
                }
            }
        }
    }
    String::new()
}

/// Walks the tree carrying the class path, recording `native` methods.
fn collect_from(
    source: &[u8],
    node: &Node<'_>,
    package: &str,
    class_path: &mut Vec<String>,
    symbols: &mut Vec<BoundarySymbol>,
) {
    match node.kind() {
        "class_declaration" | "interface_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                class_path.push(node_text(source, &name));
                walk_children(source, node, package, class_path, symbols);
                class_path.pop();
                return;
            }
        }
        "method_declaration" if is_native(source, node) => {
            if let Some(name) = node.child_by_field_name("name") {
                let method = node_text(source, &name);
                symbols.push(BoundarySymbol {
                    name: mangled_name(package, class_path, &method),
                    kind: BoundaryKind::Jni,
                    role: BoundaryRole::Declares,
                    local: method,
                });
            }
            return;
        }
        _ => {}
    }
    walk_children(source, node, package, class_path, symbols);
}

fn walk_children(
    source: &[u8],
    node: &Node<'_>,
    package: &str,
    class_path: &mut Vec<String>,
    symbols: &mut Vec<BoundarySymbol>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_from(source, &child, package, class_path, symbols);
    }
}

/// Whether a method declaration carries the `native` modifier.
fn is_native(source: &[u8], node: &Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        child.kind() == "modifiers"
            && node_text(source, &child)
                .split_whitespace()
                .any(|word| word == "native")
    })
}

/// The JNI name for one method: `Java_<package>_<class>_<method>`, each
/// part escaped, class nesting joined as the runtime joins it.
fn mangled_name(package: &str, class_path: &[String], method: &str) -> String {
    let mut name = String::from("Java_");
    if !package.is_empty() {
        name.push_str(&escape(package));
        name.push('_');
    }
    for class in class_path {
        name.push_str(&escape(class));
        name.push('_');
    }
    name.push_str(&escape(method));
    name
}

/// The JNI escaping of one name part.
fn escape(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for ch in part.chars() {
        match ch {
            '_' => out.push_str("_1"),
            '.' | '/' => out.push('_'),
            ';' => out.push_str("_2"),
            '[' => out.push_str("_3"),
            _ if ch.is_ascii_alphanumeric() => out.push(ch),
            _ => {
                let _ = write!(out, "_0{:04x}", u32::from(ch));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sephera_compression::{SupportedLanguage, with_parser};

    fn harvest(source: &str) -> Option<FileBoundary> {
        with_parser(SupportedLanguage::Java, source.len(), |parser| {
            let tree = parser
                .parse(source.as_bytes(), None)
                .expect("the source parses");
            Ok(collect_boundary(source.as_bytes(), &tree))
        })
        .expect("a parser exists")
    }

    fn names(source: &str) -> Vec<String> {
        harvest(source)
            .expect("a boundary is harvested")
            .symbols
            .into_iter()
            .map(|symbol| symbol.name)
            .collect()
    }

    #[test]
    fn a_native_method_is_harvested_under_its_mangled_name() {
        let found = names(
            "package com.acme;\n\
             public class Native {\n\
                 public static native int add(int a, int b);\n\
                 public int ordinary(int a) { return a; }\n\
             }\n",
        );
        assert_eq!(found, ["Java_com_acme_Native_add"]);
    }

    #[test]
    fn a_nested_class_joins_the_name_with_an_underscore() {
        let found = names(
            "package com.acme;\n\
             public class Outer {\n\
                 public static class Inner {\n\
                     public native void run();\n\
                 }\n\
             }\n",
        );
        assert_eq!(found, ["Java_com_acme_Outer_Inner_run"]);
    }

    #[test]
    fn underscores_in_names_are_escaped() {
        let found = names(
            "package com.my_lib;\n\
             public class Native {\n\
                 public native void do_thing();\n\
             }\n",
        );
        assert_eq!(found, ["Java_com_my_1lib_Native_do_1thing"]);
    }

    #[test]
    fn the_default_package_has_no_prefix_part() {
        let found =
            names("public class Native { public native void run(); }\n");
        assert_eq!(found, ["Java_Native_run"]);
    }

    #[test]
    fn a_plain_java_file_has_no_boundary() {
        assert!(
            harvest(
                "public class Plain { public int add(int a) { return a; } }\n"
            )
            .is_none()
        );
    }
}
