//! Dry-run a rename or move: what a real rename would have to
//! touch, and what would block it, without editing the tree.
//!
//! A rename is the most common refactor and the one taken on faith:
//! the tool renames the declaration and every import it can find,
//! and the breakage shows up in CI. This measures the rename
//! beforehand. The consumer set is exactly `breaking`'s — the same
//! scan, shared, not copied — because a consumer is a consumer
//! whether the symbol broke or is about to move: a resolved edge to
//! the declaring file naming the symbol, an unresolved item half
//! whose parent statement lands there, a namespace import of the
//! whole file. What `breaking` cannot say, this adds: the
//! destination collision. Renaming onto a name the destination file
//! already declares is a duplicate definition in every language, so
//! it blocks the rename outright. A direct consumer that already
//! declares the new name is a shadowing warning instead — the import
//! would rebind a name the file already owns, an error in Rust and
//! a confusion in Python — because the severity is language
//! dependent and the report should not pretend otherwise.
//!
//! The prediction is mechanically checkable, and the corpus test
//! checks it: perform the rename for real in a fixture — the
//! declaration line plus every direct consumer's import statement —
//! rebuild, and the predicted direct-consumer set is exactly the
//! set of files the real rename rewrote, and a predicted
//! destination collision is exactly a duplicate declaration of the
//! new name in the destination file.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

use sephera_core::declarations::DeclarationIndex;

use crate::breaking::{ResolutionOracle, consumers, downstream_of};
use crate::types::GraphReport;

/// One file a rename would have to touch, with the evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimulatedConsumer {
    /// The file that names the symbol from its current file.
    pub file: String,
    /// Import paths evidencing the use.
    pub imports: Vec<String>,
}

/// How a name at the destination would collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionKind {
    /// The destination file already declares the new name: a
    /// duplicate definition, so the rename cannot happen.
    Destination,
    /// A direct consumer already declares the new name: the
    /// renamed import would rebind a name the file owns.
    Shadowing,
}

/// One name collision a rename would run into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimulatedCollision {
    /// The file where the new name already exists.
    pub file: String,
    /// Whether it blocks the rename or only warns.
    pub kind: CollisionKind,
}

/// What a rename or move would touch, and what would block it.
///
/// Field order is the JSON order: the CLI and the MCP tool
/// serialize this struct directly, so the declaration order is
/// the emitted order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimulateReport {
    /// The file that declares the symbol today.
    pub file: String,
    /// The symbol's name today.
    pub symbol: String,
    /// The name it would take.
    pub new_name: String,
    /// The file it would live in: the move target, or the file it
    /// already lives in for a pure rename.
    pub new_file: String,
    /// True when a destination collision blocks the rename.
    pub blocked: bool,
    /// Files that name the symbol and so must update their import.
    pub direct: Vec<SimulatedConsumer>,
    /// Files importing the file's namespace wholesale, which may
    /// use the symbol without naming it.
    pub namespace_users: Vec<String>,
    /// Files downstream of a direct or namespace user: untouched by
    /// the rename, named so the exposure is whole.
    pub downstream: Vec<String>,
    /// Name collisions at the destination and in direct consumers.
    pub collisions: Vec<SimulatedCollision>,
}

/// Why a rename cannot even be simulated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulateError {
    /// The file does not declare the symbol.
    SymbolNotFound {
        /// The file the caller named.
        file: String,
        /// The symbol the caller named.
        symbol: String,
    },
    /// The move target is not a file in the tree.
    TargetNotFound {
        /// The file the caller named.
        file: String,
    },
    /// The new name and file are the ones the symbol already has.
    NoChange,
}

impl fmt::Display for SimulateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymbolNotFound { file, symbol } => {
                write!(f, "{file} does not declare `{symbol}`")
            }
            Self::TargetNotFound { file } => {
                write!(f, "{file} is not a file in this tree")
            }
            Self::NoChange => {
                write!(
                    f,
                    "the new name and file are the ones the symbol already has"
                )
            }
        }
    }
}

impl std::error::Error for SimulateError {}

/// Predict what renaming `symbol` in `file` to `new_name` — and,
/// when `new_file` is present, moving it there — would touch and
/// what would block it.
///
/// `index` and `report` describe one tree, the working tree the
/// rename would run against. A destination collision blocks the
/// report's verdict, never the report itself: the caller asked what
/// would happen, and "it cannot happen" is an answer.
///
/// # Errors
///
/// Returns `SymbolNotFound` when the file does not declare the
/// symbol, `TargetNotFound` when the move target is not a file in
/// the tree, and `NoChange` when the new name and file are the ones
/// the symbol already has.
pub fn measure_simulate(
    index: &DeclarationIndex,
    report: &GraphReport,
    file: &str,
    symbol: &str,
    new_name: &str,
    new_file: Option<&str>,
) -> Result<SimulateReport, SimulateError> {
    if index.kind_in(file, symbol).is_none() {
        return Err(SimulateError::SymbolNotFound {
            file: file.to_owned(),
            symbol: symbol.to_owned(),
        });
    }
    let destination = new_file.unwrap_or(file);
    if destination != file && !is_known_file(index, report, destination) {
        return Err(SimulateError::TargetNotFound {
            file: destination.to_owned(),
        });
    }
    if new_name == symbol && destination == file {
        return Err(SimulateError::NoChange);
    }

    let oracle = ResolutionOracle::from_report(report);
    let (direct, namespace_users) = consumers(report, &oracle, file, symbol);
    let seeds: BTreeSet<&str> = direct
        .iter()
        .map(|user| user.file.as_str())
        .chain(namespace_users.iter().map(String::as_str))
        .collect();
    let downstream = downstream_of(report, file, &seeds);

    let mut collisions = Vec::new();
    if index
        .declared_in(destination)
        .iter()
        .any(|(name, _)| *name == new_name)
    {
        collisions.push(SimulatedCollision {
            file: destination.to_owned(),
            kind: CollisionKind::Destination,
        });
    }
    for user in &direct {
        if user.file != destination
            && index.kind_in(&user.file, new_name).is_some()
        {
            collisions.push(SimulatedCollision {
                file: user.file.clone(),
                kind: CollisionKind::Shadowing,
            });
        }
    }
    collisions.sort_by(|a, b| a.file.cmp(&b.file));
    let blocked = collisions
        .iter()
        .any(|collision| collision.kind == CollisionKind::Destination);

    Ok(SimulateReport {
        file: file.to_owned(),
        symbol: symbol.to_owned(),
        new_name: new_name.to_owned(),
        new_file: destination.to_owned(),
        blocked,
        direct: direct
            .into_iter()
            .map(|user| SimulatedConsumer {
                file: user.file,
                imports: user.imports,
            })
            .collect(),
        namespace_users,
        downstream,
        collisions,
    })
}

/// Whether a file belongs to the tree: declared in the index, a
/// node of the report, or an endpoint of one of its edges. A move
/// target with no declarations is still a real file, so the index
/// alone is not the test.
fn is_known_file(
    index: &DeclarationIndex,
    report: &GraphReport,
    file: &str,
) -> bool {
    index.files().any(|known| known == file)
        || report.nodes.iter().any(|node| node.file_path == file)
        || report
            .edges
            .iter()
            .any(|edge| edge.from == file || edge.to.as_deref() == Some(file))
}

#[cfg(test)]
mod tests;
