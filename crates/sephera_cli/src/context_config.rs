use anyhow::Result;
use sephera_core::core::runtime::{
    ContextCommandInput, SourceRequest, resolve_context_command,
};

use crate::args::{ContextArgs, ContextCompress, ContextFormat};

/// Builds a resolved context command from CLI `ContextArgs`.
///
/// Converts the CLI-facing `ContextArgs` into the runtime `ContextCommandInput` (including
/// mapping optional `compress` and `format` enum values into their string names) and produces
/// a `ResolvedContextCommand` describing the resolved context options.
///
/// # Examples
///
/// ```
/// // construct arguments (fill fields as needed)
/// let args = ContextArgs {
///     path: Some(".".into()),
///     ..Default::default()
/// };
/// let resolved = resolve_context_options(args).unwrap();
/// ```
pub fn resolve_context_options(
arguments: ContextArgs,
) -> Result<ResolvedContextCommand> {
pub fn resolve_context_options(
    arguments: ContextArgs,
) -> Result<ResolvedContextCommand> {
    resolve_context_command(ContextCommandInput {
        source: SourceRequest {
            path: arguments.path,
            url: arguments.url,
            git_ref: arguments.git_ref,
        },
        config: arguments.config,
        no_config: arguments.no_config,
        profile: arguments.profile,
        list_profiles: arguments.list_profiles,
        ignore: arguments.ignore,
        focus: arguments.focus,
        diff: arguments.diff,
        budget: arguments.budget,
        compress: arguments.compress.map(context_compress_name),
        format: arguments.format.map(context_format_name),
        output: arguments.output,
    })
}

/// Maps a `ContextCompress` variant to its string identifier.
///
/// # Returns
/// The string identifier corresponding to the provided `ContextCompress`.
///
/// # Examples
///
/// ```
/// let s = context_compress_name(ContextCompress::Signatures);
/// assert_eq!(s, "signatures");
/// ```
fn context_compress_name(compress: ContextCompress) -> String {
    match compress {
        ContextCompress::Signatures => String::from("signatures"),
        ContextCompress::Skeleton => String::from("skeleton"),
    }
}

/// Maps a `ContextFormat` variant to its canonical string identifier.
///
/// # Returns
///
/// The string identifier corresponding to `format`.
///
/// # Examples
///
/// ```
/// let s = context_format_name(ContextFormat::Markdown);
/// assert_eq!(s, "markdown");
/// let s = context_format_name(ContextFormat::Json);
/// assert_eq!(s, "json");
/// ```
fn context_format_name(format: ContextFormat) -> String {
    match format {
        ContextFormat::Markdown => String::from("markdown"),
        ContextFormat::Json => String::from("json"),
    }
}

pub use sephera_core::core::runtime::{
    AvailableContextProfiles, ResolvedContextCommand, ResolvedContextOptions,
};
