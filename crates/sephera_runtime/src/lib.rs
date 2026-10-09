//! # Sephera Runtime
//!
//! Source resolution: local paths, git clones (shallow), remote URLs, config loading.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod context;
pub mod git;
pub mod interrupt;
pub mod settings;
pub mod source;

use sephera_core::config::CommentStyle;
use sephera_core::types::{Language, LanguageMetrics, LocReport};
use sephera_ignore::IgnoreMatcher;
use anyhow::Result;
use std::path::Path;

pub use source::{Interrupted, ResolvedSource, SourceRequest, TreeHostingStyle, resolve_source};
pub use interrupt::{CleanupGuard, INTERRUPTED_EXIT_CODE, Interrupt, interrupt};
pub use settings::{CONFIG_FILE_NAME, ConfigCommand, DeprecatedKey, LoadedConfig, ProjectSource, load_config_file};
pub use context::{AvailableContextProfiles, ContextCommandInput, ProjectSettings, ResolvedContextCommand, ResolvedContextOptions, build_context_report, load_project_settings, resolve_changed_files, resolve_context_command};