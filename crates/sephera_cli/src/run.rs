use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use sephera_core::declarations::DeclarationIndex;
use sephera_graph::{
    merge::MergeSideAnalysis,
    resolver::{
        EdgeFilters, build_focus_set, build_graph,
        build_graph_with_declarations, build_graph_with_progress,
    },
    types::{GraphFormat, GraphQuery, GraphReport},
};
use sephera_runtime::{
    INTERRUPTED_EXIT_CODE, Interrupted, ResolvedSource, SourceRequest,
    build_context_report, changed_files, checkout_worktree, commit_meta,
    first_parent_commit, load_project_settings, log_commits_for_file,
    merge_base, recent_commit_files, repo_toplevel, resolve_changed_files,
    resolve_source,
};
use sephera_scan::{CodeLoc, IgnoreMatcher};
use sephera_symbols::{SymbolAnalyzer, SymbolDetail};

use crate::{
    affected_tests, archaeology,
    args::{
        AffectedTestsArgs, ArchaeologyArgs, ArchaeologyOutputFormat,
        BehaviorArgs, BehaviorOutputFormat, BreakingArgs, BreakingOutputFormat,
        Cli, Commands, ContextArgs, CouplingArgs, CouplingOutputFormat,
        DeadArgs, DeadOutputFormat, DiffArgs, DiffOutputFormat, GraphArgs,
        GraphOutputFormat, GuardArgs, IgnoreArgs, ImpactArgs,
        ImpactOutputFormat, LocArgs, LocOutputFormat, MergeArgs,
        MergeOutputFormat, OutputArgs, ReviewArgs, ReviewOutputFormat,
        RiskArgs, RiskOutputFormat, SchemaImpactArgs, SchemaImpactOutputFormat,
        SettingsArgs, SimulateArgs, SimulateOutputFormat, SkillArgs,
        SourceArgs, SymbolOutputFormat, SymbolsArgs, TestImpactOutputFormat,
        TestPriorityArgs, TestPriorityOutputFormat, UntestedArgs,
        UntestedOutputFormat, WatchArgs, WatchTarget, WhyArgs, WhyOutputFormat,
    },
    behavior, breaking, change_impact, configure,
    context_config::{
        ResolvedContextCommand, ResolvedContextOptions, resolve_context_options,
    },
    coupling, dead, diff,
    gate::{self, Gate},
    guard, impact, merge,
    output::{
        emit_rendered_output, print_available_profiles, render_context_json,
        render_context_markdown, render_graph, render_loc_csv, render_loc_json,
        render_loc_markdown, render_report_table,
        render_symbol_declarations_table, render_symbol_json,
        render_symbol_markdown, render_symbol_table,
        render_symbols_by_file_table,
    },
    progress::{self, CliProgress, CliReporter},
    review, risk, sarif, schema_impact, simulate, skill, test_priority,
    untested, watch, why,
};

#[must_use]
pub fn main_exit_code() -> ExitCode {
    match run() {
        Ok(exit) => exit,
        Err(error) => exit_code_for(&error),
    }
}

/// Decide what a failed run means to whatever invoked it.
///
/// Pressing Ctrl+C is a decision, not a fault, so it gets its own code and no
/// `error:` line: a clone of a large repository runs for minutes, and the natural
/// thing to do about that is stop it. A script that can read 130 knows the
/// difference between "you stopped this" and "this failed", which a shared 1
/// cannot express.
fn exit_code_for(error: &anyhow::Error) -> ExitCode {
    if error.downcast_ref::<Interrupted>().is_some() {
        return ExitCode::from(INTERRUPTED_EXIT_CODE);
    }
    eprintln!("error: {error:#}");
    ExitCode::FAILURE
}

/// # Errors
///
/// Returns an error when argument parsing or command execution fails.
///
/// The `Ok` case carries an exit code rather than nothing, because a run can
/// succeed at its job and still have found something the caller asked to fail
/// on. See [`crate::gate`].
pub fn run() -> Result<ExitCode> {
    // `.sephera.toml` is folded into the arguments before `clap` parses them,
    // so a config value is validated by the same parser as a typed flag and a
    // typed flag still wins. See `configure` for why this is a pre-pass rather
    // than a second source of defaults.
    let expanded = configure::expand(std::env::args().collect(), None)?;
    let cli = Cli::parse_from(&expanded.argv);
    progress::set_mode(cli.progress);

    // `watch` keeps a blocking loop over `notify`'s synchronous receiver, so it
    // owns a runtime rather than being driven from inside one. `Handle::block_on`
    // panics when called from a runtime thread, and a watch session started from
    // within the main runtime would be exactly that.
    if let Commands::Watch(arguments) = cli.command {
        return Ok(gate::evaluate(&run_watch(&arguments)?));
    }

    // Everything else runs on one runtime. `new_multi_thread` rather than
    // `new_current_thread` for one reason: `block_in_place` panics on a
    // current-thread runtime, and the MCP server and the file watcher both call
    // into libraries that may use it. Constructing sixteen worker threads costs
    // about 240 microseconds on this machine -- measured, not assumed -- against
    // a single-threaded runtime's 0.4, which is not worth a panic.
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the async runtime")?
        .block_on(dispatch(cli.command))
}

async fn dispatch(command: Commands) -> Result<ExitCode> {
    let gate = match command {
        Commands::Loc(arguments) => run_loc(&arguments).await,
        Commands::Symbols(arguments) => run_symbols(&arguments).await,
        Commands::Context(arguments) => {
            let profile = arguments.settings.profile.clone();
            run_context(arguments, profile).await
        }
        Commands::Mcp => run_mcp().await,
        Commands::Graph(arguments) => run_graph(&arguments).await,
        // Dispatched by `run` before the runtime starts, because `run_watch` owns a
        // runtime of its own. Reaching this arm means a later edit moved that check
        // inside the `block_on`, which is the nesting the split exists to avoid.
        Commands::Watch(_) => {
            unreachable!("`watch` is dispatched before the runtime starts")
        }
        Commands::Impact(arguments) => run_impact(&arguments).await,
        Commands::AffectedTests(arguments) => {
            run_affected_tests(&arguments).await
        }
        Commands::Why(arguments) => run_why(&arguments).await,
        Commands::Coupling(arguments) => run_coupling(&arguments).await,
        Commands::Review(arguments) => run_review(&arguments).await,
        Commands::Risk(arguments) => run_risk(&arguments).await,
        Commands::Dead(arguments) => run_dead(&arguments).await,
        Commands::Breaking(arguments) => run_breaking(&arguments).await,
        Commands::Merge(arguments) => run_merge(&arguments).await,
        Commands::Diff(arguments) => run_diff(&arguments).await,
        Commands::Archaeology(arguments) => run_archaeology(&arguments).await,
        Commands::Behavior(arguments) => run_behavior(&arguments).await,
        Commands::Simulate(arguments) => run_simulate(&arguments).await,
        Commands::Untested(arguments) => run_untested(&arguments).await,
        Commands::TestPriority(arguments) => {
            run_test_priority(&arguments).await
        }
        Commands::SchemaImpact(arguments) => {
            run_schema_impact(&arguments).await
        }
        Commands::Guard(arguments) => run_guard(&arguments).await,
        Commands::Skill(arguments) => run_skill(&arguments),
    }?;
    Ok(gate::evaluate(&gate))
}

/// Commands that analyse a tree collect thresholds as they go and hand them
/// back, so `--fail-on-*` cannot mean one thing for `graph` and another for
/// `impact`. Commands with nothing to enforce return an empty slice.
const fn no_gates() -> Vec<Gate> {
    Vec::new()
}

/// Re-run a chosen analysis whenever the watched tree changes.
///
/// `--once` short-circuits to a single run, which keeps the watch argument
/// parsing usable from scripts without needing a separate code path.
fn run_watch(arguments: &WatchArgs) -> Result<Vec<Gate>> {
    // Unreachable through clap, which requires `--target` outright: the old
    // `required_unless_present = "once"` spelling let `watch --once` parse
    // and die here claiming the target was required *unless* `--once` was
    // passed — while `--once` was exactly what had been passed.
    let Some(target) = arguments.target else {
        anyhow::bail!("`--target` is required");
    };

    if target == WatchTarget::DependsOn && arguments.on.is_none() {
        anyhow::bail!("`--on <file>` is required with `--target depends-on`");
    }

    let root = watch::resolve_root(arguments.path.as_deref());
    let ignore = arguments.ignore_args.ignore.clone();

    // `watch` runs outside the main runtime and owns this one, so the blocking
    // `run_watch_target` calls below can `block_on` without nesting runtimes.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the watch runtime")?;

    if arguments.once {
        return runtime.block_on(run_watch_target(
            target,
            arguments.on.as_deref(),
            &root,
            &ignore,
            &arguments.settings,
        ));
    }

    // The live loop prints "Watching ..." before it ever touches the root,
    // so a nonexistent root or a file-as-root used to announce a watch that
    // never started and only then fail inside the watcher. `--once` fails
    // cleanly through the analysis command's own path check; the live loop
    // gets the same check here, in the same words as `sephera_scan`'s
    // project-file validation, before anything claims to be watching.
    if !root.is_dir() {
        if !root.exists() {
            anyhow::bail!("path `{}` does not exist", root.display());
        }
        anyhow::bail!("path `{}` is not a directory", root.display());
    }

    println!(
        "Watching {} for changes. Press Ctrl+C to stop.",
        root.display()
    );

    // A watch loop has no single moment to evaluate a threshold against, so the
    // per-run gates are reported as they happen and the final exit code is
    // success. `watch --once` is the form to use in a pipeline that needs a
    // threshold.
    watch::watch(&root, || {
        let gates = runtime.block_on(run_watch_target(
            target,
            arguments.on.as_deref(),
            &root,
            &ignore,
            &arguments.settings,
        ))?;
        report_gates(&gates);
        Ok(())
    })?;

    Ok(no_gates())
}

/// Print any crossed gate to stderr.
///
/// Used where an exit code cannot carry the verdict, which today is only the
/// watch loop: it runs until interrupted, so there is no final exit code to set.
fn report_gates(gates: &[Gate]) {
    let crossed: Vec<&Gate> =
        gates.iter().filter(|gate| gate.crossed()).collect();
    for gate in crossed {
        eprintln!("error: {}", gate.failure_line());
    }
}

/// Run one analysis pass for the watch target.
async fn run_watch_target(
    target: WatchTarget,
    on: Option<&str>,
    root: &std::path::Path,
    ignore: &[String],
    config: &SettingsArgs,
) -> Result<Vec<Gate>> {
    // The shared groups are built once and cloned into each arm, so a value is
    // never spelled differently in one arm than in another. Flattening the
    // argument groups is what made this necessary, and building them here is
    // what keeps it from being noisy.
    let source = || SourceArgs {
        path: Some(root.to_path_buf()),
        ..SourceArgs::default()
    };
    let ignore_args = || IgnoreArgs {
        ignore: ignore.to_vec(),
        ..IgnoreArgs::default()
    };

    match target {
        WatchTarget::Loc => {
            run_loc(&LocArgs {
                source: source(),
                ignore_args: ignore_args(),
                settings: config.to_owned(),
                format: LocOutputFormat::Table,
                output_args: OutputArgs::default(),
            })
            .await
        }
        WatchTarget::Symbols => {
            run_symbols(&SymbolsArgs {
                source: source(),
                ignore_args: ignore_args(),
                settings: config.to_owned(),
                format: SymbolOutputFormat::Table,
                output_args: OutputArgs::default(),
                detail: false,
                by_file: false,
            })
            .await
        }
        WatchTarget::Graph => {
            run_graph(&GraphArgs {
                source: source(),
                ignore_args: ignore_args(),
                settings: config.to_owned(),
                focus: Vec::new(),
                depth: None,
                what_depends_on: None,
                exclude_types: false,
                fail_on_cycles: None,
                fail_on_unresolved: None,
                diff: None,
                format: GraphOutputFormat::Markdown,
                output_args: OutputArgs::default(),
            })
            .await
        }
        WatchTarget::DependsOn => {
            let Some(target_path) = on else {
                anyhow::bail!(
                    "`--on <file>` is required with `--target depends-on`"
                );
            };
            run_graph(&GraphArgs {
                source: source(),
                ignore_args: ignore_args(),
                settings: config.to_owned(),
                focus: Vec::new(),
                depth: None,
                what_depends_on: Some(target_path.to_owned()),
                exclude_types: false,
                fail_on_cycles: None,
                fail_on_unresolved: None,
                diff: None,
                format: GraphOutputFormat::Markdown,
                output_args: OutputArgs::default(),
            })
            .await
        }
    }
}

/// Serve the MCP protocol.
///
/// Awaited rather than driven by a runtime of its own: the command dispatch
/// already runs on one, and `block_on` from inside a runtime thread panics.
async fn run_mcp() -> Result<Vec<Gate>> {
    sephera_mcp::run_mcp_server().await?;
    Ok(no_gates())
}

/// Build the exclusion policy for one invocation.
///
/// The repository's own ignore files are honoured unless the user asked
/// otherwise. Routing every command through one function means `--no-gitignore`
/// cannot mean one thing for `graph` and another for `context`.
///
/// `[project]` patterns from `.sephera.toml` are merged in ahead of the flags,
/// so a repository states its exclusions once instead of on every command line.
fn build_ignore_matcher(
    base_path: &std::path::Path,
    config: &SettingsArgs,
    patterns: &[String],
    no_gitignore: bool,
) -> Result<IgnoreMatcher> {
    let settings = load_project_settings(
        base_path,
        config.config.as_deref(),
        config.no_config,
    )?;
    let merged = settings.merged_ignore(patterns);

    if no_gitignore {
        IgnoreMatcher::from_patterns_without_ignore_files(&merged)
    } else {
        IgnoreMatcher::from_patterns(&merged)
    }
}

/// A focus scope must name something inside the analysis base that
/// exists.
///
/// A scope naming nothing used to narrow the answer to nothing with no
/// complaint: `graph --focus nope.py` reported an empty graph and
/// `impact --focus nope-dir` reported "No file imports this one", and
/// both read as findings about the repository rather than as typos.
/// `context --focus` already refuses a path it cannot resolve; a scope
/// is the same kind of reference and gets the same refusal. A scope
/// that exists but holds no recognised file still reports empty, which
/// is a real answer about a real place.
fn check_focus_paths_exist(
    base_path: &std::path::Path,
    focus: &[PathBuf],
) -> Result<()> {
    let base = std::path::absolute(base_path)
        .unwrap_or_else(|_| base_path.to_path_buf());
    for path in focus {
        let candidate = if path.is_absolute() {
            path.clone()
        } else {
            base_path.join(path)
        };
        let absolute_candidate = std::path::absolute(&candidate)
            .unwrap_or_else(|_| candidate.clone());
        if absolute_candidate.strip_prefix(&base).is_err() {
            anyhow::bail!(
                "focus path `{}` must resolve inside `{}`",
                path.display(),
                base_path.display()
            );
        }
        if !candidate.exists() {
            anyhow::bail!(
                "focus path `{}` does not exist inside `{}`",
                path.display(),
                base_path.display()
            );
        }
    }
    Ok(())
}

async fn run_loc(arguments: &LocArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Analyzing line counts...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    let mut report = CodeLoc::new(&source.analysis_path, ignore)
        .analyze_with(&CliReporter(&progress))?;
    if let Some(display_path) = source.display_path {
        report.base_path = display_path.into();
    }

    // Only the table is meant for a terminal. The other three are text meant
    // for a pipe or a file, so the spinner has to be gone before they are
    // emitted or it interleaves with the payload. The table is still text
    // once rendered, so it goes through the same `--output` path as the
    // other formats: before this it printed straight to stdout and a
    // `--output` file was silently never written.
    let rendered = match arguments.format {
        LocOutputFormat::Table => render_report_table(&report),
        LocOutputFormat::Json => render_loc_json(&report),
        LocOutputFormat::Markdown => render_loc_markdown(&report),
        LocOutputFormat::Csv => render_loc_csv(&report),
    };

    // Same rule as `graph`: a report going to a file keeps the spinner alive
    // to say so, and a report going to a terminal stops it first.
    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(no_gates())
}

async fn run_symbols(arguments: &SymbolsArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Counting declarations...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    let analyzer = SymbolAnalyzer::new(&source.analysis_path, ignore);

    // The declaration list is only collected when a format can carry it.
    // Parsing twice would double the work, so the summary path reads only the
    // report and the paths that need per-file data read both.
    let needs_symbols = arguments.detail
        || arguments.by_file
        || matches!(arguments.format, SymbolOutputFormat::Json);
    let mut detail = if needs_symbols {
        analyzer.analyze_detailed()?
    } else {
        SymbolDetail::from(analyzer.analyze()?)
    };
    if let Some(display_path) = source.display_path {
        detail.report.base_path = display_path.into();
    }
    progress.finish();

    let rendered = match arguments.format {
        // Rendered to a string like the other two formats, so `--output`
        // works here too: the print forms could only go to stdout, which
        // is how a table report with `--output` used to print to the
        // terminal and never create the file.
        SymbolOutputFormat::Table => {
            let mut rendered = if arguments.by_file {
                render_symbols_by_file_table(&detail)
            } else {
                render_symbol_table(&detail.report)
            };
            // `--detail` reaches every format, not just markdown and json.
            // It used to change nothing here, so a table run with the flag
            // answered the summary question while the caller asked for the
            // declaration list.
            if arguments.detail
                && let Some(declarations) =
                    render_symbol_declarations_table(&detail)
            {
                if !rendered.is_empty() {
                    rendered.push_str("\n\n");
                }
                rendered.push_str(&declarations);
            }
            Some(rendered)
        }
        // `by_file` reaches every format, not just the table. It used to be
        // honoured only here, so `--by-file --format json` produced a
        // well-formed report with no per-file breakdown in it and nothing said so.
        SymbolOutputFormat::Json => {
            Some(render_symbol_json(&detail, arguments.by_file))
        }
        SymbolOutputFormat::Markdown => {
            Some(render_symbol_markdown(&detail, arguments.by_file))
        }
    };

    if let Some(rendered) = rendered {
        emit_rendered_output(
            arguments.output_args.output.as_deref(),
            &rendered,
        )?;
    }
    Ok(no_gates())
}

async fn run_context(
    arguments: ContextArgs,
    profile: Option<String>,
) -> Result<Vec<Gate>> {
    let closure = arguments.closure;
    match resolve_context_options(arguments, profile).await? {
        ResolvedContextCommand::Execute(mut resolved) => {
            if closure {
                inject_dependency_closure(&mut resolved)?;
            }
            execute_context(&resolved)
        }
        ResolvedContextCommand::ListProfiles(profiles) => {
            print_available_profiles(&profiles);
            Ok(no_gates())
        }
    }
}

/// Build the import graph and hand its edges to the context
/// builder as the closure map.
///
/// The matcher is the one the context build itself uses -- the
/// resolved options' patterns, with the repository's ignore files
/// applied unless the invocation turned them off -- so the graph
/// and the pack are built over the same file set. Edges are the
/// graph's resolved ones; an unresolved import names no file the
/// pack could include.
fn inject_dependency_closure(
    options: &mut ResolvedContextOptions,
) -> Result<()> {
    let matcher = if options.read_ignore_files {
        IgnoreMatcher::from_patterns(&options.ignore)?
    } else {
        IgnoreMatcher::from_patterns_without_ignore_files(&options.ignore)?
    };
    let graph =
        build_graph(&options.source.analysis_path, &matcher, &[], None, None)?;

    options.dependency_closure =
        Some(sephera_graph::resolver::forward_dependencies(&graph));
    Ok(())
}

fn execute_context(arguments: &ResolvedContextOptions) -> Result<Vec<Gate>> {
    let progress = CliProgress::start("Preparing context inputs...");
    progress.set_message("Building context pack...");
    let report = build_context_report(arguments)?;

    progress.set_message("Rendering context pack...");
    let rendered = match arguments.format.as_str() {
        "markdown" => render_context_markdown(&report),
        "json" => render_context_json(&report),
        other => unreachable!("unexpected resolved context format `{other}`"),
    };

    // `ResolvedContextOptions` is built in core, where the field is still flat.
    let writes_to_stdout = arguments.output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    report_unresolved_symbols(&arguments.unresolved_symbols);
    emit_rendered_output(arguments.output.as_deref(), &rendered)?;
    Ok(no_gates())
}

/// Warn about `--focus-symbol` names that matched nothing or too much.
///
/// Reported on stderr rather than as an error so the pack still reaches the
/// caller: a typo in one name should not discard the declarations that did
/// resolve, but it must not pass unnoticed either.
fn report_unresolved_symbols(names: &[String]) {
    if names.is_empty() {
        return;
    }

    let listed: Vec<String> =
        names.iter().map(|name| format!("  `{name}`")).collect();
    eprintln!(
        "warning: {} `--focus-symbol` name(s) matched no single declaration and were left out of the pack:\n{}",
        names.len(),
        listed.join("\n")
    );
}

/// Report the blast radius of one or more files.
///
/// The graph is built **once** and every target is measured against it. Building
/// it per target is the obvious implementation and the wrong one: on cargo,
/// five targets cost 2,874 ms that way against 663 ms for the single build that
/// answers all five questions.
async fn run_impact(arguments: &ImpactArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Computing blast radius...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    check_focus_paths_exist(&source.analysis_path, &arguments.focus)?;

    progress.set_message("Extracting imports...");
    // No query: the report must cover the whole repository, because a
    // `DependsOn` query would restrict it to one target's dependents and leave
    // the others with no edges to walk.
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        arguments.depth,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: arguments.exclude_types,
        },
        &CliReporter(&progress),
    )?;

    // Normalised the same way `graph --focus` normalises, so an absolute
    // `--focus` is compared in the graph's spelling rather than against its own.
    let focus = build_focus_set(&source.analysis_path, &arguments.focus)
        .into_iter()
        .collect::<Vec<_>>();

    let (rendered, gates) = impact_outcome(arguments, &report, &focus)?;

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(gates)
}

/// Measure one impact run and render it, with the gates it crosses.
///
/// Split out of `run_impact` because the symbol branch doubled the
/// measuring code: the source resolution and graph build above are
/// shared, everything from the scope note down is this question --
/// file-level or symbol-level -- asked of the same report.
fn impact_outcome(
    arguments: &ImpactArgs,
    report: &sephera_graph::types::GraphReport,
    focus: &[String],
) -> Result<(String, Vec<Gate>)> {
    // Keyed off the *effective* scope rather than the flags as typed: `--focus .`
    // is spelled as a scope but normalises to the whole analysis base, and
    // claiming a narrowed count for an unscoped one would be a false statement
    // in the one line someone reads to decide whether to merge.
    let scope_note = if focus.is_empty() {
        String::new()
    } else {
        " within the requested scope".to_owned()
    };

    // One gate per offending target rather than a single gate for the widest,
    // so the stderr line names the file that broke the rule rather than a count
    // with no owner.
    //
    // With `--focus`, the count is of dependents *inside the scope*, not in the
    // repository. The label says so, because a CI log reading "3 files depend on
    // X, limit is 40" is indistinguishable from X having three dependents in
    // total, and those two warrant different decisions.
    //
    // With `--symbol`, both the report and the gate narrow together: the count
    // compared is the files that name the symbol, and the label names the
    // symbol, so a gate that fires on a rename does not read as one that fired
    // on the whole file.
    let (rendered, gates) = if let Some(symbol) = &arguments.symbol {
        let impacts = impact::measure_symbol_all(
            report,
            &arguments.files,
            "",
            symbol,
            arguments.depth,
            focus,
        )?;
        let rendered = match arguments.format {
            ImpactOutputFormat::Markdown => {
                impact::render_symbol_report(&impacts)
            }
            ImpactOutputFormat::Json => {
                impact::render_symbol_report_json(&impacts)
            }
        };
        let gates = arguments.fail_on.map_or_else(Vec::new, |limit| {
            impacts
                .iter()
                .filter(|impact| impact::symbol_direct_count(impact) >= limit)
                .map(|impact| {
                    Gate::new(
                        format!(
                            "files importing `{symbol}` from `{}`{scope_note}",
                            impact.target
                        ),
                        impact::symbol_direct_count(impact),
                        limit,
                    )
                })
                .collect()
        });
        (rendered, gates)
    } else {
        let radii = impact::measure_all(
            report,
            &arguments.files,
            "",
            arguments.depth,
            focus,
        )?;
        let rendered = match arguments.format {
            ImpactOutputFormat::Markdown => impact::render_report(&radii),
            ImpactOutputFormat::Json => impact::render_report_json(&radii),
        };
        let gates = arguments.fail_on.map_or_else(Vec::new, |limit| {
            radii
                .iter()
                .filter(|radius| impact::dependent_count(radius) >= limit)
                .map(|radius| {
                    Gate::new(
                        format!(
                            "files depending on `{}`{scope_note}",
                            radius.target
                        ),
                        impact::dependent_count(radius),
                        limit,
                    )
                })
                .collect()
        });
        (rendered, gates)
    };
    Ok((rendered, gates))
}

/// Show why one file depends on another.
///
/// The graph build is `run_impact`'s, step for step: the whole
/// repository, no query. A `DependsOn` query would leave the report
/// holding only one file's dependents, and a chain can pass through
/// any file at all, so a narrowed report could report "no path" for a
/// coupling that exists outside the narrowing.
async fn run_why(arguments: &WhyArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Finding the dependency chain...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    progress.set_message("Extracting imports...");
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        None,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: arguments.exclude_types,
        },
        &CliReporter(&progress),
    )?;

    let path =
        why::explain(&report, &arguments.from_file, &arguments.to_file, "")?;

    // `explain` refuses a path that matches nothing, so both of these
    // resolve; the fallback only spells the request back, and exists
    // because rendering needs a spelling either way.
    let canonical = |raw: &str| {
        impact::match_path(&report, raw, "").unwrap_or_else(|| raw.to_owned())
    };
    let (from, to) = path.as_ref().map_or_else(
        || {
            (
                canonical(&arguments.from_file),
                canonical(&arguments.to_file),
            )
        },
        |path| (path.from.clone(), path.to.clone()),
    );

    let rendered = match arguments.format {
        WhyOutputFormat::Markdown => {
            why::render_markdown(path.as_ref(), &from, &to)
        }
        WhyOutputFormat::Json => why::render_json(path.as_ref(), &from, &to),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(Vec::new())
}

/// Report which tests one or more files have to answer for.
///
/// The graph build and the scope normalisation are `run_impact`'s, step for
/// step: the two commands are one analysis with two filters, and a reader
/// comparing them should never have to wonder whether the difference is in
/// the answer or in how the question was built.
/// Find hidden coupling: history's pairs that the graph does not join.
///
/// Two readings of the same tree, intersected. The graph is built over
/// the whole repository, as `run_impact`'s is, because a coupling can
/// join any two files in it; the history is read at the repository
/// root, where git spells paths from, and rebased onto the analysis
/// base's spelling before the two are compared.
async fn run_coupling(arguments: &CouplingArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        // History commands read the history: a depth-1 clone would
        // cap every window at one commit, so URL runs need the full
        // clone. Local paths ignore this flag.
        true,
    )
    .await?;
    let progress = CliProgress::start("Reading history...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    // History first: a tree with no readable history is an error this
    // command exists to state plainly, and it should arrive before the
    // graph build spends its time rather than after.
    let commits = recent_commit_files(&source.repo_root, arguments.commits)
        .with_context(|| {
            format!(
                "coupling reads the git history of {}, and none could be read",
                source.repo_root.display()
            )
        })?;

    progress.set_message("Extracting imports...");
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        None,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: false,
        },
        &CliReporter(&progress),
    )?;

    let base_prefix = history_base_prefix(&source);
    let rebased = coupling::rebase_commit_paths(&commits, &base_prefix);
    let measured = coupling::measure_coupling(
        &report,
        &rebased,
        coupling::CouplingOptions {
            min_co_changes: arguments.min_co_changes,
            ..coupling::CouplingOptions::default()
        },
    );

    let rendered = match arguments.format {
        CouplingOutputFormat::Markdown => {
            coupling::render_markdown(&measured, arguments.min_co_changes)
        }
        CouplingOutputFormat::Json => coupling::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(Vec::new())
}

async fn run_review(arguments: &ReviewArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        // As coupling's: the coupling warnings read the history, so
        // URL runs need the full clone, not a depth-1 one.
        true,
    )
    .await?;
    let progress = CliProgress::start("Reviewing the change...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    // The change and the history first, for the reason `coupling`
    // reads its history first: both errors are about git, and they
    // should arrive before the graph build spends its time.
    let against = arguments.against.as_deref();
    let changed =
        changed_files(&source.repo_root, against).with_context(|| {
            format!(
                "review reads the working tree of {} against {}, and \
                 git could not make the comparison",
                source.repo_root.display(),
                against.unwrap_or("HEAD")
            )
        })?;
    let commits = recent_commit_files(&source.repo_root, arguments.commits)
        .with_context(|| {
            format!(
                "review reads the git history of {} for its coupling \
                 warnings, and none could be read",
                source.repo_root.display()
            )
        })?;

    progress.set_message("Extracting imports...");
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        None,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: false,
        },
        &CliReporter(&progress),
    )?;

    let base_prefix = history_base_prefix(&source);
    let rebased_commits = coupling::rebase_commit_paths(&commits, &base_prefix);
    let rebased_changed = review::rebase_changed_paths(&changed, &base_prefix);
    let measured = review::measure_review(
        &report,
        &rebased_changed,
        &rebased_commits,
        review::ReviewOptions {
            depth: None,
            coupling: coupling::CouplingOptions {
                min_co_changes: arguments.min_co_changes,
                ..coupling::CouplingOptions::default()
            },
        },
    );

    let against_label = against.unwrap_or("HEAD");
    // The fourth review question, composed rather than measured
    // inside review: what broke by name between the ref and the
    // working tree, computed exactly the way `breaking` computes it
    // when --to is absent. The ref side lives in a detached
    // worktree so the working tree is never disturbed, and the new
    // side is a fresh build over the analysis directory — the
    // focus-filtered report above cannot serve as it, because
    // breaking assumes both sides share one basis. Two extra
    // builds is the stated cost of the answer.
    progress.set_message("Checking for broken symbols...");
    let breaking =
        measure_review_breaking(&source, arguments, against_label, &ignore)?;

    let rendered = match arguments.format {
        ReviewOutputFormat::Markdown => {
            review::render_markdown(&measured, against_label, &breaking)
        }
        ReviewOutputFormat::Json => {
            review::render_json(&measured, against_label, &breaking)
        }
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(Vec::new())
}

async fn run_risk(arguments: &RiskArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        // As coupling's: churn and coupling read the history, so
        // URL runs need the full clone, not a depth-1 one.
        true,
    )
    .await?;
    let progress = CliProgress::start("Ranking risk...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    // History first, as `coupling` reads it: a tree with no readable
    // history is an error this command exists to state, and it
    // should arrive before the graph build spends its time.
    let commits = recent_commit_files(&source.repo_root, arguments.commits)
        .with_context(|| {
            format!(
                "risk reads the git history of {}, and none could be read",
                source.repo_root.display()
            )
        })?;

    progress.set_message("Extracting imports...");
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        None,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: false,
        },
        &CliReporter(&progress),
    )?;

    let base_prefix = history_base_prefix(&source);
    let rebased = coupling::rebase_commit_paths(&commits, &base_prefix);
    let measured = risk::measure_risk(
        &report,
        &rebased,
        risk::RiskOptions {
            coupling: coupling::CouplingOptions {
                min_co_changes: arguments.min_co_changes,
                ..coupling::CouplingOptions::default()
            },
            limit: usize::try_from(arguments.limit).unwrap_or(20),
        },
    );

    let rendered = match arguments.format {
        RiskOutputFormat::Markdown => risk::render_markdown(&measured),
        RiskOutputFormat::Json => risk::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(Vec::new())
}

async fn run_dead(arguments: &DeadArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Finding dead files...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        None,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: false,
        },
        &CliReporter(&progress),
    )?;

    // A named root that matches nothing is a typo in the one input
    // the whole verdict rests on; refuse it the way every other
    // command refuses a path that matches no file.
    let mut extra_roots = Vec::new();
    for raw in &arguments.root {
        let matched =
            impact::match_path(&report, raw, "").with_context(|| {
                format!("no file in the analysis matches `{raw}`")
            })?;
        extra_roots.push(matched);
    }

    let measured = dead::measure_dead(&report, &extra_roots);

    let rendered = match arguments.format {
        DeadOutputFormat::Markdown => dead::render_markdown(&measured),
        DeadOutputFormat::Json => dead::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(Vec::new())
}

/// The repo-relative prefix SARIF URIs need for one analysis directory.
///
/// The repo-relative path of the analysis directory: empty when
/// the analysis directory IS the repo root, the subdirectory
/// otherwise, and empty again when the analysis directory is not
/// under the repo root at all, which keeps the old
/// analysis-relative spelling rather than inventing a path.
///
/// The two fields of `ResolvedSource` do not answer this on their
/// own: for a local path, `resolve_source` sets `repo_root` equal to
/// `analysis_path`, so stripping one from the other is empty even
/// inside a repository. The strip below therefore only covers the
/// remote-checkout case, where the two genuinely differ; for local
/// paths the work-tree root comes from git, with the analysis
/// directory canonicalised first so a relative `--path` or a path
/// through a symlink still strips cleanly.
///
/// GitHub resolves SARIF URIs from the repository root, while a
/// report's file paths are relative to the analysis directory, so
/// a run over a subdirectory must carry this prefix and a root run
/// must carry none. The same prefix also scopes a comparison's old
/// side to `--path` (see `build_old_side`): both uses need the
/// directory the analysis sits under, not a rendering detail.
#[must_use]
fn analysis_prefix(source: &ResolvedSource) -> PathBuf {
    let relative =
        sarif::relative_path(&source.analysis_path, &source.repo_root);
    if !relative.as_os_str().is_empty() {
        return relative;
    }
    let canonical = std::fs::canonicalize(&source.analysis_path)
        .unwrap_or_else(|_| source.analysis_path.clone());
    let Some(toplevel) = repo_toplevel(&source.analysis_path) else {
        return PathBuf::new();
    };
    sarif::relative_path(&canonical, &toplevel)
}

/// The `/`-separated repo-relative prefix history spellings carry
/// but graph spellings do not, for rebasing git paths onto the
/// graph's spelling.
///
/// `base_prefix_for` cannot answer this for a local path:
/// `resolve_source` sets `repo_root` equal to `analysis_path`, so
/// stripping one from the other is empty even inside a repository,
/// while git spells history from the work-tree root. A `--path pkg`
/// run therefore compared root-relative history (`pkg/a.py`) against
/// base-relative graph paths (`a.py`) and found nothing: coupling
/// empty, risk churn zero, test-priority churn zero, and review
/// changed files reported absent. `analysis_prefix` asks git for
/// the work-tree root, so the rebase strips the real prefix.
#[must_use]
fn history_base_prefix(source: &ResolvedSource) -> String {
    analysis_prefix(source)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// SARIF's spelling of `analysis_prefix`: `/`-separated, for the
/// URI joins in `sarif::prefixed_path`.
#[must_use]
fn sarif_repo_prefix(source: &ResolvedSource) -> String {
    let stripped = sarif::path_prefix(&source.analysis_path, &source.repo_root);
    if !stripped.is_empty() {
        return stripped;
    }
    let canonical = std::fs::canonicalize(&source.analysis_path)
        .unwrap_or_else(|_| source.analysis_path.clone());
    let Some(toplevel) = repo_toplevel(&source.analysis_path) else {
        return String::new();
    };
    sarif::path_prefix(&canonical, &toplevel)
}

/// The repo-relative spelling of an archaeology file argument.
///
/// The help promises the file is spelled relative to the analysis
/// root, but the snapshots are whole-tree builds whose index spells
/// files from the repository root, and the commit log is a git
/// pathspec: the two providers only agree when the file reaches
/// them repo-relative. A scoped run therefore has two spellings
/// that must both work — the analysis-relative one the help
/// promises (`lib.py` under `--path pkg`) and the root-relative
/// one root runs already use (`pkg/lib.py`) — and a third shape
/// that must not silently degrade: a root-relative spelling under
/// a scoped run used to make the log query run from the analysis
/// directory, where the pathspec matched nothing, so the walk fell
/// back to visiting every parent and the window counted commits
/// that never touched the file. Normalising here, and running the
/// git providers from the work-tree root, makes every spelling
/// take the file-log walk.
///
/// The one spelling this cannot disambiguate is an
/// analysis-relative file whose spelling already starts with the
/// analysis prefix (`pkg/lib.py` meaning `pkg/pkg/lib.py` under
/// `--path pkg`): the root-relative reading wins, because it is
/// the spelling root runs already established.
#[must_use]
fn normalize_archaeology_file(source: &ResolvedSource, file: &str) -> String {
    let mut current = file;
    while let Some(rest) = current.strip_prefix("./") {
        current = rest;
    }
    let mut path = PathBuf::from(current);
    if path.is_absolute() {
        if let Some(toplevel) = repo_toplevel(&source.analysis_path)
            && let Ok(relative) = path.strip_prefix(&toplevel)
        {
            path = relative.to_path_buf();
        }
        return path.to_string_lossy().replace('\\', "/");
    }
    let prefix = analysis_prefix(source);
    if prefix.as_os_str().is_empty()
        || path == prefix
        || path.starts_with(&prefix)
    {
        return path.to_string_lossy().replace('\\', "/");
    }
    prefix.join(path).to_string_lossy().replace('\\', "/")
}

/// The graph's spelling of a file inside one analysed tree.
///
/// The graph spells files relative to the analysis root, so a
/// `./`-prefixed or absolute spelling of a file that is in the
/// tree names a declaration the exact-match lookup then misses,
/// and the error claims the file declares nothing — wrong about
/// a file that does. When the spelling resolves to a real file
/// under the analysis root, it is rewritten to that relative
/// spelling; anything else passes through untouched, so a missing
/// file still errors exactly as before.
#[must_use]
fn normalize_tree_file(analysis_path: &Path, file: &str) -> String {
    let candidate = Path::new(file);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        analysis_path.join(candidate)
    };
    if let (Ok(canonical), Ok(base)) = (
        std::fs::canonicalize(&absolute),
        std::fs::canonicalize(analysis_path),
    ) && let Ok(relative) = canonical.strip_prefix(&base)
    {
        return relative.to_string_lossy().replace('\\', "/");
    }
    let mut stripped = file;
    while let Some(rest) = stripped.strip_prefix("./") {
        stripped = rest;
    }
    stripped.replace('\\', "/")
}

/// The base the old side of a comparison is built from.
///
/// When `scoped` is set, the build is restricted to the analysis
/// directory inside the checkout, so its file paths share the new
/// side's basis — the comparison `--path` scopes. Unscoped, the
/// whole checkout is built, which is the `--to` shape. An empty
/// prefix leaves the checkout untouched either way, so root runs
/// behave exactly as before.
#[must_use]
fn old_side_base(
    old_worktree_path: &Path,
    source: &ResolvedSource,
    scoped: bool,
) -> PathBuf {
    if scoped {
        old_worktree_path.join(analysis_prefix(source))
    } else {
        old_worktree_path.to_path_buf()
    }
}

/// Build the old side of a comparison from its checkout worktree,
/// with its own side's ignore matcher built from the build base —
/// the breaking command's per-side rule.
///
/// A scope that did not exist at the old ref is an empty side, not
/// an error: everything in it is new, so nothing in it broke. The
/// caller never sees a fabricated directory for it; the empty
/// report carries the scope as its base path and no nodes at all.
fn build_old_side(
    old_worktree_path: &Path,
    source: &ResolvedSource,
    scoped: bool,
    settings: &SettingsArgs,
    ignore_patterns: &[String],
    no_gitignore: bool,
) -> Result<(GraphReport, DeclarationIndex)> {
    let base = old_side_base(old_worktree_path, source, scoped);
    let ignore =
        build_ignore_matcher(&base, settings, ignore_patterns, no_gitignore)?;
    if base.is_dir() {
        build_graph_with_declarations(&base, &ignore)
    } else {
        Ok((
            GraphReport {
                base_path: base,
                ..Default::default()
            },
            DeclarationIndex::default(),
        ))
    }
}

async fn run_breaking(arguments: &BreakingArgs) -> Result<Vec<Gate>> {
    // A remote source has no two local refs to compare — its checkout
    // is one ref already — so breaking refuses it plainly rather than
    // comparing a clone against a worktree of itself.
    if arguments.source.url.is_some() {
        anyhow::bail!(
            "breaking compares two refs of a local repository; \
             --url is not supported for it"
        );
    }
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Comparing the two refs...");

    // The older side always lives in a worktree: its tree must not
    // move while the newer side is built. The newer side is the
    // working tree when --to is absent, and a second worktree
    // otherwise, so the working tree is never disturbed either way.
    // Without --to the old side is scoped to --path, like the new
    // side; with --to both sides are whole-tree checkouts.
    let old_worktree = checkout_worktree(&source.repo_root, &arguments.from)
        .with_context(|| {
            format!(
                "breaking checks `{}` out of {} and git could not do it",
                arguments.from,
                source.repo_root.display()
            )
        })?;
    progress.set_message("Extracting the older side...");
    let (old_report, old_index) = build_old_side(
        old_worktree.path(),
        &source,
        arguments.to.is_none(),
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    let to_label = arguments.to.as_deref().unwrap_or("working tree");
    let (new_report, new_index) =
        build_new_side(&source, arguments, &progress)?;
    drop(old_worktree);

    let measured = breaking::measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        &arguments.from,
        to_label,
    );

    // The SARIF prefix, per side: both sides share one basis, so
    // both locations take the same prefix — the analysis prefix
    // when --to is absent (the old side is scoped, like the new
    // side), empty with --to (both sides are whole-tree checkouts,
    // so all paths are root-relative already). Passing the prefix
    // unconditionally would double-prefix the subdir + --to runs.
    // The two prefixes are passed separately because the locations
    // come from different sides: a future change to one side's
    // basis must not silently corrupt the other's URIs.
    let consumer_prefix = if arguments.to.is_some() {
        String::new()
    } else {
        sarif_repo_prefix(&source)
    };

    let rendered = match arguments.format {
        BreakingOutputFormat::Markdown => breaking::render_markdown(&measured),
        BreakingOutputFormat::Json => breaking::render_json(&measured),
        BreakingOutputFormat::Sarif => breaking::render_sarif(
            &measured,
            &consumer_prefix,
            &consumer_prefix,
        ),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    // A break nobody uses does not fail the run: it is the clean kind
    // of breaking, and a gate that fired on it would train a pipeline
    // to ignore the gate.
    let gates = arguments.fail_on.map_or_else(Vec::new, |limit| {
        let with_consumers = u64::try_from(
            measured
                .broken
                .iter()
                .filter(|broken| !broken.direct.is_empty())
                .count(),
        )
        .unwrap_or(u64::MAX);
        if with_consumers >= limit {
            vec![Gate::new(
                "broken symbols with direct consumers",
                with_consumers,
                limit,
            )]
        } else {
            Vec::new()
        }
    });
    Ok(gates)
}

/// Build one side of a three-way comparison in a worktree of its
/// own: the side's tree must not move while the other sides are
/// built, and the working tree is never disturbed either way.
fn build_side_worktree(
    source: &ResolvedSource,
    arguments: &MergeArgs,
    reference: &str,
) -> Result<(GraphReport, DeclarationIndex)> {
    let worktree = checkout_worktree(&source.repo_root, reference)
        .with_context(|| {
            format!(
                "merge checks `{reference}` out of {} and git could not do it",
                source.repo_root.display()
            )
        })?;
    let ignore = build_ignore_matcher(
        worktree.path(),
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    build_graph_with_declarations(worktree.path(), &ignore)
}

async fn run_merge(arguments: &MergeArgs) -> Result<Vec<Gate>> {
    // A remote source has no three local refs to compare — its
    // checkout is one ref already — so merge refuses it plainly
    // rather than comparing a clone against worktrees of itself.
    if arguments.source.url.is_some() {
        anyhow::bail!(
            "merge compares three refs of a local repository; \
             --url is not supported for it"
        );
    }
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Comparing the three sides...");

    // The fork point is the merge base when the caller does not
    // name one: it is where the two sides diverged, so the two
    // diffs judged are the ones whose interaction matters.
    let base_ref = match &arguments.base {
        Some(base) => base.clone(),
        None => {
            merge_base(&source.repo_root, &arguments.ours, &arguments.theirs)
                .context(
                    "merge could not find the fork point of the two sides",
                )?
        }
    };

    progress.set_message("Extracting the base tree...");
    let (base_report, base_index) =
        build_side_worktree(&source, arguments, &base_ref)?;
    progress.set_message("Extracting the ours tree...");
    let (ours_report, ours_index) =
        build_side_worktree(&source, arguments, &arguments.ours)?;
    progress.set_message("Extracting the theirs tree...");
    let (theirs_report, theirs_index) =
        build_side_worktree(&source, arguments, &arguments.theirs)?;

    let measured = merge::measure_merge(
        &MergeSideAnalysis {
            index: &base_index,
            report: &base_report,
            ref_name: &base_ref,
        },
        &MergeSideAnalysis {
            index: &ours_index,
            report: &ours_report,
            ref_name: &arguments.ours,
        },
        &MergeSideAnalysis {
            index: &theirs_index,
            report: &theirs_report,
            ref_name: &arguments.theirs,
        },
    );

    let rendered = match arguments.format {
        MergeOutputFormat::Markdown => merge::render_markdown(&measured),
        MergeOutputFormat::Json => merge::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    // Only proven conflicts fail the run: a warning is
    // suspicious, not proven, and a gate that fired on it would
    // train a pipeline to ignore the gate.
    let gates = arguments.fail_on.map_or_else(Vec::new, |limit| {
        let proven = u64::try_from(
            measured
                .conflicts
                .iter()
                .filter(|conflict| conflict.kind.is_proven())
                .count(),
        )
        .unwrap_or(u64::MAX);
        if proven >= limit {
            vec![Gate::new("proven merge conflicts", proven, limit)]
        } else {
            Vec::new()
        }
    });
    Ok(gates)
}

async fn run_diff(arguments: &DiffArgs) -> Result<Vec<Gate>> {
    // A remote source has no two local refs to compare — its checkout
    // is one ref already — so diff refuses it plainly rather than
    // comparing a clone against a worktree of itself.
    if arguments.source.url.is_some() {
        anyhow::bail!(
            "diff compares two refs of a local repository; \
             --url is not supported for it"
        );
    }
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Comparing the two refs...");

    // The older side always lives in a worktree: its tree must not
    // move while the newer side is built. The newer side is the
    // working tree when --to is absent, and a second worktree
    // otherwise, so the working tree is never disturbed either way.
    // Without --to the old side is scoped to --path, like the new
    // side and like breaking's old side: building it whole-tree
    // while the new side is scoped compares root-relative paths
    // against subdir-relative ones, and every scoped symbol reads
    // as removed plus added. With --to both sides are whole-tree
    // checkouts, the same rule breaking documents.
    let old_worktree = checkout_worktree(&source.repo_root, &arguments.from)
        .with_context(|| {
            format!(
                "diff checks `{}` out of {} and git could not do it",
                arguments.from,
                source.repo_root.display()
            )
        })?;
    progress.set_message("Extracting the older side...");
    let (_, old_index) = build_old_side(
        old_worktree.path(),
        &source,
        arguments.to.is_none(),
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    drop(old_worktree);

    let to_label = arguments.to.as_deref().unwrap_or("working tree");
    let (_, new_index) = if let Some(to) = &arguments.to {
        let new_worktree = checkout_worktree(&source.repo_root, to)
            .with_context(|| {
                format!(
                    "diff checks `{to}` out of {} and git could not do it",
                    source.repo_root.display()
                )
            })?;
        let new_ignore = build_ignore_matcher(
            new_worktree.path(),
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        progress.set_message("Extracting the newer side...");
        let built =
            build_graph_with_declarations(new_worktree.path(), &new_ignore)?;
        drop(new_worktree);
        built
    } else {
        let new_ignore = build_ignore_matcher(
            &source.analysis_path,
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        progress.set_message("Extracting the working tree...");
        build_graph_with_declarations(&source.analysis_path, &new_ignore)?
    };

    let measured =
        diff::measure_diff(&old_index, &new_index, &arguments.from, to_label);

    let rendered = match arguments.format {
        DiffOutputFormat::Markdown => diff::render_markdown(&measured),
        DiffOutputFormat::Json => diff::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    // A diff is a report, not a gate: additions are the normal shape
    // of a release, so no count of changes fails the run.
    Ok(Vec::new())
}

/// The commit-log provider for archaeology: the commits touching
/// a file from a ref, newest first, as graph history commits.
fn archaeology_log(
    repo_root: &Path,
    from_ref: &str,
    file: &str,
) -> Result<Vec<archaeology::HistoryCommit>, archaeology::ArchaeologyError> {
    log_commits_for_file(repo_root, from_ref, file)
        .map(|commits| {
            commits
                .into_iter()
                .map(|meta| archaeology::HistoryCommit {
                    hash: meta.hash,
                    author: meta.author,
                    date: meta.date,
                    message: meta.message,
                })
                .collect()
        })
        .map_err(|error| {
            archaeology::ArchaeologyError::History(error.to_string())
        })
}

/// The build provider for archaeology: one commit's tree, built in
/// a detached worktree that is removed when the build returns.
fn archaeology_build(
    repo_root: &Path,
    settings: &SettingsArgs,
    ignore_args: &IgnoreArgs,
    commit: &archaeology::HistoryCommit,
) -> Result<archaeology::HistorySnapshot, archaeology::ArchaeologyError> {
    let worktree =
        checkout_worktree(repo_root, &commit.hash).map_err(|error| {
            archaeology::ArchaeologyError::Build(error.to_string())
        })?;
    let ignore = build_ignore_matcher(
        worktree.path(),
        settings,
        &ignore_args.ignore,
        ignore_args.no_gitignore,
    )
    .map_err(|error| archaeology::ArchaeologyError::Build(error.to_string()))?;
    let (report, index) =
        build_graph_with_declarations(worktree.path(), &ignore).map_err(
            |error| archaeology::ArchaeologyError::Build(error.to_string()),
        )?;
    Ok(archaeology::HistorySnapshot {
        commit: commit.clone(),
        index,
        report,
    })
}

/// The parent provider for archaeology: a commit's first-parent
/// parent, as a graph history commit.
fn archaeology_parent(
    repo_root: &Path,
    hash: &str,
) -> Result<Option<archaeology::HistoryCommit>, archaeology::ArchaeologyError> {
    first_parent_commit(repo_root, hash)
        .map(|parent| {
            parent.map(|meta| archaeology::HistoryCommit {
                hash: meta.hash,
                author: meta.author,
                date: meta.date,
                message: meta.message,
            })
        })
        .map_err(|error| {
            archaeology::ArchaeologyError::History(error.to_string())
        })
}

async fn run_archaeology(arguments: &ArchaeologyArgs) -> Result<Vec<Gate>> {
    // One local repository's history, like diff and behavior: a
    // remote source has no local refs to walk.
    if arguments.source.url.is_some() {
        anyhow::bail!(
            "archaeology traces one local repository's history; \
             --url is not supported for it"
        );
    }
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Tracing the symbol through history...");

    // The git providers run from the work-tree root and the file
    // reaches them repo-relative (see `normalize_archaeology_file`):
    // the providers only agree on one basis, and the root is the
    // one the whole-tree snapshots already use. Outside a
    // repository the root falls back to the analysis path, and the
    // first git call fails as before.
    let file = normalize_archaeology_file(&source, &arguments.file);
    let repo_root =
        repo_toplevel(&source.analysis_path).unwrap_or(source.repo_root);

    // The providers trace_symbol walks on, as thin closures over
    // the named functions below: the commit log, one commit's
    // built tree, and the first-parent parent.
    let log = |from_ref: &str, file: &str| {
        archaeology_log(&repo_root, from_ref, file)
    };
    let build = |commit: &archaeology::HistoryCommit| {
        archaeology_build(
            &repo_root,
            &arguments.settings,
            &arguments.ignore_args,
            commit,
        )
    };
    let parent = |hash: &str| archaeology_parent(&repo_root, hash);

    let start =
        commit_meta(&repo_root, &arguments.start_ref).with_context(|| {
            format!(
                "archaeology resolves `{}` and git could not do it",
                arguments.start_ref
            )
        })?;
    let start = archaeology::HistoryCommit {
        hash: start.hash,
        author: start.author,
        date: start.date,
        message: start.message,
    };
    let measured = archaeology::trace_symbol(
        &start,
        &file,
        &arguments.symbol,
        arguments.window,
        &log,
        &build,
        &parent,
    )
    .map_err(|error| match error {
        archaeology::ArchaeologyError::SymbolNotFound => anyhow::anyhow!(
            "`{}` is not declared in `{}` at `{}`",
            arguments.symbol,
            arguments.file,
            arguments.start_ref
        ),
        other => anyhow::Error::new(other),
    })?;

    let rendered = match arguments.format {
        ArchaeologyOutputFormat::Markdown => {
            archaeology::render_markdown(&measured)
        }
        ArchaeologyOutputFormat::Json => archaeology::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    // A trail is a report, not a gate.
    Ok(Vec::new())
}

async fn run_behavior(arguments: &BehaviorArgs) -> Result<Vec<Gate>> {
    // Two refs of a local repository, like diff: a remote source
    // has no two local refs to compare.
    if arguments.source.url.is_some() {
        anyhow::bail!(
            "behavior compares two refs of a local repository; \
             --url is not supported for it"
        );
    }
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Comparing the two refs...");

    let old_worktree = checkout_worktree(&source.repo_root, &arguments.from)
        .with_context(|| {
            format!(
                "behavior checks `{}` out of {} and git could not do it",
                arguments.from,
                source.repo_root.display()
            )
        })?;
    // Without --to the old side is scoped to --path, like the new
    // side and like breaking's old side: call sites and constants
    // are keyed by file, so a whole-tree old side against a scoped
    // new side pairs nothing and silently reports no changes.
    // The facts come from the same scoped base the index does, so
    // their file spellings line up. With --to both sides are
    // whole-tree checkouts, the same rule breaking documents.
    let scoped = arguments.to.is_none();
    progress.set_message("Extracting the older side...");
    let (_, old_index) = build_old_side(
        old_worktree.path(),
        &source,
        scoped,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    let old_base = old_side_base(old_worktree.path(), &source, scoped);
    let old_ignore = build_ignore_matcher(
        &old_base,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    let old_facts = behavior::collect_facts(&old_base, &old_ignore);
    drop(old_worktree);

    let to_label = arguments.to.as_deref().unwrap_or("working tree");
    let (new_report, new_index, new_facts) = if let Some(to) = &arguments.to {
        let new_worktree = checkout_worktree(&source.repo_root, to)
            .with_context(|| {
                format!(
                    "behavior checks `{to}` out of {} and git could not do it",
                    source.repo_root.display()
                )
            })?;
        let new_ignore = build_ignore_matcher(
            new_worktree.path(),
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        progress.set_message("Extracting the newer side...");
        let built =
            build_graph_with_declarations(new_worktree.path(), &new_ignore)?;
        let facts = behavior::collect_facts(new_worktree.path(), &new_ignore);
        drop(new_worktree);
        (built.0, built.1, facts)
    } else {
        let new_ignore = build_ignore_matcher(
            &source.analysis_path,
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        progress.set_message("Extracting the working tree...");
        let built =
            build_graph_with_declarations(&source.analysis_path, &new_ignore)?;
        let facts = behavior::collect_facts(&source.analysis_path, &new_ignore);
        (built.0, built.1, facts)
    };

    let measured = behavior::measure_behavior(
        &old_index,
        &new_index,
        &new_report,
        &old_facts,
        &new_facts,
        &arguments.from,
        to_label,
    );

    let rendered = match arguments.format {
        BehaviorOutputFormat::Markdown => behavior::render_markdown(&measured),
        BehaviorOutputFormat::Json => behavior::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    // A behaviour scan is a report, not a gate: its precision is
    // measured by its corpus, and a gate on an unproven detector
    // would punish asking.
    Ok(Vec::new())
}

async fn run_simulate(arguments: &SimulateArgs) -> Result<Vec<Gate>> {
    // One tree, not two refs, and no git requirement: the rename is
    // simulated against the tree as it stands, so a remote source
    // is as good a tree as a local one.
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Simulating the rename...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    progress.set_message("Extracting the tree...");
    let (report, index) =
        build_graph_with_declarations(&source.analysis_path, &ignore)?;

    // A simulated rename that names nothing is a typo, not a
    // report: the measurement errors, and the error names the file
    // and symbol. The file spellings are normalised to the
    // graph's first, so a `./`-prefixed or absolute spelling of a
    // file that is in the tree is not misread as naming nothing.
    let file = normalize_tree_file(&source.analysis_path, &arguments.file);
    let to_file = arguments
        .to_file
        .as_deref()
        .map(|to| normalize_tree_file(&source.analysis_path, to));
    let measured = simulate::measure_simulate(
        &index,
        &report,
        &file,
        &arguments.symbol,
        &arguments.to,
        to_file.as_deref(),
    )?;

    let rendered = match arguments.format {
        SimulateOutputFormat::Markdown => simulate::render_markdown(&measured),
        SimulateOutputFormat::Json => simulate::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    // A simulation is a report, not a gate: even a blocked rename
    // is an answer, and failing the run on it would punish asking.
    Ok(Vec::new())
}

async fn run_untested(arguments: &UntestedArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Finding untested symbols...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    let (report, index) =
        build_graph_with_declarations(&source.analysis_path, &ignore)?;
    let measured = untested::measure_untested(&report, &index);

    // The whole report comes from one tree rooted at the analysis
    // directory, so every location takes the same repo-relative
    // prefix — empty for root runs, the subdirectory otherwise.
    let prefix = sarif_repo_prefix(&source);

    let rendered = match arguments.format {
        UntestedOutputFormat::Markdown => untested::render_markdown(&measured),
        UntestedOutputFormat::Json => untested::render_json(&measured),
        UntestedOutputFormat::Sarif => {
            untested::render_sarif(&measured, &prefix)
        }
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    let gates = arguments.fail_on.map_or_else(Vec::new, |limit| {
        let count = u64::try_from(measured.untested.len()).unwrap_or(u64::MAX);
        if count >= limit {
            vec![Gate::new("untested symbols", count, limit)]
        } else {
            Vec::new()
        }
    });
    Ok(gates)
}

async fn run_test_priority(arguments: &TestPriorityArgs) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        // As coupling's: churn and coupling read the history, so
        // URL runs need the full clone, not a depth-1 one.
        true,
    )
    .await?;
    let progress = CliProgress::start("Ranking untested symbols...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    // History first, as `risk` reads it: a tree with no readable
    // history is an error this command exists to state, and it
    // should arrive before the graph build spends its time.
    let commits = recent_commit_files(&source.repo_root, arguments.window)
        .with_context(|| {
            format!(
                "test-priority reads the git history of {}, and none could be read",
                source.repo_root.display()
            )
        })?;

    let (report, index) =
        build_graph_with_declarations(&source.analysis_path, &ignore)?;

    let base_prefix = history_base_prefix(&source);
    let rebased = coupling::rebase_commit_paths(&commits, &base_prefix);
    let measured = test_priority::measure_test_priority(
        &report,
        &index,
        &rebased,
        test_priority::TestPriorityOptions {
            coupling: coupling::CouplingOptions::default(),
            limit: arguments
                .top
                .map(|top| usize::try_from(top).unwrap_or(usize::MAX)),
        },
    );

    let rendered = match arguments.format {
        TestPriorityOutputFormat::Markdown => {
            test_priority::render_markdown(&measured)
        }
        TestPriorityOutputFormat::Json => test_priority::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(Vec::new())
}

/// Build the newer side of a breaking comparison.
///
/// A named `--to` is checked out into a worktree; without one, the
/// newer side is the working tree itself. Either way the returned
/// report and index are plain data, so the worktree can drop before
/// the measurement runs.
/// The breaking side of a review: the ref in a detached worktree
/// against the working tree, the ref side scoped to the analysis
/// directory like the working-tree side, with its own side's
/// ignore matcher — the breaking command's per-side rule, not one
/// matcher shared across sides. A review has no `--to`, so its
/// breaking section is always the scoped shape.
fn measure_review_breaking(
    source: &ResolvedSource,
    arguments: &ReviewArgs,
    against_label: &str,
    ignore: &IgnoreMatcher,
) -> Result<breaking::BreakingReport> {
    let old_worktree = checkout_worktree(&source.repo_root, against_label)
        .with_context(|| {
            format!(
                "review checks `{against_label}` out of {} and git could not do it",
                source.repo_root.display()
            )
        })?;
    let (old_report, old_index) = build_old_side(
        old_worktree.path(),
        source,
        true,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    let (new_report, new_index) =
        build_graph_with_declarations(&source.analysis_path, ignore)?;
    drop(old_worktree);
    Ok(breaking::measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        against_label,
        "working tree",
    ))
}

/// Build one side's schema index from a base directory, with the
/// side's own ignore matcher: the schema walk respects the same
/// ignore rules as the code graph, so a proto file the graph ignores
/// is not a schema file either.
fn build_schema_side(
    base: &std::path::Path,
    settings: &SettingsArgs,
    ignore_patterns: &[String],
    no_gitignore: bool,
) -> Result<sephera_graph::schema::SchemaIndex> {
    let ignore =
        build_ignore_matcher(base, settings, ignore_patterns, no_gitignore)?;
    let files = sephera_graph::schema::collect_schema_files(base, &ignore)
        .map_err(anyhow::Error::new)?;
    sephera_graph::schema::build_schema_index(&files)
        .map_err(anyhow::Error::new)
}

/// Build one side's consumer texts (Go/Python files) from a base
/// directory, with the side's own ignore matcher.
fn build_consumer_texts_side(
    base: &std::path::Path,
    settings: &SettingsArgs,
    ignore_patterns: &[String],
    no_gitignore: bool,
) -> Result<Vec<(String, String)>> {
    let ignore =
        build_ignore_matcher(base, settings, ignore_patterns, no_gitignore)?;
    sephera_graph::schema::collect_consumer_texts(base, &ignore)
        .map_err(anyhow::Error::new)
}

/// One side of a schema comparison: its graph, its schema index,
/// and its consumer texts.
type SchemaSide = (
    GraphReport,
    sephera_graph::schema::SchemaIndex,
    Vec<(String, String)>,
);

/// Build the newer side of a schema comparison from one base
/// directory: its graph, its schema index, and its consumer texts.
fn build_new_schema_side(
    base: &Path,
    settings: &SettingsArgs,
    ignore_patterns: &[String],
    no_gitignore: bool,
) -> Result<SchemaSide> {
    // The schema measurement takes graph reports and schema indexes
    // only, never a declaration index, so the plain graph build is
    // the whole input: building declarations here would be work the
    // caller throws away.
    let matcher =
        build_ignore_matcher(base, settings, ignore_patterns, no_gitignore)?;
    Ok((
        build_graph(base, &matcher, &[], None, None)?,
        build_schema_side(base, settings, ignore_patterns, no_gitignore)?,
        build_consumer_texts_side(
            base,
            settings,
            ignore_patterns,
            no_gitignore,
        )?,
    ))
}

/// Only proven consumers fail the gate: a break nobody uses does
/// not fail the run, and a break with only name-match consumers
/// never fails it.
/// Fail before the repository check when `--path` names something that
/// is not a directory at all.
///
/// A file inside a repository used to be reported as "not inside
/// one", and a nonexistent path the same, because `repo_toplevel`
/// answers `None` for both. The single-tree commands fail this way
/// through `sephera_scan`'s project-file validation; this command
/// checks the same thing in the same words before asking about the
/// repo, and the MCP tool mirrors it so the two cannot disagree.
fn ensure_schema_analysis_dir(source: &ResolvedSource) -> Result<()> {
    if source.analysis_path.is_dir() {
        return Ok(());
    }
    if !source.analysis_path.exists() {
        anyhow::bail!(
            "path `{}` does not exist",
            source.analysis_path.display()
        );
    }
    anyhow::bail!(
        "path `{}` is not a directory",
        source.analysis_path.display()
    );
}

fn schema_impact_gates(
    measured: &schema_impact::SchemaImpactReport,
    fail_on: Option<u64>,
) -> Vec<Gate> {
    fail_on.map_or_else(Vec::new, |limit| {
        let with_proven = u64::try_from(
            measured
                .breaks
                .iter()
                .filter(|broken| !broken.proven.is_empty())
                .count(),
        )
        .unwrap_or(u64::MAX);
        if with_proven >= limit {
            vec![Gate::new(
                "schema breaks with proven consumers",
                with_proven,
                limit,
            )]
        } else {
            Vec::new()
        }
    })
}

async fn run_schema_impact(arguments: &SchemaImpactArgs) -> Result<Vec<Gate>> {
    // A remote source has no two local refs to compare — its checkout
    // is one ref already — so schema-impact refuses it plainly,
    // exactly as breaking does.
    if arguments.source.url.is_some() {
        anyhow::bail!(
            "schema-impact compares two refs of a local repository; \
             --url is not supported for it"
        );
    }
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    // A path that is not a directory at all must say so, rather than
    // falling through to the repository check below; the check lives
    // in its own function so this already-long function stays under
    // clippy's line limit.
    ensure_schema_analysis_dir(&source)?;
    // A tree that is not a git repository has no refs to compare:
    // an error, never an empty report.
    if repo_toplevel(&source.analysis_path).is_none() {
        anyhow::bail!(
            "schema-impact needs a git repository; {} is not inside one",
            source.analysis_path.display()
        );
    }
    let progress = CliProgress::start("Comparing the two schemas...");

    // The older side always lives in a worktree, scoped to --path
    // when --to is absent (like breaking's old side) and whole-tree
    // otherwise. The schema index is built from the same scoped base
    // as the old graph, so both sides share one path basis.
    let old_worktree = checkout_worktree(&source.repo_root, &arguments.from)
        .with_context(|| {
            format!(
                "schema-impact checks `{}` out of {} and git could not do it",
                arguments.from,
                source.repo_root.display()
            )
        })?;
    progress.set_message("Extracting the older side...");
    let (old_report, _old_index) = build_old_side(
        old_worktree.path(),
        &source,
        arguments.to.is_none(),
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    let old_base =
        old_side_base(old_worktree.path(), &source, arguments.to.is_none());
    let old_schema = build_schema_side(
        &old_base,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    let to_label = arguments.to.as_deref().unwrap_or("working tree");
    let (new_report, new_schema, new_texts) = if let Some(to) = &arguments.to {
        let new_worktree = checkout_worktree(&source.repo_root, to)
            .with_context(|| {
                format!(
                    "schema-impact checks `{to}` out of {} and git could not do it",
                    source.repo_root.display()
                )
            })?;
        progress.set_message("Extracting the newer side...");
        let built = build_new_schema_side(
            new_worktree.path(),
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        drop(new_worktree);
        built
    } else {
        progress.set_message("Extracting the working tree...");
        build_new_schema_side(
            &source.analysis_path,
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?
    };
    drop(old_worktree);

    let measured = schema_impact::measure_schema_impact(
        &old_schema,
        &new_schema,
        &old_report,
        &new_report,
        &new_texts,
        &arguments.from,
        to_label,
    );

    let rendered = match arguments.format {
        SchemaImpactOutputFormat::Markdown => {
            schema_impact::render_markdown(&measured)
        }
        SchemaImpactOutputFormat::Json => schema_impact::render_json(&measured),
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    let gates = schema_impact_gates(&measured, arguments.fail_on);
    Ok(gates)
}

fn build_new_side(
    source: &ResolvedSource,
    arguments: &BreakingArgs,
    progress: &CliProgress,
) -> Result<(GraphReport, DeclarationIndex)> {
    if let Some(to) = &arguments.to {
        let new_worktree = checkout_worktree(&source.repo_root, to)
            .with_context(|| {
                format!(
                    "breaking checks `{to}` out of {} and git could not do it",
                    source.repo_root.display()
                )
            })?;
        let new_ignore = build_ignore_matcher(
            new_worktree.path(),
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        progress.set_message("Extracting the newer side...");
        let built =
            build_graph_with_declarations(new_worktree.path(), &new_ignore)?;
        drop(new_worktree);
        Ok(built)
    } else {
        let new_ignore = build_ignore_matcher(
            &source.analysis_path,
            &arguments.settings,
            &arguments.ignore_args.ignore,
            arguments.ignore_args.no_gitignore,
        )?;
        progress.set_message("Extracting the working tree...");
        build_graph_with_declarations(&source.analysis_path, &new_ignore)
    }
}

async fn run_affected_tests(
    arguments: &AffectedTestsArgs,
) -> Result<Vec<Gate>> {
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        false,
    )
    .await?;
    let progress = CliProgress::start("Finding affected tests...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;

    progress.set_message("Extracting imports...");
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        arguments.depth,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: arguments.exclude_types,
        },
        &CliReporter(&progress),
    )?;

    let focus = build_focus_set(&source.analysis_path, &arguments.focus)
        .into_iter()
        .collect::<Vec<_>>();

    let impacts = sephera_graph::affected_tests::measure_tests_all(
        &report,
        &arguments.files,
        "",
        arguments.depth,
        &focus,
    )?;

    let plan = affected_tests::runner_plan(&impacts);
    let rendered = match arguments.format {
        TestImpactOutputFormat::Markdown => {
            affected_tests::render_report(&impacts, &plan)
        }
        TestImpactOutputFormat::Json => {
            affected_tests::render_report_json(&impacts, &plan)
        }
    };

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;

    if arguments.run {
        // Said out loud rather than folded into the plan's success: a file
        // listed as manual was *not* run, and a green run that quietly
        // skipped a third of the plan is the failure this command exists to
        // prevent.
        for file in &plan.manual {
            eprintln!(
                "note: `{file}` has no automatic runner mapping and was not run"
            );
        }
        affected_tests::execute_plan(&plan, &source.analysis_path)?;
    }

    // One gate per unwitnessed target, as `impact` names one per wide
    // target. The measurement is inverted deliberately: a gate crosses when
    // its count reaches its limit, and the rule here is "this change is
    // unwitnessed", so the count is of unwitnessed changes -- one, for the
    // target named.
    let gates = if arguments.require_tests {
        impacts
            .iter()
            .filter(|impact| impact.tests.is_empty())
            .map(|impact| {
                Gate::new(
                    format!("unwitnessed change to `{}`", impact.target),
                    1,
                    1,
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(gates)
}

/// The file a guard run judges: the one named by `--file`, or the one the
/// hook payload on stdin names.
///
/// # Errors
///
/// Errors when stdin cannot be read. A payload that names no file is not an
/// error -- it is the common case of a hook firing for a non-edit tool.
fn guard_target(arguments: &GuardArgs) -> Result<Option<String>> {
    if let Some(file) = &arguments.file {
        return Ok(Some(file.clone()));
    }
    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .context("failed to read the hook payload from stdin")?;
    Ok(guard::file_path_from_hook_input(&payload))
}

/// Install the change-safety skill, or say where it would go.
///
/// The skill ships no gates: installing it enforces nothing, so the
/// run returns no thresholds for the gate evaluator. It is a plain
/// function, not an async one, because it awaits nothing — an async
/// wrapper with no await would be a lint failure, not a convenience.
fn run_skill(arguments: &SkillArgs) -> Result<Vec<Gate>> {
    skill::execute(arguments)?;
    Ok(no_gates())
}

/// Judge one agent edit before it happens.
///
/// Reads the hook payload, measures the edited file, prints the decision
/// JSON on stdout and the reasoning on stderr. Every outcome that is not a
/// denial is an allow, including "no file could be found in the payload"
/// and "the file is not in the graph": a guard that blocked edits it could
/// not judge would block them all the first time it was misconfigured, and
/// be removed by lunch. See [`crate::guard`].
async fn run_guard(arguments: &GuardArgs) -> Result<Vec<Gate>> {
    let edited = guard_target(arguments)?;

    let Some(edited) = edited else {
        eprintln!(
            "sephera guard: no edited file in the hook payload; allowing"
        );
        println!(
            "{}",
            guard::decision_json(false, "no edited file in the hook payload")
        );
        return Ok(Vec::new());
    };

    let source = resolve_source(
        &SourceRequest {
            path: arguments.path.clone(),
            url: None,
            git_ref: None,
        },
        false,
    )
    .await?;
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &SettingsArgs::default(),
        &[],
        false,
    )?;
    let report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &[],
        arguments.depth,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: false,
        },
        &CliReporter(&CliProgress::start("Measuring the edit...")),
    )?;

    let spelled = guard::graph_spelling(&source.analysis_path, &edited);
    let Some(canonical) = impact::match_path(&report, &spelled, "") else {
        eprintln!(
            "sephera guard: `{spelled}` is not in the analysed graph; allowing"
        );
        println!(
            "{}",
            guard::decision_json(
                false,
                "the edited file is not in the analysed graph"
            )
        );
        return Ok(Vec::new());
    };

    let radius = impact::measure(&report, &canonical, arguments.depth)?;
    let dependents = impact::dependent_count(&radius);
    let tests = sephera_graph::affected_tests::measure_tests(
        &report,
        &canonical,
        arguments.depth,
        &[],
    )?;
    let test_count = sephera_graph::affected_tests::affected_test_count(&tests);

    eprintln!(
        "sephera guard: `{canonical}` has {dependents} dependent(s) and \
         {test_count} affected test(s)"
    );

    let blocked = arguments
        .max_dependents
        .is_some_and(|limit| dependents >= limit);
    let reason = if blocked {
        format!(
            "`{canonical}` has {dependents} dependents, at or over the limit \
             of {}. Measure it with `sephera impact {canonical}` and get a \
             person's agreement before editing it.",
            arguments.max_dependents.unwrap_or_default()
        )
    } else {
        format!(
            "`{canonical}` has {dependents} dependent(s) and {test_count} \
             affected test(s)"
        )
    };
    println!("{}", guard::decision_json(blocked, &reason));

    if blocked {
        Ok(vec![Gate::new(
            format!("files depending on `{canonical}`"),
            dependents,
            arguments.max_dependents.unwrap_or_default(),
        )])
    } else {
        Ok(Vec::new())
    }
}

/// Report what a change reaches, rather than what the repository depends on.
///
/// Builds the graph once and measures every changed file against it. Building
/// one blast radius per file would re-parse the repository once per changed
/// file, which turns the cheapest useful CI check into the most expensive one.
///
/// Synchronous despite the caller being async: the source is already resolved by
/// the time this runs, and a diff against it is file reads rather than a clone.
fn run_graph_diff(
    source: &ResolvedSource,
    ignore: &IgnoreMatcher,
    arguments: &GraphArgs,
    spec: &str,
) -> Result<Vec<Gate>> {
    let progress = CliProgress::start("Measuring change impact...");
    let selection = resolve_changed_files(source, spec)?;
    let requested: Vec<String> = selection
        .changed_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();

    progress.set_message("Extracting imports...");
    let report = build_graph_with_progress(
        &source.analysis_path,
        ignore,
        &[],
        arguments.depth,
        change_impact::diff_query(),
        EdgeFilters {
            exclude_type_aliases: arguments.exclude_types,
        },
        &CliReporter(&progress),
    )?;

    let base_prefix = source
        .analysis_path
        .strip_prefix(&selection.repo_root)
        .unwrap_or_else(|_| std::path::Path::new(""))
        .to_string_lossy()
        .into_owned();

    let changes = change_impact::measure_changes(
        &report,
        &requested,
        &base_prefix,
        arguments.depth,
    )?;

    // A changed path that matched no graph node is either deleted or outside the
    // analysis base. Reporting those separately is the difference between "this
    // change reaches nothing" and "this change touched files I could not see",
    // which are very different sentences in a pull request.
    let matched: std::collections::BTreeSet<&str> =
        changes.iter().map(|change| change.file.as_str()).collect();
    let skipped: Vec<String> = requested
        .iter()
        .filter(|path| {
            !matched.contains(path.as_str())
                && !changes.iter().any(|change| {
                    change
                        .file
                        .ends_with(&format!("/{}", path.replace('\\', "/")))
                })
        })
        .cloned()
        .collect();

    let rendered = match arguments.format {
        GraphOutputFormat::Json => {
            change_impact::render_json(&changes, spec, &skipped)
        }
        GraphOutputFormat::Markdown => {
            change_impact::render_markdown(&changes, spec, &skipped)
        }
        GraphOutputFormat::Xml => {
            change_impact::render_xml(&changes, spec, &skipped)
        }
        GraphOutputFormat::Dot => {
            change_impact::render_dot(&changes, spec, &skipped)
        }
    };

    // Both thresholds the non-diff path honours. `--fail-on-cycles` used
    // to be accepted here and then dropped, so a diff run over a cyclic
    // tree exited 0 where the same tree without `--diff` exited 2.
    let mut gates = Vec::new();
    if let Some(limit) = arguments.fail_on_cycles {
        gates.push(Gate::new(
            "circular dependencies",
            report.metrics.circular_dependencies,
            limit,
        ));
    }
    if let Some(limit) = arguments.fail_on_unresolved {
        gates.push(Gate::new(
            "unresolved local paths",
            report.metrics.unresolved_local_edges,
            limit,
        ));
    }

    if arguments.output_args.output.is_some() {
        progress.set_message("Writing output...");
    }
    progress.finish();
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(gates)
}

async fn run_graph(arguments: &GraphArgs) -> Result<Vec<Gate>> {
    // `--diff` resolves its base ref by walking back from the tip, so a shallow
    // clone could not answer `HEAD~1`. Naming a base ref is the same request as
    // naming a ref to check out: both mean "something other than where the branch
    // points".
    let source = resolve_source(
        &SourceRequest {
            path: arguments.source.path.clone(),
            url: arguments.source.url.clone(),
            git_ref: arguments.source.git_ref.clone(),
        },
        arguments.diff.is_some(),
    )
    .await?;
    let progress = CliProgress::start("Analyzing dependency graph...");
    let ignore = build_ignore_matcher(
        &source.analysis_path,
        &arguments.settings,
        &arguments.ignore_args.ignore,
        arguments.ignore_args.no_gitignore,
    )?;
    check_focus_paths_exist(&source.analysis_path, &arguments.focus)?;

    if let Some(spec) = arguments.diff.as_deref() {
        return run_graph_diff(&source, &ignore, arguments, spec);
    }

    progress.set_message("Extracting imports...");
    let query = arguments
        .what_depends_on
        .as_ref()
        .map(|path| GraphQuery::DependsOn(path.clone()));
    let mut report = build_graph_with_progress(
        &source.analysis_path,
        &ignore,
        &arguments.focus,
        arguments.depth,
        query,
        EdgeFilters {
            exclude_type_aliases: arguments.exclude_types,
        },
        &CliReporter(&progress),
    )?;
    if let Some(display_path) = source.display_path {
        report.base_path = display_path.into();
    }

    let graph_format = match arguments.format {
        GraphOutputFormat::Json => GraphFormat::Json,
        GraphOutputFormat::Markdown => GraphFormat::Markdown,
        GraphOutputFormat::Xml => GraphFormat::Xml,
        GraphOutputFormat::Dot => GraphFormat::Dot,
    };

    progress.set_message("Rendering graph...");
    let rendered = render_graph(&report, graph_format);

    // Thresholds are read off the report that was just built, before it is
    // rendered, so a gate and the number printed next to it cannot disagree.
    let mut gates = Vec::new();
    if let Some(limit) = arguments.fail_on_cycles {
        gates.push(Gate::new(
            "circular dependencies",
            report.metrics.circular_dependencies,
            limit,
        ));
    }
    if let Some(limit) = arguments.fail_on_unresolved {
        gates.push(Gate::new(
            "unresolved local paths",
            report.metrics.unresolved_local_edges,
            limit,
        ));
    }

    let writes_to_stdout = arguments.output_args.output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    emit_rendered_output(arguments.output_args.output.as_deref(), &rendered)?;
    Ok(gates)
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use sephera_runtime::{INTERRUPTED_EXIT_CODE, Interrupted};

    use std::process::ExitCode;

    use super::exit_code_for;

    #[test]
    fn a_stopped_clone_reports_that_it_was_stopped() {
        // Wrapped in context on the way out of `resolve_source`, so the downcast
        // has to see through the chain -- that is the whole mechanism, and it is
        // the difference between a user who pressed Ctrl+C seeing nothing and
        // seeing `error:`.
        let error = anyhow::Error::new(Interrupted).context(
            "failed to clone `https://github.com/Reim-developer/Sephera`",
        );

        assert_eq!(
            exit_code_for(&error),
            ExitCode::from(INTERRUPTED_EXIT_CODE),
            "an interrupted clone is a decision, not a fault"
        );
    }

    #[test]
    fn an_ordinary_failure_is_not_mistaken_for_a_stopped_one() {
        // The failure that matters most is the one that looks similar: git's own
        // refusal arrives as ordinary context, and must not be reported as the
        // user having asked to stop.
        let error = anyhow!("failed to clone `https://example.invalid/x`");

        assert_eq!(exit_code_for(&error), ExitCode::FAILURE);
    }
}
