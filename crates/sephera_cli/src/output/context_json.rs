use sephera_core::core::context::ContextReport;

/// # Panics
///
/// Panics when serializing `ContextReport` to JSON unexpectedly fails.
#[must_use]
pub fn render_context_json(report: &ContextReport) -> String {
    serde_json::to_string_pretty(report)
        .expect("serializing ContextReport to JSON must succeed")
}
