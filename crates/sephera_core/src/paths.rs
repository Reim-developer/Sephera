//! Path-string helpers that every crate needs and none of them owns.

/// Rewrites a path to use `/` as its separator.
///
/// Two callers, two crates, and one line of work: normalising an
/// ignore pattern's path and normalising a graph edge's path are the same
/// question. It lives here rather than in either caller because making
/// `sephera_ignore` depend on `sephera_graph` -- or the reverse -- to share one
/// function is a dependency cycle waiting for the first feature that needs it.
///
/// Only backslashes are rewritten, and only on Windows. On Unix a backslash is a
/// legal character in a file name, so converting it unconditionally would let a
/// pattern meant for one file silently apply to a whole directory.
#[must_use]
pub fn forward_slashes(value: &str) -> String {
    if cfg!(windows) {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    }
}
