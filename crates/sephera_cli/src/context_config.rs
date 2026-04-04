use anyhow::Result;
use sephera_core::core::runtime::{
    ContextCommandInput, SourceRequest, resolve_context_command,
};

use crate::args::{ContextArgs, ContextCompress, ContextFormat};

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

fn context_compress_name(compress: ContextCompress) -> String {
    match compress {
        ContextCompress::Signatures => String::from("signatures"),
        ContextCompress::Skeleton => String::from("skeleton"),
    }
}

fn context_format_name(format: ContextFormat) -> String {
    match format {
        ContextFormat::Markdown => String::from("markdown"),
        ContextFormat::Json => String::from("json"),
    }
}

pub use sephera_core::core::runtime::{
    AvailableContextProfiles, ResolvedContextCommand, ResolvedContextOptions,
};
