// `std::process::Command` still runs the short-lived queries below -- rev-parse,
// ls-remote -- because they answer in milliseconds, and awaiting a future to find
// that out costs more than the answer. Only the clone is worth a runtime.
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};

use super::interrupt::interrupt;

pub fn run_git<I, S>(
    working_directory: Option<&Path>,
    args: I,
    action: &str,
) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let mut command = Command::new("git");
    if let Some(working_directory) = working_directory {
        command.current_dir(working_directory);
    }
    command.args(args.iter().map(AsRef::as_ref));

    let output = command.output().with_context(|| {
        format!("failed to invoke git while trying to {action}")
    })?;

    if output.status.success() {
        Ok(output)
    } else {
        bail!(
            "failed to {action}: {}\nstdout:\n{}\nstderr:\n{}",
            render_command(&args),
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        );
    }
}

pub fn git_stdout_string<I, S>(
    working_directory: &Path,
    args: I,
    action: &str,
) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(Some(working_directory), args, action)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn git_stdout_bytes<I, S>(
    working_directory: &Path,
    args: I,
    action: &str,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(run_git(Some(working_directory), args, action)?.stdout)
}

/// How a long git command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOutcome {
    /// It ran to completion and succeeded.
    Finished,
    /// The user interrupted it.
    Interrupted,
}

/// Run one long git command with its own output going straight to the terminal.
///
/// Every other call here captures both streams, because the output is only wanted
/// to describe a failure. That is the wrong trade for a clone: it moves a
/// multi-hundred-megabyte download that takes minutes, and for its duration the
/// tool prints nothing at all. A run with no output for that long reads as a hung
/// process, and the first reasonable thing a user does about a hung process is
/// kill it.
///
/// git's own progress is better than anything invented here -- it knows the byte
/// counts and the rate -- so the child inherits the terminal and writes there
/// directly. Nothing is captured, which means a failure's diagnostics are already
/// on screen and the error raised here names the command rather than repeating
/// what the user has just watched scroll past.
///
/// `async` buys cancellation, not speed. A clone is minutes of network wait and
/// `tokio::process` waits exactly as long as `std::process` does -- on Windows it
/// blocks a thread on `WaitForSingleObject` either way. What it adds is somewhere to
/// stop: `tokio::select!` against the process's Ctrl+C listener, after which the
/// child is killed and reaped, so the temporary checkout is removed by its own
/// guard rather than left behind mid-download.
pub async fn run_git_streaming<I, S>(
    working_directory: Option<&Path>,
    args: I,
    action: &str,
) -> Result<GitOutcome>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let interrupt = interrupt();
    let mut command = tokio::process::Command::new("git");
    // A backstop for the paths that never reach the interrupt branch below --
    // a panic unwinding through here, for instance. The branch itself kills and
    // waits explicitly, because that is what the cleanup on the caller's side
    // actually needs.
    command.kill_on_drop(true);
    if let Some(working_directory) = working_directory {
        command.current_dir(working_directory);
    }
    command.args(args.iter().map(AsRef::as_ref));

    let mut child = command.spawn().with_context(|| {
        format!("failed to invoke git while trying to {action}")
    })?;

    let status = tokio::select! {
        status = child.wait() => {
            status.with_context(|| format!("failed while waiting for git to {action}"))?
        }
        () = interrupt.fired() => {
            // Kill *and reap*, rather than dropping the handle and hoping. The
            // caller's guard removes the checkout on the way out, and on Windows a
            // git that has been told to die but has not yet exited still holds
            // handles to files inside it -- so a dropped handle would leave the
            // directory the cleanup is trying to delete. A kill that reports an
            // error is not worth failing over: the outcome being reported is
            // `Interrupted` either way, and the child is on its way out.
            let _ = child.kill().await;
            return Ok(GitOutcome::Interrupted);
        }
    };

    if status.success() {
        Ok(GitOutcome::Finished)
    } else {
        bail!("failed to {} (`git {}`)", action, render_command(&args));
    }
}

/// The files each of the most recent `limit` commits touched, newest
/// first.
///
/// Paths are spelled as git spells them: relative to the repository
/// root. A commit that names no files -- a merge, whose file set is
/// its parents' rather than a change anyone made -- contributes an
/// empty list, so the length of the result is the number of commits
/// read, which a caller reporting "scanned N commits" needs to be
/// true.
///
/// # Errors
///
/// Returns an error when git cannot read the history, most often
/// because `repo_root` is not inside a git repository at all. That
/// is reported rather than read as "no history": the analyses built
/// on this answer a question about history, and an empty history and
/// an unreadable one warrant different responses.
pub fn recent_commit_files(
    repo_root: &Path,
    limit: u32,
) -> Result<Vec<Vec<String>>> {
    // `core.quotepath=false`: without it git quotes every
    // non-ASCII path in octal (`"caf\303\251.py"`), and that quoted
    // spelling never meets the graph's UTF-8 spelling — so a
    // repository with unicode filenames counted zero history, the
    // same silent-empty shape as a wrong analysis prefix.
    let output = git_stdout_string(
        repo_root,
        [
            "-c",
            "core.quotepath=false",
            "log",
            "--name-only",
            "--format=%x1e%H",
            "-n",
            &limit.to_string(),
            "--",
        ],
        "read the commit history",
    )?;
    Ok(parse_commit_files(&output))
}

/// The files a change touches, spelled from the repository root.
///
/// The change is the working tree read against `reference` -- `HEAD`
/// when none is named, so an unstaged edit, a staged one, and a
/// deletion all count. Committed differences between two refs are
/// the caller's `git diff A..B` concern; this answers the question a
/// review asks before anything is committed.
///
/// Untracked files are always included. A new file is the likeliest
/// thing in a change to break the build -- it is the one nothing
/// references yet and the one whose own imports nobody has run --
/// and a review that ignored it for having no history in the index
/// would miss exactly the file it exists to check.
///
/// # Errors
///
/// Returns an error when git cannot make the comparison, most often
/// because `repo_root` is not a repository or the reference does not
/// exist. That is reported rather than read as "nothing changed":
/// an empty change and an unreadable one warrant different responses.
pub fn changed_files(
    repo_root: &Path,
    reference: Option<&str>,
) -> Result<Vec<String>> {
    // Both readings must run from the work-tree root, or their
    // spellings disagree: `git diff` names files from the root
    // wherever it runs, while `git ls-files --others` names them
    // from the working directory. A caller passing a subdirectory
    // would otherwise get tracked changes in root spelling and
    // untracked files in base spelling, and the rebase onto the
    // graph's spelling could only ever match one of the two. A
    // directory that is not in a repository keeps today's working
    // directory, so the same error arrives as before.
    let root =
        repo_toplevel(repo_root).unwrap_or_else(|| repo_root.to_path_buf());
    let diff = git_stdout_string(
        &root,
        ["diff", "--name-only", "-z", reference.unwrap_or("HEAD")],
        "read the changed files",
    )?;
    let untracked = git_stdout_string(
        &root,
        ["ls-files", "--others", "--exclude-standard", "-z"],
        "read the untracked files",
    )?;

    let mut files: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for output in [&diff, &untracked] {
        files.extend(
            output
                .split('\0')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned),
        );
    }
    Ok(files.into_iter().collect())
}

/// One commit's identity for history walks: its hash, author,
/// short date, and subject line.
pub struct CommitMeta {
    /// Full hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Short date, as git spells it.
    pub date: String,
    /// Subject line.
    pub message: String,
}

/// Parse one `git log --format` line: hash, author, date, subject,
/// separated by unit-separator bytes. A malformed line is an
/// error, not a skipped commit: a silently wrong trail is worse
/// than a failed one.
fn parse_commit_meta(line: &str) -> Result<CommitMeta> {
    let fields: Vec<&str> = line.split('\u{1f}').collect();
    if fields.len() != 4 {
        bail!(
            "git log returned a line that is not hash, author, date, subject"
        );
    }
    Ok(CommitMeta {
        hash: fields[0].to_owned(),
        author: fields[1].to_owned(),
        date: fields[2].to_owned(),
        message: fields[3].to_owned(),
    })
}

/// The commits touching a file from a ref, newest first.
///
/// First-parent only: a merge's second parents are not followed,
/// so a rename that landed through a merge is attributed to the
/// merge commit itself, where the new identity first appears on
/// the branch. The file is spelled from the repository root, as
/// the graph spells it.
///
/// # Errors
///
/// Returns an error when git cannot read the history, most often
/// because `repo_root` is not inside a git repository at all or
/// the ref does not exist.
pub fn log_commits_for_file(
    repo_root: &Path,
    from_ref: &str,
    file: &str,
) -> Result<Vec<CommitMeta>> {
    let output = git_stdout_string(
        repo_root,
        [
            "log",
            "--first-parent",
            "--format=%H%x1f%an%x1f%ad%x1f%s",
            "--date=short",
            from_ref,
            "--",
            file,
        ],
        "read the commits touching a file",
    )?;
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_commit_meta)
        .collect()
}

/// One commit's metadata, by hash or ref.
///
/// # Errors
///
/// Returns an error when git cannot resolve the commit.
pub fn commit_meta(repo_root: &Path, commit: &str) -> Result<CommitMeta> {
    let output = git_stdout_string(
        repo_root,
        [
            "log",
            "-1",
            "--format=%H%x1f%an%x1f%ad%x1f%s",
            "--date=short",
            commit,
        ],
        "read one commit's metadata",
    )?;
    parse_commit_meta(output.lines().next().unwrap_or(""))
}

/// A commit's first-parent parent, or `None` at the root.
///
/// A rev-parse failure at a commit that exists means the commit
/// has no parent; a failure at a commit that does not exist is
/// reported, so a corrupt history cannot read as a root.
///
/// # Errors
///
/// Returns an error when git cannot run at all, or when the
/// commit itself does not resolve.
pub fn first_parent_commit(
    repo_root: &Path,
    commit: &str,
) -> Result<Option<CommitMeta>> {
    let parent = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", &format!("{commit}^1")])
        .output()
        .with_context(|| {
            format!("failed to invoke git while looking for {commit}'s parent")
        })?;
    if parent.status.success() {
        let hash = String::from_utf8_lossy(&parent.stdout).trim().to_owned();
        return Ok(Some(commit_meta(repo_root, &hash)?));
    }
    let exists = Command::new("git")
        .current_dir(repo_root)
        .args(["cat-file", "-e", commit])
        .output()
        .with_context(|| {
            format!("failed to invoke git while checking {commit} exists")
        })?;
    if exists.status.success() {
        Ok(None)
    } else {
        bail!("git cannot resolve {commit} while looking for its parent");
    }
}

/// Split `git log --name-only` output into one file list per commit.
///
/// Records begin with a unit-separator byte and the commit hash, so
/// the split is on a character that cannot appear in a path. Blank
/// lines are separators git puts between the header and the names
/// and between commits; filtering them makes the parse indifferent
/// to exactly where git puts them.
fn parse_commit_files(output: &str) -> Vec<Vec<String>> {
    let mut commits = Vec::new();
    for record in output.split('\u{1e}') {
        let lines: Vec<&str> = record
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        // The first line is the hash; the rest are the files.
        commits
            .push(lines[1..].iter().map(|line| (*line).to_owned()).collect());
    }
    commits
}

fn render_command<S>(args: &[S]) -> String
where
    S: AsRef<OsStr>,
{
    let rendered_args = args
        .iter()
        .map(|arg| arg.as_ref().to_string_lossy())
        .collect::<Vec<_>>();
    format!("git {}", rendered_args.join(" "))
}

/// A detached worktree holding one ref's tree, removed on drop.
///
/// Comparing two refs needs both trees on disk: a graph is built from
/// files, not from git objects. A worktree is the cheapest honest way
/// to get one — the repository's own objects, checked out beside the
/// main tree, without touching the caller's checkout or index. The
/// worktree holds tracked files only, exactly like the commit it
/// names; that is the point of comparing refs rather than working
/// trees.
///
/// Removal is best-effort by design: `git worktree remove --force`
/// unregisters and deletes, and the temporary directory's own drop
/// cleans anything left behind.
pub struct DetachedWorktree {
    repo: std::path::PathBuf,
    dir: tempfile::TempDir,
}

impl DetachedWorktree {
    /// The directory the ref's tree is checked out into.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for DetachedWorktree {
    fn drop(&mut self) {
        let _ = run_git(
            Some(&self.repo),
            [
                std::ffi::OsString::from("worktree"),
                std::ffi::OsString::from("remove"),
                std::ffi::OsString::from("--force"),
                self.dir.path().as_os_str().to_owned(),
            ],
            "remove the temporary worktree",
        );
    }
}

/// Check `reference` out into a fresh temporary worktree of `repo`.
///
/// The reference is verified as a commit first (with the same
/// `origin/` fallback the source resolver uses for branch names), so
/// a mistyped ref fails here with git's own answer rather than
/// downstream as an empty tree.
///
/// # Errors
///
/// Returns an error when `repo` is not a repository, the reference
/// does not resolve to a commit, or git cannot create the worktree.
pub fn checkout_worktree(
    repo: &Path,
    reference: &str,
) -> Result<DetachedWorktree> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        bail!("git ref must not be empty");
    }
    let resolved = resolve_ref(repo, trimmed)?;
    let dir =
        tempfile::tempdir().context("create a temporary worktree directory")?;
    run_git(
        Some(repo),
        [
            std::ffi::OsString::from("worktree"),
            std::ffi::OsString::from("add"),
            std::ffi::OsString::from("--detach"),
            dir.path().as_os_str().to_owned(),
            std::ffi::OsString::from(&resolved),
        ],
        &format!("check `{trimmed}` out into a worktree"),
    )?;
    Ok(DetachedWorktree {
        repo: repo.to_path_buf(),
        dir,
    })
}

/// Resolve a ref to a commit, with the `origin/` fallback the
/// source resolver uses for branch names: a bare branch name means
/// the local branch when one exists, else the remote-tracking one.
fn resolve_ref(repo: &Path, reference: &str) -> Result<String> {
    super::source::resolve_commitish(repo, reference).or_else(|_| {
        let remote_ref = format!("origin/{reference}");
        super::source::resolve_commitish(repo, &remote_ref)
    })
}

/// The fork point of two refs: the merge base git itself would use.
///
/// A three-way comparison needs this, not a guess. Base is where
/// the two sides diverged, so base→ours and base→theirs are the two
/// diffs whose interaction the comparison judges; any other base
/// would blame one side for the other's history.
///
/// # Errors
///
/// Returns an error when `repo` is not a repository, either ref
/// does not resolve to a commit, or the two histories share no
/// ancestor. An unrelated pair has no fork point, and that is
/// reported rather than read as an empty base.
pub fn merge_base(repo: &Path, ours: &str, theirs: &str) -> Result<String> {
    let ours_trimmed = ours.trim();
    let theirs_trimmed = theirs.trim();
    if ours_trimmed.is_empty() || theirs_trimmed.is_empty() {
        bail!("git ref must not be empty");
    }
    let ours_resolved = resolve_ref(repo, ours_trimmed)?;
    let theirs_resolved = resolve_ref(repo, theirs_trimmed)?;
    git_stdout_string(
        repo,
        ["merge-base", &ours_resolved, &theirs_resolved],
        "find the merge base",
    )
}

/// The work-tree root containing `dir`, or `None` when `dir` is not
/// inside a git repository.
///
/// Best-effort only: callers that need a repo-relative spelling use
/// it to prefix analysis-relative paths, and fall back to no prefix
/// when it answers `None` rather than failing a report whose paths
/// were already usable. `rev-parse --show-toplevel` answers in
/// milliseconds, so this stays a short-lived query like the other
/// rev-parse calls here. The root is canonicalised, so it compares
/// equal with a canonicalised analysis directory even when the
/// caller's path reached it through a symlink.
#[must_use]
pub fn repo_toplevel(dir: &Path) -> Option<PathBuf> {
    git_stdout_string(
        dir,
        ["rev-parse", "--show-toplevel"],
        "find the work-tree root",
    )
    .ok()
    .filter(|toplevel| !toplevel.is_empty())
    .map(|toplevel| {
        let path = PathBuf::from(toplevel);
        path.canonicalize().unwrap_or(path)
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use super::{
        changed_files, checkout_worktree, git_stdout_string, merge_base,
        parse_commit_files, recent_commit_files,
    };

    #[test]
    fn parse_splits_records_and_drops_the_hash_line() {
        // The shape `git log --name-only --format=%x1e%H` produces:
        // records led by the separator and hash, blank lines between
        // the names and the next record, and a merge-style record
        // holding a hash and no names at all.
        let output = "\u{1e}abc123\n\nsrc/a.rs\nsrc/b.rs\n\n\u{1e}def456\n\n\u{1e}789aaa\nsrc/c.rs\n";
        assert_eq!(
            parse_commit_files(output),
            vec![
                vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()],
                Vec::new(),
                vec!["src/c.rs".to_owned()],
            ]
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .expect("git runs");
        assert!(status.status.success(), "{status:?}");
    }

    fn rev_parse(dir: &Path, reference: &str) -> String {
        let output = Command::new("git")
            .args(["rev-parse", reference])
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn merge_base_is_the_fork_point_of_two_branches() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.txt"), "base\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "base"]);
        let base = rev_parse(dir, "HEAD");

        // A branch recorded at the base, then one commit on each
        // side — the default branch name is never spelled, so the
        // test does not depend on what `init` chose.
        git(dir, &["branch", "side"]);
        fs::write(dir.join("a.txt"), "base main\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "main"]);
        let main_tip = rev_parse(dir, "HEAD");
        git(dir, &["checkout", "-q", "side"]);
        fs::write(dir.join("b.txt"), "side\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "side"]);

        assert_eq!(
            merge_base(dir, "side", &main_tip).expect("base reads"),
            base,
            "the fork point, whichever order the sides are named"
        );
        assert_eq!(
            merge_base(dir, &main_tip, "side").expect("base reads"),
            base
        );
        // A ref against itself bases at itself, not at its parent.
        assert_eq!(
            merge_base(dir, "side", "side").expect("base reads"),
            rev_parse(dir, "side")
        );
    }

    #[test]
    fn merge_base_without_a_common_ancestor_is_an_error() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.txt"), "one\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);
        let first = rev_parse(dir, "HEAD");

        git(dir, &["checkout", "-q", "--orphan", "unrelated"]);
        git(dir, &["rm", "-rf", "."]);
        fs::write(dir.join("other.txt"), "two\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "unrelated"]);

        assert!(
            merge_base(dir, &first, "unrelated").is_err(),
            "unrelated histories have no fork point to report"
        );
        assert!(
            merge_base(dir, &first, "no-such-ref").is_err(),
            "an unknown ref fails before any base is computed"
        );
        assert!(
            merge_base(dir, "", &first).is_err(),
            "an empty ref is not a ref"
        );
    }

    #[test]
    fn recent_commit_files_reads_a_real_repository() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.txt"), "one\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);
        fs::write(dir.join("b.txt"), "two\n").expect("write");
        fs::write(dir.join("a.txt"), "one more\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "second"]);

        let commits = recent_commit_files(dir, 10).expect("history reads");
        assert_eq!(commits.len(), 2, "one list per commit");
        assert_eq!(
            commits[0],
            vec!["a.txt".to_owned(), "b.txt".to_owned()],
            "newest first, both files of the second commit"
        );
        assert_eq!(commits[1], vec!["a.txt".to_owned()]);

        let one = recent_commit_files(dir, 1).expect("history reads");
        assert_eq!(one.len(), 1, "the limit is honoured");
    }

    #[test]
    fn a_worktree_holds_the_refs_tree_and_is_removed_on_drop() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.txt"), "one\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);
        fs::write(dir.join("b.txt"), "two\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "second"]);

        let worktree_path;
        {
            let worktree = checkout_worktree(dir, "HEAD~1").expect("worktree");
            worktree_path = worktree.path().to_path_buf();
            // The older ref's tree: a.txt as it was, and no b.txt.
            assert_eq!(
                fs::read_to_string(worktree_path.join("a.txt")).expect("read"),
                "one\n"
            );
            assert!(!worktree_path.join("b.txt").exists());
        }

        assert!(!worktree_path.exists(), "the drop removes the worktree");
        let list =
            git_stdout_string(dir, ["worktree", "list", "--porcelain"], "list")
                .expect("worktree list reads");
        assert_eq!(
            list.matches("worktree ").count(),
            1,
            "only the main tree remains: {list}"
        );
    }

    #[test]
    fn an_unknown_ref_fails_the_checkout() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.txt"), "one\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);

        assert!(checkout_worktree(dir, "no-such-ref").is_err());
    }

    #[test]
    fn changed_files_reads_the_working_tree_against_a_reference() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.txt"), "one\n").expect("write");
        fs::write(dir.join("gone.txt"), "soon\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);

        // An unstaged edit, a deletion, and a file git has never seen.
        fs::write(dir.join("a.txt"), "one more\n").expect("write");
        fs::remove_file(dir.join("gone.txt")).expect("remove");
        fs::write(dir.join("new.txt"), "uncommitted\n").expect("write");

        let changed = changed_files(dir, None).expect("the change reads");
        assert_eq!(
            changed,
            vec![
                "a.txt".to_owned(),
                "gone.txt".to_owned(),
                "new.txt".to_owned()
            ],
            "edited, deleted and untracked, sorted, each once"
        );

        // A commit makes the same tree clean against the new HEAD and
        // changed against the first commit.
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "second"]);
        assert_eq!(
            changed_files(dir, None).expect("clean reads"),
            Vec::<String>::new()
        );
        let against_first = changed_files(dir, Some("HEAD~1"))
            .expect("the older reference reads");
        assert_eq!(
            against_first,
            vec![
                "a.txt".to_owned(),
                "gone.txt".to_owned(),
                "new.txt".to_owned()
            ]
        );
    }

    #[test]
    fn regression_recent_commit_files_spells_unicode_names_unquoted() {
        // Git quotes non-ASCII paths in octal by default, and the
        // quoted spelling never meets the graph's UTF-8 spelling:
        // history over a repository with unicode filenames counted
        // zero commits. The log call disables quoting, so the names
        // come back exactly as the files are spelled.
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("café.py"), "x = 1\n").expect("write");
        fs::write(dir.join("münchen.py"), "y = 2\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);
        fs::write(dir.join("café.py"), "x = 1\n# 2\n").expect("write");
        fs::write(dir.join("münchen.py"), "y = 2\n# 2\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "second"]);

        let commits = recent_commit_files(dir, 10).expect("history reads");
        assert_eq!(commits.len(), 2);
        assert_eq!(
            commits[0],
            vec!["café.py".to_owned(), "münchen.py".to_owned()],
            "the names as the files are spelled, not git's octal quoting"
        );
    }

    #[test]
    fn regression_changed_files_from_a_subdir_spells_both_readings_from_the_root()
     {
        // `git diff` names files from the repository root wherever
        // it runs, while `git ls-files --others` names them from
        // the working directory. Called with a subdirectory, the
        // change used to mix the two spellings: a tracked edit came
        // back root-relative and an untracked file base-relative,
        // so no single rebase could match both to the graph.
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path();
        git(dir, &["init", "-q"]);
        fs::create_dir(dir.join("pkg")).expect("mkdir");
        fs::write(dir.join("pkg/a.py"), "x = 1\n").expect("write");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "first"]);

        fs::write(dir.join("pkg/a.py"), "x = 2\n").expect("write");
        fs::write(dir.join("pkg/new.py"), "z = 3\n").expect("write");

        let changed =
            changed_files(&dir.join("pkg"), None).expect("the change reads");
        assert_eq!(
            changed,
            vec!["pkg/a.py".to_owned(), "pkg/new.py".to_owned()],
            "both readings in the repository root's spelling"
        );
    }

    #[test]
    fn a_directory_without_history_is_an_error_not_an_empty_answer() {
        let temp = tempfile::tempdir().expect("temp dir");
        assert!(
            recent_commit_files(temp.path(), 10).is_err(),
            "a non-repository must not read as a repository with no past"
        );
    }
}
