//! `sephera guard`: the pre-edit seatbelt for coding agents.
//!
//! Agent harnesses such as Claude Code run a hook before every file edit
//! and hand it a JSON description of the tool call on stdin. This module is
//! the half of that conversation that is pure: finding the file being
//! edited inside the hook's JSON, spelling the edited file the way the
//! graph spells it, and rendering the allow/deny decision. Building the
//! graph and measuring the file live in `run`, with the other commands.
//!
//! The decision protocol is the harness's own: stdout carries a JSON
//! `permissionDecision`, and a blocked edit also fails the command, so a
//! harness that only honours exit codes and one that only reads JSON both
//! stop the edit. A guard that could not work out which file was being
//! edited allows the edit and says why on stderr -- a seatbelt that locked
//! the doors whenever it was confused would be uninstalled within the day.

use std::path::{Path, PathBuf};

/// Find the file a hook payload says is being edited.
///
/// Harnesses spell this a few ways: Claude Code puts an absolute
/// `file_path` inside `tool_input` for its edit and write tools, notebook
/// edits use `notebook_path`, and some tools say only `path`. All three are
/// looked for, in that order, first inside `tool_input` and then at the top
/// level. Anything else -- a shell command payload, malformed JSON, a
/// payload about some other tool -- yields `None`, which the caller treats
/// as "not an edit this guard can judge", never as an empty path.
#[must_use]
pub fn file_path_from_hook_input(input: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;

    let scopes = [value.get("tool_input"), Some(&value)];
    for scope in scopes.into_iter().flatten() {
        for key in ["file_path", "notebook_path", "path"] {
            if let Some(path) =
                scope.get(key).and_then(serde_json::Value::as_str)
                && !path.is_empty()
            {
                return Some(path.to_owned());
            }
        }
    }
    None
}

/// Spell an edited file the way the graph spells paths: relative to the
/// analysis base, with forward slashes.
///
/// Hook payloads carry absolute paths; graph paths are relative. An
/// absolute path is made relative to the canonical analysis base -- the
/// file itself may not exist yet (a write tool creating it), so its parent
/// is canonicalised rather than the file. A path that is not under the
/// base, or already relative, passes through with only its separators
/// normalised, and the caller's node matching decides whether it names a
/// file in the graph at all.
#[must_use]
pub fn graph_spelling(analysis_base: &Path, edited: &str) -> String {
    let edited_path = Path::new(edited);
    if edited_path.is_absolute()
        && let Ok(base) = analysis_base.canonicalize()
    {
        let absolute = canonicalise_lenient(edited_path);
        if let Ok(relative) = absolute.strip_prefix(&base) {
            return relative.to_string_lossy().replace('\\', "/");
        }
    }
    edited.replace('\\', "/")
}

/// Canonicalise a path whose final component may not exist yet.
fn canonicalise_lenient(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(name),
        _ => path.to_path_buf(),
    }
}

/// Render the hook decision for stdout.
///
/// `reason` reaches the agent verbatim when the decision is a denial, so it
/// is written for the agent: what file, how wide, what to do instead.
#[must_use]
pub fn decision_json(blocked: bool, reason: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": if blocked { "deny" } else { "allow" },
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{decision_json, file_path_from_hook_input, graph_spelling};

    #[test]
    fn a_claude_code_edit_payload_yields_its_file() {
        let payload = r#"{
            "tool_name": "Edit",
            "tool_input": {"file_path": "/repo/src/lib.rs", "old_string": "a"}
        }"#;
        assert_eq!(
            file_path_from_hook_input(payload).as_deref(),
            Some("/repo/src/lib.rs")
        );
    }

    #[test]
    fn notebook_and_plain_path_spellings_are_found() {
        assert_eq!(
            file_path_from_hook_input(
                r#"{"tool_input": {"notebook_path": "/repo/n.ipynb"}}"#
            )
            .as_deref(),
            Some("/repo/n.ipynb")
        );
        assert_eq!(
            file_path_from_hook_input(
                r#"{"tool_input": {"path": "src/a.rs"}}"#
            )
            .as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(
            file_path_from_hook_input(r#"{"file_path": "top.rs"}"#).as_deref(),
            Some("top.rs")
        );
    }

    #[test]
    fn payloads_without_an_edited_file_yield_none() {
        // A shell command, an empty input object, and malformed JSON are
        // all "not an edit", and none of them may become an empty path that
        // later code would measure.
        assert_eq!(
            file_path_from_hook_input(r#"{"tool_input": {"command": "ls"}}"#),
            None
        );
        assert_eq!(file_path_from_hook_input("{}"), None);
        assert_eq!(file_path_from_hook_input("not json"), None);
        assert_eq!(file_path_from_hook_input(""), None);
    }

    #[test]
    fn an_empty_file_path_is_not_a_file() {
        assert_eq!(
            file_path_from_hook_input(r#"{"tool_input": {"file_path": ""}}"#),
            None
        );
    }

    #[test]
    fn graph_spelling_makes_an_absolute_path_relative() {
        let base = std::env::temp_dir();
        let edited = base.join("crates/x/src/lib.rs");
        assert_eq!(
            graph_spelling(&base, &edited.to_string_lossy()),
            "crates/x/src/lib.rs"
        );
    }

    #[test]
    fn graph_spelling_passes_relative_paths_through() {
        assert_eq!(graph_spelling(Path::new("."), "src\\lib.rs"), "src/lib.rs");
    }

    #[test]
    fn the_decision_names_the_verdict_in_the_harness_shape() {
        let denied = decision_json(true, "too wide");
        let parsed: serde_json::Value =
            serde_json::from_str(&denied).expect("valid json");
        assert_eq!(parsed["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            parsed["hookSpecificOutput"]["permissionDecisionReason"],
            "too wide"
        );

        let allowed = decision_json(false, "fine");
        let parsed: serde_json::Value =
            serde_json::from_str(&allowed).expect("valid json");
        assert_eq!(parsed["hookSpecificOutput"]["permissionDecision"], "allow");
    }
}
