use std::io::Read as _;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use sephera_graph::{
    resolver::{EdgeFilters, build_focus_set, build_graph_with_progress},
    types::{GraphFormat, GraphQuery},
};
use sephera_runtime::{
    INTERRUPTED_EXIT_CODE, Interrupted, ResolvedSource, SourceRequest,
    build_context_report, changed_files, load_project_settings,
    recent_commit_files, resolve_changed_files, resolve_source,
};
use sephera_scan::{CodeLoc, IgnoreMatcher};
use sephera_symbols::{SymbolAnalyzer, SymbolDetail};

use crate::{
    affected_tests,
    args::{
        AffectedTestsArgs, Cli, Commands, ContextArgs, CouplingArgs,
        CouplingOutputFormat, DeadArgs, DeadOutputFormat, GraphArgs,
        GraphOutputFormat, GuardArgs, IgnoreArgs, ImpactArgs,
        ImpactOutputFormat, LocArgs, LocOutputFormat, OutputArgs, ReviewArgs,
        ReviewOutputFormat, RiskArgs, RiskOutputFormat, SettingsArgs,
        SourceArgs, SymbolOutputFormat, SymbolsArgs, TestImpactOutputFormat,
        WatchArgs, WatchTarget, WhyArgs, WhyOutputFormat,
    },
    change_impact, configure,
    context_config::{
        ResolvedContextCommand, ResolvedContextOptions, resolve_context_options,
    },
    coupling, dead,
    gate::{self, Gate},
    guard, impact,
    output::{
        emit_rendered_output, print_available_profiles, print_report,
        print_symbol_report, print_symbols_by_file, render_context_json,
        render_context_markdown, render_graph, render_loc_csv, render_loc_json,
        render_loc_markdown, render_symbol_json, render_symbol_markdown,
    },
    progress::{self, CliProgress, CliReporter},
    review, risk, watch, why,
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
        Commands::Guard(arguments) => run_guard(&arguments).await,
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
    let Some(target) = arguments.target else {
        anyhow::bail!("`--target` is required unless `--once` is passed");
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
    // emitted or it interleaves with the payload.
    let rendered = match arguments.format {
        LocOutputFormat::Table => {
            progress.finish();
            print_report(&report);
            return Ok(no_gates());
        }
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
        SymbolOutputFormat::Table => {
            if arguments.by_file {
                print_symbols_by_file(&detail);
            } else {
                print_symbol_report(&detail.report);
            }
            None
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
    match resolve_context_options(arguments, profile).await? {
        ResolvedContextCommand::Execute(resolved) => execute_context(&resolved),
        ResolvedContextCommand::ListProfiles(profiles) => {
            print_available_profiles(&profiles);
            Ok(no_gates())
        }
    }
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
        false,
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

    let base_prefix = sephera_graph::blast_radius::base_prefix_for(
        &source.repo_root,
        &source.analysis_path,
    );
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
        false,
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

    let base_prefix = sephera_graph::blast_radius::base_prefix_for(
        &source.repo_root,
        &source.analysis_path,
    );
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
    let rendered = match arguments.format {
        ReviewOutputFormat::Markdown => {
            review::render_markdown(&measured, against_label)
        }
        ReviewOutputFormat::Json => {
            review::render_json(&measured, against_label)
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
        false,
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

    let base_prefix = sephera_graph::blast_radius::base_prefix_for(
        &source.repo_root,
        &source.analysis_path,
    );
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
        _ => change_impact::render_markdown(&changes, spec, &skipped),
    };

    let gates = arguments
        .fail_on_unresolved
        .map(|limit| {
            vec![Gate::new(
                "unresolved local paths",
                report.metrics.unresolved_local_edges,
                limit,
            )]
        })
        .unwrap_or_default();

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
