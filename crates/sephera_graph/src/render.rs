//! Graph output rendering in JSON, Markdown, XML, and DOT formats.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use sephera_graph::{manifests::DependencyKind, types::{GraphFormat, GraphQuery, GraphReport}};

/// Renders the graph report in the requested format.
#[must_use]
pub fn render_graph(report: &GraphReport, format: GraphFormat) -> String {
    match format {
        GraphFormat::Json => render_graph_json(report),
        GraphFormat::Markdown => render_graph_markdown(report),
        GraphFormat::Xml => render_graph_xml(report),
        GraphFormat::Dot => render_graph_dot(report),
    }
}

/// Renders the graph report as pretty-printed JSON.
fn render_graph_json(report: &GraphReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// Renders the graph report as a Markdown document with a Mermaid diagram.
fn render_graph_markdown(report: &GraphReport) -> String {
    let mut output = String::new();
    output.push_str("# Dependency Graph Report\n\n");

    let _ = write!(
        output,
        "**Base path:** `{}`\n\n",
        report.base_path.display()
    );
    render_markdown_selection(&mut output, report);

    render_markdown_summary(&mut output, report);
    render_markdown_blast_radius(&mut output, report);
    render_markdown_lists(&mut output, report);
    render_markdown_mermaid(&mut output, report);

    output
}

fn render_markdown_selection(output: &mut String, report: &GraphReport) {
    if !report.focus_paths.is_empty() {
        let _ = writeln!(
            output,
            "**Focus paths:** `{}`\n",
            report.focus_paths.join("`, `")
        );
    }

    if let Some(depth) = report.depth {
        let _ = writeln!(output, "**Depth:** `{depth}`\n");
    }

    if let Some(query) = &report.query {
        let _ = writeln!(output, "**Query:** `{}`\n", format_query(query));
    }
}

fn render_markdown_summary(output: &mut String, report: &GraphReport) {
    output.push_str("## Summary\n\n");
    output.push_str("| Metric | Value |\n|--------|-------|\n");
    let _ = writeln!(
        output,
        "| Files analyzed | {} |",
        report.metrics.total_files
    );
    let _ = writeln!(
        output,
        "| Internal edges | {} |",
        report.metrics.total_internal_edges
    );
    let _ = writeln!(
        output,
        "| External edges | {} |",
        report.metrics.total_external_edges
    );
    // Shown when non-zero, and placed directly under the internal-edge count
    // because the two are the same measurement read two ways: `Internal edges`
    // deliberately excludes self-references, so a reader comparing it against a
    // file count had no way to know 60 references had been set aside. Sephera
    // itself finds 60, almost all `use super::*;` inside test modules -- real
    // references that say nothing about how files depend on each other, and
    // invisible in every human-readable format until this row existed.
    if report.metrics.self_references > 0 {
        let _ = writeln!(
            output,
            "| Self-references (excluded above) | {} |",
            report.metrics.self_references
        );
    }
    // Only shown when non-zero: a resolver gap, not a property of the project,
    // and a permanently empty row would train readers to ignore it.
    if report.metrics.unresolved_local_edges > 0 {
        let _ = writeln!(
            output,
            "| Unresolved local paths | {} |",
            report.metrics.unresolved_local_edges
        );
        // Shown when non-zero for the same reason as the row above: a limitation to
        // state, not a permanent fixture. These edges only compile when the feature
        // is on, so a blast radius that counted them silently claims a dependency
        // the build may not have.
        if report.metrics.cfg_gated_edges > 0 {
            let _ = writeln!(
                output,
                "| Feature-gated edges | {} |",
                report.metrics.cfg_gated_edges
            );
        }
    }
    let _ = writeln!(
        output,
        "| Declared dependencies | {} |",
        report.metrics.declared_dependency_edges
    );
    let _ = writeln!(
        output,
        "| Local crate edges | {} |",
        report.metrics.local_crate_edges
    );
    let _ = writeln!(
        output,
        "| Standard library edges | {} |",
        report.metrics.builtin_edges
    );
    let _ = write!(
        output,
        "| Circular dependencies | {} |\n\n",
        report.metrics.circular_dependencies
    );
}

/// The answer to the question a `--what-depends-on` query asks.
///
/// Placed immediately after the summary, because until this existed the report
/// showed only rankings — `Most Imported Files` counting how many references
/// reach a file, `Most Importing Files` counting how many a file makes — and
/// left a reader of the headline feature to reconstruct which files break from
/// a diagram at the bottom of the page.
///
/// The names each dependent imports are shown as well as the file, because
/// "rename `tokenize` and these two files break" is actionable and a list of
/// paths is not.
fn render_markdown_blast_radius(output: &mut String, report: &GraphReport) {
    let Some(GraphQuery::DependsOn(target)) = &report.query else {
        return;
    };

    // `target` is the normalized query path, which is the same form edge targets
    // take, so this is an exact comparison rather than a path-shape guess.
    //
    // Two exclusions, both of which the count was previously missing:
    //
    // * self-edges. A `use super::*;` resolves to the file it is written in, so
    //   the target was being listed as one of the files that imports it. On
    //   Sephera itself that made the section claim 9 direct and 9 indirect
    //   dependents, 18 in total, for a blast radius of 17 -- a sum larger than
    //   the number of files that exist.
    // * unresolved edges. An edge the resolver could not place is a path it
    //   failed to find, not a coupling, and listing it as a dependency claims
    //   exactly the connection the report is supposed to be honest about.
    let mut direct: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in &report.edges {
        if edge.to.as_deref() != Some(target.as_str()) || !edge.resolved {
            continue;
        }
        if edge.from == *target {
            continue;
        }
        direct
            .entry(edge.from.as_str())
            .or_default()
            .insert(edge.import_path.as_str());
    }

    let _ = writeln!(output, "## Blast radius for `{target}`\n");

    if direct.is_empty() {
        // A clear negative beats an absent section. Without this the reader is
        // left wondering whether the query failed or the answer was empty.
        output.push_str("No file in the analysed scope imports this one.\n\n");
        return;
    }

    let _ = writeln!(
        output,
        "**{} file{} import{} it directly.**\n",
        direct.len(),
        plural(direct.len()),
        if direct.len() == 1 { "s" } else { "" },
    );
    output.push_str("| File | Imports from it |\n");
    output.push_str("|------|------------------|\n");
    for (file, names) in &direct {
        let listed = names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(output, "| `{file}` | {listed} |");
    }
    output.push('\n');

    // What the direct list does not show, and why that matters.
    let indirect: Vec<&str> = report
        .nodes
        .iter()
        .map(|node| node.file_path.as_str())
        .filter(|path| *path != target.as_str())
        .filter(|path| !direct.contains_key(path))
        .collect();

    if !indirect.is_empty() {
        let _ = writeln!(
            output,
            "**{} further file{} reach{} {} indirectly**, through the files above.\n",
            indirect.len(),
            plural(indirect.len()),
            if indirect.len() == 1 { "es" } else { "" },
            if indirect.len() == 1 { "it" } else { "them" },
        );
    }

    // `--depth` bounds the walk, so the list above can be a prefix of the real
    // blast radius. Saying nothing lets a reader treat a truncated answer as a
    // complete one, which is the same failure as reporting a resolver gap
    // without a count.
    if let Some(depth) = report.depth {
        let _ = writeln!(
            output,
            "> Bounded by `--depth {depth}`. Omit the flag for the full \
             blast radius.\n"
        );
    }
}

/// The plural `s` for a count of one thing.
const fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn render_markdown_lists(output: &mut String, report: &GraphReport) {
    // What the unresolved edges actually refer to. This is the section that
    // answers "which dependency do I need to bump", so it comes before the file
    // rankings.
    if !report.metrics.dependencies.is_empty() {
        output.push_str("## Dependencies\n\n");
        output.push_str("| Package | Kind | Version | Import paths |\n");
        output.push_str("|---------|------|---------|--------------|\n");
        for dependency in &report.metrics.dependencies {
            let _ = writeln!(
                output,
                "| `{}` | {} | {} | {} |",
                dependency.name,
                dependency_kind_label(dependency.kind),
                dependency.version.as_deref().unwrap_or("unknown"),
                dependency.edge_count
            );
        }
        output.push('\n');
    }

    // Most imported files
    if !report.metrics.most_imported.is_empty() {
        output.push_str("## Most Imported Files\n\n");
        output.push_str("| File | Imported by |\n|------|-------------|\n");
        for metric in &report.metrics.most_imported {
            let _ = writeln!(
                output,
                "| `{}` | {} |",
                metric.file_path, metric.count
            );
        }
        output.push('\n');
    }

    // Most importing files
    if !report.metrics.most_importing.is_empty() {
        output.push_str("## Most Importing Files\n\n");
        output.push_str("| File | Imports |\n|------|---------|\n");
        for metric in &report.metrics.most_importing {
            let _ = writeln!(
                output,
                "| `{}` | {} |",
                metric.file_path, metric.count
            );
        }
        output.push('\n');
    }

    // Circular dependencies
    if !report.metrics.cycles.is_empty() {
        output.push_str("## Circular Dependencies\n\n");
        for (index, cycle) in report.metrics.cycles.iter().enumerate() {
            let _ =
                writeln!(output, "{}. `{}`", index + 1, cycle.join("` → `"));
        }
        output.push('\n');
    }
}

/// A short human label for what kind of reference a package name came from.
const fn dependency_kind_label(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Local => "workspace",
        DependencyKind::Builtin => "stdlib",
        DependencyKind::Declared => "declared",
        DependencyKind::Undeclared => "undeclared",
    }
}

/// How many arrows the diagram draws before it collapses into a summary.
///
/// Past this the graph is denser than anyone can read, and the point of the
/// diagram is the shape rather than the inventory.
const MAX_DRAWN_EDGES: usize = 50;

/// Edges worth drawing: resolved, between two different files, and deduplicated.
///
/// A file that names another in six separate `use` statements would otherwise
/// draw six identical arrows, and a `use super::*;` would draw an arrow from a
/// node to itself. Neither says anything about the shape of the graph.
fn diagram_edges(report: &GraphReport) -> Vec<(usize, usize)> {
    let node_ids: std::collections::BTreeMap<&str, usize> = report
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.file_path.as_str(), index))
        .collect();

    let mut seen = std::collections::BTreeSet::new();
    let mut drawn = Vec::new();
    for edge in &report.edges {
        if !edge.resolved {
            continue;
        }
        let Some(to) = edge.to.as_deref() else {
            continue;
        };
        if to == edge.from {
            continue;
        }
        let (Some(from_id), Some(to_id)) =
            (node_ids.get(edge.from.as_str()), node_ids.get(to))
        else {
            continue;
        };
        if seen.insert((*from_id, *to_id)) {
            drawn.push((*from_id, *to_id));
        }
    }
    drawn
}

fn render_markdown_mermaid(output: &mut String, report: &GraphReport) {
    let drawn = diagram_edges(report);
    if drawn.is_empty() {
        return;
    }

    output.push_str("## Dependency Diagram\n\n");
    output.push_str("```mermaid\ngraph LR\n");

    let node_ids: std::collections::BTreeMap<&str, String> = report
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.file_path.as_str(), format!("n{i}")))
        .collect();

    for node in &report.nodes {
        let short_name = node
            .file_path
            .rsplit_once('/')
            .map_or(node.file_path.as_str(), |(_, name)| name);
        if let Some(node_id) = node_ids.get(node.file_path.as_str()) {
            let _ = writeln!(output, "    {node_id}[\"{short_name}\"]");
        }
    }

    for (from_id, to_id) in drawn.iter().take(MAX_DRAWN_EDGES) {
        let _ = writeln!(output, "    n{from_id} --> n{to_id}");
    }

    if drawn.len() > MAX_DRAWN_EDGES {
        let _ = writeln!(
            output,
            "    %% ... and {} more edges",
            drawn.len() - MAX_DRAWN_EDGES
        );
    }

    output.push_str("```\n");
}

/// Renders the graph report as an XML document.
fn render_graph_xml(report: &GraphReport) -> String {
    let mut output = String::new();

    output.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    output.push_str("<dependency-graph>\n");
    let _ = writeln!(
        output,
        "  <base-path>{}</base-path>",
        xml_escape(&report.base_path.to_string_lossy())
    );
    render_xml_selection(&mut output, report);

    render_xml_metrics(&mut output, report);
    render_xml_nodes(&mut output, report);
    render_xml_edges(&mut output, report);

    output.push_str("</dependency-graph>\n");
    output
}

fn render_xml_selection(output: &mut String, report: &GraphReport) {
    if report.focus_paths.is_empty()
        && report.depth.is_none()
        && report.query.is_none()
    {
        return;
    }

    output.push_str("  <selection>\n");
    if !report.focus_paths.is_empty() {
        output.push_str("    <focus-paths>\n");
        for focus_path in &report.focus_paths {
            let _ = writeln!(
                output,
                "      <focus-path>{}</focus-path>",
                xml_escape(focus_path)
            );
        }
        output.push_str("    </focus-paths>\n");
    }

    if let Some(depth) = report.depth {
        let _ = writeln!(output, "    <depth>{depth}</depth>");
    }

    if let Some(query) = &report.query {
        let _ = writeln!(
            output,
            "    <query>{}</query>",
            xml_escape(&format_query(query))
        );
    }
    output.push_str("  </selection>\n");
}

fn render_xml_metrics(output: &mut String, report: &GraphReport) {
    output.push_str("  <metrics>\n");
    let _ = writeln!(
        output,
        "    <total-files>{}</total-files>",
        report.metrics.total_files
    );
    let _ = writeln!(
        output,
        "    <internal-edges>{}</internal-edges>",
        report.metrics.total_internal_edges
    );
    let _ = writeln!(
        output,
        "    <external-edges>{}</external-edges>",
        report.metrics.total_external_edges
    );
    if report.metrics.self_references > 0 {
        let _ = writeln!(
            output,
            "    <self-references>{}</self-references>",
            report.metrics.self_references
        );
    }
    if report.metrics.unresolved_local_edges > 0 {
        let _ = writeln!(
            output,
            "    <unresolved-local-edges>{}</unresolved-local-edges>",
            report.metrics.unresolved_local_edges
        );
        for sample in &report.metrics.unresolved_local_samples {
            let _ = writeln!(
                output,
                "    <unresolved-local>{sample}</unresolved-local>"
            );
        }
    }
    let _ = writeln!(
        output,
        "    <circular-dependencies>{}</circular-dependencies>",
        report.metrics.circular_dependencies
    );

    if !report.metrics.most_imported.is_empty() {
        output.push_str("    <most-imported>\n");
        for metric in &report.metrics.most_imported {
            let _ = writeln!(
                output,
                "      <file path=\"{}\" count=\"{}\"/>",
                xml_escape(&metric.file_path),
                metric.count
            );
        }
        output.push_str("    </most-imported>\n");
    }

    if !report.metrics.most_importing.is_empty() {
        output.push_str("    <most-importing>\n");
        for metric in &report.metrics.most_importing {
            let _ = writeln!(
                output,
                "      <file path=\"{}\" count=\"{}\"/>",
                xml_escape(&metric.file_path),
                metric.count
            );
        }
        output.push_str("    </most-importing>\n");
    }

    if !report.metrics.cycles.is_empty() {
        output.push_str("    <cycles>\n");
        for cycle in &report.metrics.cycles {
            output.push_str("      <cycle>\n");
            for node in cycle {
                let _ = writeln!(
                    output,
                    "        <node>{}</node>",
                    xml_escape(node)
                );
            }
            output.push_str("      </cycle>\n");
        }
        output.push_str("    </cycles>\n");
    }

    output.push_str("  </metrics>\n");
}

fn render_xml_nodes(output: &mut String, report: &GraphReport) {
    output.push_str("  <nodes>\n");
    for node in &report.nodes {
        let lang = node.language.unwrap_or("unknown");
        let _ = writeln!(
            output,
            "    <node path=\"{}\" language=\"{}\" imports=\"{}\" imported-by=\"{}\"/>",
            xml_escape(&node.file_path),
            xml_escape(lang),
            node.imports_count,
            node.imported_by_count,
        );
    }
    output.push_str("  </nodes>\n");
}

fn render_xml_edges(output: &mut String, report: &GraphReport) {
    output.push_str("  <edges>\n");
    for edge in &report.edges {
        let to = edge.to.as_deref().unwrap_or("(unresolved)");
        let _ = writeln!(
            output,
            "    <edge from=\"{}\" to=\"{}\" import=\"{}\" resolved=\"{}\"/>",
            xml_escape(&edge.from),
            xml_escape(to),
            xml_escape(&edge.import_path),
            edge.resolved,
        );
    }
    output.push_str("  </edges>\n");
}

/// Renders the graph report as a Graphviz DOT file.
fn render_graph_dot(report: &GraphReport) -> String {
    let mut output = String::new();

    output.push_str("digraph dependencies {\n");
    output.push_str("    rankdir=LR;\n");
    output.push_str(
        "    node [shape=box, fontname=\"Helvetica\", fontsize=10];\n",
    );
    output.push_str("    edge [fontsize=8];\n\n");

    // Node definitions with labels
    for node in &report.nodes {
        let short_name = node
            .file_path
            .rsplit_once('/')
            .map_or(node.file_path.as_str(), |(_, name)| name);
        let label = dot_escape(short_name);
        let id = dot_node_id(&node.file_path);
        let _ = writeln!(output, "    {id} [label=\"{label}\"];");
    }

    output.push('\n');

    // Edges (only resolved/internal)
    for edge in &report.edges {
        if !edge.resolved {
            continue;
        }
        if let Some(ref to) = edge.to {
            let from_id = dot_node_id(&edge.from);
            let to_id = dot_node_id(to);
            let _ = writeln!(output, "    {from_id} -> {to_id};");
        }
    }

    output.push_str("}\n");
    output
}

/// Escapes special XML characters.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Escapes special DOT label characters.
fn dot_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Converts a file path to a valid DOT node ID.
fn dot_node_id(path: &str) -> String {
    path.replace(['/', '.', '-'], "_")
}

fn format_query(query: &GraphQuery) -> String {
    match query {
        GraphQuery::DependsOn(path) => format!("depends_on:{path}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sephera_graph::types::{
        GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
    };

    use super::*;

    fn sample_report() -> GraphReport {
        GraphReport {
            base_path: PathBuf::from("/tmp/test"),
            focus_paths: vec![],
            depth: Some(0),
            query: Some(GraphQuery::DependsOn("src/lib.rs".to_owned())),
            nodes: vec![
                GraphNode {
                    file_path: "src/main.rs".to_owned(),
                    language: Some("Rust"),
                    imports_count: 1,
                    imported_by_count: 0,
                },
                GraphNode {
                    file_path: "src/lib.rs".to_owned(),
                    language: Some("Rust"),
                    imports_count: 0,
                    imported_by_count: 1,
                },
            ],
            edges: vec![GraphEdge {
                from: "src/main.rs".to_owned(),
                to: Some("src/lib.rs".to_owned()),
                import_path: "crate::lib".to_owned(),
                resolved: true,
                kind: ImportKind::Dependency,
                local_gap: false,
                cfg_gated: false,
            }],
            metrics: GraphMetrics {
                total_files: 2,
                total_internal_edges: 1,
                self_references: 0,
                total_external_edges: 0,
                unresolved_local_edges: 0,
                unresolved_local_samples: Vec::new(),
                cfg_gated_edges: 0,
                dependencies: Vec::new(),
                declared_dependency_edges: 0,
                local_crate_edges: 0,
                builtin_edges: 0,
                circular_dependencies: 0,
                most_importing: vec![],
                most_imported: vec![],
                cycles: vec![],
            },
        }
    }

    #[test]
    fn markdown_shows_self_references_when_present() {
        // `total_internal_edges` excludes self-references, so a report that
        // showed only that number left a reader with no way to know how many
        // references had been set aside. Sephera itself has 60, all `use
        // super::*;` inside test modules.
        let mut report = sample_report();
        report.metrics.self_references = 60;

        let markdown = render_graph(&report, GraphFormat::Markdown);
        assert!(
            markdown.contains("| Self-references (excluded above) | 60 |"),
            "self-references must be visible in Markdown: {markdown}"
        );
    }

    #[test]
    fn markdown_omits_self_references_when_there_are_none() {
        // A permanently zero row would train readers to ignore it, the same
        // reason unresolved-local-edges is conditional.
        let markdown = render_graph(&sample_report(), GraphFormat::Markdown);

        assert!(
            !markdown.contains("Self-references"),
            "a zero row is noise: {markdown}"
        );
    }

    #[test]
    fn xml_reports_self_references_too() {
        let mut report = sample_report();
        report.metrics.self_references = 60;

        let xml = render_graph(&report, GraphFormat::Xml);
        assert!(
            xml.contains("<self-references>60</self-references>"),
            "{xml}"
        );
    }

    #[test]
    fn blast_radius_does_not_count_the_target_as_its_own_dependent() {
        // A `use super::*;` resolves to the file it is written in, so the target
        // used to be listed among the files that import it. On Sephera itself
        // that made the section claim 9 direct plus 9 indirect dependents for a
        // blast radius of 17 -- a sum larger than the number of files involved.
        let mut report = sample_report();
        report.query = Some(GraphQuery::DependsOn("src/lib.rs".to_owned()));
        report.nodes.push(GraphNode {
            file_path: "src/lib.rs".to_owned(),
            language: Some("Rust"),
            imports_count: 0,
            imported_by_count: 2,
        });
        // Remove the duplicate node the push above introduced.
        let mut seen = std::collections::BTreeSet::new();
        report
            .nodes
            .retain(|node| seen.insert(node.file_path.clone()));
        report.edges = vec![
            GraphEdge {
                from: "src/lib.rs".to_owned(),
                to: Some("src/lib.rs".to_owned()),
                import_path: "crate::lib".to_owned(),
                resolved: true,
                kind: ImportKind::Dependency,
                local_gap: false,
                cfg_gated: false,
            },
            GraphEdge {
                from: "src/main.rs".to_owned(),
                to: Some("src/lib.rs".to_owned()),
                import_path: "crate::lib".to_owned(),
                resolved: true,
                kind: ImportKind::Dependency,
                local_gap: false,
                cfg_gated: false,
            },
        ];

        let markdown = render_graph(&report, GraphFormat::Markdown);

        assert!(
            markdown.contains("**1 file imports it directly.**"),
            "only `src/main.rs` imports the target: {markdown}"
        );
        assert!(
            !markdown.contains("further file"),
            "there is nobody left over to reach indirectly: {markdown}"
        );
    }

    #[test]
    fn blast_radius_ignores_an_unresolved_edge_to_the_target() {
        // An edge the resolver could not place is a path it failed to find, not
        // a coupling. Listing it claims the connection the report exists to be
        // honest about.
        let mut report = sample_report();
        report.query = Some(GraphQuery::DependsOn("src/lib.rs".to_owned()));
        report.edges = vec![GraphEdge {
            from: "src/main.rs".to_owned(),
            to: Some("src/lib.rs".to_owned()),
            import_path: "crate::lib".to_owned(),
            resolved: false,
            kind: ImportKind::Dependency,
            local_gap: true,
            cfg_gated: false,
        }];

        let markdown = render_graph(&report, GraphFormat::Markdown);
        assert!(
            markdown
                .contains("No file in the analysed scope imports this one."),
            "an unresolved edge is not a dependency: {markdown}"
        );
    }

    #[test]
    fn json_output_is_valid() {
        let report = sample_report();
        let json = render_graph(&report, GraphFormat::Json);
        assert!(json.contains("\"total_files\""));
        assert!(json.contains("src/main.rs"));
        let _parsed: serde_json::Value =
            serde_json::from_str(&json).expect("must be valid JSON");
    }

    #[test]
    fn markdown_output_contains_diagram() {
        let report = sample_report();
        let md = render_graph(&report, GraphFormat::Markdown);
        assert!(md.contains("# Dependency Graph Report"));
        assert!(md.contains("**Depth:** `0`"));
        assert!(md.contains("**Query:** `depends_on:src/lib.rs`"));
        assert!(md.contains("```mermaid"));
        assert!(md.contains("graph LR"));
    }

    #[test]
    fn markdown_names_the_files_that_break() {
        // The report used to answer this question only through a diagram at the
        // bottom of the page, and through two rankings that count references
        // without naming the files. This is the section that closes that gap.
        let md = render_graph(&sample_report(), GraphFormat::Markdown);

        assert!(
            md.contains("## Blast radius for `src/lib.rs`"),
            "got:\n{md}"
        );
        assert!(md.contains("**1 file imports it directly.**"), "got:\n{md}");
        assert!(
            md.contains("| `src/main.rs` | `crate::lib` |"),
            "got:\n{md}"
        );
    }

    #[test]
    fn markdown_says_so_when_nothing_imports_the_target() {
        // An absent section reads as "the query failed". A stated negative is
        // an answer.
        let mut report = sample_report();
        report.edges.clear();

        let md = render_graph(&report, GraphFormat::Markdown);

        assert!(
            md.contains("No file in the analysed scope imports this one."),
            "got:\n{md}"
        );
    }

    #[test]
    fn markdown_flags_a_depth_bounded_answer_as_bounded() {
        // Reporting a truncated blast radius without saying so is the same
        // failure as reporting a resolver gap without a count.
        let mut report = sample_report();
        report.depth = Some(1);

        let md = render_graph(&report, GraphFormat::Markdown);

        assert!(md.contains("Bounded by `--depth 1`"), "got:\n{md}");
    }

    #[test]
    fn markdown_leaves_out_the_blast_radius_without_a_query() {
        let mut report = sample_report();
        report.query = None;

        let md = render_graph(&report, GraphFormat::Markdown);

        assert!(
            !md.contains("Blast radius"),
            "a whole-graph report has no blast radius to state, got:\n{md}"
        );
    }

    #[test]
    fn markdown_lists_each_imported_name_once_per_file() {
        let mut report = sample_report();
        report.edges.push(GraphEdge {
            from: "src/main.rs".to_owned(),
            to: Some("src/lib.rs".to_owned()),
            import_path: "crate::lib".to_owned(),
            resolved: true,
            kind: ImportKind::Dependency,
            local_gap: false,
            cfg_gated: false,
        });
        report.edges.push(GraphEdge {
            from: "src/main.rs".to_owned(),
            to: Some("src/lib.rs".to_owned()),
            import_path: "crate::other".to_owned(),
            resolved: true,
            kind: ImportKind::Dependency,
            local_gap: false,
            cfg_gated: false,
        });

        let md = render_graph(&report, GraphFormat::Markdown);

        assert!(md.contains("**1 file imports it directly.**"), "got:\n{md}");
        assert_eq!(
            md.matches("`crate::lib`").count(),
            1,
            "a repeated import of the same name is one entry, got:\n{md}"
        );
        assert!(md.contains("`crate::other`"), "got:\n{md}");
    }

    #[test]
    fn markdown_reports_files_that_only_reach_the_target_indirectly() {
        let mut report = sample_report();
        report.depth = None;
        report.nodes.push(GraphNode {
            file_path: "src/consumer.rs".to_owned(),
            language: Some("Rust"),
            imports_count: 1,
            imported_by_count: 0,
        });

        let md = render_graph(&report, GraphFormat::Markdown);

        assert!(
            md.contains("**1 further file reaches it indirectly**"),
            "got:\n{md}"
        );
        assert!(!md.contains("Bounded by"), "got:\n{md}");
    }

    #[test]
    fn the_diagram_draws_one_arrow_per_pair_of_files() {
        let edge = |from: &str, to: Option<&str>| GraphEdge {
            from: from.to_owned(),
            to: to.map(ToOwned::to_owned),
            import_path: "x".to_owned(),
            resolved: to.is_some(),
            kind: ImportKind::Dependency,
            local_gap: false,
            cfg_gated: false,
        };
        let report = GraphReport {
            base_path: PathBuf::from("/tmp/test"),
            focus_paths: vec![],
            depth: Some(0),
            query: None,
            nodes: vec![
                GraphNode {
                    file_path: "a.rs".to_owned(),
                    language: Some("Rust"),
                    imports_count: 3,
                    imported_by_count: 1,
                },
                GraphNode {
                    file_path: "b.rs".to_owned(),
                    language: Some("Rust"),
                    imports_count: 1,
                    imported_by_count: 3,
                },
            ],
            edges: vec![
                // Two files naming each other in several places: one arrow.
                edge("a.rs", Some("b.rs")),
                edge("a.rs", Some("b.rs")),
                // A glob import resolves to the file it is written in.
                edge("a.rs", Some("a.rs")),
                // Never resolved, so it has no arrow at all.
                edge("a.rs", None),
            ],
            metrics: sample_report().metrics,
        };

        let arrows = diagram_edges(&report);
        assert_eq!(
            arrows,
            vec![(0, 1)],
            "duplicate arrows and self-references say nothing about the shape"
        );
    }

    #[test]
    fn a_file_that_only_imports_itself_draws_no_diagram() {
        let report = GraphReport {
            base_path: PathBuf::from("/tmp/test"),
            focus_paths: vec![],
            depth: Some(0),
            query: None,
            nodes: vec![GraphNode {
                file_path: "a.rs".to_owned(),
                language: Some("Rust"),
                imports_count: 1,
                imported_by_count: 0,
            }],
            edges: vec![GraphEdge {
                from: "a.rs".to_owned(),
                to: Some("a.rs".to_owned()),
                import_path: "super".to_owned(),
                resolved: true,
                kind: ImportKind::Dependency,
                local_gap: false,
                cfg_gated: false,
            }],
            metrics: sample_report().metrics,
        };

        let md = render_graph(&report, GraphFormat::Markdown);
        assert!(
            !md.contains("graph LR"),
            "an empty diagram is noise: there is no shape to show"
        );
    }

    #[test]
    fn xml_output_is_structured() {
        let report = sample_report();
        let xml = render_graph(&report, GraphFormat::Xml);
        assert!(xml.contains("<?xml version="));
        assert!(xml.contains("<dependency-graph>"));
        assert!(xml.contains("<selection>"));
        assert!(xml.contains("<query>depends_on:src/lib.rs</query>"));
        assert!(xml.contains("<nodes>"));
        assert!(xml.contains("<edges>"));
        assert!(xml.contains("</dependency-graph>"));
    }

    #[test]
    fn dot_output_is_valid() {
        let report = sample_report();
        let dot = render_graph(&report, GraphFormat::Dot);
        assert!(dot.contains("digraph dependencies"));
        assert!(dot.contains("rankdir=LR"));
        assert!(dot.contains("->"));
    }
}
