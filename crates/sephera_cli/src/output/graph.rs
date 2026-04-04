//! Graph output rendering in JSON, Markdown, XML, and DOT formats.

use std::fmt::Write;

use sephera_core::core::graph::types::{GraphFormat, GraphQuery, GraphReport};

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
    let _ = write!(
        output,
        "| Circular dependencies | {} |\n\n",
        report.metrics.circular_dependencies
    );
}

fn render_markdown_lists(output: &mut String, report: &GraphReport) {
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

fn render_markdown_mermaid(output: &mut String, report: &GraphReport) {
    let internal_edges: Vec<_> =
        report.edges.iter().filter(|e| e.resolved).collect();

    if internal_edges.is_empty() {
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

    for edge in internal_edges.iter().take(50) {
        if let Some(ref to) = edge.to {
            if let (Some(from_id), Some(to_id)) =
                (node_ids.get(edge.from.as_str()), node_ids.get(to.as_str()))
            {
                let _ = writeln!(output, "    {from_id} --> {to_id}");
            }
        }
    }

    if internal_edges.len() > 50 {
        let _ = writeln!(
            output,
            "    %% ... and {} more edges",
            internal_edges.len() - 50
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
    let cleaned = path.replace(['/', '.', '-'], "_");
    format!("\"{cleaned}\"")
}

fn format_query(query: &GraphQuery) -> String {
    match query {
        GraphQuery::DependsOn(path) => format!("depends_on:{path}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sephera_core::core::graph::types::{
        GraphEdge, GraphMetrics, GraphNode, GraphReport,
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
            }],
            metrics: GraphMetrics {
                total_files: 2,
                total_internal_edges: 1,
                total_external_edges: 0,
                circular_dependencies: 0,
                most_importing: vec![],
                most_imported: vec![],
                cycles: vec![],
            },
        }
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
