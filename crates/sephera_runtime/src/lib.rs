//! # Sephera Runtime
//!
//! Where the code being analysed lives: a local path, a temporary checkout of a
//! git repository, or a `.sephera.toml` that names one. Also the Ctrl+C policy,
//! which lives here because it is a property of the run rather than of any one
//! command.
//!
//! Everything downstream consumes this through the re-exports below. The five
//! crates behind it are separate because they have separate reasons to change --
//! the parser cache moves with Tree-sitter, the traversal with the file format --
//! and this one exists so a caller has one place to ask "where do I read from"
//! rather than five.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod context;
mod git;
mod interrupt;
mod settings;
mod source;

pub use context::{
    AvailableContextProfiles, ContextCommandInput, ProjectSettings,
    ResolvedContextCommand, ResolvedContextOptions, build_context_report,
    load_project_settings, resolve_changed_files, resolve_context_command,
};
pub use interrupt::{
    CleanupGuard, INTERRUPTED_EXIT_CODE, Interrupt, interrupt,
};
pub use settings::{
    CONFIG_FILE_NAME, CommandToml, ConfigCommand, DeprecatedKey, LoadedConfig,
    ProfileToml, ProjectSource, load_config_file,
};
pub use source::{
    Interrupted, ResolvedSource, SourceRequest, TreeHostingStyle,
    resolve_source,
};
