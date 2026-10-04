"""Print the real tree-sitter node kinds for symbol-counting samples.

Used to check whether a failing assertion is a wrong expectation in the test
or a genuine gap in the rules table.
"""

import sys

from tree_sitter import Language, Parser

import tree_sitter_rust, tree_sitter_python, tree_sitter_typescript, tree_sitter_go, tree_sitter_java, tree_sitter_c


GRAMMARS = {
    "rust": tree_sitter_rust.language(),
    "python": tree_sitter_python.language(),
    "typescript": tree_sitter_typescript.language_typescript(),
    "go": tree_sitter_go.language(),
    "java": tree_sitter_java.language(),
    "c": tree_sitter_c.language(),
}

SAMPLES = {
    "rust": b"""
struct Point { x: i32 }
enum Colour { Red }
const LIMIT: usize = 10;
static NAME: &str = "x";
trait Shape { fn area(&self) -> f64; }
impl Shape for Point {
    fn area(&self) -> f64 { 0.0 }
}
fn main() {
    let helper = || 1;
}
""",
    "go": b"""package main
type Server struct { addr string }
func (s *Server) Start() error { return nil }
func main() {}
""",
    "typescript": b"""interface User { name: string }
enum Role { Admin }
class Account { balance(): number { return 0; } }
function topLevel(): void {}
const arrow = (): void => {};
""",
    "python": b"""class Service:
    def run(self):
        pass
def helper():
    pass
""",
}


def walk(node, depth=0):
    yield node, depth
    for child in node.children:
        yield from walk(child, depth + 1)


def main() -> int:
    langs = sys.argv[1:] or ["rust", "go", "typescript"]
    for name in langs:
        if name not in SAMPLES:
            print(f"no sample for {name}")
            continue
        parser = Parser(Language(GRAMMARS[name]))
        tree = parser.parse(SAMPLES[name])
        print(f"===== {name} =====")
        interesting = {
            "function_item", "function_declaration", "function_definition",
            "function_signature_item", "method_definition", "method_declaration",
            "method_signature", "function_signature", "generator_function_declaration",
            "generator_function",
            "struct_item", "struct_specifier", "trait_item", "union_item", "type_item",
            "mod_item", "class_declaration", "class_definition", "interface_declaration",
            "type_alias_declaration", "abstract_class_declaration", "record_declaration",
            "enum_item", "enum_declaration", "enum_specifier",
            "const_item", "static_item", "const_declaration", "field_declaration",
            "type_spec", "lexical_declaration", "variable_declarator",
        }
        for node, _ in walk(tree.root_node):
            if node.type in interesting:
                print(f"  {node.type}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())