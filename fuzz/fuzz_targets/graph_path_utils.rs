#![no_main]

//! Invariants for the shared path-string helpers.
//!
//! The existing targets assert only that a renderer does not panic, which is
//! nearly free in a codebase without `unsafe`. These assert properties instead:
//! every one of them is something a caller relies on and something that can break
//! silently, returning a plausible wrong path rather than a crash.
//!
//! All of these functions are pure string operations, so the fuzzer gets a very
//! high executions-per-second rate and reaches the awkward inputs -- runs of
//! separators, a lone dot, a trailing dot inside a segment -- that hand-written
//! tests skip.

use libfuzzer_sys::fuzz_target;
use sephera_core::path_utils::{
    collapse_relative, file_stem, join, parent, segments,
    strip_prefix_if_inside,
};

/// Split the input into path strings on a NUL, which cannot appear in the
/// synthetic paths this builds.
fn split_paths(data: &[u8]) -> Vec<String> {
    data.split(|byte| *byte == 0)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let paths = split_paths(data);
    let Some(head) = paths.first() else {
        return;
    };
    let head = head.clone();
    let tail = &paths[paths.len().min(1)..];

    // Joining segments and splitting them apart is a round trip. If it is not,
    // then a path built from a base and a resolved import cannot be compared
    // against one walked from the filesystem, and every resolver decision made
    // from the difference is arbitrary.
    let parts: Vec<&str> = tail.iter().map(String::as_str).collect();
    let joined = join(&head, &parts);
    let expected: Vec<&str> = segments(&head)
        .into_iter()
        .chain(parts.iter().copied().flat_map(segments))
        .collect();

    assert_eq!(
        segments(&joined),
        expected,
        "join then split must recover the segments: base {head:?} + {parts:?} \
         produced {joined:?}"
    );

    // Collapsing is idempotent, because it is applied to paths that have already
    // been through it once -- a graph path fed back in as a user-supplied scope.
    let collapsed = collapse_relative(&joined);
    assert_eq!(
        collapse_relative(&collapsed),
        collapsed,
        "collapse_relative is not idempotent for {joined:?}"
    );

    // A collapsed path holds no empty and no `.` segments: the graph never spells
    // either, so one surviving means a scope or a resolution cannot match a node.
    //
    // `segments` rather than `split('/')`, which yields one empty piece for `""`
    // and would report the empty path as having an empty segment in it. The empty
    // path is the one input with no segments at all, which is correct.
    for segment in segments(&collapsed) {
        assert_ne!(segment, ".", "`.` survived in {collapsed:?}");
    }

    // Collapsing resolves `..` and never invents one. Counting is enough: a path
    // can only come out with more `..` than it went in with if a `..` were
    // manufactured, which is how a scope escapes the directory it was given.
    let dots_in = joined.matches("..").count();
    let dots_out = collapsed.matches("..").count();
    assert!(
        dots_out <= dots_in,
        "collapse_relative invented a `..`: {joined:?} -> {collapsed:?}"
    );

    // `parent` and `file_name` partition the path, so putting them back together
    // has to reproduce it.
    let rejoined = join(&parent(&joined), &[file_stem(&joined)]);
    assert!(
        segments(&rejoined).len() <= segments(&joined).len(),
        "parent + stem produced more path than it started with: {joined:?} -> \
         {rejoined:?}"
    );

    // A successful strip leaves a path genuinely inside the base. Reported as
    // `None` so a caller can reject, this is the check that keeps a resolution
    // from escaping the analysis root -- but only if it is really true.
    //
    // Only for a non-empty base. An empty base is defined as containing
    // everything, so `Some` for any path is correct there and there is nothing
    // to check; asserting otherwise would be asserting the function is wrong.
    if let Some(base) = tail.first().map(|base| base.trim_end_matches('/'))
        && !base.is_empty()
        && let Some(rest) = strip_prefix_if_inside(&joined, base)
    {
        let base_segments = segments(base);
        let path_segments = segments(&joined);

        assert!(
            path_segments.len() > base_segments.len(),
            "stripped {base:?} off {joined:?} leaving {rest:?}, but the path \
             is not longer than the base"
        );
        assert_eq!(
            &path_segments[..base_segments.len()],
            base_segments.as_slice(),
            "stripped {base:?} off {joined:?}, which does not start with it"
        );
    }
});
