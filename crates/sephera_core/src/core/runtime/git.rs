use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};

/// Runs `git` with the given arguments in an optional working directory and returns the process output.
///
/// The `action` string is used in error messages to describe what was being attempted. If `working_directory` is `Some`, the command is executed with that directory as its current directory. On failure (non-zero exit status) the error includes the rendered `git ...` command and the trimmed UTF-8-lossy `stdout` and `stderr`.
///
/// # Parameters
///
/// - `working_directory`: Optional path to use as the command's current directory.
/// - `args`: Iterable collection of arguments passed to `git`.
/// - `action`: Short description of the attempted action included in error messages.
///
/// # Returns
///
/// `Ok(std::process::Output)` containing the child process output when the command exits successfully; otherwise an error containing the rendered command and trimmed stdout/stderr.
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # fn try_run() -> anyhow::Result<()> {
/// let out = crate::core::runtime::git::run_git(None::<&Path>, ["--version"], "check git version")?;
/// assert!(out.status.success());
/// # Ok(()) }
/// ```
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

/// Runs `git` with the given arguments in `working_directory` and returns the command's
/// standard output as an owned string with leading and trailing whitespace removed.
///
/// # Returns
///
/// `String` containing the command's `stdout`, trimmed of leading and trailing whitespace.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// let out = crate::core::runtime::git::git_stdout_string(Path::new("."), ["status", "--porcelain"], "check status").unwrap();
/// println!("{}", out);
/// ```
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

/// Obtain the raw stdout bytes produced by running a `git` command in the given working directory.
///
/// Executes `git` with the provided arguments inside `working_directory` and returns the captured stdout as a `Vec<u8>`.
///
/// # Errors
///
/// Returns an error if the `git` process could not be started or if it exits with a non-zero status; the error will include the rendered command and the trimmed stdout/stderr.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// // Get raw stdout bytes from `git rev-parse --abbrev-ref HEAD` in the current repository
/// let bytes = git_stdout_bytes(Path::new("."), ["rev-parse", "--abbrev-ref", "HEAD"], "determine current branch")?;
/// let branch = String::from_utf8_lossy(&bytes);
/// println!("current branch: {}", branch.trim());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// Render a `git` command string from a slice of arguments.
///
/// Each argument is converted via `AsRef<OsStr>` and rendered lossily to UTF-8,
/// then joined with spaces and prefixed with `git`.
///
/// # Examples
///
/// ```
/// let args = ["commit", "-m", "update"];
/// let cmd = render_command(&args);
/// assert_eq!(cmd, "git commit -m update");
/// ```
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
