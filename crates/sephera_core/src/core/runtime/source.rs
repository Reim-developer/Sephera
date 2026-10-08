use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;
use url::Url;

use super::git::{GitOutcome, git_stdout_string, run_git, run_git_streaming};
use super::interrupt::interrupt;

/// A long git command was stopped at the user's request rather than finishing.
///
/// Its own type because the exit code is different and because "you pressed
/// Ctrl+C" is not a failure worth an `error:` line. A clone of a large repository
/// runs for minutes; interrupting one should read as a decision, not as a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("interrupted")
    }
}

impl std::error::Error for Interrupted {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceRequest {
    pub path: Option<PathBuf>,
    pub url: Option<String>,
    pub git_ref: Option<String>,
}

#[derive(Debug)]
pub struct ResolvedSource {
    pub analysis_path: PathBuf,
    pub repo_root: PathBuf,
    pub display_path: Option<String>,
    pub display_repo_root: Option<String>,
    pub(crate) checkout_guard: Option<TempDir>,
}

impl ResolvedSource {
    #[must_use]
    pub const fn is_remote(&self) -> bool {
        self.checkout_guard.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeHostingStyle {
    GitHub,
    GitLab,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedRemoteSource {
    Repo {
        clone_url: String,
        display_repo_root: String,
    },
    Tree {
        clone_url: String,
        display_repo_root: String,
        style: TreeHostingStyle,
        tail_segments: Vec<String>,
    },
}

/// Resolves a local path or remote repository URL into a concrete analysis
/// source on disk.
///
/// # Errors
///
/// Returns an error when the request is invalid, the URL cannot be parsed,
/// cloning or checkout fails, or a tree URL cannot be resolved to a valid
/// directory in the temporary checkout.
///
/// `requires_history` says the caller will compare the checkout against
/// something other than its tip -- a Git base ref behind `--diff`. It is a
/// parameter rather than a field on [`SourceRequest`] because it is not a
/// property of the source: it describes what the caller intends to do next, and
/// the twenty-odd places that name a `path`, a `url` and a `ref` should not each
/// have to answer a question about a later step.
pub async fn resolve_source(
    request: &SourceRequest,
    requires_history: bool,
) -> Result<ResolvedSource> {
    match (&request.path, &request.url) {
        (Some(_), Some(_)) => {
            bail!("`path` and `url` are mutually exclusive");
        }
        (None, None) => {
            if request.git_ref.is_some() {
                bail!("`ref` requires `url`");
            }

            Ok(ResolvedSource {
                analysis_path: PathBuf::from("."),
                repo_root: PathBuf::from("."),
                display_path: None,
                display_repo_root: None,
                checkout_guard: None,
            })
        }
        (Some(path), None) => {
            if request.git_ref.is_some() {
                bail!("`ref` requires `url`");
            }

            Ok(ResolvedSource {
                analysis_path: path.clone(),
                repo_root: path.clone(),
                display_path: None,
                display_repo_root: None,
                checkout_guard: None,
            })
        }
        (None, Some(raw_url)) => {
            let parsed_source = parse_remote_source(raw_url)?;
            if matches!(parsed_source, ParsedRemoteSource::Tree { .. })
                && request.git_ref.is_some()
            {
                bail!("`ref` cannot be combined with a tree URL");
            }

            resolve_remote_source(
                parsed_source,
                request.git_ref.as_deref(),
                requires_history,
            )
            .await
        }
    }
}

async fn resolve_remote_source(
    parsed_source: ParsedRemoteSource,
    git_ref: Option<&str>,
    requires_history: bool,
) -> Result<ResolvedSource> {
    let checkout_root =
        TempDir::new().context("failed to create temp checkout")?;
    let repo_root = checkout_root.path().join("repo");

    let clone_url = match &parsed_source {
        ParsedRemoteSource::Repo { clone_url, .. }
        | ParsedRemoteSource::Tree { clone_url, .. } => clone_url,
    };

    // A tree URL carries its own branch, and it is rarely the default one.
    let scope = match &parsed_source {
        ParsedRemoteSource::Repo { .. } => BranchScope::Default,
        ParsedRemoteSource::Tree { .. } => BranchScope::AllAtDepthOne,
    };

    // Held across the clone and released once the `?` below has unwound, so the
    // listener knows a deletion is under way and must not exit out from under it.
    let cleanup = interrupt().cleanup_guard();
    let outcome = run_git_streaming(
        None,
        clone_arguments(
            clone_url,
            repo_root.as_os_str(),
            git_ref.is_some() || requires_history,
            scope,
        ),
        &format!("clone `{clone_url}`"),
    )
    .await;
    // Dropped here rather than at the end of the function: `ResolvedSource` takes
    // the `TempDir` and keeps the checkout on disk for the rest of the run, so a
    // guard held until then would answer "is a deletion in flight" with yes long
    // after the only deletion that needed watching was done.
    drop(cleanup);

    match outcome? {
        GitOutcome::Finished => {}
        // The checkout is removed by the guard on the way out, so interrupting a
        // clone leaves nothing behind rather than a partial repository.
        GitOutcome::Interrupted => return Err(Interrupted.into()),
    }

    let (analysis_path, display_path, display_repo_root) = match parsed_source {
        ParsedRemoteSource::Repo {
            display_repo_root, ..
        } => {
            let resolved_ref = git_ref
                .map(|raw_ref| resolve_checkout_ref(&repo_root, raw_ref))
                .transpose()?;
            let display_repo_root = resolved_ref.map_or_else(
                || display_repo_root.clone(),
                |raw_ref| format!("{display_repo_root}@{raw_ref}"),
            );
            (
                repo_root.clone(),
                Some(display_repo_root.clone()),
                Some(display_repo_root),
            )
        }
        ParsedRemoteSource::Tree {
            display_repo_root,
            style,
            tail_segments,
            ..
        } => {
            let (resolved_ref, sub_path) =
                resolve_tree_checkout(&repo_root, &tail_segments)?;
            let display_repo_root_with_ref =
                format!("{display_repo_root}@{resolved_ref}");
            let display_path = render_tree_display_url(
                style,
                &display_repo_root,
                &resolved_ref,
                sub_path.as_deref(),
            );
            let analysis_path = sub_path.as_ref().map_or_else(
                || repo_root.clone(),
                |sub_path| repo_root.join(sub_path),
            );
            (
                analysis_path,
                Some(display_path),
                Some(display_repo_root_with_ref),
            )
        }
    };

    if !analysis_path.exists() {
        bail!(
            "resolved analysis path `{}` does not exist in the checkout",
            analysis_path.display()
        );
    }
    if !analysis_path.is_dir() {
        bail!(
            "resolved analysis path `{}` is not a directory",
            analysis_path.display()
        );
    }

    Ok(ResolvedSource {
        analysis_path,
        repo_root,
        display_path,
        display_repo_root,
        checkout_guard: Some(checkout_root),
    })
}

/// Which branches a shallow clone has to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchScope {
    /// Only the branch the remote's HEAD points at.
    ///
    /// Right for a repo URL, which analyses whatever the default branch is.
    Default,
    /// Every branch, one commit deep.
    ///
    /// A tree URL names its own branch in the path, and that branch is usually
    /// not the default: `https://github.com/o/r/tree/feature/docs` on a
    /// repository whose default is `main`. `--single-branch` would fetch `main`,
    /// the named branch would never arrive, and the checkout would fail on a
    /// repository that has it. One commit per branch is still a fraction of a
    /// full history, so this buys correctness at a small price rather than
    /// dropping the shallow clone.
    AllAtDepthOne,
}

/// The `git clone` arguments for one analysis, shallow unless history is asked for.
///
/// A clone that fetches every commit downloads a repository's entire history to
/// read the one commit at its tip. On the kernel that measured 792 seconds against
/// 201 for the shallow clone of the same repository, with byte-identical line
/// counts, so the history was bought and thrown away.
///
/// Two cases cannot be shallow, and both are about naming something other than
/// the tip. `--ref` may name any commit, and a `--diff` base ref is resolved by
/// walking back from the tip -- `HEAD~1` does not exist in a clone one commit
/// deep.
///
/// The checkout is temporary either way, so there is no cache to keep warm and
/// nothing about a shallow clone survives the command. That is the argument for
/// it: the cost is paid once, inside one command, and only for the files that are
/// actually read.
fn clone_arguments(
    clone_url: &str,
    destination: &OsStr,
    requires_history: bool,
    scope: BranchScope,
) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("clone")];

    if !requires_history {
        arguments.push(OsString::from("--depth=1"));
        if scope == BranchScope::Default {
            arguments.push(OsString::from("--single-branch"));
        }
    }

    arguments.push(OsString::from(clone_url));
    arguments.push(OsString::from(destination));
    arguments
}

fn resolve_tree_checkout(
    repo_root: &Path,
    tail_segments: &[String],
) -> Result<(String, Option<String>)> {
    if tail_segments.is_empty() {
        bail!("tree URL is missing the ref segment");
    }

    for prefix_len in (1..=tail_segments.len()).rev() {
        let raw_ref = tail_segments[..prefix_len].join("/");
        if let Ok(resolved_ref) = resolve_checkout_ref(repo_root, &raw_ref) {
            let sub_path = if prefix_len == tail_segments.len() {
                None
            } else {
                Some(tail_segments[prefix_len..].join("/"))
            };
            return Ok((resolved_ref, sub_path));
        }
    }

    bail!(
        "failed to resolve a git ref from tree URL segments `{}`",
        tail_segments.join("/")
    )
}

fn resolve_checkout_ref(repo_root: &Path, raw_ref: &str) -> Result<String> {
    let trimmed_ref = raw_ref.trim();
    if trimmed_ref.is_empty() {
        bail!("git ref must not be empty");
    }

    let resolved_ref =
        resolve_commitish(repo_root, trimmed_ref).or_else(|_| {
            let remote_ref = format!("origin/{trimmed_ref}");
            resolve_commitish(repo_root, &remote_ref)
        })?;

    run_git(
        Some(repo_root),
        [
            OsString::from("checkout"),
            OsString::from("--detach"),
            OsString::from(&resolved_ref),
        ],
        &format!("checkout `{trimmed_ref}`"),
    )?;

    Ok(trimmed_ref.to_owned())
}

fn resolve_commitish(repo_root: &Path, candidate: &str) -> Result<String> {
    git_stdout_string(
        repo_root,
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from(format!("{candidate}^{{commit}}")),
        ],
        &format!("resolve git ref `{candidate}`"),
    )
}

fn parse_remote_source(raw_url: &str) -> Result<ParsedRemoteSource> {
    let trimmed_url = raw_url.trim();
    if trimmed_url.is_empty() {
        bail!("URL must not be empty");
    }

    if is_scp_style_git_url(trimmed_url) {
        return Ok(ParsedRemoteSource::Repo {
            clone_url: trimmed_url.to_owned(),
            display_repo_root: strip_trailing_git_suffix(trimmed_url),
        });
    }

    let parsed_url = Url::parse(trimmed_url)
        .with_context(|| format!("failed to parse URL `{trimmed_url}`"))?;

    match parsed_url.scheme() {
        "http" | "https" => parse_http_remote_source(&parsed_url),
        "ssh" | "file" => Ok(ParsedRemoteSource::Repo {
            clone_url: strip_trailing_slash(trimmed_url),
            display_repo_root: strip_trailing_git_suffix(trimmed_url),
        }),
        scheme => bail!("unsupported URL scheme `{scheme}`"),
    }
}

fn parse_http_remote_source(parsed_url: &Url) -> Result<ParsedRemoteSource> {
    let path_segments = parsed_url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if path_segments.len() < 2 {
        bail!("URL must include a repository path");
    }

    if path_segments
        .get(2)
        .is_some_and(|segment| *segment == "blob")
    {
        bail!("blob URLs are not supported");
    }

    if let Some(marker_index) =
        path_segments.iter().position(|segment| *segment == "-")
        && path_segments
            .get(marker_index + 1)
            .is_some_and(|segment| *segment == "tree")
    {
        if marker_index < 2 {
            bail!("GitLab tree URL must include a repository path");
        }

        if path_segments.get(marker_index + 2).is_none() {
            bail!("tree URL is missing the ref segment");
        }

        let repo_segments = &path_segments[..marker_index];
        let tail_segments = path_segments[(marker_index + 2)..]
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect::<Vec<_>>();
        let repo_root = build_http_repo_root(parsed_url, repo_segments);

        return Ok(ParsedRemoteSource::Tree {
            clone_url: repo_root.clone(),
            display_repo_root: repo_root,
            style: TreeHostingStyle::GitLab,
            tail_segments,
        });
    }

    if path_segments
        .get(2)
        .is_some_and(|segment| *segment == "tree")
    {
        if path_segments.get(3).is_none() {
            bail!("tree URL is missing the ref segment");
        }

        let repo_segments = &path_segments[..2];
        let tail_segments = path_segments[3..]
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect::<Vec<_>>();
        let repo_root = build_http_repo_root(parsed_url, repo_segments);

        return Ok(ParsedRemoteSource::Tree {
            clone_url: repo_root.clone(),
            display_repo_root: repo_root,
            style: TreeHostingStyle::GitHub,
            tail_segments,
        });
    }

    if path_segments.contains(&"blob") {
        bail!("blob URLs are not supported");
    }

    Ok(ParsedRemoteSource::Repo {
        clone_url: strip_trailing_slash(parsed_url.as_str()),
        display_repo_root: strip_trailing_git_suffix(parsed_url.as_str()),
    })
}

fn build_http_repo_root(parsed_url: &Url, repo_segments: &[&str]) -> String {
    let mut repo_root = format!(
        "{}://{}",
        parsed_url.scheme(),
        parsed_url.host_str().expect("HTTP URLs must have a host"),
    );
    if let Some(port) = parsed_url.port() {
        repo_root.push(':');
        repo_root.push_str(&port.to_string());
    }
    repo_root.push('/');
    repo_root.push_str(&repo_segments.join("/"));
    repo_root
}

fn render_tree_display_url(
    style: TreeHostingStyle,
    repo_root: &str,
    git_ref: &str,
    sub_path: Option<&str>,
) -> String {
    let mut rendered = match style {
        TreeHostingStyle::GitHub => format!("{repo_root}/tree/{git_ref}"),
        TreeHostingStyle::GitLab => format!("{repo_root}/-/tree/{git_ref}"),
    };

    if let Some(sub_path) = sub_path
        && !sub_path.is_empty()
    {
        rendered.push('/');
        rendered.push_str(sub_path);
    }

    rendered
}

fn is_scp_style_git_url(raw_url: &str) -> bool {
    raw_url.contains('@')
        && raw_url.contains(':')
        && !raw_url.contains("://")
        && raw_url
            .split_once(':')
            .is_some_and(|(left, right)| !left.is_empty() && !right.is_empty())
}

fn strip_trailing_slash(raw_url: &str) -> String {
    raw_url.trim_end_matches('/').to_owned()
}

fn strip_trailing_git_suffix(raw_url: &str) -> String {
    strip_trailing_slash(raw_url)
        .trim_end_matches(".git")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn a_clone_without_a_ref_is_shallow() {
        let arguments = clone_arguments(
            "https://example.invalid/r",
            OsStr::new("/tmp/repo"),
            false,
            BranchScope::Default,
        )
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert_eq!(
            arguments,
            vec![
                "clone",
                "--depth=1",
                "--single-branch",
                "https://example.invalid/r",
                "/tmp/repo",
            ]
        );
    }

    #[test]
    fn a_clone_naming_a_ref_keeps_the_history() {
        // `--depth 1` only carries the branch tip, so a ref that is not the tip
        // cannot be resolved from a shallow clone. Cloning in full is the price
        // of `--ref`, and the alternative -- cloning shallow and then failing to
        // find the commit -- is worse, because it reports a missing ref for a
        // repository that has it.
        let arguments = clone_arguments(
            "https://example.invalid/r",
            OsStr::new("/tmp/repo"),
            true,
            BranchScope::Default,
        )
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert_eq!(
            arguments,
            vec!["clone", "https://example.invalid/r", "/tmp/repo",]
        );
    }

    #[test]
    fn the_clone_is_not_asked_to_be_quiet() {
        // git's own progress is the only accurate report of a download that takes
        // minutes, and `run_git_streaming` lets it reach the terminal. Asking for
        // quiet here would trade a real progress bar for a silent wait, which is
        // the opposite of the point -- a clone that prints nothing for three
        // minutes is indistinguishable from a hung process.
        let arguments = clone_arguments(
            "https://example.invalid/r",
            OsStr::new("d"),
            false,
            BranchScope::Default,
        );

        assert!(
            !arguments.iter().any(|argument| argument == "--quiet"),
            "the clone must be allowed to report its progress"
        );
    }

    #[test]
    fn a_tree_url_takes_every_branch_at_depth_one() {
        // A tree URL names its branch in the path, and that branch is usually not
        // the default: `.../tree/feature/docs` on a repository whose default is
        // `main`. `--single-branch` fetches `main`, the named branch never
        // arrives, and the checkout fails on a repository that has it. The depth
        // stays at one, so this costs one commit per branch rather than a
        // history.
        let arguments = clone_arguments(
            "https://example.invalid/r",
            OsStr::new("/tmp/repo"),
            false,
            BranchScope::AllAtDepthOne,
        )
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert_eq!(
            arguments,
            vec![
                "clone",
                "--depth=1",
                "https://example.invalid/r",
                "/tmp/repo",
            ],
            "no `--single-branch`: the branch the URL names is not the default"
        );
    }

    #[test]
    fn a_repo_url_takes_only_the_default_branch() {
        // The common case, and the one the performance measurement is about: a
        // repository's thousands of branches are not what the analysis reads.
        let arguments = clone_arguments(
            "https://example.invalid/r",
            OsStr::new("/tmp/repo"),
            false,
            BranchScope::Default,
        )
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert!(
            arguments
                .iter()
                .any(|argument| argument == "--single-branch")
        );
    }

    #[test]
    fn history_needed_overrides_both_branch_scopes() {
        // Whichever branches a clone would carry, asking for history means
        // fetching all of it, so `--depth` and `--single-branch` are both wrong.
        for scope in [BranchScope::Default, BranchScope::AllAtDepthOne] {
            let arguments = clone_arguments(
                "https://example.invalid/r",
                OsStr::new("d"),
                true,
                scope,
            );
            let rendered = arguments
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>();

            assert_eq!(
                rendered,
                vec!["clone", "https://example.invalid/r", "d"],
                "a full clone carries every branch at every depth, {scope:?}"
            );
        }
    }

    fn run_git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo_root)
            .args(args)
            .output()
            .unwrap_or_else(|error| {
                panic!("failed to run git {args:?}: {error}")
            });
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn init_repo(repo_root: &Path) {
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["config", "user.name", "Sephera Tests"]);
        run_git(repo_root, &["config", "user.email", "tests@example.com"]);
    }

    fn commit_all(repo_root: &Path, message: &str) {
        run_git(repo_root, &["add", "-A"]);
        run_git(repo_root, &["commit", "-m", message]);
    }

    fn write_file(repo_root: &Path, relative_path: &str, contents: &str) {
        let absolute_path = repo_root.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute_path, contents).unwrap();
    }

    #[test]
    fn parses_github_tree_url() {
        let parsed_source = parse_remote_source(
            "https://github.com/Reim-developer/Sephera/tree/main/docs/src",
        )
        .unwrap();

        assert_eq!(
            parsed_source,
            ParsedRemoteSource::Tree {
                clone_url: String::from(
                    "https://github.com/Reim-developer/Sephera"
                ),
                display_repo_root: String::from(
                    "https://github.com/Reim-developer/Sephera"
                ),
                style: TreeHostingStyle::GitHub,
                tail_segments: vec![
                    String::from("main"),
                    String::from("docs"),
                    String::from("src"),
                ],
            }
        );
    }

    #[test]
    fn parses_gitlab_tree_url() {
        let parsed_source = parse_remote_source(
            "https://gitlab.com/group/subgroup/repo/-/tree/main/docs",
        )
        .unwrap();

        assert_eq!(
            parsed_source,
            ParsedRemoteSource::Tree {
                clone_url: String::from(
                    "https://gitlab.com/group/subgroup/repo"
                ),
                display_repo_root: String::from(
                    "https://gitlab.com/group/subgroup/repo"
                ),
                style: TreeHostingStyle::GitLab,
                tail_segments: vec![String::from("main"), String::from("docs")],
            }
        );
    }

    #[test]
    fn rejects_blob_urls() {
        let error = parse_remote_source(
            "https://github.com/Reim-developer/Sephera/blob/main/README.md",
        )
        .unwrap_err();

        assert!(error.to_string().contains("blob URLs are not supported"));
    }

    #[test]
    fn parses_scp_style_repo_url() {
        let parsed_source =
            parse_remote_source("git@github.com:Reim-developer/Sephera.git")
                .unwrap();

        assert_eq!(
            parsed_source,
            ParsedRemoteSource::Repo {
                clone_url: String::from(
                    "git@github.com:Reim-developer/Sephera.git"
                ),
                display_repo_root: String::from(
                    "git@github.com:Reim-developer/Sephera"
                ),
            }
        );
    }

    // `tokio::test` rather than `test` because the clone registers a signal handler
    // for the interrupt branch, which needs a runtime with `enable_all`.
    #[tokio::test]
    async fn resolves_file_repo_url_and_selected_ref() {
        let temp_dir = tempdir().unwrap();
        init_repo(temp_dir.path());
        write_file(temp_dir.path(), "README.md", "# main\n");
        commit_all(temp_dir.path(), "main");
        run_git(temp_dir.path(), &["tag", "v1.0.0"]);

        let source = resolve_source(
            &SourceRequest {
                path: None,
                url: Some(format!("file://{}", temp_dir.path().display())),
                git_ref: Some(String::from("v1.0.0")),
            },
            false,
        )
        .await
        .unwrap();

        assert!(source.is_remote());
        assert!(source.analysis_path.join("README.md").is_file());
        let expected_display =
            format!("file://{}@v1.0.0", temp_dir.path().display());
        assert_eq!(
            source.display_path.as_deref(),
            Some(expected_display.as_str())
        );
    }

    #[tokio::test]
    async fn resolves_tree_url_with_branch_names_that_contain_slashes() {
        let temp_dir = tempdir().unwrap();
        init_repo(temp_dir.path());
        write_file(temp_dir.path(), "src/lib.rs", "pub fn main() {}\n");
        commit_all(temp_dir.path(), "initial");
        run_git(temp_dir.path(), &["checkout", "-b", "feature/docs"]);
        write_file(temp_dir.path(), "docs/guide.md", "# guide\n");
        commit_all(temp_dir.path(), "docs");

        let source = resolve_source(
            &SourceRequest {
                path: None,
                url: Some(
                    "https://github.com/Reim-developer/Sephera/tree/feature/docs/docs"
                        .to_string(),
                ),
                git_ref: None,
            },
            false,
        )
        .await;

        assert!(source.is_err());

        let source = resolve_remote_source(
            ParsedRemoteSource::Tree {
                clone_url: format!("file://{}", temp_dir.path().display()),
                display_repo_root: String::from(
                    "https://github.com/Reim-developer/Sephera",
                ),
                style: TreeHostingStyle::GitHub,
                tail_segments: vec![
                    String::from("feature"),
                    String::from("docs"),
                    String::from("docs"),
                ],
            },
            None,
            false,
        )
        .await
        .unwrap();

        assert!(
            source.analysis_path.join("guide.md").is_file(),
            "expected tree URL to resolve to the docs subdirectory"
        );
        assert_eq!(
            source.display_path.as_deref(),
            Some(
                "https://github.com/Reim-developer/Sephera/tree/feature/docs/docs"
            )
        );
        assert_eq!(
            source.display_repo_root.as_deref(),
            Some("https://github.com/Reim-developer/Sephera@feature/docs")
        );
    }

    #[tokio::test]
    async fn rejects_ref_without_url() {
        let error = resolve_source(
            &SourceRequest {
                path: Some(PathBuf::from(".")),
                url: None,
                git_ref: Some(String::from("main")),
            },
            false,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("`ref` requires `url`"));
    }
}
