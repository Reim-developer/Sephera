// `std::process::Command` still runs the short-lived queries below -- rev-parse,
// ls-remote -- because they answer in milliseconds, and awaiting a future to find
// that out costs more than the answer. Only the clone is worth a runtime.
use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};

pub(super) fn run_git<I, S>(
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

pub(super) fn git_stdout_string<I, S>(
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

pub(super) fn git_stdout_bytes<I, S>(
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
pub(super) enum GitOutcome {
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
/// stop: `tokio::select!` against Ctrl+C, after which the child is killed and the
/// temporary checkout is removed by its guard, so an interrupted clone does not
/// leave a multi-gigabyte directory behind.
pub(super) async fn run_git_streaming<I, S>(
    working_directory: Option<&Path>,
    args: I,
    action: &str,
) -> Result<GitOutcome>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
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
        _ = tokio::signal::ctrl_c() => {
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
