mod context_json;
mod context_markdown;
mod graph;
pub mod grid;
mod loc;
mod profiles;
mod symbols;
mod table;
mod timing;
mod write;

pub use context_json::render_context_json;
pub use context_markdown::render_context_markdown;
pub use graph::render_graph;
pub use loc::{render_loc_csv, render_loc_json, render_loc_markdown};
pub use profiles::print_available_profiles;
pub use symbols::{
    print_symbol_report, print_symbols_by_file, render_symbol_json,
    render_symbol_markdown,
};
pub use table::{print_report, render_report_table};
pub use write::emit_rendered_output;
