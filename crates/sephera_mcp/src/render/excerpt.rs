//! Per-file excerpt rendering for the context pack.

use std::fmt::Write as _;

use sephera_context::{ContextFile, ContextGroupKind};

use super::yes_no;

/// Write a single file's header block and fenced excerpt.
///
/// The fence uses four backticks so that a snippet containing its own triple
/// backticks cannot terminate the block early.
pub(super) fn write_excerpt(
    output: &mut String,
    file: &ContextFile,
    group_kind: ContextGroupKind,
) {
    writeln!(output, "### File: `{}`", file.relative_path)
        .expect("writing to String must succeed");
    writeln!(output, "- Group: {}", group_kind.label())
        .expect("writing to String must succeed");
    writeln!(output, "- Language: {}", file.language.unwrap_or("unknown"))
        .expect("writing to String must succeed");
    writeln!(output, "- Reason: {}", file.selection_class.as_str())
        .expect("writing to String must succeed");
    writeln!(output, "- Size: {} bytes", file.size_bytes)
        .expect("writing to String must succeed");
    writeln!(output, "- Estimated tokens: {}", file.estimated_tokens)
        .expect("writing to String must succeed");
    writeln!(output, "- Truncated: {}", yes_no(file.truncated))
        .expect("writing to String must succeed");
    writeln!(
        output,
        "- Lines: {}-{}",
        file.excerpt.line_start, file.excerpt.line_end
    )
    .expect("writing to String must succeed");
    writeln!(output).expect("writing to String must succeed");

    write_fenced(
        output,
        fence_language(&file.relative_path),
        &file.excerpt.content,
    );
}

/// Write a fenced code block, omitting the language tag when there is none.
fn write_fenced(output: &mut String, language: &str, content: &str) {
    if language.is_empty() {
        writeln!(output, "````").expect("writing to String must succeed");
    } else {
        writeln!(output, "````{language}")
            .expect("writing to String must succeed");
    }
    writeln!(output, "{content}").expect("writing to String must succeed");
    writeln!(output, "````").expect("writing to String must succeed");
}

/// Map a file extension to a Markdown fence language tag.
///
/// Returns an empty string for extensions with no conventional tag, which makes
/// `write_fenced` emit a bare fence.
fn fence_language(relative_path: &str) -> &str {
    std::path::Path::new(relative_path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map_or("", |extension| match extension {
            "rs" => "rust",
            "py" => "python",
            "ts" => "ts",
            "tsx" => "tsx",
            "js" => "js",
            "jsx" => "jsx",
            "go" => "go",
            "java" => "java",
            "c" => "c",
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
            "json" => "json",
            "md" => "markdown",
            "toml" => "toml",
            "yml" | "yaml" => "yaml",
            "sh" => "bash",
            _ => "",
        })
}

#[cfg(test)]
mod tests {
    use super::fence_language;

    #[test]
    fn known_extensions_map_to_fence_tags() {
        assert_eq!(fence_language("src/main.rs"), "rust");
        assert_eq!(fence_language("app/main.py"), "python");
        assert_eq!(fence_language("README.md"), "markdown");
        assert_eq!(fence_language("a/b/c.cpp"), "cpp");
        assert_eq!(fence_language("conf/config.yaml"), "yaml");
        assert_eq!(fence_language("run.sh"), "bash");
    }

    #[test]
    fn unknown_or_missing_extensions_yield_no_tag() {
        assert_eq!(fence_language("LICENSE"), "");
        assert_eq!(fence_language("data.unknown"), "");
        assert_eq!(fence_language("weird.xyz"), "");
    }
}
