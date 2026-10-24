//! Install the `sephera-change-safety` agent skill, or say where it would go.
//!
//! The skill's files are embedded in the binary at compile time, so an
//! installed binary carries the skill with it instead of depending on
//! the source tree being present at install time. The embedded text is
//! the repo's skill files, read by `include_str!` on every build, so
//! the installed skill can never drift from the documented one —
//! a build from a tree where the skill files were edited installs
//! the edited skill.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::args::{SkillArgs, SkillCommand};

/// The skill's directory name, which the spec requires to equal the
/// `name` frontmatter in `SKILL.md`.
const SKILL_NAME: &str = "sephera-change-safety";

/// One line saying what the skill is for, printed by `--print`.
const SUMMARY: &str = "sephera-change-safety: check a file's blast radius before editing, review before committing, gate the merge on breaking symbols with consumers.";

const SKILL_MD: &str =
    include_str!("../../../skills/sephera-change-safety/SKILL.md");
const COMMANDS_MD: &str = include_str!(
    "../../../skills/sephera-change-safety/references/commands.md"
);
const TOOLS_MD: &str =
    include_str!("../../../skills/sephera-change-safety/references/tools.md");
const THRESHOLDS_MD: &str = include_str!(
    "../../../skills/sephera-change-safety/references/thresholds.md"
);

/// The files the skill ships, as (relative path, contents) pairs.
const FILES: [(&str, &str); 4] = [
    ("SKILL.md", SKILL_MD),
    ("references/commands.md", COMMANDS_MD),
    ("references/tools.md", TOOLS_MD),
    ("references/thresholds.md", THRESHOLDS_MD),
];

/// Install the skill, or print where it would go.
///
/// `--print` and `install` together are an error, not a coin toss:
/// one says "copy nothing" and the other says "copy", and picking
/// silently would make one of the two spellings a lie.
pub fn execute(arguments: &SkillArgs) -> Result<()> {
    if arguments.print && arguments.command.is_some() {
        anyhow::bail!("`--print` cannot be combined with `install`; drop one");
    }
    let target = target_dir(arguments)?;
    match &arguments.command {
        Some(SkillCommand::Install { force }) => install(&target, *force),
        None => {
            println!("{}", target.display());
            println!("{SUMMARY}");
            Ok(())
        }
    }
}

/// Where the skill goes: `<skills-dir>/sephera-change-safety`.
fn target_dir(arguments: &SkillArgs) -> Result<PathBuf> {
    let project = match &arguments.path {
        Some(path) => path.clone(),
        None => std::env::current_dir()
            .context("could not read the current directory")?,
    };
    let skills = match &arguments.dir {
        Some(dir) if dir.is_absolute() => dir.clone(),
        Some(dir) => project.join(dir),
        None => project.join(".claude").join("skills"),
    };
    Ok(skills.join(SKILL_NAME))
}

/// Copy the embedded files to the target.
///
/// An existing skill is an error unless forced: the target may hold
/// a skill the user tuned by hand, and overwriting it without being
/// asked would destroy work this tool cannot reconstruct.
fn install(target: &Path, force: bool) -> Result<()> {
    if target.exists() && !force {
        anyhow::bail!(
            "skill already exists at {}; re-run with `install --force` to overwrite it",
            target.display()
        );
    }
    for (relative, contents) in FILES {
        let path = target.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("could not create {}", parent.display())
            })?;
        }
        fs::write(&path, contents)
            .with_context(|| format!("could not write {}", path.display()))?;
    }
    println!("Installed {SKILL_NAME} to {}", target.display());
    Ok(())
}
