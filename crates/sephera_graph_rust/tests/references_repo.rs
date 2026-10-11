//! The use extractor, measured against this repository.
//!
//! The unit tests check the extractor's judgments one file at a time, on sources
//! written to make a point. This checks them at the scale the feature has to
//! survive: every `.rs` file in `crates/`, which is a real Rust project rather
//! than a fixture someone wrote to pass.
//!
//! Three things are measured, each against the repository's own source rather
//! than against expectations.
//!
//! **Every recorded use must be real.** A recorded `(file, line, name)` is
//! checked against that line of that file. A name the extractor invented, or
//! recorded against the wrong line, is a coupling the project does not have --
//! that is the precision contract, and it is the half that makes a blast radius
//! usable rather than a wall of maybe.
//!
//! **A name a file binds locally is not attributed to anybody.** The unit test
//! proves the rule on a two-line source. This proves it on the hundreds of `let`
//! bindings this repository actually contains, and it has teeth: remove the
//! pre-pass and every one of those files starts recording uses of its own local
//! variables.
//!
//! **The couplings this project has must be found.** The ground truth here is the
//! source: a file that mentions a declared name is a candidate use, and the
//! fraction the extractor found is the recall proxy for the ≥ 70% contract.
//! It is a proxy and is called one -- a mention inside a comment or a doc string
//! is not a use, and the index correctly records none -- so the figure reads
//! low against a real tool. The threshold is set where it measures the extractor,
//! not the tokenizer.
//!
//! What is *not* measured here is the resolution half, which does not exist yet.
//! An index that finds a use is not a claim that any particular file breaks; that
//! claim belongs to the resolver, and it gets its own test with its own ground
//! truth.
//!
//! No test here asserts an absolute count of uses. A count falls on every
//! refactor, and a test that fails for that reason is a test that trains you to
//! ignore it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sephera_core::references::ReferenceIndex;
use sephera_graph_rust::extract_uses;

/// The repository root, from the crate this test is compiled into.
fn repo_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `<repo>/crates/sephera_graph_rust`, and the root is
    // two up. A test's working directory is the crate, so a relative path from it
    // found nothing at all -- which is how the first run of this file reported
    // zero files in a repository of several hundred.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits inside the repository")
        .to_path_buf()
}

/// Every `.rs` under `crates/`, as paths relative to the repository root.
fn rust_files() -> Vec<String> {
    let root = repo_root();
    let mut found = Vec::new();
    let mut queue = vec![root.join("crates")];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string()
                    .replace('\\', "/");
                found.push(relative);
            }
        }
    }
    found.sort();
    found
}

/// The index for every Rust file in this repository.
fn repository_index() -> (ReferenceIndex, usize) {
    let root = repo_root();
    let mut index = ReferenceIndex::default();
    let mut files = 0;
    for relative in rust_files() {
        let Ok(source) = std::fs::read(root.join(&relative)) else {
            continue;
        };
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("the Rust grammar loads");
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        index.record(&relative, &extract_uses(&source, &tree));
        files += 1;
    }
    (index, files)
}

/// The lines of an indexed file, read back through the repository root.
///
/// The index is keyed by repository-relative paths because that is what a report
/// should print, and every read of one has to go back through the root to find
/// the bytes.
fn source_lines(file: &str) -> Vec<String> {
    let path = repo_root().join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!("{file} was indexed but cannot be read: {error}")
        })
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

/// The lines the index records `name` on, within `file`.
fn uses_in(index: &ReferenceIndex, file: &str, name: &str) -> Vec<usize> {
    index
        .users_of(name)
        .filter(|(user, _)| *user == file)
        .flat_map(|(_, lines)| lines.iter().copied())
        .collect()
}

#[test]
fn every_recorded_use_is_a_use() {
    let (index, files) = repository_index();
    assert!(files > 100, "expected a real project, found {files} files");

    let mut checked = 0usize;
    for name in index.names() {
        for (file, lines) in index.users_of(name) {
            let lines_of = source_lines(file);
            for line in lines {
                let Some(text) = lines_of.get(line.saturating_sub(1)) else {
                    panic!(
                        "{file} records {name} on line {line}, which does not exist"
                    );
                };
                assert!(
                    text.contains(name),
                    "{file} records {name} on line {line}, but that line is {text:?} \
                     -- a coupling the project does not have"
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 1000,
        "expected thousands of uses, found {checked}"
    );
}

/// The name a `let` binds on this line, if it binds exactly one.
///
/// Deliberately narrow: only the shape whose binding is unambiguous from the
/// text. `let (a, b) = ...` binds two names and a test built on reading it as
/// one would be testing the reader, not the extractor.
fn let_binding(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("let ")?;
    let name = rest
        .trim_start()
        .strip_prefix("mut ")
        .unwrap_or(rest)
        .trim_start();
    let end = name
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(name.len());
    let name = &name[..end];
    if name.is_empty()
        || name == "_"
        || name.starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    Some(name)
}

#[test]
fn a_name_a_file_binds_locally_is_not_attributed_to_anybody() {
    let (index, _) = repository_index();
    let mut bindings = 0usize;

    for file in rust_files() {
        let lines = source_lines(&file);
        for (at, line) in lines.iter().enumerate() {
            let Some(name) = let_binding(line) else {
                continue;
            };
            // Only interesting when the name is mentioned again below the
            // binding -- otherwise there is nothing for the rule to have caught.
            if !lines[at + 1..]
                .iter()
                .any(|later| later.split_whitespace().any(|word| word == name))
            {
                continue;
            }
            bindings += 1;
            assert!(
                uses_in(&index, &file, name).is_empty(),
                "{file} binds `{name}` on line {} and then mentions it, so the \
                 local-binding rule should have dropped every use of it",
                at + 1
            );
        }
    }

    assert!(
        bindings > 200,
        "a project this size has hundreds of local bindings; found {bindings}"
    );
}

/// Every identifier each file mentions, with comments left out.
///
/// This is the recall denominator, and it is built from the parse tree rather
/// than from the text for one reason: a mention in a comment is not a use, and
/// counting those as misses measures the corpus's documentation rather than the
/// extractor. Walking the tree also drops string literals and macro bodies that
/// happen to contain a word, which are equally not uses.
///
/// No rule of the extractor's own is applied here. This is the raw set of
/// mentions, so the difference between it and what the index recorded is
/// recall, including the drops the extractor makes deliberately.
fn mentions_by_file() -> Vec<(String, BTreeSet<String>)> {
    let root = repo_root();
    let mut found = Vec::new();
    for file in rust_files() {
        let Ok(source) = std::fs::read(root.join(&file)) else {
            continue;
        };
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("the Rust grammar loads");
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let mut names = BTreeSet::new();
        collect_identifiers(tree.root_node(), &source, &mut names);
        found.push((file, names));
    }
    found
}

/// The names under `node`, skipping comments.
fn collect_identifiers(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    into: &mut BTreeSet<String>,
) {
    if matches!(node.kind(), "line_comment" | "block_comment") {
        return;
    }
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        let text = node.utf8_text(source).unwrap_or_default();
        if !text.is_empty() && !matches!(text, "self" | "super" | "crate") {
            into.insert(text.trim_start_matches("r#").to_owned());
        }
    }
    for child in node.children(&mut node.walk()) {
        collect_identifiers(child, source, into);
    }
}

/// Names the repository declares as public API, and where.
///
/// A name used in exactly one file is not evidence of anything: it is as likely
/// to be a private helper the extractor handled correctly as a name it invented.
/// So the recall measurement is taken over names the project publishes, where a
/// missing use means a missing coupling someone would feel.
fn declared_public_names() -> BTreeMap<String, BTreeSet<String>> {
    let mut declared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in rust_files() {
        for line in source_lines(&file) {
            let trimmed = line.trim_start();
            let Some(after_pub) = trimmed.strip_prefix("pub") else {
                continue;
            };
            let after_pub = after_pub.trim_start();
            // `pub(crate)` and `pub(super)` are still public to the crate.
            let body = match after_pub.strip_prefix('(') {
                Some(rest) => match rest.find(')') {
                    Some(at) => rest[at + 1..].trim_start(),
                    None => continue,
                },
                None => after_pub,
            };
            let Some(body) = body.strip_prefix("struct ").or_else(|| {
                body.strip_prefix("enum ")
                    .or_else(|| body.strip_prefix("trait "))
                    .or_else(|| body.strip_prefix("type "))
                    .or_else(|| body.strip_prefix("fn "))
                    .or_else(|| body.strip_prefix("const "))
                    .or_else(|| body.strip_prefix("static "))
            }) else {
                continue;
            };
            let end = body
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(body.len());
            if end == 0 {
                continue;
            }
            declared
                .entry(body[..end].to_owned())
                .or_default()
                .insert(file.clone());
        }
    }
    declared
}

#[test]
fn the_recall_proxy_clears_the_contract() {
    let (index, _) = repository_index();
    let declared = declared_public_names();
    let mentions = mentions_by_file();

    let mut mentionable = 0usize;
    let mut found = 0usize;
    let mut missed: Vec<String> = Vec::new();
    for (name, declaring_files) in &declared {
        for (file, said) in &mentions {
            // A file cannot be coupled to itself. The declaring file is where the
            // extractor deliberately drops every mention, and counting those as
            // misses would measure that decision rather than the extraction.
            if declaring_files.contains(file) {
                continue;
            }
            if !said.contains(name) {
                continue;
            }
            mentionable += 1;
            if uses_in(&index, file, name).is_empty() {
                if missed.len() < 25 {
                    missed.push(format!("{name} in {file}"));
                }
            } else {
                found += 1;
            }
        }
    }

    let ratio = f64::from(u32::try_from(found).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(mentionable).unwrap_or(1));
    eprintln!(
        "recall proxy: {found}/{mentionable} = {:.1}%",
        ratio * 100.0
    );
    if !missed.is_empty() {
        eprintln!("first misses: {missed:?}");
    }

    assert!(mentionable > 500, "not enough ground truth: {mentionable}");
    // The contract is recall ≥ 70%. The denominator here is every identifier
    // mention outside a comment, which includes the ones the extractor drops on
    // purpose -- a file's own declaration name, and any name it binds locally
    // anywhere. Those drops are the price of the precision contract and they are
    // inside this number on purpose, because a recall figure that excluded them
    // would be reporting the extractor against a rule it was never given.
    assert!(
        ratio >= 0.70,
        "recall proxy {found}/{mentionable} = {:.1}%, under the 70% contract",
        ratio * 100.0
    );
}

#[test]
fn the_couplings_this_project_actually_has_are_found() {
    let (index, _) = repository_index();

    // `BlastRadius` is declared in `sephera_graph` and rendered by the CLI --
    // crossing a crate boundary, which is the case the feature exists for.
    assert!(
        !uses_in(&index, "crates/sephera_cli/src/impact.rs", "BlastRadius")
            .is_empty(),
        "the CLI renders BlastRadius and the index found no use of it"
    );

    // `SymbolKind` crosses out of `sephera_symbols` into the CLI's renderer.
    assert!(
        !uses_in(
            &index,
            "crates/sephera_cli/src/output/symbols.rs",
            "SymbolKind"
        )
        .is_empty(),
        "the CLI renders SymbolKind"
    );

    // The shared vocabulary, used from outside the crate that declares it. Not
    // "many files": `sephera_symbols/src/lib.rs` re-exports it, and a `pub use`
    // is not a use, so the count is one lower than a grep would suggest.
    let outside: Vec<_> = index
        .users_of("SymbolEntry")
        .map(|(file, _)| file)
        .filter(|file| !file.starts_with("crates/sephera_symbols/"))
        .collect();
    assert!(
        !outside.is_empty(),
        "SymbolEntry is declared in sephera_symbols and used nowhere else, so \
         nothing crosses the boundary with it"
    );
}

#[test]
fn an_alias_records_the_path_the_name_came_in_by() {
    // `use foo::Bar as Baz;` means a later `Baz` is only resolvable through the
    // path. Recorded here so the resolver does not have to re-derive it, and
    // checked through the index because that is how the resolver will read it.
    let source = b"use sephera_graph::Leaked as Internals;\nfn f() -> Internals { todo!() }\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("the Rust grammar loads");
    let tree = parser.parse(source, None).expect("the source parses");
    let found = extract_uses(source, &tree);

    let internals = found
        .iter()
        .find(|u| u.name == "Internals")
        .expect("the alias is used");
    assert_eq!(internals.alias_of.as_deref(), Some("sephera_graph::Leaked"));

    let mut index = ReferenceIndex::default();
    index.record("a.rs", &found);
    // Twice: once at the import, which is the coupling the compiler enforces, and
    // once at the mention. Two lines in one file is one file, which is what a
    // blast radius counts.
    assert_eq!(uses_in(&index, "a.rs", "Internals"), vec![1, 2]);
}
