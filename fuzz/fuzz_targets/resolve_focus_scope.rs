#![no_main]

//! Invariants for focus-scope normalisation.
//!
//! Three separate correctness bugs lived in this one function, and all three
//! failed the same way: a scope that meant "the whole repository" was narrowed
//! into something narrower, so the answer came back as a smaller number and
//! nothing said why. A smaller number is the most plausible-looking wrong answer
//! a tool like this can produce.
//!
//! `build_focus_set` is pure and takes paths, so this runs at a high
//! executions-per-second rate against exactly the spellings that broke: `.`,
//! `./x`, `a/b/..`, mixed separators, a scope outside the base.

use std::path::{Path, PathBuf};

use libfuzzer_sys::fuzz_target;
use sephera_core::core::graph::resolver::build_focus_set;

/// Split the input on NUL into at most two scopes.
///
/// Enough to cover the union, which is where the narrowing bug was.
fn split_scopes(data: &[u8]) -> Vec<String> {
    data.split(|byte| *byte == 0)
        .take(2)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
}

/// Rewrite arbitrary bytes into a relative path with no drive prefix and no root.
///
/// `.` and `/` are left in on purpose: they are the cases that broke. Leading
/// separators would root the path and `:` reads as a drive prefix on Windows, so
/// both go.
fn relative_bytes(chunk: &[u8], keep_parents: bool) -> PathBuf {
    let text = String::from_utf8_lossy(chunk);
    let cleaned: String = text
        .chars()
        .filter(|ch| *ch != ':')
        .map(|ch| if ch == '\\' { '/' } else { ch })
        .filter(|ch| keep_parents || *ch != '.')
        .collect();

    PathBuf::from(cleaned.trim_start_matches('/'))
}

/// An analysis base that is a real directory in the current working directory,
/// so absolute scopes under it exercise the strip-the-base branch.
fn analysis_base() -> PathBuf {
    Path::new("fuzz/fixtures/base").to_path_buf()
}

fuzz_target!(|data: &[u8]| {
    let raw = split_scopes(data);
    if raw.is_empty() {
        return;
    }

    // Two input classes, because `build_focus_set` makes two kinds of promise.
    //
    // `parented` keeps `..`, so it can still be a `..` that escapes the front --
    // which the function returns exactly as typed, with no spelling guarantee. Only
    // the invariants that hold for *any* input are asserted against it.
    //
    // `clean` has no `..` at all, so every one of its scopes is normalisable and
    // the output must be canonical, unconditionally. Asserting that on
    // constructed input rather than conditioning it on a guessed precondition is
    // what took six attempts to get to; each earlier attempt was wrong about
    // which inputs get normalised, and every one of those failures was this
    // target's fault rather than the function's.
    let parented: Vec<PathBuf> = raw
        .iter()
        .map(|scope| relative_bytes(scope.as_bytes(), true))
        .collect();
    let clean: Vec<PathBuf> = raw
        .iter()
        .map(|scope| relative_bytes(scope.as_bytes(), false))
        .collect();

    let base = analysis_base();

    let combined = build_focus_set(&base, &parented);

    // A scope naming the whole base means the whole union is the whole base.
    // This is the invariant the narrowing bug violated: `--focus . --focus
    // src/core` returned `{src/core}`, so a `--fail-on` limit stopped firing
    // without the rule changing.
    for scope in &parented {
        if build_focus_set(&base, std::slice::from_ref(scope)).is_empty() {
            assert!(
                combined.is_empty(),
                "scope {scope:?} names the whole base, so the union must too, \
                 but the union came back {combined:?}"
            );
        }
    }

    // Order is not an input. A set built from the same scopes must not depend on
    // which one the user typed first, or the same flags mean two things.
    let mut reversed = parented.clone();
    reversed.reverse();
    assert_eq!(
        combined,
        build_focus_set(&base, &reversed),
        "the same scopes in a different order gave a different set"
    );

    // Feeding the result back in is a fixed point. A scope spelled the way the
    // graph spells it must select exactly what selected it the first time, or
    // `--focus` and `impact --focus` cannot both be right.
    let round_trip: Vec<PathBuf> =
        combined.iter().map(PathBuf::from).collect();
    assert_eq!(
        combined,
        build_focus_set(&base, &round_trip),
        "normalising an already-normalised scope changed it: {combined:?}"
    );

    // Narrowing is only ever allowed to narrow. Adding a scope can remove
    // dependents, but it cannot turn a scope that selected nothing into one that
    // selects something outside the base.
    for scope in &parented {
        let alone = build_focus_set(&base, std::slice::from_ref(scope));
        for kept in &alone {
            assert!(
                combined.contains(kept) || combined.is_empty(),
                "adding more scopes dropped {kept:?}, which {scope:?} selected \
                 on its own"
            );
        }
    }

    // Canonical spelling, on the class of input that promises it. Every scope in
    // `clean` is relative with no root, no drive prefix and no `..`, so every one
    // of them is rewritten and the output must equal what the graph would spell.
    // Unconditional, with no precondition to have got wrong.
    for scope in build_focus_set(&base, &clean) {
        // `split('/')` rather than `segments`: the set holds only non-empty
        // strings, so an empty piece here is a defect rather than the empty-path
        // case.
        for segment in scope.split('/') {
            assert_ne!(segment, "", "empty segment in {scope:?}");
            assert_ne!(segment, ".", "`.` in {scope:?}");
        }
        assert!(
            !scope.ends_with('/'),
            "trailing separator in {scope:?}, which no node path carries"
        );
    }

    // The absolute branch, which `relative_bytes` cannot reach: it strips the base
    // lexically and then makes no further checks, so `..` can survive into a
    // relative-looking scope. Both directions are asserted from fuzz-derived
    // components, because a fixed path here would exercise one branch shape per
    // run and give the impression of coverage it does not have.
    let absolute_base =
        std::path::absolute(analysis_base()).expect("an absolute base");
    let component = clean_component(data);

    // Inside the base: must come back relative, or it is compared as a whole
    // drive-and-directory string against base-relative node paths, matches
    // nothing, and reports a radius of zero with no warning.
    let inside = absolute_base.join(&component);
    if inside.starts_with(&absolute_base) {
        let normalised = build_focus_set(&base, std::slice::from_ref(&inside));

        assert!(
            normalised
                .iter()
                .all(|scope| !Path::new(scope).is_absolute()),
            "{inside:?} is inside the base but came back as {normalised:?}"
        );
    }

    // Outside the base: passed through untouched, so the caller can see a scope
    // was not recognised rather than finding a silently narrower answer.
    let outside = absolute_base
        .parent()
        .unwrap_or(&absolute_base)
        .join("elsewhere")
        .join(&component);

    if !outside.starts_with(&absolute_base) {
        let kept = build_focus_set(&base, &[outside]);

        assert!(
            kept.iter().any(|scope| Path::new(scope).is_absolute()),
            "a scope outside the base came back relative, so it reads like a \
             path the graph could have: {kept:?}"
        );
    }
});

/// A single path component, safe to join onto any directory.
///
/// No separators, no `..`, no drive prefix, so joining it cannot change what
/// directory it lands in -- which is what makes the two branches above
/// distinguishable.
fn clean_component(data: &[u8]) -> String {
    let raw: String = String::from_utf8_lossy(data)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .take(24)
        .collect();

    if raw.is_empty() { "core".to_owned() } else { raw }
}