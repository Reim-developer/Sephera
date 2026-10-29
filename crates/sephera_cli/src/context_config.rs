use anyhow::Result;
use sephera_runtime::{
    ContextCommandInput, SourceRequest, resolve_context_command,
};

use crate::args::{ContextArgs, ContextCompress, ContextFormat};

pub async fn resolve_context_options(
    arguments: ContextArgs,
) -> Result<ResolvedContextCommand> {
    resolve_context_command(ContextCommandInput {
        source: SourceRequest {
            path: arguments.source.path,
            url: arguments.source.url,
            git_ref: arguments.source.git_ref,
        },
        config: arguments.settings.config,
        no_config: arguments.settings.no_config,
        // The profile is the command's own flattened settings field
        // (see `SettingsArgs`): it is read off the arguments being moved
        // in, not passed as a separate parameter, because the caller
        // already owns it inside `arguments` and cloning it out only
        // to hand it back was a copy with no purpose. Context is the
        // one command that still resolves its own config internally
        // -- it is the one with a token budget -- so it needs the
        // selected name here.
        profile: arguments.settings.profile,
        list_profiles: arguments.list_profiles,
        ignore: arguments.ignore_args.ignore,
        no_gitignore: arguments.ignore_args.no_gitignore,
        focus: arguments.focus,
        focus_symbol: arguments.focus_symbol,
        diff: arguments.diff,
        budget: arguments.budget,
        compress: arguments.compress.map(context_compress_name),
        format: arguments.format.map(context_format_name),
        output: arguments.output_args.output,
    })
    .await
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

pub use sephera_runtime::{
    AvailableContextProfiles, ResolvedContextCommand, ResolvedContextOptions,
};
