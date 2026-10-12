//! The `references` command: presentation only.
//!
//! The analysis is `sephera_graph::references::find`, the same module `impact`
//! renders rather than reimplements -- a command that computed its own answer
//! would be a second implementation that could disagree with the first.
//!
//! What the output carries beyond the file list, and why each is load-bearing:
//!
//! * **How each file was attributed.** `import` is evidence: the file's own
//!   import path led here, through re-exports if it had to. `unique` and
//!   `same-crate` are arithmetic. Someone deciding whether to trust a radius needs
//!   to know which they are reading, so the ordering is a ranking.
//! * **What could not be placed.** Under `--show-unattributed`, each mention the
//!   resolver refused to assign, with how many files declare the name. Refusing to
//!   guess is the precision contract; showing what was refused is what makes a
//!   refused answer usable rather than merely absent.
//! * **Nothing** about mentions no file declares. `drop` and every
//!   standard-library name would bury the few that matter.

use std::fmt::Write as _;
use std::io::Write;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::args::ReferencesFormat;
use sephera_graph::SymbolFacts;
use sephera_graph::references::{Attribution, SymbolTarget, find};
use sephera_graph::types::GraphReport;

/// One symbol's answer, after scoping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Answer {
    /// The declaration asked about.
    pub target: SymbolTarget,
    /// Files that reference it, strongest attribution first.
    pub users: Vec<Entry>,
    /// Files whose mention could not be tied to it. Empty unless asked for.
    pub unattributed: Vec<Unplaced>,
}

/// One referencing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The referencing file.
    pub file: String,
    /// Lines it references the symbol on.
    pub lines: Vec<usize>,
    /// Why it was attributed here: `import`, `unique` or `same_crate`.
    pub via: &'static str,
}

/// One mention that could not be placed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unplaced {
    /// The mentioning file.
    pub file: String,
    /// Lines it mentions the name on.
    pub lines: Vec<usize>,
    /// How many files declare a name that could be meant.
    pub candidates: usize,
}

/// Answer every symbol in `symbols`, in the order given.
///
/// A symbol that declares nothing is an error rather than an empty answer: an
/// empty answer and a typo are indistinguishable once they are on a screen, and a
/// reader who mistyped a path deserves to be told rather than shown nothing.
///
/// # Errors
///
/// Errors when a symbol is not spelled `PATH::NAME`, or when the file does not
/// declare the name.
pub fn measure_all(
    report: &GraphReport,
    facts: &SymbolFacts,
    symbols: &[String],
    focus: &[String],
    show_unattributed: bool,
) -> Result<Vec<Answer>> {
    symbols
        .iter()
        .map(|spelling| {
            let target = parse_symbol(spelling)?;
            let found = find(report, facts, &target).map_err(|_| {
                // The same two failures the MCP tool distinguishes, for the same
                // reason: an agent that reads "does not declare `init`" about a
                // JavaScript file concludes the symbol does not exist.
                if is_rust(&target.file) {
                    anyhow::anyhow!("`{}` does not declare `{}`", target.file, target.name)
                } else {
                    anyhow::anyhow!(
                        "`{}` is not a Rust file, so its names cannot be resolved yet: \
                         `references` covers Rust only. Use `impact` on the file for a \
                         file-level radius, which works across all six languages.",
                        target.file
                    )
                }
            })?;

            // Scoping narrows the answer, not the analysis: a symbol outside the
            // focus is still answered, coming back with whatever of its radius is
            // inside. `graph --focus` says the same thing about the same flag.
            let inside = |file: &str| {
                focus.is_empty() || focus.iter().any(|f| inside_scope(f, file))
            };

            Ok(Answer {
                target,
                users: found
                    .users
                    .into_iter()
                    .filter(|user| inside(&user.file))
                    .map(|user| Entry {
                        file: user.file,
                        lines: user.lines,
                        via: attribution_name(user.via),
                    })
                    .collect(),
                unattributed: if show_unattributed {
                    found
                        .unattributed
                        .into_iter()
                        .filter(|entry| inside(&entry.file))
                        .map(|entry| Unplaced {
                            file: entry.file,
                            lines: entry.lines,
                            candidates: entry.candidates,
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            })
        })
        .collect()
}

/// Whether a path is a Rust file, on any platform.
///
/// Case-insensitively, because a Windows path may be written `FOO.RS` and a file
/// named that way is a Rust file. The extension check is the whole of what the
/// caller can know -- the alternative is to look at the declaration table, which
/// would make a JavaScript repository look like a tool bug rather than like a
/// language this tool does not cover yet.
fn is_rust(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

/// Split `PATH::NAME` into its halves.
///
/// Cut on the *last* `::`, not the first: a name cannot contain a separator and a
/// path can, so the last one is the only place the split is unambiguous.
fn parse_symbol(spelling: &str) -> Result<SymbolTarget> {
    const SHAPE: &str = "write it PATH::NAME, as in src/lib.rs::Resolver";

    let Some((file, name)) = spelling.rsplit_once("::") else {
        bail!("`{spelling}` is not a symbol; {SHAPE}");
    };
    let (file, name) = (file.trim(), name.trim());
    if file.is_empty() || name.is_empty() {
        bail!("`{spelling}` is not a symbol; {SHAPE}");
    }
    Ok(SymbolTarget {
        file: file.replace('\\', "/"),
        name: name.to_owned(),
    })
}

/// Whether `file` is inside `scope`, matching a path exactly or as a prefix at a
/// `/` boundary. The same rule `graph --focus` uses, so the two cannot disagree.
fn inside_scope(scope: &str, file: &str) -> bool {
    let scope = scope.trim_end_matches('/');
    file == scope || file.starts_with(&format!("{scope}/"))
}

const fn attribution_name(via: Attribution) -> &'static str {
    match via {
        Attribution::Import => "import",
        Attribution::Unique => "unique",
        Attribution::SameCrate => "same_crate",
    }
}

/// Write the answers to `out`.
///
/// # Errors
///
/// Propagates a write failure. Nothing here computes an answer, so there is no
/// second way for this to fail, and swallowing it would lose the report
/// half-written with no indication of which part landed.
pub fn render(
    answers: &[Answer],
    format: ReferencesFormat,
    out: &mut impl Write,
) -> Result<()> {
    let body = match format {
        ReferencesFormat::Text => render_text(answers),
        ReferencesFormat::Markdown => render_markdown(answers),
        ReferencesFormat::Json => serde_json::to_string_pretty(answers)
            .context("rendering references as JSON")?,
        ReferencesFormat::Xml => render_xml(answers),
    };
    writeln!(out, "{body}").context("writing the references report")
}

fn render_text(answers: &[Answer]) -> String {
    let mut out = String::new();
    for answer in answers {
        let _ = writeln!(out, "{}", describe(&answer.target));
        if answer.users.is_empty() && answer.unattributed.is_empty() {
            out.push_str("  no references\n\n");
            continue;
        }
        for user in &answer.users {
            let _ = writeln!(
                out,
                "  {:<10} {}:{}",
                user.via,
                user.file,
                lines(&user.lines)
            );
        }
        for unplaced in &answer.unattributed {
            let _ = writeln!(
                out,
                "  {:<10} {}:{}  ({} files declare the name)",
                "unplaced",
                unplaced.file,
                lines(&unplaced.lines),
                unplaced.candidates
            );
        }
        out.push('\n');
    }
    out.trim_end().to_owned()
}

fn render_markdown(answers: &[Answer]) -> String {
    let mut out = String::new();
    for answer in answers {
        let _ = writeln!(out, "## `{}`", describe(&answer.target));
        if answer.users.is_empty() {
            out.push_str("\n_No references._\n");
            continue;
        }
        out.push_str("\n| attribution | file | lines |\n|---|---|---|\n");
        for user in &answer.users {
            let _ = writeln!(
                out,
                "| {} | `{}` | {} |",
                user.via,
                user.file,
                lines(&user.lines)
            );
        }
        out.push('\n');
    }
    out.trim_end().to_owned()
}

/// XML written by hand, like the other commands' writers: borrowing one would mean
/// claiming a schema this command does not have.
fn render_xml(answers: &[Answer]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<references>",
    );
    for answer in answers {
        let _ = write!(
            out,
            "\n  <symbol file=\"{}\" name=\"{}\">",
            answer.target.file, answer.target.name
        );
        for user in &answer.users {
            let _ = write!(
                out,
                "\n    <user via=\"{}\" file=\"{}\" lines=\"{}\" />",
                user.via,
                user.file,
                lines(&user.lines)
            );
        }
        for unplaced in &answer.unattributed {
            let _ = write!(
                out,
                "\n    <unplaced file=\"{}\" lines=\"{}\" candidates=\"{}\" />",
                unplaced.file,
                lines(&unplaced.lines),
                unplaced.candidates
            );
        }
        out.push_str("\n  </symbol>");
    }
    out.push_str("\n</references>");
    out
}

/// A symbol as the reader spelled it.
fn describe(target: &SymbolTarget) -> String {
    format!("{}::{}", target.file, target.name)
}

/// A line list as `4, 11, 19`.
fn lines(lines: &[usize]) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symbol_splits_on_the_last_separator() {
        let target = parse_symbol("src/lib.rs::Resolver").expect("a symbol");
        assert_eq!(target.file, "src/lib.rs");
        assert_eq!(target.name, "Resolver");
    }

    #[test]
    fn a_bare_name_is_not_a_symbol() {
        // The reason the command takes a path: `Cli` alone is two questions, and
        // this repository has two `Cli` structs in two crates.
        let error =
            parse_symbol("Cli").expect_err("a bare name is not a symbol");
        assert!(
            error.to_string().contains("PATH::NAME"),
            "the error should say what to write: {error}"
        );
    }

    #[test]
    fn an_empty_half_is_not_a_symbol() {
        assert!(parse_symbol("::Resolver").is_err());
        assert!(parse_symbol("src/lib.rs::").is_err());
    }

    #[test]
    fn a_windows_path_is_normalised_to_the_graph_spelling() {
        let target = parse_symbol("src\\lib.rs::Resolver").expect("a symbol");
        assert_eq!(target.file, "src/lib.rs");
    }

    #[test]
    fn focus_matches_a_path_or_a_directory_prefix() {
        assert!(inside_scope("crates/x", "crates/x/a.rs"));
        assert!(inside_scope("crates/x", "crates/x"));
        // Not a prefix of a sibling whose name merely starts the same.
        assert!(!inside_scope("crates/x", "crates/xy/a.rs"));
    }

    #[test]
    fn a_file_that_references_nothing_says_so_rather_than_printing_an_empty_list()
     {
        let mut out = Vec::new();
        render(
            &[Answer {
                target: SymbolTarget {
                    file: "a.rs".into(),
                    name: "Foo".into(),
                },
                users: Vec::new(),
                unattributed: Vec::new(),
            }],
            ReferencesFormat::Text,
            &mut out,
        )
        .expect("rendering succeeds");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.contains("a.rs::Foo"), "{text}");
        assert!(text.contains("no references"), "{text}");
    }
}
