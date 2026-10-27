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

const SKILL_MD: &str = include_str!("../skills/sephera-change-safety/SKILL.md");
const COMMANDS_MD: &str =
    include_str!("../skills/sephera-change-safety/references/commands.md");
const TOOLS_MD: &str =
    include_str!("../skills/sephera-change-safety/references/tools.md");
const THRESHOLDS_MD: &str =
    include_str!("../skills/sephera-change-safety/references/thresholds.md");

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
    let (project, target) = target_dir(arguments)?;
    match &arguments.command {
        Some(SkillCommand::Install { force }) => {
            install(&target, &project, *force)
        }
        None => {
            println!("{}", target.display());
            println!("{SUMMARY}");
            Ok(())
        }
    }
}

/// Where the skill goes: `<skills-dir>/sephera-change-safety`, plus the
/// project root the symlink check is bounded by.
fn target_dir(arguments: &SkillArgs) -> Result<(PathBuf, PathBuf)> {
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
    Ok((project, skills.join(SKILL_NAME)))
}

/// Copy the embedded files to the target.
///
/// An existing skill is an error unless forced: the target may hold
/// a skill the user tuned by hand, and overwriting it without being
/// asked would destroy work this tool cannot reconstruct.
///
/// A symlink at, under, or above the target up to the project root is
/// refused even with `--force`: `fs::write` and `fs::create_dir_all`
/// follow links, so a link a repository committed under
/// `.claude/skills/` — or at `.claude/skills` or `.claude` itself —
/// would send the install's bytes to whatever the link names. Only
/// the project root itself and anything above it are the caller's own
/// path, and are not checked.
fn install(target: &Path, project: &Path, force: bool) -> Result<()> {
    reject_symlinks(target, project)?;
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

/// Refuse a target that a symlink redirects.
///
/// Every path the install will create or overwrite is checked: the
/// target itself, each written file's parent directories down to
/// the target, and each written file — and, since a link above the
/// target redirects the target's own creation just as surely, every
/// ancestor of the target down to the project root. A repository
/// that commits `.claude/skills` (or `.claude` itself) as a symlink
/// used to pass the old check, which stopped at the target, and the
/// install then wrote the skill into whatever the link named.
/// A link anywhere on those paths would redirect a write, so the
/// install stops before writing anything. A path that does not
/// exist yet is not a link.
///
/// The project root itself is the boundary and is never checked:
/// a symlinked project root — a worktree path, a symlinked home —
/// is the caller's path, not the skill's. An absolute `--dir`
/// outside the project likewise contributes no ancestors, because
/// no ancestor of the target lies inside the project to check.
fn reject_symlinks(target: &Path, project: &Path) -> Result<()> {
    let mut candidates = vec![target.to_path_buf()];
    for (relative, _) in FILES {
        let path = target.join(relative);
        let mut ancestor = path.parent();
        while let Some(dir) = ancestor {
            if !dir.starts_with(target) {
                break;
            }
            candidates.push(dir.to_path_buf());
            ancestor = dir.parent();
        }
        candidates.push(path);
    }
    let mut ancestor = target.parent();
    while let Some(dir) = ancestor {
        if !dir.starts_with(project) || dir == project {
            break;
        }
        candidates.push(dir.to_path_buf());
        ancestor = dir.parent();
    }
    for path in candidates {
        let is_link = path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        if is_link {
            anyhow::bail!(
                "refusing to install over a symlink at {}; remove the link first",
                path.display()
            );
        }
    }
    Ok(())
}
