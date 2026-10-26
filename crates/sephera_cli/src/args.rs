use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::budget::parse_token_budget;

/// Generates a command's argument struct with the shared groups already in it.
///
/// Five groups -- where to read from, what to skip, where the report goes, which
/// config to read, and which profile within it -- were written out once per
/// command, twenty-five times in all, and they had already drifted: `watch` called
/// `--config` "Which `.sephera.toml` to read" while every other command called it
/// "Shared settings source", and `symbols` listed `--ignore` last where `loc`
/// listed it second, so the same `--help` ordered the same flags differently per
/// command for no reason a reader could infer.
///
/// One struct holding all five would not do it either, because the commands do
/// not all take all five: `watch` watches a local directory and writes no file,
/// so [`WatchArgs`] declares its own. What is left to share is the field
/// declarations, and the only construct that can share them is a macro emitting
/// the whole struct. A `macro_rules!` invocation cannot go where an enum variant
/// goes, nor where a struct field goes, and a field-generating macro cannot be
/// reached even as nested expansion from a struct-generating one -- so
/// `flatten_source!()` inside a struct body is rejected, and so is the same call
/// written inside this macro's expansion. Writing the shared fields once *here*,
/// as part of one struct item, is what gets past the limit.
///
/// A command's own flags come third, between `ignore` and `output`. That ordering
/// is a choice, not a behaviour: the previous per-command orders differed from
/// each other and no test pinned any of them.
macro_rules! tree_command_args {
    (
        $(#[$meta:meta])*
        $name:ident { $( $own:tt )* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Args)]
        pub struct $name {
            /// Where to read from
            #[command(flatten)]
            pub source: SourceArgs,

            /// How to decide which files to leave out
            #[command(flatten)]
            pub ignore_args: IgnoreArgs,

            $( $own )*

            /// Where the rendered report goes
            #[command(flatten)]
            pub output_args: OutputArgs,

            /// Shared settings source and named profile
            #[command(flatten)]
            pub settings: SettingsArgs,
        }
    };
}

#[derive(Debug, Parser)]
#[command(
    name = "sephera",
    version,
    about = "Analyze project structure and line counts",
    long_about = "Sephera analyzes source trees for line counts, builds LLM-ready context packs, and maps dependency graphs.\n\nUse `loc` to inspect language-level line metrics, `context` to export a curated Markdown or JSON context pack for downstream review, debugging, or prompting workflows, and `graph` to analyze and visualize the dependency structure of your codebase. The `context` command can also load defaults and named profiles from `.sephera.toml`, let explicit CLI flags override them, and build packs centered on Git changes via `--diff`.",
    after_long_help = "Examples:\n  sephera loc --path . --ignore target --ignore \"*.min.js\"\n  sephera loc --url https://github.com/Reim-developer/Sephera\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --url https://github.com/Reim-developer/Sephera --ref master --diff HEAD~1\n  sephera context --path . --profile review\n  sephera context --path . --list-profiles\n  sephera context --path . --config .sephera.toml\n  sephera context --path . --no-config --format json --output reports/context.json\n  sephera graph --path . --format markdown\n  sephera graph --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format dot --output deps.dot",
    arg_required_else_help = true
)]
pub struct Cli {
    /// When to draw a progress bar
    ///
    /// `auto`, the default, draws only when stderr is a terminal: the bar is for
    /// a person watching, and everything machine-readable goes to stdout, so a
    /// script that pipes Sephera's output is unaffected either way. `always` is
    /// for a log or a demo where the bar is wanted in a record that has no
    /// terminal. `never` is for a terminal-shaped session that should stay still.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub progress: ProgressMode,

    #[command(subcommand)]
    pub command: Commands,
}

/// When to draw a progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ProgressMode {
    /// Draw when stderr is a terminal.
    #[default]
    Auto,
    /// Draw even without a terminal.
    Always,
    /// Never draw.
    Never,
}

#[derive(Debug, Subcommand)]
#[command(
    // Last occurrence of a repeated flag wins, rather than "cannot be used
    // multiple times".
    //
    // `.sephera.toml` is applied by inserting its values as arguments ahead of
    // the user's, so a config-supplied `--format markdown` and a typed
    // `--format json` meet on the same command line by design. Rejecting that as
    // a duplicate would make config unusable for every scalar flag, and ignoring
    // one of them would make which one wins depend on which parser looked first.
    args_override_self = true
)]
pub enum Commands {
    /// Count lines of code for supported languages in a directory tree
    #[command(
        long_about = "Count lines of code, comment lines, empty lines, and file sizes for supported languages inside a directory tree.\n\nUse `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs. Ignore patterns containing `*`, `?`, or `[` are treated as globs and matched against both the file name and the path relative to the base. All other ignore patterns are compiled as regular expressions and matched against that same relative path.",
        after_long_help = "Examples:\n  sephera loc --path .\n  sephera loc --path crates --ignore target --ignore \"*.snap\"\n  sephera loc --url https://github.com/Reim-developer/Sephera\n  sephera loc --url https://github.com/Reim-developer/Sephera/tree/master/crates"
    )]
    Loc(LocArgs),
    /// Count declarations per language
    #[command(
        long_about = "Count declarations per language: functions, types, enums, and constants.\n\nCounts come from Tree-sitter parse trees rather than text matching, so a keyword inside a comment or string is not counted and a function nested inside an impl block or class body is attributed correctly. Supported languages: Rust, Python, TypeScript, JavaScript, Go, Java, C, and C++.\n\nUnlike `loc`, which measures how much code exists, this reports what is declared in it. Use `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs.",
        after_long_help = "Examples:\n  sephera symbols --path .\n  sephera symbols --path . --format markdown\n  sephera symbols --path . --detail\n  sephera symbols --path . --format json --output reports/symbols.json\n  sephera symbols --url https://github.com/Reim-developer/Sephera\n  sephera symbols --path crates --ignore \"*.snap\""
    )]
    Symbols(SymbolsArgs),
    /// Build an LLM-ready context pack for a repository or focused sub-paths
    #[command(
        long_about = "Build a deterministic context pack for a repository or a focused sub-tree.\n\nThe command ranks useful files, enforces an approximate token budget, and renders either Markdown for direct copy-paste into LLM tools or JSON for automation pipelines. Configuration precedence is: built-in defaults, then `[context]` in `.sephera.toml`, then an optional named profile, then explicit CLI flags. Use `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs. Use `--diff` to center the pack on Git changes from a base ref or working-tree mode; URL mode supports base refs but rejects working-tree keywords.",
        after_long_help = "Examples:\n  sephera context --path .\n  sephera context --path . --profile review\n  sephera context --path . --list-profiles\n  sephera context --path . --config .sephera.toml\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --path . --diff origin/master\n  sephera context --path . --diff HEAD~1\n  sephera context --path . --diff working-tree\n  sephera context --path . --diff staged\n  sephera context --url https://github.com/Reim-developer/Sephera --ref master --diff HEAD~1\n  sephera context --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format json\n  sephera context --path . --no-config --format markdown --output reports/context.md\n  sephera context --path . --format json --output reports/context.json"
    )]
    Context(ContextArgs),
    /// Re-run an analysis whenever the tree changes
    #[command(
        long_about = "Re-run an analysis whenever the watched directory changes.\n\nChoose what to re-run with `--target`: `graph` for the dependency report, `symbols` for declaration counts, `loc` for line metrics, or `depends-on` to keep a reverse dependency query live. Writes are debounced, so a burst of editor or build activity produces a single run rather than one per file.\n\nBuild output and dependency trees such as `target`, `node_modules`, and `.git` are never watched. Press Ctrl+C to stop.",
        after_long_help = "Examples:\n  sephera watch --target graph --path .\n  sephera watch --target symbols\n  sephera watch --target depends-on --on src/core/graph.rs\n  sephera watch --target graph --path crates --ignore \"*.snap\"\n  sephera watch --target loc --once"
    )]
    Watch(WatchArgs),
    /// Start an MCP (Model Context Protocol) server over stdio
    #[command(
        long_about = "Start an MCP server that exposes Sephera tools (loc, context, graph) over the Model Context Protocol.\n\nThis allows AI agents such as Claude Desktop, Cursor, and other MCP-compatible clients to call Sephera directly, including URL-mode analysis of remote repositories."
    )]
    Mcp,
    /// Analyze dependency graph via Tree-sitter import extraction
    #[command(
        long_about = "Analyze the dependency graph of a project by extracting import statements using Tree-sitter AST parsing.\n\nSupported languages: Rust, Python, TypeScript, JavaScript, Go, Java, C++, C.\n\nUse `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs. The graph command identifies internal file dependencies, detects circular dependencies, and computes metrics such as most-imported and most-importing files.",
        after_long_help = "Examples:\n  sephera graph --path .\n  sephera graph --path . --format dot --output deps.dot\n  sephera graph --path . --focus crates/sephera_core --format markdown\n  sephera graph --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format xml --output graph.xml\n  sephera graph --path . --what-depends-on src/core/context/builder.rs"
    )]
    Graph(GraphArgs),
    /// Report what breaks if one file changes
    #[command(
        long_about = "Report what breaks if one file changes.\n\nAnswers the question a reviewer, a pre-commit hook, or a nervous contributor asks before editing: if I touch this file, what else stops working? The answer is the file's blast radius -- every file that imports it, directly or through however many hops, along with the name each one imports.\n\nThis is `graph --what-depends-on` as its own command. The same analysis, but reachable without reading `graph --help`, composable in a script, and with `--fail-on` so a pipeline can refuse a change whose blast radius is too wide.",
        after_long_help = "Examples:\n  sephera impact src/core/graph/resolver.rs\n  sephera impact src/lib.rs --depth 1\n  sephera impact src/core/ignore.rs --format json\n  sephera impact src/lib.rs --fail-on 40\n  sephera impact crates/sephera_core/src/core/graph.rs --fail-on 10 --output impact.md"
    )]
    Impact(ImpactArgs),
    /// Report which tests a change to one file has to answer for
    #[command(
        long_about = "Report which tests a change to one file has to answer for.\n\nThe blast radius says what breaks; this says which tests would notice. It is the same reachability walk, filtered to test files -- files under a `tests` directory and the test naming conventions of every supported language -- with each test's distance from the changed file, closest first. The report ends with the commands that run those tests where a mapping exists: Cargo test targets for Rust integration tests, package directories for Go, files for pytest. Other ecosystems are listed without an invented command.\n\nWith `--run` the commands are executed. With `--require-tests` the run fails when a changed file reaches no test at all, which is the CI form of the question: not \"is the radius wide\" but \"is this change unwitnessed\".",
        after_long_help = "Examples:\n  sephera affected-tests src/lib.rs\n  sephera affected-tests src/lib.rs src/parser.rs --format json\n  sephera affected-tests src/lib.rs --run\n  sephera affected-tests src/lib.rs --require-tests"
    )]
    AffectedTests(AffectedTestsArgs),
    /// Show why one file depends on another, as the shortest import chain
    #[command(
        long_about = "Show why one file depends on another: the shortest chain of imports connecting them.\n\n`impact` answers what breaks with a set of files; the question that follows it in a review is why one particular file is in that set. This is that question as a command. The chain is the shortest one -- a longer route also connects the files, but it is not why they are coupled, because the coupling would survive its removal -- and every hop names the import path as written, so each link can be checked in the source and, if the coupling is unwanted, broken at a chosen link.\n\nThis is Bazel's `somepath` question asked of a source tree instead of a build graph. When no chain exists the command says so and still succeeds: two files that never touch are the normal case, not an error. A path that matches no file in the analysis is an error, as it is everywhere else.",
        after_long_help = "Examples:\n  sephera why src/main.rs src/lib.rs\n  sephera why crates/sephera_cli/src/run.rs crates/sephera_graph/src/types.rs\n  sephera why src/run.rs src/types.rs --format json"
    )]
    Why(WhyArgs),
    /// Find file pairs that change together but share no import between them
    #[command(
        long_about = "Find hidden coupling: file pairs that change together in the commit history but share no import between them.\n\nThe graph says what a file declares it depends on; history says what actually moves together. A pair that commit after commit changes in the same commit while neither imports the other is coupled in a way no source analysis can see, because there is no statement to read -- and it is exactly the coupling that breaks a change whose author only knew about one of the two files. The report intersects the last `--commits` commits with the graph of the current tree: pairs joined by an import in either direction are excluded, because their coupling is already the graph's to report, and commits touching more than fifty files in the tree are skipped, because a mass reformat couples everything and means nothing.\n\nThe analysis needs a git history. A tree that is not a git repository is an error, not an empty report.",
        after_long_help = "Examples:\n  sephera coupling --path .\n  sephera coupling --path . --commits 1000 --min-co-changes 5\n  sephera coupling --path . --format json"
    )]
    Coupling(CouplingArgs),
    /// Review a change before it lands: blast radius, affected tests, breaking symbols, and coupling warnings for the files it touches
    #[command(
        long_about = "Review a change before it lands: for each file the change touches, its blast radius, the tests that reach it, and a warning for every file that history keeps changing with it but the change does not touch. Between the changed files and those warnings, the report names what broke by name: the symbols the change removed or reshaped, joined to the files that still name them, measured exactly as `breaking` measures them.\n\nThe change is the working tree read against a reference -- HEAD by default, so staged edits, unstaged edits, deletions, and untracked files all count; `--against` names another reference to compare the working tree with, which is how a whole branch is reviewed before it is pushed. Dependents and tests are measured with the same walks `impact` and `affected-tests` use, and the coupling warnings come from the same measurement as `coupling`, read against the changed set: a pair where both sides changed is history working as it should and does not warn.\n\nA changed file the graph does not hold -- deleted by the change, or outside the analysis base -- is reported as absent rather than dropped: a deletion is the change whose dependents matter most, and the report says the graph cannot name them instead of implying there are none.\n\nThe analysis needs a git history for the coupling warnings. A tree that is not a git repository is an error, not an empty report.",
        after_long_help = "Examples:\n  sephera review --path .\n  sephera review --path . --against master\n  sephera review --path . --format json"
    )]
    Review(ReviewArgs),
    /// Rank the files where a change is most likely to hurt
    #[command(
        long_about = "Rank the files where a change is most likely to hurt, by the three readings of a tree that each name a different danger: how much depends on the file (the graph), how often it changes (the history), and how many hidden partners its changes drag along (the coupling the graph cannot see).\n\nThe score is a product -- dependents × churn × (1 + coupled partners) -- because the danger compounds rather than adds, and the coupling term amplifies rather than creates: a file nothing depends on is not made risky by its partners. Every factor is printed beside the score, and each is a count the other commands also report, so a row can be audited against `impact`, the history, and `coupling` rather than taken on faith.\n\nChurn is counted over the same commits `coupling` counts, under the same rule. A file is listed only when it is both depended on and in motion inside the window: stable infrastructure and churning leaves are each safe in their own way, and a ranking that listed them would be warning about files whose changes cannot hurt anyone, or cannot happen.\n\nThe analysis needs a git history. A tree that is not a git repository is an error, not an empty ranking.",
        after_long_help = "Examples:\n  sephera risk --path .\n  sephera risk --path . --limit 10\n  sephera risk --path . --commits 1000 --format json"
    )]
    Risk(RiskArgs),
    /// Find the files no entry point reaches
    #[command(
        long_about = "Find the files no entry point reaches: the part of the tree that is compiled, bundled, shipped, and reviewed as if it were alive, while nothing that runs ever imports it.\n\nThe walk starts from the tree's entry points and follows import edges forward. Roots are read off the paths by the conventions ecosystems agree on: test files, files named `main.*`, `lib.rs`, or `build.rs`, Python's `__main__.py`, files under a `bin`, `cmd`, `examples`, `benches`, or `scripts` directory, and `index.*` files. A file with no importers is not a root by that fact alone -- an orphaned cluster imports itself happily, and this analysis exists to refuse that reading: dead means unreachable from positive evidence of entry.\n\nThe report lists the roots it used, so a verdict can always be checked against the entry points it was measured from. Entry points a path cannot show -- a Java entry class, a plugin loaded by name, a file a framework scans for -- are named with `--root`, one per file.\n\nA dead file's importers and imports are printed with it: imported by nobody and importing nothing is one orphaned file; imported by three and importing five is an abandoned subsystem, and the second kind deserves a look before anything is deleted.",
        after_long_help = "Examples:\n  sephera dead --path .\n  sephera dead --path . --root src/worker.rs\n  sephera dead --path . --format json"
    )]
    Dead(DeadArgs),
    /// Compare two refs and report which declared symbols broke, joined to the files that still use them, with a fix suggestion when the newer tree says something actionable
    #[command(
        long_about = "Compare two refs and report which declared symbols broke, joined to the files that still use them.\\n\\nA diff between two declaration indexes says what vanished or changed shape — a list any semver checker can produce, and one that answers the wrong question. A removed name nobody imports breaks nothing; a removed name with nine importers breaks nine files. This command makes that join: every symbol the newer side no longer declares, or declares as a different kind of thing, is reported with the files in the newer tree that still name it from the file that used to declare it — read from the new tree's edges, where a statement that used to resolve and now lands nowhere is itself the evidence of the break.\\n\\n`--from` names the older side and is required. `--to` names the newer side; when it is absent, the newer side is the working tree, which is the form a branch review takes before it is pushed. Only Rust and Python collect declarations today, so only their symbols can be reported broken — but consumers are read in every language, because a JavaScript file importing a removed name through a boundary is as broken as a Rust caller.\\n\\nThe analysis needs a git repository. A tree that is not one is an error, not an empty report. Each entry also carries a fix suggestion when the newer tree says something actionable: where a removed name moved, when exactly one other file declares it, or the call shape a changed signature now takes.",
        after_long_help = "Examples:\\n  sephera breaking --path . --from origin/main\\n  sephera breaking --path . --from v1.0.0 --to v2.0.0\\n  sephera breaking --path . --from origin/main --format json"
    )]
    Breaking(BreakingArgs),
    /// Predict the merges git completes cleanly but that break the build, by comparing the fork point against both sides
    #[command(
        long_about = "Predict the merges git completes cleanly but that break the build.\\n\\nGit compares lines, so contradictory changes in different files merge without complaint: one side renames, removes, or reshapes a symbol while the other side adds a consumer of the old contract. This command compares declarations and consumers across three trees — base, ours, theirs — and reports each contradiction by symbol, with the evidence on both sides and the fix when the changing side states one.\\n\\n`--ours` and `--theirs` name the two sides and are required. `--base` names the fork point; when it is absent, the merge base of the two sides is used. Only what is proven fails `--fail-on`: a removal or kind change against a new consumer, and two sides reshaping the same symbol differently. A signature change against a new consumer is a warning, never a gate failure, because without call-site shapes the tool cannot prove the new call matches the old shape rather than the new one. A consumer kept since base is never a conflict: it breaks because of the contract change alone, which breaking already reports.\\n\\nThe analysis needs a git repository. A tree that is not one is an error, not an empty report.",
        after_long_help = "Examples:\\n  sephera merge --path . --ours feature-a --theirs feature-b\\n  sephera merge --path . --ours HEAD --theirs origin/main --base v1.0.0\\n  sephera merge --path . --ours a --theirs b --fail-on 1\\n  sephera merge --path . --ours a --theirs b --format json"
    )]
    Merge(MergeArgs),
    /// Compare two refs and report every declared symbol that was added, removed, or reshaped: the API changelog
    #[command(
        long_about = "Compare two refs and report every declared symbol that was added, removed, or reshaped: the API changelog.\\n\\n`breaking` answers what breaks consumers; this answers what changed in the API surface at all, including additions, which break nothing and so never appear in a breaking report. A changelog without additions hides half the release. A symbol that moved files reads as what it is at the index level, a removal plus an addition: claiming a move would need the suggestion machinery, and a diff that guesses identity is a diff that lies.\\n\\n`--from` names the older side and is required. `--to` names the newer side; when it is absent, the newer side is the working tree, which is the form a branch review takes before it is pushed. The comparison rules are the same as `breaking`'s, so the two commands never disagree about a symbol both name: kinds compared outright, call shapes for functions only, a shape on only one side stays silent. Only Rust and Python collect declarations today, so only their symbols can be reported.\\n\\nThe analysis needs a git repository. A tree that is not one is an error, not an empty report.",
        after_long_help = "Examples:\\n  sephera diff --path . --from origin/main\\n  sephera diff --path . --from v1.0.0 --to v2.0.0\\n  sephera diff --path . --from origin/main --format json"
    )]
    Diff(DiffArgs),
    /// Trace one symbol through history: every rename and move, its former names, and who changed it
    #[command(
        long_about = "Trace one symbol through history: every rename and move, its former names, and who changed it.\\n\\nIdentity is proven by consumer continuity, never by name similarity: a symbol in one commit is the same symbol as one in an older commit when exactly one declared symbol in the older tree has the same direct consumer set. A rename that also gains or loses a consumer in the same commit is not proven and ends the trail, and an unconsumed symbol's rename cannot be proven at all, so the trail stops as ambiguous rather than guessing. The walk is first-parent only, and one whole-tree build happens per commit that touched the tracked file, so `--window` bounds the cost as well as the depth.\\n\\nThe symbol is queried as it stands at `--at` (HEAD by default). The analysis needs a git repository. This is a report, not a gate: there is no `--fail-on`.",
        after_long_help = "Examples:\\n  sephera archaeology lib.py --symbol divide --path .\\n  sephera archaeology lib.py --symbol divide --path . --at v2.0.0\\n  sephera archaeology lib.py --symbol divide --path . --format json"
    )]
    Archaeology(ArchaeologyArgs),
    /// Compare two refs and report the changes that still compile but change behaviour: swapped call arguments and numeric constant changes
    #[command(
        long_about = "Compare two refs and report the changes that still compile but change behaviour: swapped call arguments and numeric constant changes.\\n\\nTwo detectors, both deliberately narrow, because the general question is undecidable and a detector that answers it by guessing trains a team to ignore it. An argument-order swap is a call site whose arguments are the same multiset in a different order, where the callee's signature did not change — in Rust the swap must also type check at the swapped positions, or it is a compile error, not a silent change. A numeric constant change is a module-level constant whose numeric value changed while still used, in its own file or through an import. String constants are never flagged: a version bump and a rewritten query are indistinguishable statically.\\n\\n`--from` names the older side and is required. `--to` names the newer side; when it is absent, the newer side is the working tree. The analysis needs a git repository. A tree that is not one is an error, not an empty report. This is a report, not a gate: there is no `--fail-on`, because the detector's precision is measured by its corpus, and a gate on an unproven detector would punish asking.",
        after_long_help = "Examples:\\n  sephera behavior --path . --from origin/main\\n  sephera behavior --path . --from v1.0.0 --to v2.0.0\\n  sephera behavior --path . --from origin/main --format json"
    )]
    Behavior(BehaviorArgs),
    /// Dry-run a rename or move: what a real rename would have to touch, and what would block it, without editing the tree
    #[command(
        long_about = "Dry-run a rename or move: what a real rename would have to touch, and what would block it, without editing the tree.\\n\\nA rename is the most common refactor and the one taken on faith: the tool renames the declaration and every import it can find, and the breakage shows up in CI. This measures the rename beforehand. The consumer set is exactly `breaking`'s — a resolved edge to the declaring file naming the symbol, an unresolved item half whose parent statement lands there, a namespace import of the whole file — because a consumer is a consumer whether the symbol broke or is about to move. What `breaking` cannot say, this adds: the destination collision. Renaming onto a name the destination file already declares is a duplicate definition in every language, so it blocks the rename outright; a direct consumer that already declares the new name is a shadowing warning instead, because the severity is language dependent.\\n\\nOnly Rust and Python collect declarations today, so only their symbols can be renamed. The analysis needs one tree, not a git repository: it simulates against the working tree as it stands.",
        after_long_help = "Examples:\\n  sephera simulate lib.py --symbol add --to sum\\n  sephera simulate lib.py --symbol add --to sum --to-file other.py\\n  sephera simulate lib.py --symbol add --to sum --format json"
    )]
    Simulate(SimulateArgs),
    /// Report the declared symbols no test exercises
    #[command(
        long_about = "Report the declared symbols no test exercises, measured statically through the import graph — no test run, no instrumentation.\\n\\nA test file is a root; the files its imports reach are the code the tests can execute; a symbol is exercised when a reached file names it. A symbol a test file names itself is exercised directly, and one named only by a file the tests reach is exercised indirectly: both count as tested, and the report keeps the tiers apart because they mean different things. A namespace import names nothing, so it is never evidence, and only Rust and Python collect declarations, so only their symbols can be reported.\\n\\nTest files are found by the conventions ecosystems agree on: a test, tests, __tests__, spec, or specs directory, or a file named test_*, *_test.*, *.test.*, or *.spec.*. Inline unit tests do not cross an import edge — a Rust #[cfg(test)] module inside lib.rs tests its own file without importing it — so those symbols read as untested unless a separate test file names them; the report's limit, stated plainly, is that it measures coverage through the import graph, the way a new test file would see the code.",
        after_long_help = "Examples:\\n  sephera untested --path .\\n  sephera untested --path . --format json\\n  sephera untested --path . --fail-on 10"
    )]
    Untested(UntestedArgs),
    /// Rank the untested symbols by what hurts most
    #[command(
        long_about = "Rank the declared symbols no test exercises by what hurts most: downstream × churn × (1 + coupled partners).\n\nThis is `untested` joined to `risk`, not a new detector. `untested` says which symbols no test exercises; this says which of those to test next, with the same product `risk` uses but file-level dependents replaced by symbol-level downstream — the files that import that symbol specifically. A file ranking tells you where to look; a symbol ranking tells you which test to write next.\n\nThe ranking lists the FULL untested set, ranked, never filtered: a symbol with zero downstream scores 0 and ranks last, but stays listed, because dropping it would hide work. Tested symbols never appear, direct and indirect tiers alike. The order is total — score desc, file asc, symbol asc — so the same tree twice gives the same ranking. The score is a ranking heuristic, not a probability; file-level inputs (churn, coupled partners) are shared by every symbol in a file, and only downstream refines within a file.\n\nOnly Rust and Python collect declarations, so only their symbols can be ranked. The analysis needs a git history. A tree that is not a git repository is an error, not an empty ranking.",
        after_long_help = "Examples:\n  sephera test-priority --path .\n  sephera test-priority --path . --top 10\n  sephera test-priority --path . --window 1000 --format json"
    )]
    TestPriority(TestPriorityArgs),
    /// Judge one agent edit before it happens, from a hook payload on stdin
    #[command(
        long_about = "Judge one agent edit before it happens.\n\nReads a coding-agent hook payload (Claude Code's PreToolUse shape) on stdin, finds the file being edited, and reports its blast radius and affected tests on stderr. Stdout carries the harness's decision JSON: `allow`, or `deny` when `--max-dependents` is set and the file's blast radius reaches it. A blocked edit also exits 2, so harnesses that honour only exit codes stop the edit too. A payload with no edited file is allowed, with the reason on stderr.\n\nWire it as a PreToolUse hook for the edit tools, passing the project path and the widest radius an agent may touch without asking:\n\n  sephera guard --path . --max-dependents 25",
        after_long_help = "Examples:\n  sephera guard --path . --max-dependents 25\n  sephera guard --file src/lib.rs --path ."
    )]
    Guard(GuardArgs),
    /// Install the change-safety agent skill, or show where it would go
    #[command(
        long_about = "Install the `sephera-change-safety` agent skill into a project, or show where it would go.\n\n`sephera skill install` copies the skill — the workflow that says when to check a change, what threshold stops an edit, and how to fix a break from its suggestion — into the project's agent-skills directory (`.claude/skills` by default, `--dir` overrides). An existing skill is left untouched unless the install is forced.\n\n`sephera skill --print` prints the install path and a one-line summary instead of copying, for agents and scripts that prefer to do it themselves. With no subcommand, `skill` prints the same summary.",
        after_long_help = "Examples:\n  sephera skill install\n  sephera skill install --dir .agents/skills\n  sephera skill install --force\n  sephera skill --print"
    )]
    Skill(SkillArgs),
}

tree_command_args!(
    LocArgs {
        /// Output format for the line-count report
    #[arg(
        long,
        value_enum,
        default_value = "table",
        value_name = "FORMAT",
        help = "Output format for the line-count report. Supports table, markdown, json, and csv.",
        long_help = "Output format for the line-count report. Supports table (default), markdown, json, and csv. `table` is the only format intended for a terminal; the other three are for piping into another tool or writing to a file."
    )]
    pub format: LocOutputFormat,

    }
);

/// Which `.sephera.toml` a command reads, whether to read one at all, and which
/// profile inside it to apply.
///
/// One struct rather than two. `--config`, `--no-config` and `--profile` are a
/// single question -- which settings apply -- and every command that read config
/// read all three: five of six carried the two as adjacent
/// fields, and `watch` carried them too. Splitting them bought nothing a reader
/// could act on, and it was one more group for [`tree_command_args`] to keep
/// consistent across commands.
///
/// Mixed into every command's argument struct so that these flags cannot mean
/// one thing for `context` and another for `graph`. Sharing the struct rather
/// than repeating the fields is what keeps the discovery rule identical.
///
/// The layering rules live here rather than on each flag that participates in
/// them. `--ignore` on `context` used to carry a sentence about `.sephera.toml`
/// and profile precedence that the same flag on `loc` did not, so the
/// precedence model was documented on one command and invisible on five.
#[derive(Debug, Clone, Default, Args)]
pub struct SettingsArgs {
    /// Read shared settings from this file instead of discovering one
    #[arg(
        long,
        value_name = "FILE",
        help = "Explicit `.sephera.toml` path.",
        long_help = "Explicit `.sephera.toml` path, resolved from the current working directory. When present, Sephera skips auto-discovery and loads only this file. The `[project]` table applies to every command; per-command tables such as `[context]` apply only to the commands that read them."
    )]
    pub config: Option<PathBuf>,

    /// Disable `.sephera.toml` loading for this invocation
    #[arg(
        long,
        conflicts_with = "config",
        help = "Disable `.sephera.toml` loading for this invocation.",
        long_help = "Disable `.sephera.toml` loading for this invocation. Both auto-discovery and an explicit `--config` are skipped, and Sephera falls back to built-in defaults plus CLI flags. `[project]` patterns are not applied, which widens the analysis. `.gitignore` and `.sepheraignore` are unaffected; use `--no-gitignore` for those."
    )]
    pub no_config: bool,

    /// Named profile to apply on top of the config
    ///
    /// Flattened into each command rather than declared once as a global
    /// argument, because a global flag disappears from `sephera loc --help` --
    /// and `--profile` is a flag someone has to discover before they can use it.
    /// Declaring it per command keeps it in each command's help, and keeps the
    /// description accurate: what a profile contains depends on the command it is
    /// applied to.
    #[arg(
        long,
        value_name = "NAME",
        conflicts_with = "no_config",
        help = "Apply a named profile from `.sephera.toml`.",
        long_help = "Apply `[profiles.<name>.<command>]` from `.sephera.toml`, layered on top of that command's own table and the shared `[project]` values. For `context` that is `[profiles.<name>.context]`, for `graph` it is `[profiles.<name>.graph]`, and so on for every command. Explicit flags still win over a profile. This flag requires config loading to stay enabled."
    )]
    pub profile: Option<String>,
}

/// Where a rendered report goes when it is not going to the terminal.
///
/// Flattened into every command that can render text. `graph` and `context`
/// each carried their own wording for the same flag, so the one command whose
/// output is a file rather than a table had a differently-worded `--output`
/// from the rest.
#[derive(Debug, Clone, Default, Args)]
pub struct OutputArgs {
    /// Write the report to a file instead of standard output
    #[arg(
        long,
        value_name = "FILE",
        help = "Write the report to a file instead of standard output.",
        long_help = "Write the report to a file instead of standard output. Parent directories are created automatically when needed. Omitting this writes to standard output, which is what a pipe or a redirect expects."
    )]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LocOutputFormat {
    /// Human-readable terminal table.
    #[value(name = "table", help = "Render a terminal table of line metrics.")]
    Table,
    /// Markdown table, for pasting into a pull request or issue.
    #[value(
        name = "markdown",
        help = "Render a Markdown table of line metrics."
    )]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render structured JSON.")]
    Json,
    /// Comma-separated values, for spreadsheet imports.
    #[value(
        name = "csv",
        help = "Render comma-separated values for spreadsheet import."
    )]
    Csv,
}

/// Where an analysis reads from: a local directory, or a repository URL.
///
/// Flattened into every command that can analyse a tree. These three flags were
/// previously written out five times, which meant a change to one copy's wording
/// reached four of the five users and left the fifth describing the old
/// behaviour. A flag's help text is part of the tool's contract, so it belongs in
/// one place.
#[derive(Debug, Clone, Default, Args)]
pub struct SourceArgs {
    /// Path to the project directory to analyze
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "url",
        help = "Path to the project directory to analyze.",
        long_help = "Path to the project directory to analyze. Relative paths are resolved from the current working directory."
    )]
    pub path: Option<PathBuf>,

    /// Git repository URL to analyze directly
    #[arg(
        long,
        value_name = "URL",
        conflicts_with = "path",
        help = "Git repository URL to analyze directly.",
        long_help = "Git repository URL to analyze directly. Supports cloneable repo URLs plus GitHub/GitLab tree URLs."
    )]
    pub url: Option<String>,

    /// Git ref to check out before analysis
    #[arg(
        long = "ref",
        value_name = "REF",
        requires = "url",
        conflicts_with = "path",
        help = "Git ref to check out before analysis.",
        long_help = "Git ref to check out before analysis. This flag only applies to repo URLs and cannot be combined with tree URLs."
    )]
    pub git_ref: Option<String>,
}

/// How an analysis decides which files to leave out.
///
/// Flattened into every command that walks a tree, including `watch`. The
/// `--ignore` wording used to differ on `context`, which appended a sentence
/// about `.sephera.toml` layering that the other five commands did not have --
/// so the same flag read differently depending on which command printed it. The
/// layering is documented once on [`SettingsArgs`] instead.
#[derive(Debug, Clone, Default, Args)]
pub struct IgnoreArgs {
    /// Ignore pattern. Patterns containing `*`, `?`, or `[` are treated as globs; otherwise they are compiled as regexes.
    #[arg(
        long,
        value_name = "PATTERN",
        help = "Ignore pattern for files or directories.",
        long_help = "Ignore pattern for files or directories. Patterns containing `*`, `?`, or `[` are treated as globs and matched against both the file name and the path relative to the base, so `--ignore \"dist/**\"` and `--ignore \"**/node_modules/**\"` both exclude a whole tree. All other patterns are compiled as regular expressions and matched against the relative path, where an unanchored pattern such as `target` matches anywhere in it. Repeat this flag to combine multiple patterns. Patterns from `.sephera.toml` are applied first and flags are appended, so a pattern you typed is never undone by the config file."
    )]
    pub ignore: Vec<String>,

    /// Analyse without reading `.gitignore` or `.sepheraignore`
    #[arg(
        long = "no-gitignore",
        help = "Ignore the repository's own .gitignore and .sepheraignore files.",
        long_help = "Do not apply the repository's own ignore rules. Patterns written in `.gitignore` and `.sepheraignore` are skipped, which counts vendored and generated files that are normally excluded. Explicit `--ignore` patterns and the always-skipped generated trees (`target`, `node_modules`, `dist`, `vendor`, and the rest) still apply, so this widens the analysis rather than disabling exclusion."
    )]
    pub no_gitignore: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SymbolOutputFormat {
    /// Human-readable terminal table.
    #[value(
        name = "table",
        help = "Render a terminal table of declaration counts."
    )]
    Table,
    /// Markdown summary with a table and, in detail mode, every declaration.
    #[value(
        name = "markdown",
        help = "Render Markdown with a per-language table and listed declarations."
    )]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render structured JSON.")]
    Json,
}

tree_command_args!(
    SymbolsArgs {
        /// Output format for the symbol report
    #[arg(
        long,
        value_enum,
        default_value_t = SymbolOutputFormat::Table,
        value_name = "FORMAT",
        help = "Output format for the symbol report. Supports table, markdown, and json."
    )]
    pub format: SymbolOutputFormat,

    /// List every declaration instead of only per-language totals
    #[arg(
        long,
        help = "List every declaration found, with its file, line, and kind.",
        long_help = "List every declaration found, with its file, line, and kind. This produces far more output than the per-language summary, so it is opt-in."
    )]
    pub detail: bool,

    /// Break the report down per file instead of per language
    #[arg(
        long,
        help = "Break the symbol report down per file, heaviest first.",
        long_help = "Break the symbol report down per file, heaviest first. This answers which files carry the most declarations, where the per-language summary cannot."
    )]
    pub by_file: bool,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WatchTarget {
    /// Re-run the dependency graph report.
    #[value(
        name = "graph",
        help = "Watch and re-run the dependency graph analysis."
    )]
    Graph,
    /// Re-run the symbol counts.
    #[value(name = "symbols", help = "Watch and re-run the symbol counts.")]
    Symbols,
    /// Re-run the line-count report.
    #[value(name = "loc", help = "Watch and re-run the line-count analysis.")]
    Loc,
    /// Re-run a reverse dependency query, so the blast radius updates live.
    #[value(
        name = "depends-on",
        help = "Watch and re-run a reverse dependency query."
    )]
    DependsOn,
}

/// `watch` is the one tree command that does not use [`tree_command_args`].
///
/// It watches a local directory, so it has no `--url` and no
/// [`SourceArgs`]; and it renders to the terminal on every change, so it has no
/// [`OutputArgs`]. That leaves two of the four shared groups, which is not enough
/// repetition to justify a second macro and its own way of drifting.
#[derive(Debug, Args)]
pub struct WatchArgs {
    /// What to re-run on change
    #[arg(
        long,
        value_enum,
        value_name = "TARGET",
        required_unless_present = "once",
        help = "What to re-run when the tree changes.",
        long_help = "What to re-run when the tree changes. Choose one of graph, symbols, loc, or depends-on. Required unless --once is passed."
    )]
    pub target: Option<WatchTarget>,

    /// Path to the project directory to watch
    #[arg(
        long,
        value_name = "PATH",
        default_value = ".",
        help = "Path to the project directory to watch.",
        long_help = "Path to the project directory to watch. Relative paths are resolved from the current working directory."
    )]
    pub path: Option<PathBuf>,

    /// Target path for a depends-on query
    #[arg(
        long,
        value_name = "FILE",
        help = "File to trace reverse dependencies for, used with --target depends-on.",
        long_help = "File to trace reverse dependencies for, used with --target depends-on. Runs the query after every change."
    )]
    pub on: Option<String>,

    /// Exit after one run instead of continuing to watch
    #[arg(
        long,
        help = "Run once and exit, which is what invoking the analysis command directly would do.",
        long_help = "Run once and exit. Combined with --target this behaves exactly like invoking that analysis command, which is useful when a script wants to exercise the same argument parsing."
    )]
    pub once: bool,

    /// How to decide which files to leave out
    #[command(flatten)]
    pub ignore_args: IgnoreArgs,

    /// Shared settings source and named profile
    #[command(flatten)]
    pub settings: SettingsArgs,
}

tree_command_args!(
    ContextArgs {
        /// List available context profiles from the resolved `.sephera.toml` file and exit.
    #[arg(
        long,
        conflicts_with = "no_config",
        conflicts_with = "ignore",
        conflicts_with = "focus",
        conflicts_with = "diff",
        conflicts_with = "budget",
        conflicts_with = "format",
        conflicts_with = "output",
        conflicts_with = "closure",
        help = "List available context profiles and exit.",
        long_help = "List available context profiles from the resolved `.sephera.toml` file and exit. Sephera uses either `--config <FILE>` or the normal auto-discovery rules. This mode does not build a context pack."
    )]
    pub list_profiles: bool,

    /// Focus path inside the base path. Repeat to prioritize multiple files or directories.
    #[arg(
        long,
        value_name = "PATH",
        help = "Focused file or directory inside the base path.",
        long_help = "Focused file or directory inside the selected analysis base. Repeat this flag to prioritize multiple files or directories. Values from `.sephera.toml` are loaded first, then profile values are appended, then repeated CLI flags are appended. Focused paths must resolve inside the local `--path` or the remote repo/tree scope selected by `--url`."
    )]
    pub focus: Vec<PathBuf>,

    /// Focus the pack on a named declaration instead of a whole file
    #[arg(
        long,
        value_name = "NAME",
        help = "Pack only the declaration with this name, rather than its whole file. Repeatable.",
        long_help = "Pack only the declaration with this name, rather than its whole file. Repeat this flag to pack several declarations. The name is matched case-insensitively and partially, so `resolve` also finds `resolve_source`. A name matching several declarations is reported rather than guessed; a name matching none is reported while the rest of the pack still builds. Cannot be combined with `--diff`, which selects whole changed files."
    )]
    pub focus_symbol: Vec<String>,

    /// Pack each file's in-project dependencies directly behind it
    #[arg(
        long,
        help = "Pack each file's in-project dependencies directly behind it.",
        long_help = "Build the import graph and pack each file's dependency chain directly behind it, ahead of unrelated files that ranked below it -- so a file in the pack is accompanied by what it imports, instead of its dependencies competing as general files and losing on size to files nothing in the pack needs. The pack's metadata counts the files closure pulled in. Costs one graph build. Cannot be combined with `--focus-symbol`, which packs parts of files rather than whole files behind importers."
    )]
    pub closure: bool,

    /// Git diff source used to prioritize changed files in the context pack.
    #[arg(
        long,
        value_name = "SPEC",
        help = "Git diff source used to prioritize changed files in the context pack.",
        long_help = "Git diff source used to prioritize changed files in the context pack. Built-in keywords are `working-tree`, `staged`, and `unstaged`. Any other value is treated as a single Git base ref and compared against `HEAD` through merge-base semantics. Values from `.sephera.toml` are loaded first, then profile values override them, then an explicit CLI value wins."
    )]
    pub diff: Option<String>,

    /// Approximate token budget, for example `32000`, `32k`, or `1m`
    #[arg(
        long,
        value_parser = parse_token_budget,
        value_name = "TOKENS",
        help = "Approximate token budget, for example `32000`, `32k`, or `1m`.",
        long_help = "Approximate token budget for the generated context pack. This is a model-agnostic estimate, not tokenizer-exact accounting. Supported suffixes are `k` for thousands and `m` for millions. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise the built-in default of `128k`."
    )]
    pub budget: Option<u64>,

    /// Compression mode for context excerpts using Tree-sitter AST extraction
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        help = "Compress context excerpts using Tree-sitter AST extraction.",
        long_help = "Compress context excerpts using Tree-sitter AST extraction. Use `signatures` to keep only function signatures, type definitions, and imports (typically 50–70 % fewer tokens). Use `skeleton` to also keep top-level control flow. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise no compression."
    )]
    pub compress: Option<ContextCompress>,

    /// Output format for the generated context pack
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Output format for the generated context pack.",
        long_help = "Output format for the generated context pack. Use `markdown` for a human-readable export that is easy to paste into chat tools, or `json` for machine-readable automation. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise the built-in default of `markdown`."
    )]
    pub format: Option<ContextFormat>,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
pub enum ContextCompress {
    #[value(
        name = "signatures",
        help = "Extract only function signatures, type definitions, and imports."
    )]
    #[serde(rename = "signatures")]
    Signatures,
    #[value(
        name = "skeleton",
        help = "Extract signatures plus top-level control flow."
    )]
    #[serde(rename = "skeleton")]
    Skeleton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
pub enum ContextFormat {
    #[value(
        name = "markdown",
        help = "Render a human-readable context pack for copy-paste workflows."
    )]
    #[serde(rename = "markdown")]
    Markdown,
    #[value(
        name = "json",
        help = "Render a machine-readable context pack for automation."
    )]
    #[serde(rename = "json")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
pub enum GraphOutputFormat {
    #[value(
        name = "json",
        help = "Render the dependency graph as structured JSON."
    )]
    #[serde(rename = "json")]
    Json,
    #[value(
        name = "markdown",
        help = "Render the dependency graph as Markdown with a Mermaid diagram."
    )]
    #[serde(rename = "markdown")]
    Markdown,
    #[value(
        name = "xml",
        help = "Render the dependency graph as structured XML."
    )]
    #[serde(rename = "xml")]
    Xml,
    #[value(
        name = "dot",
        help = "Render the dependency graph in Graphviz DOT format."
    )]
    #[serde(rename = "dot")]
    Dot,
}

tree_command_args!(
    GraphArgs {
        /// Focus path inside the base path. Repeat to analyze only specific files or directories.
    #[arg(
        long,
        value_name = "PATH",
        help = "Focused file or directory inside the base path.",
        long_help = "Focused file or directory inside the selected analysis base. Repeat this flag to limit the graph to specific files or directories."
    )]
    pub focus: Vec<PathBuf>,

    /// Maximum depth for transitive dependency resolution
    #[arg(
        long,
        value_name = "DEPTH",
        help = "Maximum depth for transitive dependencies.",
        long_help = "Maximum depth for transitive dependency resolution. 0 means direct dependencies only. Omit for unlimited depth."
    )]
    pub depth: Option<u32>,

    /// Output format for the graph report
    #[arg(
        long,
        value_enum,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format for the dependency graph report.",
        long_help = "Output format for the dependency graph report. Supports json, markdown (with Mermaid diagram), xml, and dot (Graphviz)."
    )]
    pub format: GraphOutputFormat,

    /// Show what depends on the specified file
    #[arg(
        long,
        value_name = "FILE",
        help = "Show all files that depend on the specified file.",
        long_help = "Show all files that import or depend on the specified file path. The path should be relative to the selected analysis base."
    )]
    pub what_depends_on: Option<String>,

    /// Fail with exit code 2 when the project has this many or more circular dependencies
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many circular dependencies.",
        long_help = "Exit with code 2 when the project has at least this many circular dependencies. The report is still printed; only the exit code changes. The limit is the first failing value, so `--fail-on-cycles 1` fails on a single cycle."
    )]
    pub fail_on_cycles: Option<u64>,

    /// Fail with exit code 2 when this many or more local import paths fail to resolve
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many unresolved local paths.",
        long_help = "Exit with code 2 when at least this many local-looking import paths fail to resolve. An unresolved path was meant to name a file in this project and was not found, so it is a resolver gap rather than a dependency, and a blast radius that counts one silently omits a file. The report is still printed; only the exit code changes."
    )]
    pub fail_on_unresolved: Option<u64>,

    /// Report the blast radius of every file changed since a Git base
    #[arg(
        long,
        value_name = "SPEC",
        conflicts_with = "what_depends_on",
        help = "Report what every changed file reaches, against a Git base.",
        long_help = "Report the blast radius of every file changed against a Git base, so a reviewer or a pre-commit hook can see what this change reaches. Accepts a ref such as `HEAD~1` or `origin/master`, or the keywords `working-tree` and `staged`. The graph is built once and every changed file is measured against it. Deleted files are skipped and listed separately, because a file that no longer exists has no blast radius. Cannot be combined with `--what-depends-on`, which answers about one named file rather than about a change."
    )]
    pub diff: Option<String>,

    /// Drop imports that describe types rather than runtime dependencies
    #[arg(
        long,
        help = "Exclude type aliases and wildcard imports from the graph.",
        long_help = "Exclude type aliases and wildcard imports from the graph. A `type X = Y` alias or an `import ... .*` wildcard names a namespace rather than a runtime dependency, so the edges they create never fail at runtime."
    )]
    pub exclude_types: bool,

    }
);

tree_command_args!(
    #[doc = "Report the blast radius of one file."]
    ImpactArgs {
        /// Files to report the blast radius of
    #[arg(
        value_name = "FILE",
        required = true,
        num_args = 1..,
        help = "File whose blast radius to report. Repeat for several.",
        long_help = "Path of the file whose blast radius to report, relative to the analysis base. Repeat the flag or pass several paths to ask about more than one; the graph is built once and every path is measured against it, so asking about five files costs about the same as asking about one. Paths are listed widest first. A path that matches nothing in the analysis is an error rather than an empty report, because an empty report and a typo are indistinguishable once it is on a screen."
    )]
    pub files: Vec<String>,

    /// Maximum distance from the file whose dependents are reported
    #[arg(
        long,
        value_name = "DEPTH",
        help = "Maximum distance to report.",
        long_help = "How many hops away a dependent may be. 1 reports only files that import this one directly; 2 also reports files that import those. Omit for the whole transitive closure."
    )]
    pub depth: Option<u32>,

    /// Restrict the reported dependents to a subtree
    #[arg(
        long,
        value_name = "PATH",
        help = "Report only the dependents inside this path. Repeat for several.",
        long_help = "Report only the dependents inside this path, which matches a path exactly or as a directory prefix at a `/` boundary. Repeat the flag to accept several scopes. This narrows the answer, not the analysis: the target itself is reported wherever it lives, so `--focus crates/x` against a file in `crates/y` answers what in `x` would break from a change in `y`. Without this flag every dependent in the repository is reported."
    )]
    pub focus: Vec<PathBuf>,

    /// Drop imports that describe types rather than runtime dependencies
    #[arg(
        long,
        help = "Exclude type aliases and wildcard imports from the blast radius.",
        long_help = "Exclude type aliases and wildcard imports from the blast radius. A `type X = Y` alias or an `import ... .*` wildcard names a namespace rather than a runtime dependency, so the edges they create never fail at runtime."
    )]
    pub exclude_types: bool,

    /// Narrow the report to one exported name in the file
    #[arg(
        long,
        value_name = "NAME",
        help = "Report only the files that use this name from the target.",
        long_help = "Narrow the report from the file to one name it exports: the files that import that name from the target, the files that import the target's namespace wholesale (they may use the name without naming it, and are reported apart rather than counted as users), and the files downstream of those. A file that imports the target only for a different name is not in this report at all. With `--fail-on`, the threshold compares against the files that name the symbol. The name is matched as the importing statements spell it -- an item path's last segment, or the names a JavaScript import clause records -- so it is the name as declared in the target, not a local alias for it."
    )]
    pub symbol: Option<String>,

    /// Fail with exit code 2 when this many or more files depend on a target
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many dependents.",
        long_help = "Exit with code 2 when at least this many files depend on any one of the targets. With several targets, every target at or over the limit is named, because a run that violates three rules should not take three CI runs to discover. The report is still printed; only the exit code changes. The limit is the first failing value, so `--fail-on 40` fails on the fortieth dependent and not the thirty-ninth. With `--symbol`, the count compared is the files that name the symbol."
    )]
    pub fail_on: Option<u64>,

    /// Output format for the blast radius
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the blast radius. Supports markdown and json.",
        long_help = "Output format for the blast radius. Supports markdown and json."
    )]
    pub format: ImpactOutputFormat,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImpactOutputFormat {
    /// Markdown list of dependents.
    #[value(name = "markdown", help = "Render the blast radius as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the blast radius as JSON.")]
    Json,
}

tree_command_args!(
    #[doc = "Report which tests a change has to answer for."]
    AffectedTestsArgs {
        /// Files whose affected tests to report
    #[arg(
        value_name = "FILE",
        required = true,
        num_args = 1..,
        help = "File whose affected tests to report. Repeat for several.",
        long_help = "Path of the file whose affected tests to report, relative to the analysis base. Repeat the flag or pass several paths to ask about more than one; the graph is built once and every path is measured against it. Paths are listed most affected first. A path that matches nothing in the analysis is an error rather than an empty report, because an empty report and a typo are indistinguishable once it is on a screen."
    )]
    pub files: Vec<String>,

    /// Maximum distance from the file whose tests are reported
    #[arg(
        long,
        value_name = "DEPTH",
        help = "Maximum distance to report.",
        long_help = "How many hops away a test may be. 1 reports only tests that import this file directly; 2 also reports tests that import those importers. Omit for the whole transitive closure."
    )]
    pub depth: Option<u32>,

    /// Restrict the reported tests to a subtree
    #[arg(
        long,
        value_name = "PATH",
        help = "Report only the tests inside this path. Repeat for several.",
        long_help = "Report only the tests inside this path, which matches a path exactly or as a directory prefix at a `/` boundary. Repeat the flag to accept several scopes. This narrows the answer, not the analysis: the target itself is reported wherever it lives. Without this flag every test in the repository is considered."
    )]
    pub focus: Vec<PathBuf>,

    /// Drop imports that describe types rather than runtime dependencies
    #[arg(
        long,
        help = "Exclude type aliases and wildcard imports from the analysis.",
        long_help = "Exclude type aliases and wildcard imports from the analysis. A `type X = Y` alias or an `import ... .*` wildcard names a namespace rather than a runtime dependency, so the edges they create never fail at runtime and the tests behind them are not answering for a change."
    )]
    pub exclude_types: bool,

    /// Run the planned test commands after reporting them
    #[arg(
        long,
        help = "Run the planned test commands after reporting them.",
        long_help = "Execute the commands in the report's plan, from the analysis base, in plan order. Test output is the runner's own. The first failing command stops the run and fails this command, because a plan whose first suite failed has already answered. Test files with no automatic runner mapping are listed and skipped, never silently counted as run."
    )]
    pub run: bool,

    /// Fail with exit code 2 when a target reaches no test file
    #[arg(
        long,
        help = "Fail with exit code 2 when a target reaches no test file.",
        long_help = "Exit with code 2 when any one of the targets has no affected test. Every unwitnessed target is named, because a run that violates the rule three times should not take three CI runs to discover. The report is still printed; only the exit code changes."
    )]
    pub require_tests: bool,

    /// Output format for the test plan
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the test plan. Supports markdown and json.",
        long_help = "Output format for the test plan. Supports markdown and json."
    )]
    pub format: TestImpactOutputFormat,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TestImpactOutputFormat {
    /// Markdown list of test files with the commands that run them.
    #[value(name = "markdown", help = "Render the test plan as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the test plan as JSON.")]
    Json,
}

tree_command_args!(
    #[doc = "Show the shortest import chain between two files."]
    WhyArgs {
        /// The dependent file: the one that ends up needing the other
    #[arg(
        value_name = "FROM",
        help = "File that depends on the other. The chain starts here.",
        long_help = "Path of the dependent file, relative to the analysis base. The reported chain starts at this file and follows its imports, so `sephera why a.rs b.rs` reads as \"a depends on b because a imports ..., which imports b\". Reverse the pair to ask the reverse question."
    )]
    pub from_file: String,

    /// The dependency: the file the chain ends at
    #[arg(
        value_name = "TO",
        help = "File the first one depends on. The chain ends here.",
        long_help = "Path of the dependency, relative to the analysis base. Changing this file reaches FROM through exactly the chain reported, which is why it belongs in FROM's blast radius."
    )]
    pub to_file: String,

    /// Drop imports that describe types rather than runtime dependencies
    #[arg(
        long,
        help = "Exclude type aliases and wildcard imports from the chain.",
        long_help = "Exclude type aliases and wildcard imports from the chain. A `type X = Y` alias or an `import ... .*` wildcard names a namespace rather than a runtime dependency, so a chain through one never fails at runtime."
    )]
    pub exclude_types: bool,

    /// Output format for the chain
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the chain. Supports markdown and json.",
        long_help = "Output format for the chain. Supports markdown and json."
    )]
    pub format: WhyOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Find file pairs that change together but share no import between them."]
    CouplingArgs {
        /// How many commits of history to read
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "500",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Read this many recent commits.",
        long_help = "How far back the history window reaches, in commits. The window bounds every count in the report: a pair's co-changes are counted inside it, and a coupling that stopped happening before the window starts is not reported, which is the right answer for a report about how the tree changes now. Larger windows are slower to read and likelier to include a period when the tree was shaped differently."
    )]
    pub commits: u32,

    /// Minimum shared commits for a pair to be reported
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "3",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Report pairs that changed together at least this many times.",
        long_help = "The bar a pair has to clear. One shared commit is a coincidence any two files can have; three inside a few hundred is a pattern. Raising it answers a stricter question, and the report states the bar it used, so an empty report can be told apart from a bar set too high."
    )]
    pub min_co_changes: u64,

    /// Output format for the coupling report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the coupling report. Supports markdown and json.",
        long_help = "Output format for the coupling report. Supports markdown and json."
    )]
    pub format: CouplingOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Review a change before it lands."]
    ReviewArgs {
        /// Reference the working tree is compared against
    #[arg(
        long,
        value_name = "REF",
        help = "Compare the working tree against this reference (default: HEAD).",
        long_help = "The reference the change is read against. The default, HEAD, reviews everything uncommitted -- staged, unstaged, deleted, and untracked. Naming a branch or tag reviews the whole body of work since that point, committed and uncommitted together, which is the form a branch review takes before a push."
    )]
    pub against: Option<String>,

        /// How many commits of history to read for coupling warnings
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "500",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Read this many recent commits for the coupling warnings.",
        long_help = "How far back the history window reaches, in commits, for the coupling warnings. The window bounds every count behind a warning: a partner is only named on co-changes inside it. It means the same thing it means for `coupling`, whose report can be consulted for the pairs themselves."
    )]
    pub commits: u32,

    /// Minimum shared commits for a coupling warning
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "3",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Warn about partners that changed together at least this many times.",
        long_help = "The bar a hidden pair has to clear before the review warns about it. One shared commit is a coincidence any two files can have; three inside a few hundred is a pattern. The report states the bar it used."
    )]
    pub min_co_changes: u64,

    /// Output format for the review
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the review. Supports markdown and json.",
        long_help = "Output format for the review. Supports markdown and json."
    )]
    pub format: ReviewOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Rank the files where a change is most likely to hurt."]
    RiskArgs {
        /// How many commits of history to read
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "500",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Read this many recent commits.",
        long_help = "How far back the history window reaches, in commits. Churn and coupling are both counted inside it: a file that stopped changing before the window starts has no churn here, which is the right answer for a ranking about how the tree changes now. It means the same thing it means for `coupling`."
    )]
    pub commits: u32,

    /// Minimum shared commits for a pair to count as coupled
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "3",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Count a hidden pair as a coupled partner only at this many co-changes.",
        long_help = "The bar a hidden pair has to clear before it counts as one of a file's coupled partners. One shared commit is a coincidence any two files can have; three inside a few hundred is a pattern. It is `coupling`'s bar, with `coupling`'s meaning."
    )]
    pub min_co_changes: u64,

    /// How many files the ranking returns
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "20",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Return at most this many files, riskiest first.",
        long_help = "The length of the ranking. The files below the cut scored lower on the same product; the report is a head of a ranking, not the whole of it, and says how long it is."
    )]
    pub limit: u32,

    /// Output format for the ranking
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the ranking. Supports markdown and json.",
        long_help = "Output format for the ranking. Supports markdown and json."
    )]
    pub format: RiskOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Find the files no entry point reaches."]
    DeadArgs {
        /// Entry points the conventions cannot see (repeatable)
    #[arg(
        long,
        value_name = "FILE",
        help = "Name an entry point a path cannot show (repeatable).",
        long_help = "An entry point the path conventions cannot see: a Java entry class, a plugin loaded by name at runtime, a file a framework's convention scans for. Named roots are listed in the report beside the conventional ones, so the verdict can be checked against the full set it was measured from. A root that matches no file in the analysis is an error."
    )]
    pub root: Vec<String>,

    /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown and json.",
        long_help = "Output format for the report. Supports markdown and json."
    )]
    pub format: DeadOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Compare two refs and report which declared symbols broke, joined to the files that still use them."]
    BreakingArgs {
        /// The older reference: the side symbols are compared from
    #[arg(
        long,
        value_name = "REF",
        help = "The older reference to compare from.",
        long_help = "The older reference: a branch, tag, or commit the symbols are compared from. Required, because a comparison without a starting point compares against nothing. `origin/main` is the usual spelling for a branch review."
    )]
    pub from: String,

    /// The newer reference; when absent, the working tree
    #[arg(
        long,
        value_name = "REF",
        help = "The newer reference to compare to; the working tree when absent.",
        long_help = "The newer reference to compare to. When absent, the newer side is the working tree itself — staged, unstaged, and untracked files included — which is the form a branch review takes before it is pushed. When present, both sides are checked out into temporary worktrees, so the working tree is left untouched."
    )]
    pub to: Option<String>,

    /// Fail with exit code 2 when this many broken symbols have direct consumers
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many broken symbols with direct consumers.",
        long_help = "Exit with code 2 when at least this many broken symbols still have direct consumers in the newer tree. A break nobody uses does not fail the run: it is the clean kind of breaking, and a gate that fired on it would train a pipeline to ignore the gate. The report is still printed; only the exit code changes."
    )]
    pub fail_on: Option<u64>,

    /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown, json, and sarif.",
        long_help = "Output format for the report. Supports markdown, json, and sarif."
    )]
    pub format: BreakingOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Predict the semantic conflicts between two sides of a merge."]
    MergeArgs {
        /// The ours reference: one side of the merge
    #[arg(
        long,
        value_name = "REF",
        help = "The ours reference: one side of the merge.",
        long_help = "The ours reference: one side of the merge. A branch, tag, or commit whose contract changes are judged against the other side's new consumers. Required, because a comparison without both sides compares nothing."
    )]
    pub ours: String,

    /// The theirs reference: the other side of the merge
    #[arg(
        long,
        value_name = "REF",
        help = "The theirs reference: the other side of the merge.",
        long_help = "The theirs reference: the other side of the merge. A branch, tag, or commit whose new consumers are judged against the first side's contract changes. Required."
    )]
    pub theirs: String,

    /// The base reference; when absent, the merge base of ours and theirs
    #[arg(
        long,
        value_name = "REF",
        help = "The base reference; the merge base of ours and theirs when absent.",
        long_help = "The base reference: the fork point the two sides diverged from. When absent, the merge base of ours and theirs is used, which is the fork point git itself would merge from. Naming it explicitly is for the case where the fork point is a tag or a commit the branches no longer share."
    )]
    pub base: Option<String>,

    /// Fail with exit code 2 when this many proven conflicts are predicted
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many proven conflicts.",
        long_help = "Exit with code 2 when at least this many proven conflicts are predicted. Warnings are never counted: a signature change against a new consumer is suspicious, not proven, and a gate that fired on it would train a pipeline to ignore the gate. The report is still printed; only the exit code changes."
    )]
    pub fail_on: Option<u64>,

    /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown and json.",
        long_help = "Output format for the report. Supports markdown and json."
    )]
    pub format: MergeOutputFormat,

    }
);

tree_command_args!(
    #[doc = "Compare two refs and report every declared symbol that was added, removed, or reshaped: the API changelog."]
    DiffArgs {
        /// The older reference: the side symbols are compared from
    #[arg(
        long,
        value_name = "REF",
        help = "The older reference to compare from.",
        long_help = "The older reference: a branch, tag, or commit the symbols are compared from. Required, because a comparison without a starting point compares against nothing. `origin/main` is the usual spelling for a branch review."
    )]
    pub from: String,

    /// The newer reference; when absent, the working tree
    #[arg(
        long,
        value_name = "REF",
        help = "The newer reference to compare to; the working tree when absent.",
        long_help = "The newer reference to compare to. When absent, the newer side is the working tree itself — staged, unstaged, and untracked files included — which is the form a branch review takes before it is pushed. When present, both sides are checked out into temporary worktrees, so the working tree is left untouched."
    )]
    pub to: Option<String>,

    /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown and json.",
        long_help = "Output format for the report. Supports markdown and json."
    )]
    pub format: DiffOutputFormat,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MergeOutputFormat {
    /// Markdown list of predicted conflicts.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DiffOutputFormat {
    /// Markdown list of changed symbols.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
}

tree_command_args!(
    #[doc = "Trace one symbol through history: every rename and move, its former names, and who changed it."]
    ArchaeologyArgs {
        /// The file the symbol lives in, spelled as the graph spells it
    #[arg(
        value_name = "FILE",
        help = "The file the symbol lives in, spelled as the graph spells it.",
        long_help = "The file the symbol lives in, spelled as the graph spells it — relative to the analysis root, with forward slashes. The trail follows the symbol out of this file when it moves, so the file named here is where the story starts, not where it ends."
    )]
    pub file: String,

        /// The symbol to trace
    #[arg(
        long,
        value_name = "NAME",
        help = "The symbol to trace.",
        long_help = "The symbol to trace, as declared in the file. It must be declared at the starting ref; a name that is not there is an error, not an empty trail."
    )]
    pub symbol: String,

        /// The ref the symbol is queried at; HEAD when absent
    #[arg(
        long = "at",
        value_name = "REF",
        default_value = "HEAD",
        help = "The ref the symbol is queried at; HEAD when absent.",
        long_help = "The ref the symbol is queried at. HEAD by default: the symbol as it stands at the tip of the branch is the query, and the trail walks backwards from there."
    )]
    pub start_ref: String,

        /// Maximum commits examined while walking backwards
    #[arg(
        long,
        value_name = "N",
        default_value = "500",
        help = "Maximum commits examined while walking backwards.",
        long_help = "Maximum commits examined while walking backwards. One whole-tree build happens per commit that touched the tracked file, so the window bounds the cost as well as the depth; when it runs out, the trail stops with a note rather than a birth."
    )]
    pub window: usize,

        /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown and json.",
        long_help = "Output format for the report. Supports markdown and json."
    )]
    pub format: ArchaeologyOutputFormat,

    }
);

/// Output format for the archaeology report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ArchaeologyOutputFormat {
    /// Markdown report, oldest step first.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
}

tree_command_args!(
    #[doc = "Compare two refs and report the changes that still compile but change behaviour: swapped call arguments and numeric constant changes."]
    BehaviorArgs {
        /// The older reference: the side symbols are compared from
    #[arg(
        long,
        value_name = "REF",
        help = "The older reference to compare from.",
        long_help = "The older reference: a branch, tag, or commit the symbols are compared from. Required, because a comparison without a starting point compares against nothing. `origin/main` is the usual spelling for a branch review."
    )]
    pub from: String,

        /// The newer reference; when absent, the working tree
    #[arg(
        long,
        value_name = "REF",
        help = "The newer reference to compare to; the working tree when absent.",
        long_help = "The newer reference to compare to. When absent, the newer side is the working tree itself — staged, unstaged, and untracked files included — which is the form a branch review takes before it is pushed. When present, both sides are checked out into temporary worktrees, so the working tree is left untouched."
    )]
    pub to: Option<String>,

        /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown and json.",
        long_help = "Output format for the report. Supports markdown and json."
    )]
    pub format: BehaviorOutputFormat,

    }
);

/// Output format for the behavior report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BehaviorOutputFormat {
    /// Markdown report with the count first.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
}

tree_command_args!(
    #[doc = "Dry-run a rename or move: what a real rename would have to touch, and what would block it, without editing the tree."]
    SimulateArgs {
        /// The file that declares the symbol today
    #[arg(
        value_name = "FILE",
        help = "File that declares the symbol to rename.",
        long_help = "Path of the file that declares the symbol, relative to the analysis base. A path that matches nothing in the analysis is an error rather than an empty report, because an empty report and a typo are indistinguishable once it is on a screen."
    )]
    pub file: String,

        /// The symbol's name today
    #[arg(
        long,
        value_name = "NAME",
        help = "The symbol to rename.",
        long_help = "The symbol's name today, as declared in the file. Required, because a rename without a starting name renames nothing."
    )]
    pub symbol: String,

        /// The name it would take
    #[arg(
        long,
        value_name = "NAME",
        help = "The name the symbol would take.",
        long_help = "The name the symbol would take. Renaming onto a name the destination file already declares is a duplicate definition, so the report comes back blocked rather than safe."
    )]
    pub to: String,

        /// The file it would live in; the file it already lives in when absent
    #[arg(
        long = "to-file",
        value_name = "FILE",
        help = "Move the symbol to this file instead of renaming in place.",
        long_help = "The file the symbol would live in. When absent, the rename happens in place. When present, the file must already be in the tree: a move target that does not exist yet is a file creation, a different feature."
    )]
    pub to_file: Option<String>,

        /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown and json.",
        long_help = "Output format for the report. Supports markdown and json."
    )]
    pub format: SimulateOutputFormat,

    }
);

/// Output format for the simulate report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SimulateOutputFormat {
    /// Markdown report with the verdict first.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
}

tree_command_args!(
    #[doc = "Report the declared symbols no test exercises."]
    UntestedArgs {
        /// Fail with exit code 2 when this many symbols are untested
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many untested symbols.",
        long_help = "Exit with code 2 when at least this many declared symbols are exercised by nothing. The report is still printed; only the exit code changes, so a pipeline can gate on test coverage without running the tests."
    )]
    pub fail_on: Option<u64>,

    /// Output format for the report
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the report. Supports markdown, json, and sarif.",
        long_help = "Output format for the report. Supports markdown, json, and sarif."
    )]
    pub format: UntestedOutputFormat,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UntestedOutputFormat {
    /// Markdown list of untested symbols.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
    /// SARIF 2.1.0 for code scanning.
    #[value(name = "sarif", help = "Render the report as SARIF 2.1.0.")]
    Sarif,
}

tree_command_args!(
    #[doc = "Rank the untested symbols by what hurts most."]
    TestPriorityArgs {
        /// How many commits of history to read
    #[arg(
        long,
        value_name = "COUNT",
        default_value = "500",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Read this many recent commits.",
        long_help = "How far back the history window reaches, in commits. Churn and coupling are both counted inside it: a file that stopped changing before the window starts has no churn here, which is the right answer for a ranking about how the tree changes now. It means the same thing it means for `coupling`."
    )]
    pub window: u32,

        /// How many symbols the ranking returns
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Return at most this many symbols, highest score first.",
        long_help = "The length of the ranking. Without it the full untested set is listed, ranked; the tail is the zero-downstream symbols, which score 0 and rank last but stay listed because dropping them would hide work."
    )]
    pub top: Option<u32>,

        /// Output format for the ranking
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the ranking. Supports markdown and json.",
        long_help = "Output format for the ranking. Supports markdown and json."
    )]
    pub format: TestPriorityOutputFormat,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TestPriorityOutputFormat {
    /// Markdown ranking of untested symbols.
    #[value(name = "markdown", help = "Render the ranking as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the ranking as JSON.")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BreakingOutputFormat {
    /// Markdown list of broken symbols.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
    /// SARIF 2.1.0 for code scanning.
    #[value(name = "sarif", help = "Render the report as SARIF 2.1.0.")]
    Sarif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeadOutputFormat {
    /// Markdown list of dead files.
    #[value(name = "markdown", help = "Render the report as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the report as JSON.")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RiskOutputFormat {
    /// Markdown ranking.
    #[value(name = "markdown", help = "Render the ranking as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the ranking as JSON.")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReviewOutputFormat {
    /// Markdown review of the change.
    #[value(name = "markdown", help = "Render the review as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the review as JSON.")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CouplingOutputFormat {
    /// Markdown list of coupled pairs.
    #[value(
        name = "markdown",
        help = "Render the coupling report as Markdown."
    )]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the coupling report as JSON.")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WhyOutputFormat {
    /// Markdown chain of imports.
    #[value(name = "markdown", help = "Render the chain as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the chain as JSON.")]
    Json,
}

/// Arguments for `sephera guard`.
///
/// Not built by `tree_command_args!`: the guard analyses one local project
/// named by `--path` and nothing else. There is no report to write to a
/// file -- stdout is the harness's decision channel -- and no remote source
/// a hook could be pointed at, so the shared groups' URL, output, and
/// settings fields would be flags that parse and do nothing.
#[derive(Debug, Args)]
pub struct GuardArgs {
    /// Project directory to analyse
    #[arg(
        long,
        value_name = "PATH",
        help = "Path to the project directory to analyze.",
        long_help = "Path to the project directory to analyze, resolved from the current working directory. The edited file is matched against this project's graph."
    )]
    pub path: Option<PathBuf>,

    /// Judge this file instead of reading a hook payload from stdin
    #[arg(
        long,
        value_name = "FILE",
        help = "Judge this file instead of reading a hook payload from stdin.",
        long_help = "Path of the file to judge, absolute or relative to the project. Skips stdin entirely, which is how a pre-commit hook or a curious person runs the same judgement a harness would get."
    )]
    pub file: Option<String>,

    /// Maximum distance from the file to measure
    #[arg(
        long,
        value_name = "DEPTH",
        help = "Maximum distance to measure.",
        long_help = "How many hops away a dependent may be when measuring the edited file's blast radius. Omit for the whole transitive closure."
    )]
    pub depth: Option<u32>,

    /// Deny the edit at this many dependents
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Deny the edit at this many dependents.",
        long_help = "Deny the edit when at least this many files depend on the edited file, and exit 2. The limit is the first failing value, so `--max-dependents 25` denies the twenty-fifth dependent, not the twenty-sixth. Without this flag every edit is allowed and the guard is a report only."
    )]
    pub max_dependents: Option<u64>,
}

/// Arguments for `sephera skill`.
///
/// Not built by `tree_command_args!`: the skill copies files and
/// analyses nothing, so the shared groups' source, ignore, output,
/// and settings fields would be flags that parse and do nothing.
/// `--path` and `--dir` are global so they parse on either side
/// of the `install` subcommand: `skill install --dir X` and
/// `skill --dir X install` are the same request, and a flag that
/// only parsed in one spelling would be a trap, not an option.
#[derive(Debug, Args)]
pub struct SkillArgs {
    /// Project directory to install into
    #[arg(
        long,
        value_name = "PATH",
        global = true,
        help = "Project directory to install into.",
        long_help = "Project directory to install into, resolved from the current working directory. Defaults to the current directory. A relative `--dir` is resolved against it."
    )]
    pub path: Option<PathBuf>,

    /// Skills directory override
    #[arg(
        long,
        value_name = "DIR",
        global = true,
        help = "Skills directory to install into.",
        long_help = "Skills directory to install into. Relative paths resolve against the project directory. Defaults to `.claude/skills` inside the project."
    )]
    pub dir: Option<PathBuf>,

    /// Print the install path and a summary instead of installing
    #[arg(
        long,
        help = "Print the install path and a one-line summary instead of installing.",
        long_help = "Print where `install` would copy the skill and a one-line summary of what it does, instead of copying anything. Cannot be combined with `install`."
    )]
    pub print: bool,

    #[command(subcommand)]
    pub command: Option<SkillCommand>,
}

/// Subcommands of `sephera skill`.
#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Copy the skill into the project's agent-skills directory
    #[command(
        long_about = "Copy the `sephera-change-safety` skill into the project's agent-skills directory.\n\nThe target is `<skills-dir>/sephera-change-safety`, where `<skills-dir>` is `--dir` or `.claude/skills` inside the project. An existing skill is left untouched unless forced: silently clobbering a skill the user tuned would be the kind of harm this tool does not do.",
        after_long_help = "Examples:\n  sephera skill install\n  sephera skill install --dir .agents/skills\n  sephera skill install --force"
    )]
    Install {
        /// Overwrite an existing skill
        #[arg(
            long,
            help = "Overwrite the skill if it already exists.",
            long_help = "Overwrite the skill if it already exists at the target. Without it, installing over an existing skill is an error."
        )]
        force: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Commands, ContextFormat};

    /// Commands that walk a tree, and so must accept the shared groups.
    const TREE_COMMANDS: [&str; 8] = [
        "loc",
        "symbols",
        "context",
        "graph",
        "impact",
        "affected-tests",
        "watch",
        "coupling",
    ];

    /// Flags every tree-walking command must accept.
    ///
    /// These were spelled out per command, six times over, and drifted: `context`
    /// described `--ignore` differently from `loc`, and `watch` silently skipped
    /// `.sephera.toml` while every other command read it. A test is the only
    /// thing that keeps the flattened groups actually flattened -- someone
    /// forgetting one `#[command(flatten)]` gets a compile error, but someone
    /// *choosing* to drop a flag from one command would not.
    #[test]
    fn every_tree_command_accepts_the_shared_flags() {
        for name in TREE_COMMANDS {
            let flags = flag_names(name);
            assert!(
                !flags.is_empty(),
                "`sephera {name}` declares no flags, so this test would pass \
                 vacuously"
            );
            for expected in [
                "--path",
                "--ignore",
                "--no-gitignore",
                "--config",
                "--no-config",
                // `--profile` is the flag the shared groups exist to guarantee,
                // and it is the one most easily lost by choosing not to flatten a
                // group: a command that silently stopped reading `.sephera.toml`
                // would still pass a test checking the other five.
                "--profile",
            ] {
                assert!(
                    flags.iter().any(|flag| flag == expected),
                    "`sephera {name}` must accept `{expected}`"
                );
            }
        }
    }

    /// Whether a command accepts an argument vector.
    ///
    /// Parsing real argv rather than introspecting the clap `Command`. Flattened
    /// argument groups are only materialised by clap's recursive build, and the
    /// introspection route needs an internal API to reach them -- so every
    /// assertion taken that way passed vacuously, twice, before this was replaced.
    /// Asking the parser the question a user asks cannot pass by not looking.
    fn accepts(command: &str, argv: &[&str]) -> bool {
        let mut full = vec!["sephera", command];
        full.extend_from_slice(argv);
        Cli::try_parse_from(&full).is_ok()
    }

    /// A command's `--help` output, rendered the way a user sees it.
    fn help_text(command: &str) -> String {
        let mut root = Cli::command();
        let subcommand = root
            .find_subcommand_mut(command)
            .unwrap_or_else(|| panic!("`{command}` must be a subcommand"));
        subcommand.render_long_help().to_string()
    }

    /// Every printed flag must be followed by a description.
    ///
    /// A flag with no description is a flag nobody can use correctly, and the
    /// descriptions are the one thing the shared groups exist to keep identical.
    /// Checked against rendered help rather than the argument list, because what
    /// the reader sees is the contract.
    #[test]
    fn every_printed_flag_has_a_description() {
        for name in TREE_COMMANDS {
            let help = help_text(name);
            let lines: Vec<&str> = help.lines().collect();
            assert!(
                lines.len() > 10,
                "`sephera {name} --help` rendered almost nothing, so this \
                 check would pass without looking"
            );

            let mut undocumented: Vec<String> = Vec::new();
            for (index, line) in lines.iter().enumerate() {
                let Some(marker) = line.find("--") else {
                    continue;
                };
                if !line[..marker].trim().is_empty() {
                    continue;
                }
                let flag = line[marker..]
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned();

                let inline = line[marker + flag.len()..].trim();
                let below = lines
                    .get(index + 1)
                    .map(|next| next.trim())
                    .unwrap_or_default();

                if inline.is_empty() && below.is_empty() {
                    undocumented.push(flag);
                }
            }

            assert!(
                undocumented.is_empty(),
                "`sephera {name}` prints flags with no description: \
                 {undocumented:?}"
            );
        }
    }

    /// `--url` and `--ref` cannot mean the same thing on two commands.
    #[test]
    fn url_and_ref_appear_where_url_mode_is_supported() {
        for name in ["loc", "symbols", "context", "graph", "impact", "coupling"]
        {
            // `impact` takes the file to report on positionally, so its argv has
            // to name one or the parse fails on the missing argument before the
            // flags are ever considered.
            let positional: &[&str] = if name == "impact" {
                &["src/lib.rs"]
            } else {
                &[]
            };

            let mut with_url = positional.to_vec();
            with_url.extend(["--url", "https://example.invalid/r"]);
            assert!(
                accepts(name, &with_url),
                "`sephera {name}` supports URL mode and must accept --url"
            );

            let mut with_ref = with_url.clone();
            with_ref.extend(["--ref", "main"]);
            assert!(
                accepts(name, &with_ref),
                "`sephera {name}` supports URL mode and must accept --ref"
            );
        }

        // `watch` deliberately has neither: watching a temporary checkout of a
        // remote repository is not a thing anyone wants, and accepting the flags
        // only to ignore them would be worse than not offering them.
        assert!(
            !accepts("watch", &["--url", "https://example.invalid/r"]),
            "`watch` must not advertise URL mode"
        );
    }

    /// The long flag names one command accepts.
    fn flag_names(command: &str) -> Vec<String> {
        let help = help_text(command);
        let mut flags = Vec::new();
        for line in help.lines() {
            let Some(marker) = line.find("--") else {
                continue;
            };
            if !line[..marker].trim().is_empty() {
                continue;
            }
            if let Some(flag) = line[marker..].split_whitespace().next() {
                flags.push(flag.to_owned());
            }
        }
        flags
    }

    #[test]
    fn parses_loc_command_with_repeated_ignores() {
        let cli = Cli::try_parse_from([
            "sephera", "loc", "--path", "demo", "--ignore", "*.rs", "--ignore",
            "target",
        ])
        .unwrap();

        match cli.command {
            Commands::Loc(arguments) => {
                assert_eq!(
                    arguments.source.path,
                    Some(std::path::PathBuf::from("demo"))
                );
                assert_eq!(arguments.source.url, None);
                assert_eq!(arguments.source.git_ref, None);
                assert_eq!(
                    arguments.ignore_args.ignore,
                    vec!["*.rs", "target"]
                );
            }
            _ => panic!("expected loc command"),
        }
    }

    #[test]
    fn parses_context_command_with_focus_budget_and_json_output() {
        let cli = Cli::try_parse_from([
            "sephera",
            "context",
            "--path",
            "demo",
            "--config",
            ".sephera.toml",
            "--profile",
            "review",
            "--focus",
            "crates/sephera_core",
            "--diff",
            "origin/master",
            "--budget",
            "32k",
            "--format",
            "json",
            "--output",
            "reports/context.json",
        ])
        .unwrap();

        match cli.command {
            Commands::Context(arguments) => {
                assert_eq!(
                    arguments.source.path,
                    Some(std::path::PathBuf::from("demo"))
                );
                assert_eq!(arguments.source.url, None);
                assert_eq!(arguments.source.git_ref, None);
                assert_eq!(
                    arguments.settings.config,
                    Some(std::path::PathBuf::from(".sephera.toml"))
                );
                // `--profile` is flattened into the command rather than declared once on
                // `Cli`, so it lands on `ContextArgs::settings`. Asserted
                // here because a global flag would vanish from this command's
                // help, and a flag that vanishes from help is a flag nobody
                // finds.
                assert_eq!(
                    arguments.settings.profile.as_deref(),
                    Some("review")
                );
                assert_eq!(
                    arguments.focus,
                    vec![std::path::PathBuf::from("crates/sephera_core")]
                );
                assert_eq!(arguments.diff.as_deref(), Some("origin/master"));
                assert_eq!(arguments.budget, Some(32_000));
                assert_eq!(arguments.format, Some(ContextFormat::Json));
                assert_eq!(
                    arguments.output_args.output,
                    Some(std::path::PathBuf::from("reports/context.json"))
                );
            }
            _ => panic!("expected context command"),
        }
    }

    #[test]
    fn root_help_mentions_context_export_capabilities() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();

        assert!(help.contains("LLM-ready context packs"));
        assert!(help.contains(".sephera.toml"));
        assert!(help.contains("reports/context.json"));
        assert!(help.contains("--list-profiles"));
        assert!(help.contains("--url"));
        assert!(help.contains("--ref"));
    }

    #[test]
    fn context_help_mentions_output_and_formats() {
        let mut command = Cli::command();
        let context_help = command
            .find_subcommand_mut("context")
            .expect("context subcommand must exist")
            .render_long_help()
            .to_string();

        assert!(context_help.contains("markdown"));
        assert!(context_help.contains("json"));
        assert!(context_help.contains("--config <FILE>"));
        assert!(context_help.contains("--no-config"));
        assert!(context_help.contains("--profile <NAME>"));
        assert!(context_help.contains("--list-profiles"));
        assert!(context_help.contains("--diff <SPEC>"));
        assert!(context_help.contains("--output <FILE>"));
        assert!(context_help.contains("built-in defaults"));
        assert!(context_help.contains("[profiles.<name>.context]"));
        assert!(context_help.contains("reports/context.md"));
        assert!(context_help.contains("reports/context.json"));
        assert!(context_help.contains("origin/master"));
        assert!(context_help.contains("working-tree"));
        assert!(context_help.contains("--url <URL>"));
        assert!(context_help.contains("--ref <REF>"));
    }

    #[test]
    fn rejects_conflicting_context_config_flags() {
        let error = Cli::try_parse_from([
            "sephera",
            "context",
            "--path",
            "demo",
            "--config",
            ".sephera.toml",
            "--no-config",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--no-config"));
    }

    #[test]
    fn rejects_list_profiles_with_context_output_flags() {
        let error = Cli::try_parse_from([
            "sephera",
            "context",
            "--path",
            "demo",
            "--list-profiles",
            "--diff",
            "working-tree",
            "--format",
            "json",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--list-profiles"));
    }

    #[test]
    fn rejects_path_and_url_together() {
        let error = Cli::try_parse_from([
            "sephera",
            "loc",
            "--path",
            "demo",
            "--url",
            "https://github.com/Reim-developer/Sephera",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--url"));
    }

    #[test]
    fn rejects_ref_without_url() {
        let error = Cli::try_parse_from([
            "sephera", "graph", "--path", "demo", "--ref", "main",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--ref"));
    }
}
