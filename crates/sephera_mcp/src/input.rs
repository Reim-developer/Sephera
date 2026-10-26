//! Deserialization schemas for the MCP tools.
//!
//! Each struct mirrors one tool's argument object. The doc comments are part
//! of the published MCP tool schema, so they are what an agent reads when
//! deciding how to call Sephera. They therefore need to state exclusivity
//! constraints explicitly rather than leaving them to the handler.

/// Arguments accepted by the `loc` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped, which would leave the agent with a tool call that
/// appears to succeed while ignoring half of what it asked for.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocInput {
    /// Absolute or relative path to the directory to analyze. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a
    /// whole tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    /// Explicit `ignore` patterns and the always-skipped generated trees still
    /// apply, so this widens the analysis rather than disabling exclusion.
    pub no_gitignore: Option<bool>,
}

/// Arguments accepted by the `context` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional explicit config path on the local machine
    pub config: Option<String>,
    /// Disable config loading for this invocation
    pub no_config: Option<bool>,
    /// Optional named profile from `.sephera.toml`
    pub profile: Option<String>,
    /// List available profiles and return JSON instead of a context pack
    pub list_profiles: Option<bool>,
    /// Optional list of focus paths (relative to the analysis path)
    pub focus: Option<Vec<String>>,
    /// Declaration names to pack instead of whole files. Matched
    /// case-insensitively and partially, so `resolve` also finds
    /// `resolve_source`. A name matching several declarations is reported
    /// rather than guessed. Cannot be combined with `diff`.
    pub focus_symbol: Option<Vec<String>>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Optional diff source or base ref. URL mode only supports base refs such as `main` or `HEAD~1`.
    pub diff: Option<String>,
    /// Approximate token budget (default: 128000)
    pub budget: Option<u64>,
    /// Compression mode: 'none', 'signatures', or 'skeleton' (default: 'none')
    pub compress: Option<String>,
    /// Output format: 'markdown' or 'json' (default: 'json')
    pub format: Option<String>,
    /// Pack each file's in-project dependencies directly behind it,
    /// ahead of unrelated files that ranked below it, so a packed
    /// file is accompanied by what it imports. The report's
    /// metadata counts the files closure pulled in. Cannot be
    /// combined with `focus_symbol`.
    pub closure: Option<bool>,
}

/// Arguments accepted by the `symbols` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolsInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// List every declaration with its file and line, not just per-language totals.
    pub detail: Option<bool>,
}

/// Arguments accepted by the `graph` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of focus paths (relative to the analysis path)
    pub focus: Option<Vec<String>>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Optional traversal depth (0 = roots and direct neighbors)
    pub depth: Option<u32>,
    /// Optional reverse dependency target path
    pub depends_on: Option<String>,
    /// Output format: 'json' (default), 'markdown', 'xml', or 'dot'.
    /// Prefer 'markdown' for a compact summary that fits an agent's context;
    /// use 'json' when node and edge arrays need programmatic querying, and
    /// 'dot' to render with Graphviz.
    pub format: Option<String>,
}

/// Arguments accepted by the `impact` tool.
///
/// The blast radius of one or more files. Asking this before an edit is the
/// point of the command, so it gets a first-class tool rather than making an
/// agent assemble it out of `graph` with `depends_on` and parse the answer out
/// of a full dependency report.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImpactInput {
    /// File or files whose blast radius to report. One or more; several files
    /// cost about the same as one, because the graph is built once.
    pub files: Vec<String>,
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Restrict the reported dependents to these paths, matched as directory
    /// prefixes. Narrows the answer, not the analysis.
    pub focus: Option<Vec<String>>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Maximum hops from each target. 1 reports only direct importers.
    pub depth: Option<u32>,
    /// Fail the build when any target reaches this many dependents, mirroring the
    /// CLI's `--fail-on`. Reported as data in the `gate` field rather than as a
    /// tool error: a violated rule is not a broken tool, and an agent that cannot
    /// tell those apart would have to treat every threshold as fatal. The CLI exit
    /// code is included so a wrapper gets the same number the shell would.
    pub fail_on: Option<u64>,
    /// Narrow the report to one exported name in each file, mirroring the
    /// CLI's `--symbol`: `dependents` becomes the files that import that
    /// name, `dependent_count` counts them (so `fail_on` compares against
    /// the symbol's users), and the entry gains `namespace_users` (files
    /// that import the file's namespace and may use the name without
    /// naming it) and `downstream` (files downstream of those).
    pub symbol: Option<String>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `affected_tests` tool.
///
/// Which test files would notice a change to one or more files. The natural
/// follow-up to `impact` in an edit loop: the radius says what breaks, this
/// says what to run. Test files are identified by path -- a `tests`
/// directory or a language's test naming convention -- because the graph
/// does not record which files contain test code.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AffectedTestsInput {
    /// File or files whose affected tests to report. One or more; several
    /// files cost about the same as one, because the graph is built once.
    pub files: Vec<String>,
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Restrict the reported tests to these paths, matched as directory
    /// prefixes. Narrows the answer, not the analysis.
    pub focus: Option<Vec<String>>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Maximum hops from each target. 1 reports only tests that import the
    /// target directly.
    pub depth: Option<u32>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `why` tool.
///
/// Why one file depends on another, as the shortest chain of imports
/// connecting them. The follow-up to `impact` when one entry in the
/// radius needs explaining: the radius says the file is in it, this
/// says through which imports. Both endpoints are required, because a
/// chain has two ends and an empty endpoint is not a question.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WhyInput {
    /// The dependent file: the one that ends up needing the other. The
    /// chain starts here and follows its imports.
    pub from: String,
    /// The dependency: the file the chain ends at. Changing it reaches
    /// `from` through exactly the reported imports.
    pub to: String,
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `coupling` tool.
///
/// Hidden coupling is a question about a whole tree and its history,
/// so there is no file argument: the tool reads the recent commits,
/// builds the graph, and reports the pairs the two disagree about.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CouplingInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// How many recent commits to read. Defaults to 500, the CLI's
    /// window. The window bounds every count in the report.
    pub commits: Option<u32>,
    /// Report pairs that changed together at least this many times.
    /// Defaults to 3: one shared commit is a coincidence any two files
    /// can have.
    pub min_co_changes: Option<u64>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `review` tool.
///
/// A review is a question about a change in a tree, so the file set
/// comes from git rather than from a parameter: the tool reads the
/// working tree against a reference, the recent history, and the
/// graph, and reports what the change puts at risk.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Reference the working tree is compared against. Defaults to
    /// HEAD, which reviews everything uncommitted; naming a branch
    /// or tag reviews the whole body of work since that point.
    pub against: Option<String>,
    /// How many recent commits to read for the coupling warnings.
    /// Defaults to 500, the CLI's window.
    pub commits: Option<u32>,
    /// Warn about partners that changed together at least this many
    /// times. Defaults to 3.
    pub min_co_changes: Option<u64>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `risk` tool.
///
/// A ranking is a question about a whole tree and its history, so
/// there is no file argument: the tool reads the recent commits,
/// builds the graph, and ranks what the two put at risk together.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RiskInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// How many recent commits to read. Defaults to 500, the CLI's
    /// window. The window bounds every count in the ranking.
    pub commits: Option<u32>,
    /// Count a hidden pair as a coupled partner only at this many
    /// co-changes. Defaults to 3.
    pub min_co_changes: Option<u64>,
    /// How many files the ranking returns, riskiest first. Defaults
    /// to 20, the CLI's limit.
    pub limit: Option<u32>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `dead` tool.
///
/// The question is about a whole tree, so there is no file
/// argument: the tool walks forward from the entry points the path
/// conventions identify, plus any the caller names, and reports
/// what the walk never reaches.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeadInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Additional entry points the path conventions cannot see, in
    /// the analysis' path spelling: an entry class, a plugin loaded
    /// by name, a file a framework scans for. A root that matches
    /// no file in the analysis is an error.
    pub roots: Option<Vec<String>>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `breaking` tool.
///
/// The comparison is between two refs of one local repository, so
/// there is no `url` argument: a remote checkout is one ref already
/// and has nothing to compare against. `from` is required; `to`
/// defaults to the working tree, the form a branch review takes
/// before it is pushed.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakingInput {
    /// Absolute or relative path to the repository root.
    pub path: Option<String>,
    /// The older reference: a branch, tag, or commit the symbols are
    /// compared from.
    pub from: String,
    /// The newer reference to compare to. When absent, the working
    /// tree itself — staged, unstaged, and untracked files included.
    pub to: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `archaeology` tool.
///
/// The trail is one local repository's history, so there is no
/// `url` argument: a remote checkout is one ref already and has
/// no history to walk. `file` and `symbol` name the query; `at`
/// defaults to HEAD.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchaeologyInput {
    /// Absolute or relative path to the repository root.
    pub path: Option<String>,
    /// The file the symbol lives in, spelled as the graph spells
    /// it — relative to the analysis root, with forward slashes.
    pub file: String,
    /// The symbol to trace, as declared in the file at the
    /// starting ref.
    pub symbol: String,
    /// The ref the symbol is queried at. HEAD when absent.
    pub at: Option<String>,
    /// Maximum commits examined while walking backwards.
    /// Defaults to 500.
    pub window: Option<usize>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `behavior` tool.
///
/// The comparison is between two refs of one local repository, so
/// there is no `url` argument: a remote checkout is one ref already
/// and has nothing to compare against. `from` is required; `to`
/// defaults to the working tree, the form a branch review takes
/// before it is pushed.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BehaviorInput {
    /// Absolute or relative path to the repository root.
    pub path: Option<String>,
    /// The older reference: a branch, tag, or commit the symbols are
    /// compared from.
    pub from: String,
    /// The newer reference to compare to. When absent, the working
    /// tree itself — staged, unstaged, and untracked files included.
    pub to: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `merge` tool.
///
/// The comparison is between three refs of one local repository,
/// so there is no `url` argument: a remote checkout is one ref
/// already and has nothing to compare against. `ours` and
/// `theirs` are required; `base` defaults to their merge base,
/// the fork point git itself would merge from.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeInput {
    /// Absolute or relative path to the repository root.
    pub path: Option<String>,
    /// One side of the merge: a branch, tag, or commit whose
    /// contract changes are judged against the other side's new
    /// consumers.
    pub ours: String,
    /// The other side of the merge: a branch, tag, or commit
    /// whose new consumers are judged against the first side's
    /// contract changes.
    pub theirs: String,
    /// The fork point the two sides diverged from. When absent,
    /// the merge base of ours and theirs is used.
    pub base: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `diff` tool.
///
/// The comparison is between two refs of one local repository, so
/// there is no `url` argument: a remote checkout is one ref already
/// and has nothing to compare against. `from` is required; `to`
/// defaults to the working tree, the form a branch review takes
/// before it is pushed.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffInput {
    /// Absolute or relative path to the repository root.
    pub path: Option<String>,
    /// The older reference: a branch, tag, or commit the symbols are
    /// compared from.
    pub from: String,
    /// The newer reference to compare to. When absent, the working
    /// tree itself — staged, unstaged, and untracked files included.
    pub to: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `simulate` tool.
///
/// One tree, not two refs, so a remote checkout is as good a tree
/// to simulate against as a local one: `url` and `git_ref` are
/// accepted the way `impact` accepts them. `file`, `symbol`, and
/// `to` are required; `to_file` moves the symbol instead of
/// renaming it in place.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimulateInput {
    /// Absolute or relative path to the repository root.
    pub path: Option<String>,
    /// Optional remote repository URL to analyze.
    pub url: Option<String>,
    /// Optional ref to check out from the remote repository.
    pub git_ref: Option<String>,
    /// The file that declares the symbol today.
    pub file: String,
    /// The symbol's name today.
    pub symbol: String,
    /// The name the symbol would take.
    pub to: String,
    /// The file the symbol would live in. When absent, the rename
    /// happens in place.
    pub to_file: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `untested` tool.
///
/// The question is about a whole tree, so there is no file
/// argument: the tool finds the test files by convention, walks
/// forward from them, and reports the declared symbols no reached
/// file names.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UntestedInput {
    /// Absolute or relative path to the project root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Remote Git URL to analyze instead of a local path. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Git reference to analyze (a branch, tag, or commit). Only applies when `url` is set.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

/// Arguments accepted by the `test_priority` tool.
///
/// The question is about a whole tree, so there is no file
/// argument: the tool ranks the tree's untested symbols by
/// downstream × churn × coupling, highest score first.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestPriorityInput {
    /// Absolute or relative path to the project root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Remote Git URL to analyze instead of a local path. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Git reference to analyze (a branch, tag, or commit). Only applies when `url` is set.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns. Patterns containing `*`, `?`, or `[` are
    /// globs matched against both the file name and the path relative to the
    /// analysis root, so `dist/**` and `**/node_modules/**` both exclude a whole
    /// tree. Any other pattern is a regex matched against that path.
    pub ignore: Option<Vec<String>>,
    /// Do not apply the repository's own `.gitignore` and `.sepheraignore`.
    pub no_gitignore: Option<bool>,
    /// How many recent commits to read. Defaults to 500, the CLI's
    /// window. The window bounds every count in the ranking.
    pub window: Option<u32>,
    /// How many symbols the ranking returns, highest score first.
    /// Defaults to the full ranked set.
    pub top: Option<u32>,
    /// Output format: 'json' (default) or 'markdown'. Prefer 'markdown' for a
    /// compact summary that fits an agent's context.
    pub format: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ContextInput, GraphInput, LocInput};

    #[test]
    fn git_ref_is_exposed_as_ref_key() {
        let input: LocInput = serde_json::from_str(
            r#"{"url":"https://github.com/o/r","ref":"v1.2.3"}"#,
        )
        .expect("loc input must deserialize");

        assert_eq!(input.git_ref.as_deref(), Some("v1.2.3"));
        assert!(input.path.is_none());
    }

    #[test]
    fn every_field_is_optional() {
        let input: LocInput = serde_json::from_str("{}")
            .expect("loc input must accept an empty object");

        assert!(input.path.is_none());
        assert!(input.url.is_none());
        assert!(input.git_ref.is_none());
        assert!(input.ignore.is_none());
    }

    #[test]
    fn context_input_parses_full_argument_set() {
        let input: ContextInput = serde_json::from_str(
            r#"{"path":".","compress":"signatures","budget":32000,"format":"markdown","focus":["src"]}"#,
        )
        .expect("context input must deserialize");

        assert_eq!(input.compress.as_deref(), Some("signatures"));
        assert_eq!(input.budget, Some(32_000));
        assert_eq!(input.format.as_deref(), Some("markdown"));
        assert_eq!(input.focus.as_deref(), Some(["src".to_owned()].as_slice()));
    }

    #[test]
    fn graph_input_parses_format() {
        let input: GraphInput =
            serde_json::from_str(r#"{"path":".","format":"markdown"}"#)
                .expect("graph input must deserialize");

        assert_eq!(input.format.as_deref(), Some("markdown"));
    }

    #[test]
    fn graph_format_defaults_to_absent() {
        let input: GraphInput = serde_json::from_str(r#"{"path":"."}"#)
            .expect("graph input must deserialize");

        assert!(input.format.is_none(), "format must stay optional");
    }

    #[test]
    fn graph_input_parses_depth_and_depends_on() {
        let input: GraphInput = serde_json::from_str(
            r#"{"path":".","depth":1,"depends_on":"src/core.rs"}"#,
        )
        .expect("graph input must deserialize");

        assert_eq!(input.depth, Some(1));
        assert_eq!(input.depends_on.as_deref(), Some("src/core.rs"));
    }

    #[test]
    fn unknown_fields_are_rejected_rather_than_silently_ignored() {
        let result = serde_json::from_str::<LocInput>(
            r#"{"path":".","paths":"./other"}"#,
        );

        assert!(
            result.is_err(),
            "a mistyped argument must fail loudly instead of being dropped"
        );
    }
}
