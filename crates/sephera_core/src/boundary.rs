//! Cross-language boundary evidence: the names a file exposes to another
//! language through a foreign-function interface.
//!
//! A dependency graph built one language at a time has a hole wherever code
//! crosses a language boundary in source: Python imports a module that only
//! exists as a compiled Rust extension, Java declares a `native` method whose
//! body is a C function. Neither side's own resolver can place the reference,
//! because the file that satisfies it is written in the other language.
//!
//! This module is the vocabulary for closing that hole. A language plugin
//! harvests the boundary declarations its syntax carries — a `#[pymodule]`
//! and the names it registers, a `//export` line, a `native` method — and the
//! graph's resolution consults the resulting index when an import resolves
//! nowhere inside its own language. The harvest is evidence, not inference:
//! a name is only in this index when source text says it crosses.

use std::collections::{BTreeMap, BTreeSet};

/// The boundary mechanism a name crosses by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundaryKind {
    /// A Rust `#[pymodule]` and the items it registers, called from Python
    /// through `PyO3`.
    PyO3,
    /// A Rust `#[wasm_bindgen]` export, called from JavaScript or TypeScript.
    WasmBindgen,
    /// A Go `//export` name, called from C through cgo.
    Cgo,
    /// A Java `native` method, implemented in C or C++ through JNI.
    Jni,
}

/// One module a file exposes across a boundary.
///
/// `name` is the spelling the foreign side imports — for `PyO3` the
/// `#[pymodule]` function's name (or its `name = "…"` override), which is
/// what a Python `import` statement names. `exposed` holds the names visible
/// inside the module, again in their foreign spelling, so an item-level
/// reference (`from m import f`) can be checked against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryModule {
    /// The module name as the foreign side spells it.
    pub name: String,
    /// The mechanism the module crosses by.
    pub kind: BoundaryKind,
    /// The names the module exposes, in foreign spelling.
    pub exposed: BTreeSet<String>,
}

/// Which side of a boundary a file stands on for one symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundaryRole {
    /// The file declares that the name exists elsewhere: a Java `native`
    /// method, whose body the language runtime must be given.
    Declares,
    /// The file provides the name: a C function with a JNI-mangled name,
    /// whose only caller is the boundary itself.
    Implements,
}

/// One name that crosses a boundary symbol by symbol rather than as a
/// module.
///
/// JNI is the case. No import statement on either side names the other
/// file: Java declares a `native` method and C defines a function whose
/// name is the method's, mangled (`Java_com_acme_Native_add`). The pair
/// only exists because both spellings are harvested and matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundarySymbol {
    /// The name as the boundary spells it (for JNI, the mangled name).
    pub name: String,
    /// The mechanism the symbol crosses by.
    pub kind: BoundaryKind,
    /// Which side of the boundary this file is on.
    pub role: BoundaryRole,
    /// The name as this side spells it: the Java method's own name on the
    /// declaring side, the mangled name itself on the implementing side.
    pub local: String,
}

/// A declared symbol and the file that implements it, paired by the index.
///
/// The pair is what a join turns into an edge: the declaring file depends
/// on the implementing one, because calling the method requires the
/// implementation the boundary loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinedSymbol<'index> {
    /// The boundary spelling of the symbol.
    pub name: &'index str,
    /// The name as the declaring side spells it.
    pub local: &'index str,
    /// The file that declares the symbol (the edge's source).
    pub declarer: &'index str,
    /// The file that implements it (the edge's target).
    pub implementer: &'index str,
}

/// Everything one file exposes across language boundaries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileBoundary {
    /// The boundary modules this file declares.
    pub modules: Vec<BoundaryModule>,
    /// The boundary symbols this file declares or implements.
    pub symbols: Vec<BoundarySymbol>,
}

impl FileBoundary {
    /// Whether this file exposes nothing across any boundary.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.symbols.is_empty()
    }
}

/// The analysis-wide index of boundary modules, keyed by the name the
/// foreign side imports.
///
/// A module name claimed by two different files is ambiguous and answers as
/// absent: two crates in one analysis can each define a `#[pymodule]` of the
/// same name, and joining an import to the first claimant would fabricate an
/// edge to a file the import may never reach. The same rule the resolver
/// applies to duplicate crate names, for the same reason.
#[derive(Debug, Default)]
pub struct BoundaryIndex {
    modules: BTreeMap<String, (String, BoundaryModule)>,
    ambiguous: BTreeSet<String>,
    declared_symbols: BTreeMap<String, (String, BoundarySymbol)>,
    implemented_symbols: BTreeMap<String, (String, BoundarySymbol)>,
    ambiguous_symbols: BTreeSet<String>,
}

impl BoundaryIndex {
    /// Records the boundary modules and symbols one file declares.
    pub fn insert(&mut self, file: &str, boundary: FileBoundary) {
        for module in boundary.modules {
            if self.ambiguous.contains(&module.name) {
                continue;
            }
            match self.modules.get(&module.name) {
                Some((owner, _)) if owner != file => {
                    self.modules.remove(&module.name);
                    self.ambiguous.insert(module.name);
                }
                _ => {
                    self.modules
                        .insert(module.name.clone(), (file.to_owned(), module));
                }
            }
        }
        for symbol in boundary.symbols {
            if self.ambiguous_symbols.contains(&symbol.name) {
                continue;
            }
            let claimed = match symbol.role {
                BoundaryRole::Declares => {
                    self.declared_symbols.get(&symbol.name)
                }
                BoundaryRole::Implements => {
                    self.implemented_symbols.get(&symbol.name)
                }
            };
            match claimed {
                Some((owner, _)) if owner != file => {
                    match symbol.role {
                        BoundaryRole::Declares => {
                            self.declared_symbols.remove(&symbol.name);
                        }
                        BoundaryRole::Implements => {
                            self.implemented_symbols.remove(&symbol.name);
                        }
                    }
                    self.ambiguous_symbols.insert(symbol.name);
                }
                _ => match symbol.role {
                    BoundaryRole::Declares => {
                        self.declared_symbols.insert(
                            symbol.name.clone(),
                            (file.to_owned(), symbol),
                        );
                    }
                    BoundaryRole::Implements => {
                        self.implemented_symbols.insert(
                            symbol.name.clone(),
                            (file.to_owned(), symbol),
                        );
                    }
                },
            }
        }
    }

    /// Every declared symbol an unambiguous implementer answers, paired.
    ///
    /// An overloaded JNI method's implementation carries a signature
    /// suffix (`Java_..._add__II`), so an implementer also answers the
    /// declared name it extends up to the first `__`. A declared symbol
    /// with no implementer, and an implementer nobody declares, are both
    /// left out: the join only states pairs both sides evidenced.
    #[must_use]
    pub fn symbol_pairs(&self) -> Vec<JoinedSymbol<'_>> {
        let mut pairs = Vec::new();
        for (name, (implementer, _)) in &self.implemented_symbols {
            let declared_name = match name.split_once("__") {
                Some((short, _))
                    if self.declared_symbols.contains_key(short) =>
                {
                    short
                }
                _ => name.as_str(),
            };
            if let Some((declarer, symbol)) =
                self.declared_symbols.get(declared_name)
                && declarer != implementer
            {
                pairs.push(JoinedSymbol {
                    name: declared_name,
                    local: &symbol.local,
                    declarer,
                    implementer,
                });
            }
        }
        pairs
    }

    /// The file and module a foreign import name resolves to, when exactly
    /// one file claims the name.
    #[must_use]
    pub fn module_named(&self, name: &str) -> Option<(&str, &BoundaryModule)> {
        self.modules
            .get(name)
            .map(|(file, module)| (file.as_str(), module))
    }

    /// Whether no file in the analysis exposes a boundary module.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(name: &str, exposed: &[&str]) -> BoundaryModule {
        BoundaryModule {
            name: name.to_owned(),
            kind: BoundaryKind::PyO3,
            exposed: exposed.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn a_module_answers_to_its_foreign_name() {
        let mut index = BoundaryIndex::default();
        index.insert(
            "rust/src/lib.rs",
            FileBoundary {
                modules: vec![module("myext", &["add", "Point"])],
                symbols: Vec::new(),
            },
        );

        let (file, found) =
            index.module_named("myext").expect("module is indexed");
        assert_eq!(file, "rust/src/lib.rs");
        assert!(found.exposed.contains("add"));
        assert!(index.module_named("other").is_none());
    }

    #[test]
    fn a_name_claimed_by_two_files_answers_as_absent() {
        let mut index = BoundaryIndex::default();
        index.insert(
            "a/src/lib.rs",
            FileBoundary {
                modules: vec![module("myext", &["add"])],
                symbols: Vec::new(),
            },
        );
        index.insert(
            "b/src/lib.rs",
            FileBoundary {
                modules: vec![module("myext", &["subtract"])],
                symbols: Vec::new(),
            },
        );

        assert!(index.module_named("myext").is_none());
        assert!(index.is_empty());
    }

    #[test]
    fn a_third_claimant_does_not_revive_an_ambiguous_name() {
        let mut index = BoundaryIndex::default();
        for file in ["a.rs", "b.rs", "c.rs"] {
            index.insert(
                file,
                FileBoundary {
                    modules: vec![module("myext", &[])],
                    symbols: Vec::new(),
                },
            );
        }

        assert!(index.module_named("myext").is_none());
    }

    #[test]
    fn one_file_may_expose_several_modules() {
        let mut index = BoundaryIndex::default();
        index.insert(
            "rust/src/lib.rs",
            FileBoundary {
                modules: vec![module("one", &[]), module("two", &[])],
                symbols: Vec::new(),
            },
        );

        assert!(index.module_named("one").is_some());
        assert!(index.module_named("two").is_some());
    }

    fn symbol(name: &str, role: BoundaryRole, local: &str) -> BoundarySymbol {
        BoundarySymbol {
            name: name.to_owned(),
            kind: BoundaryKind::Jni,
            role,
            local: local.to_owned(),
        }
    }

    fn symbols_only(symbols: Vec<BoundarySymbol>) -> FileBoundary {
        FileBoundary {
            modules: Vec::new(),
            symbols,
        }
    }

    #[test]
    fn a_declared_symbol_pairs_with_its_implementer() {
        let mut index = BoundaryIndex::default();
        index.insert(
            "Native.java",
            symbols_only(vec![symbol(
                "Java_com_acme_Native_add",
                BoundaryRole::Declares,
                "add",
            )]),
        );
        index.insert(
            "native.c",
            symbols_only(vec![symbol(
                "Java_com_acme_Native_add",
                BoundaryRole::Implements,
                "Java_com_acme_Native_add",
            )]),
        );

        let pairs = index.symbol_pairs();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].declarer, "Native.java");
        assert_eq!(pairs[0].implementer, "native.c");
        assert_eq!(pairs[0].local, "add");
    }

    #[test]
    fn an_overloaded_implementer_pairs_with_the_short_declared_name() {
        let mut index = BoundaryIndex::default();
        index.insert(
            "Native.java",
            symbols_only(vec![symbol(
                "Java_com_acme_Native_add",
                BoundaryRole::Declares,
                "add",
            )]),
        );
        index.insert(
            "native.c",
            symbols_only(vec![symbol(
                "Java_com_acme_Native_add__II",
                BoundaryRole::Implements,
                "Java_com_acme_Native_add__II",
            )]),
        );

        let pairs = index.symbol_pairs();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].name, "Java_com_acme_Native_add");
    }

    #[test]
    fn unpaired_symbols_on_either_side_form_no_pair() {
        let mut index = BoundaryIndex::default();
        index.insert(
            "Native.java",
            symbols_only(vec![symbol(
                "Java_com_acme_Native_add",
                BoundaryRole::Declares,
                "add",
            )]),
        );
        index.insert(
            "other.c",
            symbols_only(vec![symbol(
                "Java_org_other_Thing_run",
                BoundaryRole::Implements,
                "Java_org_other_Thing_run",
            )]),
        );

        assert_eq!(index.symbol_pairs().len(), 0);
    }

    #[test]
    fn a_symbol_declared_by_two_files_pairs_with_nothing() {
        let mut index = BoundaryIndex::default();
        for file in ["a/Native.java", "b/Native.java"] {
            index.insert(
                file,
                symbols_only(vec![symbol(
                    "Java_com_acme_Native_add",
                    BoundaryRole::Declares,
                    "add",
                )]),
            );
        }
        index.insert(
            "native.c",
            symbols_only(vec![symbol(
                "Java_com_acme_Native_add",
                BoundaryRole::Implements,
                "Java_com_acme_Native_add",
            )]),
        );

        assert_eq!(index.symbol_pairs().len(), 0);
    }
}
