use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    graph::{
        resolver::{EdgeFilters, build_graph_with},
        types::{GraphFormat, GraphQuery},
    },
    runtime::{SourceRequest, build_context_report, resolve_source},
    symbols::{SymbolAnalyzer, SymbolDetail},
};

use crate::{
    args::{
        Cli, Commands, ContextArgs, GraphArgs, GraphOutputFormat, LocArgs,
        SymbolOutputFormat, SymbolsArgs, WatchArgs, WatchTarget,
    },
    context_config::{
        ResolvedContextCommand, ResolvedContextOptions, resolve_context_options,
    },
    output::{
        emit_rendered_output, print_available_profiles, print_report,
        print_symbol_report, print_symbols_by_file, render_context_json,
        render_context_markdown, render_graph, render_symbol_json,
        render_symbol_markdown,
    },
    progress::CliProgress,
    watch,
};

#[must_use]
pub fn main_exit_code() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// # Errors
///
/// Returns an error when argument parsing or command execution fails.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli)
}

fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Loc(arguments) => run_loc(arguments),
        Commands::Symbols(arguments) => run_symbols(arguments),
        Commands::Context(arguments) => run_context(arguments),
        Commands::Mcp => run_mcp(),
        Commands::Graph(arguments) => run_graph(&arguments),
        Commands::Watch(arguments) => run_watch(&arguments),
    }
}

/// Re-run a chosen analysis whenever the watched tree changes.
///
/// `--once` short-circuits to a single run, which keeps the watch argument
/// parsing usable from scripts without needing a separate code path.
fn run_watch(arguments: &WatchArgs) -> Result<()> {
    let Some(target) = arguments.target else {
        anyhow::bail!("`--target` is required unless `--once` is passed");
    };

    if target == WatchTarget::DependsOn && arguments.on.is_none() {
        anyhow::bail!("`--on <file>` is required with `--target depends-on`");
    }

    let root = watch::resolve_root(arguments.path.as_deref());
    let ignore = arguments.ignore.clone();

    if arguments.once {
        return run_watch_target(
            target,
            arguments.on.as_deref(),
            &root,
            &ignore,
        );
    }

    println!(
        "Watching {} for changes. Press Ctrl+C to stop.",
        root.display()
    );

    watch::watch(&root, || {
        run_watch_target(target, arguments.on.as_deref(), &root, &ignore)
    })
}

/// Run one analysis pass for the watch target.
fn run_watch_target(
    target: WatchTarget,
    on: Option<&str>,
    root: &std::path::Path,
    ignore: &[String],
) -> Result<()> {
    // Each arm builds its own argument struct from the same root, so a value is
    // never shared across arms.
    let path = || Some(root.to_path_buf());

    match target {
        WatchTarget::Loc => run_loc(LocArgs {
            path: path(),
            url: None,
            git_ref: None,
            ignore: ignore.to_vec(),
        }),
        WatchTarget::Symbols => run_symbols(SymbolsArgs {
            path: path(),
            url: None,
            git_ref: None,
            format: SymbolOutputFormat::Table,
            output: None,
            detail: false,
            by_file: false,
            ignore: ignore.to_vec(),
        }),
        WatchTarget::Graph => run_graph(&GraphArgs {
            path: path(),
            url: None,
            git_ref: None,
            focus: Vec::new(),
            ignore: ignore.to_vec(),
            depth: None,
            what_depends_on: None,
            exclude_types: false,
            format: GraphOutputFormat::Markdown,
            output: None,
        }),
        WatchTarget::DependsOn => {
            let Some(target_path) = on else {
                anyhow::bail!(
                    "`--on <file>` is required with `--target depends-on`"
                );
            };
            run_graph(&GraphArgs {
                path: path(),
                url: None,
                git_ref: None,
                focus: Vec::new(),
                ignore: ignore.to_vec(),
                depth: None,
                what_depends_on: Some(target_path.to_owned()),
                exclude_types: false,
                format: GraphOutputFormat::Markdown,
                output: None,
            })
        }
    }
}

fn run_mcp() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(sephera_mcp::run_mcp_server())
}

fn run_loc(arguments: LocArgs) -> Result<()> {
    let progress = CliProgress::start("Analyzing line counts...");
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let source = resolve_source(&SourceRequest {
        path: arguments.path,
        url: arguments.url,
        git_ref: arguments.git_ref,
    })?;
    let mut report = CodeLoc::new(&source.analysis_path, ignore).analyze()?;
    if let Some(display_path) = source.display_path {
        report.base_path = display_path.into();
    }
    progress.finish();
    print_report(&report);
    Ok(())
}

fn run_symbols(arguments: SymbolsArgs) -> Result<()> {
    let progress = CliProgress::start("Counting declarations...");
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let source = resolve_source(&SourceRequest {
        path: arguments.path,
        url: arguments.url,
        git_ref: arguments.git_ref,
    })?;

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
        SymbolOutputFormat::Json => Some(render_symbol_json(&detail)),
        SymbolOutputFormat::Markdown => Some(render_symbol_markdown(&detail)),
    };

    if let Some(rendered) = rendered {
        emit_rendered_output(arguments.output.as_deref(), &rendered)
    } else {
        Ok(())
    }
}

fn run_context(arguments: ContextArgs) -> Result<()> {
    match resolve_context_options(arguments)? {
        ResolvedContextCommand::Execute(resolved) => execute_context(&resolved),
        ResolvedContextCommand::ListProfiles(profiles) => {
            print_available_profiles(&profiles);
            Ok(())
        }
    }
}

fn execute_context(arguments: &ResolvedContextOptions) -> Result<()> {
    let progress = CliProgress::start("Preparing context inputs...");
    progress.set_message("Building context pack...");
    let report = build_context_report(arguments)?;

    progress.set_message("Rendering context pack...");
    let rendered = match arguments.format.as_str() {
        "markdown" => render_context_markdown(&report),
        "json" => render_context_json(&report),
        other => unreachable!("unexpected resolved context format `{other}`"),
    };

    let writes_to_stdout = arguments.output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    emit_rendered_output(arguments.output.as_deref(), &rendered)
}

fn run_graph(arguments: &GraphArgs) -> Result<()> {
    let progress = CliProgress::start("Analyzing dependency graph...");
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let source = resolve_source(&SourceRequest {
        path: arguments.path.clone(),
        url: arguments.url.clone(),
        git_ref: arguments.git_ref.clone(),
    })?;

    progress.set_message("Extracting imports...");
    let query = arguments
        .what_depends_on
        .as_ref()
        .map(|path| GraphQuery::DependsOn(path.clone()));
    let mut report = build_graph_with(
        &source.analysis_path,
        &ignore,
        &arguments.focus,
        arguments.depth,
        query,
        EdgeFilters {
            exclude_type_aliases: arguments.exclude_types,
        },
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

    let writes_to_stdout = arguments.output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    emit_rendered_output(arguments.output.as_deref(), &rendered)
}
