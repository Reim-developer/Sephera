#![allow(clippy::module_name_repetitions)]

mod budget;
mod builder;
mod candidate;
mod excerpt;
mod focus;
mod grouping;
mod line_range;
mod ranker;
mod source;
mod types;

pub use builder::ContextBuilder;
pub use line_range::LineRange;
pub use types::{
    ContextDiffMetadata, ContextDiffSelection, ContextExcerpt, ContextFile,
    ContextGroupKind, ContextGroupSummary, ContextLanguageSummary,
    ContextMetadata, ContextReport, SelectionClass,
};
