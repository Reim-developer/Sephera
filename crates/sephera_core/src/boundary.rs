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

/// Everything one file exposes across language boundaries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileBoundary {
    /// The boundary modules this file declares.
    pub modules: Vec<BoundaryModule>,
}

impl FileBoundary {
    /// Whether this file exposes nothing across any boundary.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
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
}

impl BoundaryIndex {
    /// Records the boundary modules one file declares.
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
            },
        );
        index.insert(
            "b/src/lib.rs",
            FileBoundary {
                modules: vec![module("myext", &["subtract"])],
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
            },
        );

        assert!(index.module_named("one").is_some());
        assert!(index.module_named("two").is_some());
    }
}
