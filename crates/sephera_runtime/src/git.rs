// `std::process::Command` still runs the short-lived queries below -- rev-parse,
// ls-remote -- because they answer in milliseconds, and awaiting a future to find
// that out costs more than the answer. Only the clone is worth a runtime.
use std::{
    ffi::OsStr,
    path::Path,
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
    let output = git_stdout_string(
        repo_root,
        [
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

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use super::{parse_commit_files, recent_commit_files};

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
    fn a_directory_without_history_is_an_error_not_an_empty_answer() {
        let temp = tempfile::tempdir().expect("temp dir");
        assert!(
            recent_commit_files(temp.path(), 10).is_err(),
            "a non-repository must not read as a repository with no past"
        );
    }
}
