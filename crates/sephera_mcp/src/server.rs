//! MCP server handler and tool implementations.

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::io::stdio,
};

use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    graph::{resolver::build_graph, types::GraphQuery},
    runtime::{
        ContextCommandInput, ResolvedContextCommand, SourceRequest,
        build_context_report, resolve_context_command, resolve_source,
    },
};

/// The MCP server handler for Sephera.
///
/// Holds a tool router that maps incoming MCP `tools/call` requests to the
/// correct handler method.  All operations are stateless and performed
/// on-demand using request parameters.
#[derive(Clone)]
pub struct SepheraServer {
    tool_router: ToolRouter<Self>,
}

impl SepheraServer {
    /// Create a new server instance with its tool router initialized.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for SepheraServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool implementations exposed through the Model Context Protocol.
#[tool_router]
impl SepheraServer {
    /// Counts lines of code, comment lines, and empty lines per recognized language for a given source.
    ///
    /// Given exactly one of a filesystem `path` or a repository `url` (optionally with `ref`) and an optional list of ignore patterns, analyzes the source tree and produces a human-readable summary listing per-language metrics and aggregate totals.
    ///
    /// Returns: A formatted summary string with a header (`Files scanned`, `Languages detected`), one line per language (`"{language}: {code} code, {comment} comment, {empty} empty ({bytes} bytes)"`), and a final `Total: ...` line with aggregated metrics.
    ///
    /// # Examples
    ///
    /// ```
    /// // Construct parameters for a local directory
    /// let params = LocInput {
    ///     path: Some("src".into()),
    ///     url: None,
    ///     git_ref: None,
    ///     ignore: None,
    /// };
    /// let server = SepheraServer::new();
    /// let summary = server.loc(rmcp::handler::server::wrapper::Parameters(params)).unwrap();
    /// assert!(summary.contains("Files scanned:"));
    /// ```
    #[tool(
    name = "loc",
    description = "Count lines of code, comment lines, and empty lines for supported languages in a directory tree. Accepts exactly one of path or url, plus an optional ref for repo URLs. Returns per-language metrics and aggregate totals."
    )]
    fn loc(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<LocInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher = build_ignore_matcher(param.ignore)?;
        let source = resolve_source(&SourceRequest {
            path: param.path.map(std::path::PathBuf::from),
            url: param.url,
            git_ref: param.git_ref,
        })
        .map_err(map_internal_error("source resolution failed"))?;

        let report = CodeLoc::new(&source.analysis_path, ignore_matcher)
            .analyze()
            .map_err(map_internal_error("analysis failed"))?;

        let mut output = String::new();
        output.push_str(&format!(
            "Files scanned: {}\nLanguages detected: {}\n\n",
            report.files_scanned, report.languages_detected
        ));

        for lang in &report.by_language {
            output.push_str(&format!(
                "{}: {} code, {} comment, {} empty ({} bytes)\n",
                lang.language,
                lang.metrics.code_lines,
                lang.metrics.comment_lines,
                lang.metrics.empty_lines,
                lang.metrics.size_bytes,
            ));
        }

        output.push_str(&format!(
            "\nTotal: {} code, {} comment, {} empty ({} bytes)\n",
            report.totals.code_lines,
            report.totals.comment_lines,
            report.totals.empty_lines,
            report.totals.size_bytes,
        ));

        Ok(output)
    }

    /// Builds an LLM-ready context pack for a repository or focused sub-paths.
    ///
    /// Accepts exactly one of `path` or `url`. Supports optional config loading, profile
    /// selection or listing, base-ref diffs, focus paths, ignore patterns, budget and
    /// compression modes. When `list_profiles=true` returns profile metadata as JSON;
    /// when `format=markdown` returns a rendered Markdown context pack; otherwise returns
    /// a pretty-printed JSON representation of the generated context report.
    ///
    /// # Examples
    ///
    /// ```
    /// use rmcp::handler::server::wrapper::Parameters;
    ///
    /// // Request a context pack for a local path and get pretty JSON (default).
    /// let params = Parameters(crate::inputs::ContextInput {
    ///     path: Some(".".to_string()),
    ///     url: None,
    ///     git_ref: None,
    ///     config: None,
    ///     no_config: None,
    ///     profile: None,
    ///     list_profiles: None,
    ///     focus: None,
    ///     ignore: None,
    ///     diff: None,
    ///     budget: None,
    ///     compress: None,
    ///     format: None,
    /// });
    /// // `server` is an instance of `SepheraServer`.
    /// let result = server.context(params);
    /// assert!(result.is_ok());
    /// ```
    #[tool(
    name = "context",
    description = "Build an LLM-ready context pack for a repository or focused sub-paths. Accepts exactly one of path or url, supports config loading, profiles, base-ref diffs, focus paths, and compression modes. Returns pretty JSON by default, Markdown when format=markdown, or profile JSON when list_profiles=true."
    )]
    fn context(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ContextInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let resolved = resolve_context_command(ContextCommandInput {
            source: SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            config: param.config.map(std::path::PathBuf::from),
            no_config: param.no_config.unwrap_or(false),
            profile: param.profile,
            list_profiles: param.list_profiles.unwrap_or(false),
            ignore: param.ignore.unwrap_or_default(),
            focus: param
                .focus
                .unwrap_or_default()
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect(),
            diff: param.diff,
            budget: param.budget,
            compress: param.compress,
            format: param.format,
            output: None,
        })
        .map_err(map_internal_error("context resolution failed"))?;

        match resolved {
            ResolvedContextCommand::ListProfiles(profiles) => {
                serialize_json(&profiles)
            }
            ResolvedContextCommand::Execute(options) => {
                let report = build_context_report(&options)
                    .map_err(map_internal_error("context build failed"))?;
                match options.format.as_str() {
                    "markdown" => Ok(render_context_markdown(&report)),
                    "json" => serialize_json(&report),
                    other => Err(rmcp::ErrorData::internal_error(
                        format!("resolved unexpected context format `{other}`"),
                        None,
                    )),
                }
            }
        }
    }

    /// Builds a dependency graph report for a repository or specified sub-paths.
    ///
    /// Accepts exactly one of `path` or `url` (with an optional `ref` for repository URLs), optional `focus` paths,
    /// `ignore` patterns, a traversal `depth`, and an optional reverse-dependency `depends_on` query. The resulting
    /// report is returned as pretty-printed JSON.
    ///
    /// # Returns
    ///
    /// A pretty-printed JSON string containing the graph report, including nodes, edges, depth metadata, and the
    /// optional `base_path` when a display path is available.
    ///
    /// # Examples
    ///
    /// ```
    /// use rmcp::handler::server::wrapper::Parameters;
    ///
    /// let server = SepheraServer::new();
    /// let input = GraphInput {
    ///     path: Some(".".to_string()),
    ///     url: None,
    ///     git_ref: None,
    ///     focus: None,
    ///     ignore: None,
    ///     depth: None,
    ///     depends_on: None,
    /// };
    /// let params = Parameters(input);
    /// let json = server.graph(params).expect("graph generation failed");
    /// assert!(json.trim_start().starts_with('{'));
    /// ```
    #[tool(
    name = "graph",
    description = "Build a dependency graph for a repository or focused sub-paths. Accepts exactly one of path or url, plus an optional ref for repo URLs. Supports traversal depth and reverse dependency queries through depends_on. Returns structured JSON."
    )]
    fn graph(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<GraphInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher = build_ignore_matcher(param.ignore)?;
        let source = resolve_source(&SourceRequest {
            path: param.path.map(std::path::PathBuf::from),
            url: param.url,
            git_ref: param.git_ref,
        })
        .map_err(map_internal_error("source resolution failed"))?;
        let focus_paths: Vec<std::path::PathBuf> = param
            .focus
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        let query = param.depends_on.map(GraphQuery::DependsOn);

        let mut report = build_graph(
            &source.analysis_path,
            &ignore_matcher,
            &focus_paths,
            param.depth,
            query,
        )
        .map_err(map_internal_error("graph build failed"))?;
        if let Some(display_path) = source.display_path {
            report.base_path = display_path.into();
        }

        serialize_json(&report)
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LocInput {
    /// Absolute or relative path to the directory to analyze. Mutually exclusive with `url`.
    path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    /// Optional list of ignore patterns (globs or regexes)
    ignore: Option<Vec<String>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ContextInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    /// Optional explicit config path on the local machine
    config: Option<String>,
    /// Disable config loading for this invocation
    no_config: Option<bool>,
    /// Optional named profile from `.sephera.toml`
    profile: Option<String>,
    /// List available profiles and return JSON instead of a context pack
    list_profiles: Option<bool>,
    /// Optional list of focus paths (relative to the analysis path)
    focus: Option<Vec<String>>,
    /// Optional list of ignore patterns (globs or regexes)
    ignore: Option<Vec<String>>,
    /// Optional diff source or base ref. URL mode only supports base refs such as `main` or `HEAD~1`.
    diff: Option<String>,
    /// Approximate token budget (default: 128000)
    budget: Option<u64>,
    /// Compression mode: 'none', 'signatures', or 'skeleton' (default: 'none')
    compress: Option<String>,
    /// Output format: 'markdown' or 'json' (default: 'json')
    format: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GraphInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    /// Optional list of focus paths (relative to the analysis path)
    focus: Option<Vec<String>>,
    /// Optional list of ignore patterns (globs or regexes)
    ignore: Option<Vec<String>>,
    /// Optional traversal depth (0 = roots and direct neighbors)
    depth: Option<u32>,
    /// Optional reverse dependency target path
    depends_on: Option<String>,
}

/// Builds an `IgnoreMatcher` from an optional list of ignore patterns.
///
/// If `ignore_patterns` is `None`, an empty pattern list is used. Pattern parsing
/// errors are converted into an `rmcp::ErrorData::internal_error` with a
/// prefixed message.
///
/// # Parameters
///
/// - `ignore_patterns`: Optional list of ignore patterns (glob-like strings). `None` is treated as an empty list.
///
/// # Returns
///
/// `Ok(IgnoreMatcher)` constructed from the provided patterns, `Err(rmcp::ErrorData)` if pattern parsing failed.
///
/// # Examples
///
/// ```
/// # use rmcp::ErrorData;
/// # // pretend IgnoreMatcher is in scope for the example
/// let matcher = build_ignore_matcher(Some(vec!["target/*".into(), "*.lock".into()]));
/// assert!(matcher.is_ok());
/// ```
fn build_ignore_matcher(
    ignore_patterns: Option<Vec<String>>,
) -> Result<IgnoreMatcher, rmcp::ErrorData> {
    IgnoreMatcher::from_patterns(&ignore_patterns.unwrap_or_default()).map_err(
        |error| {
            rmcp::ErrorData::internal_error(
                format!("invalid ignore pattern: {error}"),
                None,
            )
        },
    )
}

/// Creates a closure that converts an `anyhow::Error` into an `rmcp::ErrorData::internal_error`
/// whose message is prefixed with the given static `prefix`.
///
/// The returned closure formats the error as `"{prefix}: {error}"` and places it in the
/// `internal_error` variant.
///
/// # Examples
///
/// ```no_run
/// let mapper = map_internal_error("loc failed");
/// let err = anyhow::anyhow!("unable to read file");
/// let _error_data = mapper(err);
/// ```
fn map_internal_error(
    prefix: &'static str,
) -> impl Fn(anyhow::Error) -> rmcp::ErrorData {
    move |error| {
        rmcp::ErrorData::internal_error(format!("{prefix}: {error}"), None)
    }
}

/// Serializes a value to pretty-printed JSON or returns an MCP internal error.
///
/// # Returns
/// A `String` containing pretty-formatted JSON on success; an `rmcp::ErrorData::internal_error` describing the serialization failure otherwise.
///
/// # Examples
///
/// ```
/// #[derive(serde::Serialize)]
/// struct S { a: i32 }
/// let s = S { a: 1 };
/// let json = crate::serialize_json(&s).unwrap();
/// assert!(json.contains("\"a\": 1"));
/// ```
fn serialize_json<T: serde::Serialize>(
    value: &T,
) -> Result<String, rmcp::ErrorData> {
    serde_json::to_string_pretty(value).map_err(|error| {
        rmcp::ErrorData::internal_error(
            format!("JSON serialization failed: {error}"),
            None,
        )
    })
}

/// Renders a ContextReport as a Markdown "Sephera Context Pack".
///
/// Produces a complete Markdown document containing metadata, dominant languages,
/// group summaries, and per-file excerpts derived from `report`.
///
/// # Parameters
///
/// - `report`: The context report to render.
///
/// # Returns
///
/// A `String` containing the generated Markdown document.
///
/// # Examples
///
/// ```
/// let report = sephera_core::core::context::ContextReport::default();
/// let md = render_context_markdown(&report);
/// assert!(md.starts_with("# Sephera Context Pack"));
/// ```
fn render_context_markdown(
    report: &sephera_core::core::context::ContextReport,
) -> String {
    use std::fmt::Write as _;

    use sephera_core::core::context::{
        ContextFile, ContextGroupKind, ContextGroupSummary, ContextMetadata,
    };

    fn write_metadata(output: &mut String, metadata: &ContextMetadata) {
        writeln!(output, "## Metadata")
            .expect("writing to String must succeed");
        writeln!(output, "| Field | Value |")
            .expect("writing to String must succeed");
        writeln!(output, "| --- | --- |")
            .expect("writing to String must succeed");
        write_metadata_row(
            output,
            "Base path",
            &format!("`{}`", metadata.base_path.display()),
        );
        write_metadata_row(
            output,
            "Focus paths",
            &format_focus_paths(&metadata.focus_paths),
        );
        if let Some(diff) = &metadata.diff {
            write_metadata_row(
                output,
                "Diff spec",
                &format!("`{}`", diff.spec),
            );
            write_metadata_row(
                output,
                "Diff repo root",
                &format!("`{}`", diff.repo_root.display()),
            );
            write_metadata_row(
                output,
                "Changed files detected",
                &diff.changed_files_detected.to_string(),
            );
            write_metadata_row(
                output,
                "Changed files in scope",
                &diff.changed_files_in_scope.to_string(),
            );
            write_metadata_row(
                output,
                "Changed files selected",
                &diff.changed_files_selected.to_string(),
            );
            write_metadata_row(
                output,
                "Skipped deleted or missing",
                &diff.skipped_deleted_or_missing.to_string(),
            );
        }
        write_metadata_row(
            output,
            "Budget tokens",
            &metadata.budget_tokens.to_string(),
        );
        write_metadata_row(
            output,
            "Metadata budget tokens",
            &metadata.metadata_budget_tokens.to_string(),
        );
        write_metadata_row(
            output,
            "Excerpt budget tokens",
            &metadata.excerpt_budget_tokens.to_string(),
        );
        write_metadata_row(
            output,
            "Estimated total tokens",
            &metadata.estimated_tokens.to_string(),
        );
        write_metadata_row(
            output,
            "Estimated metadata tokens",
            &metadata.estimated_metadata_tokens.to_string(),
        );
        write_metadata_row(
            output,
            "Estimated excerpt tokens",
            &metadata.estimated_excerpt_tokens.to_string(),
        );
        write_metadata_row(
            output,
            "Files considered",
            &metadata.files_considered.to_string(),
        );
        write_metadata_row(
            output,
            "Files selected",
            &metadata.files_selected.to_string(),
        );
        write_metadata_row(
            output,
            "Truncated files",
            &metadata.truncated_files.to_string(),
        );
    }

    fn write_metadata_row(output: &mut String, field: &str, value: &str) {
        writeln!(output, "| {field} | {value} |")
            .expect("writing to String must succeed");
    }

    fn write_dominant_languages(
        output: &mut String,
        report: &sephera_core::core::context::ContextReport,
    ) {
        writeln!(output, "## Dominant Languages")
            .expect("writing to String must succeed");

        if report.dominant_languages.is_empty() {
            writeln!(output, "No recognized languages were found.")
                .expect("writing to String must succeed");
            return;
        }

        writeln!(output, "| Language | Files | Size (bytes) |")
            .expect("writing to String must succeed");
        writeln!(output, "| --- | ---: | ---: |")
            .expect("writing to String must succeed");

        for language in &report.dominant_languages {
            writeln!(
                output,
                "| {} | {} | {} |",
                language.language, language.files, language.size_bytes
            )
            .expect("writing to String must succeed");
        }
    }

    fn write_group_summaries(
        output: &mut String,
        report: &sephera_core::core::context::ContextReport,
    ) {
        writeln!(output, "## File Groups")
            .expect("writing to String must succeed");

        if report.groups.is_empty() {
            writeln!(output, "No files fit within the current context budget.")
                .expect("writing to String must succeed");
            return;
        }

        writeln!(output, "| Group | Files | Tokens | Truncated |")
            .expect("writing to String must succeed");
        writeln!(output, "| --- | ---: | ---: | ---: |")
            .expect("writing to String must succeed");

        for group in &report.groups {
            writeln!(
                output,
                "| {} | {} | {} | {} |",
                group.label,
                group.files,
                group.estimated_tokens,
                group.truncated_files
            )
            .expect("writing to String must succeed");
        }
    }

    fn write_group_section(
        output: &mut String,
        report: &sephera_core::core::context::ContextReport,
        group: &ContextGroupSummary,
    ) {
        writeln!(output, "## {}", group.label)
            .expect("writing to String must succeed");
        writeln!(
            output,
            "_{} files, {} estimated tokens, {} truncated_",
            group.files, group.estimated_tokens, group.truncated_files
        )
        .expect("writing to String must succeed");
        writeln!(output).expect("writing to String must succeed");

        writeln!(
            output,
            "| Path | Language | Reason | Size (bytes) | Tokens | Truncated |"
        )
        .expect("writing to String must succeed");
        writeln!(output, "| --- | --- | --- | ---: | ---: | --- |")
            .expect("writing to String must succeed");

        let group_files =
            report.files_in_group(group.group).collect::<Vec<_>>();
        for file in &group_files {
            writeln!(
                output,
                "| `{}` | {} | {} | {} | {} | {} |",
                file.relative_path,
                file.language.unwrap_or("unknown"),
                file.selection_class.as_str(),
                file.size_bytes,
                file.estimated_tokens,
                yes_no(file.truncated),
            )
            .expect("writing to String must succeed");
        }

        for file in group_files {
            writeln!(output).expect("writing to String must succeed");
            write_excerpt(output, file, group.group);
        }
    }

    fn write_excerpt(
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

        let fence_language = fence_language(&file.relative_path);
        if fence_language.is_empty() {
            writeln!(output, "````").expect("writing to String must succeed");
        } else {
            writeln!(output, "````{fence_language}")
                .expect("writing to String must succeed");
        }
        writeln!(output, "{}", file.excerpt.content)
            .expect("writing to String must succeed");
        writeln!(output, "````").expect("writing to String must succeed");
    }

    fn format_focus_paths(focus_paths: &[String]) -> String {
        if focus_paths.is_empty() {
            String::from("_none_")
        } else {
            focus_paths
                .iter()
                .map(|path| format!("`{path}`"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

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

    fn yes_no(value: bool) -> &'static str {
        if value { "yes" } else { "no" }
    }

    let mut output = String::new();
    writeln!(output, "# Sephera Context Pack")
        .expect("writing to String must succeed");
    writeln!(output).expect("writing to String must succeed");

    write_metadata(&mut output, &report.metadata);
    writeln!(output).expect("writing to String must succeed");
    write_dominant_languages(&mut output, report);
    writeln!(output).expect("writing to String must succeed");
    write_group_summaries(&mut output, report);

    for group in &report.groups {
        writeln!(output).expect("writing to String must succeed");
        write_group_section(&mut output, report, group);
    }

    output
}

#[tool_handler]
impl ServerHandler for SepheraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
    }
}

/// Starts the MCP server on stdio transport.
///
/// This function blocks until the client disconnects.
///
/// # Errors
///
/// Returns an error if the transport or server setup fails.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let server = SepheraServer::new();
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use tempfile::tempdir;

    use super::*;

    /// Writes `contents` to a file located at `base_dir`/`relative_path`, creating any missing parent directories.
    ///
    /// The `relative_path` is interpreted relative to `base_dir`. Parent directories will be created with
    /// default permissions if they do not exist. The function will panic if directory creation or file
    /// writing fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use std::path::Path;
    ///
    /// let tmp = tempfile::tempdir().unwrap();
    /// let base = tmp.path();
    /// write_file(base, "sub/dir/example.txt", b"hello");
    /// let got = fs::read_to_string(base.join("sub/dir/example.txt")).unwrap();
    /// assert_eq!(got, "hello");
    /// ```
    fn write_file(
        base_dir: &std::path::Path,
        relative_path: &str,
        contents: &[u8],
    ) {
        let absolute_path = base_dir.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute_path, contents).unwrap();
    }

    /// Runs `git` with the given arguments in `repo_root` and asserts the command succeeds.
    ///
    /// Panics if the `git` executable cannot be started or if the command exits with a non‑zero
    /// status. On failure, the panic message includes the provided arguments and the captured
    /// `stdout` and `stderr`.
    ///
    /// # Parameters
    ///
    /// - `repo_root`: working directory in which to run `git`.
    /// - `args`: slice of arguments to pass to `git` (e.g., `&["init"]`, `&["commit", "-m", "msg"]`).
    ///
    /// # Examples
    ///
    /// ```
    /// // Run `git --version` in the current directory (requires `git` to be available).
    /// run_git(std::path::Path::new("."), &["--version"]);
    /// ```
    fn run_git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo_root)
            .args(args)
            .output()
            .unwrap_or_else(|error| {
                panic!("failed to run git {:?}: {error}", args)
            });
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    /// Initializes a new Git repository at the given path and configures a local user name and email.
    ///
    /// This creates a repository (equivalent to `git init`) in `repo_root` and sets `user.name` to
    /// "Sephera Tests" and `user.email` to "tests@example.com" in the repository's local Git config.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use tempfile::tempdir;
    ///
    /// let dir = tempdir().unwrap();
    /// let repo_path = dir.path();
    /// init_git_repo(repo_path);
    /// assert!(repo_path.join(".git").exists());
    /// ```
    fn init_git_repo(repo_root: &Path) {
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["config", "user.name", "Sephera Tests"]);
        run_git(repo_root, &["config", "user.email", "tests@example.com"]);
    }

    /// Stages all changes and creates a commit in the specified Git repository using the given message.
    ///
    /// `repo_root` is the path to the repository working directory. `message` is used as the commit message.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    ///
    /// // Stage all files and commit with message "chore: update"
    /// commit_all(Path::new("/path/to/repo"), "chore: update");
    /// ```
    fn commit_all(repo_root: &Path, message: &str) {
        run_git(repo_root, &["add", "-A"]);
        run_git(repo_root, &["commit", "-m", message]);
    }

    /// Constructs a file:// URL for the given repository path.
    ///
    /// The returned string is the file URL formed by prefixing `file://` to the path's display representation.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// let p = Path::new("/tmp/myrepo");
    /// let url = remote_repo_url(p);
    /// assert_eq!(url, "file:///tmp/myrepo");
    /// ```
    fn remote_repo_url(repo_root: &Path) -> String {
        format!("file://{}", repo_root.display())
    }

    #[test]
    fn server_info_returns_expected_metadata() {
        let server = SepheraServer::new();
        let info = server.get_info();
        assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn loc_tool_valid_directory() {
        let server = SepheraServer::new();
        let current_dir = env!("CARGO_MANIFEST_DIR");
        let param = rmcp::handler::server::wrapper::Parameters(LocInput {
            path: Some(current_dir.to_string()),
            url: None,
            git_ref: None,
            ignore: None,
        });

        let result = server.loc(param);
        assert!(result.is_ok(), "loc tool should succeed for manifest dir");
        let output = result.unwrap();
        assert!(output.contains("Files scanned:"));
        assert!(output.contains("Languages detected:"));
    }

    #[test]
    fn loc_tool_invalid_directory() {
        let server = SepheraServer::new();
        let param = rmcp::handler::server::wrapper::Parameters(LocInput {
            path: Some("/path/to/nonexistent/dir/for/test/sephera".to_string()),
            url: None,
            git_ref: None,
            ignore: None,
        });

        let result = server.loc(param);
        assert!(result.is_err(), "loc tool should fail for nonexistent dir");
    }

    #[test]
    fn context_tool_valid_directory() {
        let server = SepheraServer::new();
        let current_dir = env!("CARGO_MANIFEST_DIR");
        let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
            path: Some(current_dir.to_string()),
            url: None,
            git_ref: None,
            config: None,
            no_config: Some(true),
            profile: None,
            list_profiles: None,
            focus: None,
            ignore: None,
            diff: None,
            budget: Some(1000),
            compress: Some("signatures".to_string()),
            format: Some("json".to_string()),
        });

        let result = server.context(param);
        assert!(
            result.is_ok(),
            "context tool should succeed for manifest dir"
        );
        let output = result.unwrap();
        assert!(output.contains("\"files_considered\""));
        assert!(output.contains("\"budget_tokens\""));
    }

    #[test]
    fn graph_tool_valid_directory() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", b"use crate::util;\n");
        write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");

        let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: Some(vec!["src/main.rs".to_owned()]),
            ignore: None,
            depth: Some(0),
            depends_on: None,
        });

        let result = server.graph(param);
        assert!(result.is_ok(), "graph tool should succeed for temp dir");
        let output = result.unwrap();
        let parsed_json: serde_json::Value =
            serde_json::from_str(&output).unwrap();
        assert_eq!(parsed_json["depth"], 0);
        assert!(parsed_json["nodes"].is_array());
    }

    #[test]
    fn graph_tool_depends_on_query_is_serialized() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", b"use crate::service;\n");
        write_file(temp_dir.path(), "src/service.rs", b"use crate::util;\n");
        write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");

        let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            depth: Some(1),
            depends_on: Some("src/util.rs".to_owned()),
        });

        let result = server.graph(param);
        assert!(result.is_ok(), "graph query should succeed");
        let output = result.unwrap();
        let parsed_json: serde_json::Value =
            serde_json::from_str(&output).unwrap();
        assert_eq!(parsed_json["query"]["depends_on"], "src/util.rs");
        assert_eq!(parsed_json["depth"], 1);
    }

    #[test]
    fn graph_tool_invalid_ignore_pattern_fails() {
        let server = SepheraServer::new();
        let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
            path: Some(env!("CARGO_MANIFEST_DIR").to_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: Some(vec!["(".to_owned()]),
            depth: None,
            depends_on: None,
        });

        let result = server.graph(param);
        assert!(result.is_err(), "graph tool should reject invalid ignore");
    }

    #[test]
    fn graph_tool_missing_depends_on_target_fails() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", b"fn main() {}\n");

        let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            depth: None,
            depends_on: Some("src/missing.rs".to_owned()),
        });

        let result = server.graph(param);
        assert!(
            result.is_err(),
            "graph query should fail for missing target"
        );
    }

    #[test]
    fn loc_tool_supports_url_mode() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        init_git_repo(temp_dir.path());
        write_file(temp_dir.path(), "src/main.rs", b"fn main() {}\n");
        commit_all(temp_dir.path(), "initial");

        let param = rmcp::handler::server::wrapper::Parameters(LocInput {
            path: None,
            url: Some(remote_repo_url(temp_dir.path())),
            git_ref: None,
            ignore: None,
        });

        let result = server.loc(param);
        assert!(result.is_ok(), "loc tool should support URL mode");
    }

    #[test]
    fn graph_tool_supports_url_mode() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        init_git_repo(temp_dir.path());
        write_file(temp_dir.path(), "src/main.rs", b"use crate::util;\n");
        write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");
        commit_all(temp_dir.path(), "initial");

        let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
            path: None,
            url: Some(remote_repo_url(temp_dir.path())),
            git_ref: None,
            focus: Some(vec!["src/main.rs".to_owned()]),
            ignore: None,
            depth: Some(0),
            depends_on: None,
        });

        let result = server.graph(param);
        assert!(result.is_ok(), "graph tool should support URL mode");
        let output = result.unwrap();
        let parsed_json: serde_json::Value =
            serde_json::from_str(&output).unwrap();
        assert!(
            parsed_json["base_path"]
                .as_str()
                .unwrap()
                .starts_with("file://")
        );
    }

    #[test]
    fn context_tool_supports_url_profiles_diff_and_markdown() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        init_git_repo(temp_dir.path());
        write_file(
            temp_dir.path(),
            ".sephera.toml",
            b"[context]\nfocus = [\"src/lib.rs\"]\n\n[profiles.review.context]\nfocus = [\"src/main.rs\"]\n",
        );
        write_file(
            temp_dir.path(),
            "src/lib.rs",
            b"pub fn answer() -> u64 {\n    42\n}\n",
        );
        write_file(
            temp_dir.path(),
            "src/main.rs",
            b"fn main() {\n    println!(\"demo\");\n}\n",
        );
        commit_all(temp_dir.path(), "initial");
        write_file(
            temp_dir.path(),
            "src/lib.rs",
            b"pub fn answer() -> u64 {\n    99\n}\n",
        );
        commit_all(temp_dir.path(), "second");

        let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
            path: None,
            url: Some(remote_repo_url(temp_dir.path())),
            git_ref: None,
            config: None,
            no_config: Some(false),
            profile: Some("review".to_owned()),
            list_profiles: Some(false),
            focus: None,
            ignore: None,
            diff: Some("HEAD~1".to_owned()),
            budget: Some(4_000),
            compress: None,
            format: Some("markdown".to_owned()),
        });

        let result = server.context(param);
        assert!(result.is_ok(), "context tool should support URL mode");
        let output = result.unwrap();
        assert!(output.starts_with("# Sephera Context Pack"));
        assert!(output.contains("HEAD~1"));
        assert!(output.contains("src/main.rs"));
    }

    #[test]
    fn context_tool_list_profiles_with_url_returns_json() {
        let server = SepheraServer::new();
        let temp_dir = tempdir().unwrap();
        init_git_repo(temp_dir.path());
        write_file(
            temp_dir.path(),
            ".sephera.toml",
            b"[profiles.review.context]\nfocus = [\"src\"]\n",
        );
        write_file(temp_dir.path(), "src/lib.rs", b"pub fn lib() {}\n");
        commit_all(temp_dir.path(), "initial");

        let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
            path: None,
            url: Some(remote_repo_url(temp_dir.path())),
            git_ref: None,
            config: None,
            no_config: Some(false),
            profile: None,
            list_profiles: Some(true),
            focus: None,
            ignore: None,
            diff: None,
            budget: None,
            compress: None,
            format: None,
        });

        let result = server.context(param);
        assert!(result.is_ok(), "context list_profiles should succeed");
        let output = result.unwrap();
        let parsed_json: serde_json::Value =
            serde_json::from_str(&output).unwrap();
        assert_eq!(parsed_json["profiles"][0], "review");
        assert!(
            parsed_json["source_path"]
                .as_str()
                .unwrap()
                .starts_with("file://")
        );
    }

    #[test]
    fn tools_reject_path_and_url_together() {
        let server = SepheraServer::new();
        let param = rmcp::handler::server::wrapper::Parameters(LocInput {
            path: Some(".".to_owned()),
            url: Some("file:///tmp/demo".to_owned()),
            git_ref: None,
            ignore: None,
        });

        let result = server.loc(param);
        assert!(result.is_err(), "path and url together should fail");
    }

    #[test]
    fn tools_reject_ref_without_url_and_blob_urls() {
        let server = SepheraServer::new();
        let ref_error = server.graph(
            rmcp::handler::server::wrapper::Parameters(GraphInput {
                path: Some(".".to_owned()),
                url: None,
                git_ref: Some("main".to_owned()),
                focus: None,
                ignore: None,
                depth: None,
                depends_on: None,
            }),
        );
        assert!(ref_error.is_err(), "ref without url should fail");

        let blob_error = server.context(
            rmcp::handler::server::wrapper::Parameters(ContextInput {
                path: None,
                url: Some(
                    "https://github.com/reim/sephera/blob/main/README.md"
                        .to_owned(),
                ),
                git_ref: None,
                config: None,
                no_config: Some(true),
                profile: None,
                list_profiles: Some(false),
                focus: None,
                ignore: None,
                diff: None,
                budget: None,
                compress: None,
                format: Some("json".to_owned()),
            }),
        );
        assert!(blob_error.is_err(), "blob URLs should fail");
    }
}
