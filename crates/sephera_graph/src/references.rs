//! Which files reference a declaration.
//!
//! The declaration index answers "what does this file contain" and the use index
//! answers "what does it mention". Neither answers the question that matters,
//! which is *whose* declaration a mention refers to -- and that is where a name
//! index stops being enough on its own.
//!
//! # Why the path is walked
//!
//! A name is not a symbol. `Cli` is a struct in `sephera_cli` and a different
//! struct in `sephera_tools`, and `Dependency` is declared in six files of this
//! repository. Looking a name up in a table of declarations cannot tell them
//! apart, and guessing is the one failure mode a blast radius cannot have: a
//! wrong answer puts a file in someone's radius who has nothing to do with the
//! change, and a limit that fires on those is a limit nobody trusts.
//!
//! So the attribution starts where the evidence is strongest and falls back only
//! as far as it has to:
//!
//! 1. **Follow the import.** A file that imports `crate::Widget` has already said
//!    where `Widget` comes from, and the resolver has already placed that path on
//!    a file. The walk follows it, and follows *re-exports* as well:
//!    `app.rs -> lib.rs -> widget.rs` is a two-hop chain, and stopping at the
//!    crate root would attribute every type in the crate to `lib.rs`.
//! 2. **One declarer.** When the name appears in exactly one file in the project,
//!    every mention of it is a mention of that one, and no path is needed.
//! 3. **One declarer in the same crate.** A file cannot import from a crate it is
//!    not in, so if exactly one candidate is in the reader's own crate it is the
//!    one meant. Two candidates in one crate is not enough, because two modules
//!    of a crate may declare the same name.
//! 4. **Nothing.** Files that mention the name and cannot be attributed are
//!    reported, with how many candidates there were. That count is part of the
//!    answer rather than a failure: a reader who knows four files declare
//!    `Dependency` learns something they would not from a silent omission.
//!
//! A mention no file in the project declares is *not* reported. `drop`, `todo`
//! and every name from the standard library fall here, and listing them would
//! bury the four that are genuinely unplaceable.
//!
//! # Why the target file itself is never in the answer
//!
//! A file is not coupled to itself. `widget.rs` implements `Widget` and mentions
//! it four times, and every one of those is that file's own business. The same
//! goes for a name the file binds locally, which the extractor already drops.
//!
//! # What is not resolved
//!
//! Method-level coupling is out of scope. `resolver.resolve()` names a method, and
//! this answers for `Resolver`; deciding that `resolve` is *this* `impl`'s method
//! rather than a trait's or another type's needs the receiver's type resolved
//! first, and a guess would put unrelated files in every radius. The mentions are
//! still indexed, so the data is there when the resolution is.

use std::collections::BTreeSet;

use sephera_core::declarations::DeclarationIndex;

use super::resolver::SymbolFacts;
use super::types::GraphReport;

/// The declaration a question is about.
///
/// Written the way a person would name it: a file and a name inside it. The file
/// is what makes the question answerable -- `Cli` alone would be a request for
/// two answers at once.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SymbolTarget {
    /// The file that declares it.
    pub file: String,
    /// The name, as declared.
    pub name: String,
}

/// How a file's mention of a name was tied to the target.
///
/// Ordered strongest first, so sorting a list of users by this ranks the answer
/// rather than scrambling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Attribution {
    /// The file's own import path led to the declaring file.
    ///
    /// The only one that is evidence rather than arithmetic: the file said where
    /// the name comes from, and the resolver had already placed that path.
    Import,
    /// Exactly one file in the project declares this name.
    Unique,
    /// Several files declare it, and exactly one is in the using file's crate.
    SameCrate,
}

/// One file that references the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolUser {
    /// The referencing file.
    pub file: String,
    /// Where it references it, sorted.
    pub lines: Vec<usize>,
    /// Why it was attributed here.
    pub via: Attribution,
}

/// One file that mentions the name and could not be tied to the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unattributed {
    /// The mentioning file.
    pub file: String,
    /// Where it mentions the name, sorted.
    pub lines: Vec<usize>,
    /// How many files declare a name that could be meant.
    ///
    /// Never zero. A mention nothing declares is a name from outside the project
    /// and is not reported at all; reporting it would bury the real cases.
    pub candidates: usize,
}

/// The blast radius of one declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolReferences {
    /// The declaration the question was about.
    pub target: SymbolTarget,
    /// Files that reference it, strongest attribution first.
    pub users: Vec<SymbolUser>,
    /// Files that mention the name without being attributable to the target.
    pub unattributed: Vec<Unattributed>,
}

/// The most hops a re-export chain may take.
///
/// Three is enough for `app -> lib -> module -> file`, and short enough that a
/// cycle a malformed `mod` arrangement can produce ends rather than spins. The
/// `seen` set already breaks cycles; the cap is about a chain that grows without
/// repeating, which a set cannot catch.
const MAX_REEXPORT_HOPS: usize = 3;

/// The named file does not declare the name the question was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownTarget;

impl std::fmt::Display for UnknownTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("that file does not declare that name")
    }
}

impl std::error::Error for UnknownTarget {}

/// Answer "which files reference `target`".
///
/// # Errors
///
/// Returns [`UnknownTarget`] when the file does not declare the name -- which
/// means the file or the spelling is wrong, not that the answer is empty. A name
/// nobody declares *anywhere* is not an error: it is an empty answer, because the
/// question named the file and the file has it.
pub fn find(
    report: &GraphReport,
    facts: &SymbolFacts,
    target: &SymbolTarget,
) -> Result<SymbolReferences, UnknownTarget> {
    if !facts.declarations.file_declares(&target.file, &target.name) {
        return Err(UnknownTarget);
    }

    let attributor = Attributor { report, facts };
    let mut users = Vec::new();
    let mut unattributed = Vec::new();

    for (file, lines) in facts.references.users_of(&target.name) {
        // A file is not coupled to itself. The extractor has already dropped the
        // mentions of names the file binds locally, so anything left here is the
        // file's business or another file's.
        if file == target.file {
            continue;
        }

        let mut lines = lines.to_vec();
        lines.sort_unstable();
        match attributor.attribute(&target.name, file, target) {
            Resolved::Target(via) => users.push(SymbolUser {
                file: file.to_owned(),
                lines,
                via,
            }),
            Resolved::Elsewhere => {}
            Resolved::Unplaceable(candidates) => {
                if candidates >= 2 {
                    unattributed.push(Unattributed {
                        file: file.to_owned(),
                        lines,
                        candidates,
                    });
                }
            }
        }
    }

    users.sort_by(|a, b| a.via.cmp(&b.via).then_with(|| a.file.cmp(&b.file)));
    unattributed.sort_by(|a, b| a.file.cmp(&b.file));

    Ok(SymbolReferences {
        target: target.clone(),
        users,
        unattributed,
    })
}

/// Where one file's mention of a name landed.
enum Resolved {
    /// It landed on the file being asked about.
    Target(Attribution),
    /// It landed on a different declaration of the same name.
    ///
    /// Not an error and not reported. `Cli` in `sephera_tools` is a different
    /// struct from `Cli` in `sephera_cli`, and mentioning it says nothing about
    /// the one being asked about -- which is the entire reason the question takes
    /// a file.
    Elsewhere,
    /// It landed nowhere defensible. The count is how many files declare the name.
    Unplaceable(usize),
}

/// Everything one attribution reads.
struct Attributor<'a> {
    report: &'a GraphReport,
    facts: &'a SymbolFacts,
}

impl Attributor<'_> {
    fn attribute(
        &self,
        name: &str,
        from: &str,
        target: &SymbolTarget,
    ) -> Resolved {
        // The path first, and the only rule that can prove anything.
        if let Some(resolved) = self.walk_imports(name, from, &target.file) {
            return resolved;
        }

        let declaring: Vec<&str> = self.declarations().files_declaring(name);
        match declaring.len() {
            // Nothing declares it: a name from outside the project, which is not a
            // claim about anyone.
            0 => Resolved::Unplaceable(0),
            // The target declares it (checked by `find`) and it is the only file
            // that does, so every mention is a mention of the target.
            1 => Resolved::Target(Attribution::Unique),
            _ => {
                let key = self.crate_of(from);
                let same_crate = declaring
                    .iter()
                    .filter(|file| self.crate_of(file) == key)
                    .count();
                // A file cannot import from a crate it is not in, so exactly one
                // candidate in the reader's own crate is enough. Two is not: two
                // modules of one crate may declare the same name.
                if same_crate == 1 {
                    Resolved::Target(Attribution::SameCrate)
                } else {
                    Resolved::Unplaceable(declaring.len())
                }
            }
        }
    }

    /// Follow the import chain from `from` to wherever `name` is declared.
    ///
    /// `None` means no chain led to a declaring file, which is different from a
    /// chain that led to the wrong one -- the first falls through to the name
    /// rules, the second is a firm no.
    fn walk_imports(
        &self,
        name: &str,
        from: &str,
        target_file: &str,
    ) -> Option<Resolved> {
        // Owned rather than borrowed: the chain reassigns `current` on every hop,
        // and a `BTreeSet<&str>` holding views into it would not survive that.
        // Four hops of a short path is not where a clone shows up.
        let mut current = from.to_owned();
        let mut seen: BTreeSet<String> = BTreeSet::new();

        for _ in 0..=MAX_REEXPORT_HOPS {
            if !seen.insert(current.clone()) {
                break;
            }
            if self.declarations().file_declares(&current, name) {
                return Some(if current == target_file {
                    Resolved::Target(Attribution::Import)
                } else {
                    Resolved::Elsewhere
                });
            }
            match self.import_target(&current, name) {
                Some(next) => current = next,
                None => break,
            }
        }
        None
    }

    /// The one local file `from` imports `name` from, when there is exactly one.
    ///
    /// Exactly one matters. Two imports of the same name from different files is
    /// a path the walker cannot choose between, and choosing is the thing this
    /// whole module refuses to do.
    fn import_target(&self, from: &str, name: &str) -> Option<String> {
        let mut found: Option<&str> = None;
        for edge in self.report.edges.iter().filter(|edge| edge.from == from) {
            if !edge.resolved {
                continue;
            }
            let Some(to) = edge.to.as_deref() else {
                continue;
            };
            if tail(&edge.import_path) != name {
                continue;
            }
            match found {
                None => found = Some(to),
                Some(previous) if previous == to => {}
                Some(_) => return None,
            }
        }
        found.map(ToOwned::to_owned)
    }

    /// The crate a file belongs to, for the same-crate rule.
    ///
    /// The nearest ancestor directory holding a `Cargo.toml`, which is what makes
    /// two files "the same crate" for a resolver that is otherwise
    /// language-blind. A language without cargo workspaces falls back to the
    /// file itself, where every file is its own crate -- so the rule degrades to
    /// the project-wide one rather than to nonsense.
    fn crate_of(&self, file: &str) -> String {
        let mut directory = std::path::PathBuf::from(&self.report.base_path);
        directory.push(file);
        directory.pop();
        loop {
            if directory.join("Cargo.toml").is_file() {
                return directory.display().to_string();
            }
            if !directory.pop() {
                return file.to_owned();
            }
        }
    }

    const fn declarations(&self) -> &DeclarationIndex {
        &self.facts.declarations
    }
}

/// The last segment of a module path, which is the name that path binds.
fn tail(path: &str) -> &str {
    path.rsplit("::")
        .next()
        .unwrap_or(path)
        .trim_start_matches("r#")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_segment_of_a_path_is_the_name_it_binds() {
        assert_eq!(tail("crate::graph::Leaked"), "Leaked");
        assert_eq!(tail("Leaked"), "Leaked");
        assert_eq!(tail("r#match"), "match");
        // A trailing `::` yields nothing. It cannot occur: the paths compared here
        // are import statements out of a parse tree, and `use a::b::;` is not a
        // thing Rust accepts. Asserting it anyway, because a path that produced an
        // empty tail would match no edge and the walk would fall through to the
        // name rules -- which is the safe direction, but worth knowing.
        assert_eq!(tail("crate::graph::"), "");
    }
}
