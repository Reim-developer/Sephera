//! Symbol-level diff between two refs: the API changelog.
//!
//! `breaking` answers "what breaks consumers". This module answers
//! the prior question: "what changed in the API surface at all".
//! The difference is additions — a new symbol breaks nothing, so
//! `breaking` never reports it, but a changelog without additions
//! is a changelog that hides half the release. A move reads here
//! as what it is at the index level, a removal plus an addition:
//! claiming a move would need the suggestion machinery, and a diff
//! that guesses identity is a diff that lies.
//!
//! The comparison rules are deliberately the same as `breaking`'s,
//! so the two commands never disagree about a symbol both name:
//! kinds compared outright, call shapes compared for functions
//! only, and a shape recorded on only one side stays silent rather
//! than inventing a shape the other side never had. Only Rust and
//! Python collect declarations today, so only their symbols can
//! appear here.

use serde::Serialize;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, Signature,
};

/// How one symbol changed between the two trees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DiffChange {
    /// The new tree declares the name; the old tree did not.
    Added,
    /// The old tree declared the name; the new tree does not.
    Removed,
    /// The name is still declared, as a different kind of thing.
    KindChanged {
        /// The kind the new tree declares it as.
        to: DeclarationKind,
    },
    /// The name is still declared as a function, but its call shape
    /// changed: a parameter type, a parameter name in Python, or the
    /// return annotation.
    SignatureChanged {
        /// The call shape the old tree declared.
        from: Signature,
        /// The call shape the new tree declares.
        to: Signature,
    },
}

/// One symbol that differs between the two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSymbol {
    /// The file that declares the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// The kind on the side where the symbol exists: the old kind
    /// for removals and changes, the new kind for additions.
    pub kind: DeclarationKind,
    /// How it changed.
    pub change: DiffChange,
}

/// The symbol-level diff between two refs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    /// The older ref, as the caller spelled it.
    pub from_ref: String,
    /// The newer ref, as the caller spelled it.
    pub to_ref: String,
    /// The changed symbols, file then symbol.
    pub changes: Vec<DiffSymbol>,
}

/// Compare two declaration indexes and list every symbol that was
/// added, removed, or reshaped.
#[must_use]
pub fn measure_diff(
    old_index: &DeclarationIndex,
    new_index: &DeclarationIndex,
    from_ref: &str,
    to_ref: &str,
) -> DiffReport {
    let mut changes = Vec::new();

    for file in old_index.files() {
        for (name, kind) in old_index.declared_in(file) {
            let change = match new_index.kind_in(file, name) {
                None => DiffChange::Removed,
                Some(new_kind) if new_kind != kind => {
                    DiffChange::KindChanged { to: new_kind }
                }
                Some(DeclarationKind::Function) => {
                    match (
                        old_index.signature_in(file, name),
                        new_index.signature_in(file, name),
                    ) {
                        (Some(from), Some(to)) if from != to => {
                            DiffChange::SignatureChanged {
                                from: from.clone(),
                                to: to.clone(),
                            }
                        }
                        _ => continue,
                    }
                }
                Some(_) => continue,
            };
            changes.push(DiffSymbol {
                file: file.to_owned(),
                symbol: name.to_owned(),
                kind,
                change,
            });
        }
    }

    for file in new_index.files() {
        for (name, kind) in new_index.declared_in(file) {
            if old_index.kind_in(file, name).is_none() {
                changes.push(DiffSymbol {
                    file: file.to_owned(),
                    symbol: name.to_owned(),
                    kind,
                    change: DiffChange::Added,
                });
            }
        }
    }

    changes.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    DiffReport {
        from_ref: from_ref.to_owned(),
        to_ref: to_ref.to_owned(),
        changes,
    }
}

#[cfg(test)]
mod tests;
