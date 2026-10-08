#![allow(clippy::module_name_repetitions)]

mod context;
mod git;
mod interrupt;
mod source;

pub use context::{
    AvailableContextProfiles, ContextCommandInput, ProjectSettings,
    ResolvedContextCommand, ResolvedContextOptions, build_context_report,
    load_project_settings, resolve_changed_files, resolve_context_command,
};
pub mod settings;
pub use interrupt::{
    CleanupGuard, INTERRUPTED_EXIT_CODE, Interrupt, interrupt,
};
pub use settings::{
    CONFIG_FILE_NAME, ConfigCommand, DeprecatedKey, LoadedConfig,
    ProjectSource, load_config_file,
};
pub use source::{
    Interrupted, ResolvedSource, SourceRequest, TreeHostingStyle,
    resolve_source,
};
