use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;
use url::Url;

use super::git::{git_stdout_string, run_git};

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
    /// Indicates whether the resolved source was produced from a remote checkout.
    ///
    /// `true` if the resolved source contains a `TempDir` checkout guard, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use tempfile::tempdir;
    /// use std::path::PathBuf;
    ///
    /// let local = ResolvedSource {
    ///     analysis_path: PathBuf::from("."),
    ///     repo_root: PathBuf::from("."),
    ///     display_path: None,
    ///     display_repo_root: None,
    ///     checkout_guard: None,
    /// };
    /// assert!(!local.is_remote());
    ///
    /// let guard = tempdir().unwrap();
    /// let remote = ResolvedSource {
    ///     analysis_path: PathBuf::from("."),
    ///     repo_root: PathBuf::from("."),
    ///     display_path: None,
    ///     display_repo_root: None,
    ///     checkout_guard: Some(guard),
    /// };
    /// assert!(remote.is_remote());
    /// ```
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

/// Resolve a SourceRequest into a concrete on-disk analysis source.
///
/// This accepts either a local path, an empty request (both `path` and `url` absent),
/// or a remote repository URL (including supported tree URLs), and returns the
/// concrete analysis path, repository root, optional display fields, and an optional
/// checkout guard that keeps a temporary clone alive while held.
///
/// # Errors
///
/// Returns an error when:
/// - both `path` and `url` are provided (they are mutually exclusive),
/// - `git_ref` is provided without a `url`,
/// - a `git_ref` is combined with a tree URL,
/// - the URL cannot be parsed or is unsupported,
/// - cloning or checkout fails, or
/// - the resolved analysis path does not exist or is not a directory.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// let req = crate::core::runtime::source::SourceRequest::default();
/// let resolved = crate::core::runtime::source::resolve_source(&req).unwrap();
/// assert!(resolved.analysis_path.exists());
/// ```
pub fn resolve_source(request: &SourceRequest) -> Result<ResolvedSource> {
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

            resolve_remote_source(parsed_source, request.git_ref.as_deref())
        }
    }
}

/// Clones a parsed remote repository into a temporary checkout and returns the resolved on-disk
/// analysis path along with human-facing display metadata.
///
/// For repository-style inputs this optionally checks out the provided `git_ref`. For tree-style
/// inputs it resolves the tree ref and optional subpath and uses that to determine the analysis
/// directory and display URL. The returned `ResolvedSource` contains a `checkout_guard` that keeps
/// the temporary checkout alive while the value is held.
///
/// # Errors
///
/// Returns an error if creating the temporary checkout, cloning, resolving or checking out refs,
/// or if the computed analysis path does not exist or is not a directory.
///
/// # Parameters
///
/// - `git_ref`: Optional raw git ref to checkout for repository inputs; ignored for tree inputs
///   (tree refs are resolved from the URL).
///
/// # Returns
///
/// A `ResolvedSource` containing:
/// - `repo_root`: path to the cloned repository inside the temporary checkout,
/// - `analysis_path`: path within the checkout to analyze,
/// - optional `display_path` and `display_repo_root` for presentation,
/// - `checkout_guard: Some(TempDir)` which preserves the temporary checkout for the caller.
///
/// # Examples
///
/// ```no_run
/// # use crate::core::runtime::source::{ParsedRemoteSource, ResolvedSource};
/// # // assume `parse_remote_source` exists and returns a `ParsedRemoteSource::Repo`
/// let parsed = ParsedRemoteSource::Repo {
///     clone_url: "https://example.com/org/repo".to_string(),
///     display_repo_root: "https://example.com/org/repo".to_string(),
/// };
/// let resolved: ResolvedSource = resolve_remote_source(parsed, None).expect("clone and resolve");
/// assert!(resolved.analysis_path.exists());
/// ```
fn resolve_remote_source(
    parsed_source: ParsedRemoteSource,
    git_ref: Option<&str>,
) -> Result<ResolvedSource> {
    let checkout_root =
        TempDir::new().context("failed to create temp checkout")?;
    let repo_root = checkout_root.path().join("repo");

    let clone_url = match &parsed_source {
        ParsedRemoteSource::Repo { clone_url, .. }
        | ParsedRemoteSource::Tree { clone_url, .. } => clone_url,
    };

    run_git(
        None,
        [
            OsString::from("clone"),
            OsString::from(clone_url),
            repo_root.as_os_str().to_os_string(),
        ],
        &format!("clone `{clone_url}`"),
    )?;

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

/// Resolve a git ref and an optional repository subdirectory from the tail segments of a tree-style URL.
///
/// Attempts to interpret a prefix of `tail_segments` as a git ref. If a prefix successfully resolves
/// to a commit-ish and can be checked out, returns the checked-out ref and any remaining segments
/// joined as a sub-path under the repository. If the prefix consumes all segments, the returned
/// sub-path is `None`.
///
/// # Parameters
///
/// - `repo_root`: Path to the cloned repository where refs will be resolved and checked out.
/// - `tail_segments`: The path segments after the repository portion of a tree URL; some prefix
///   of these segments will be interpreted as a git ref and the remainder (if any) as a subdirectory.
///
/// # Returns
///
/// A tuple `(resolved_ref, sub_path)` where `resolved_ref` is the ref string that was resolved and
/// checked out, and `sub_path` is `Some(path)` when remaining tail segments map to a subdirectory,
/// or `None` when there is no subdirectory.
///
/// # Errors
///
/// Returns an error if `tail_segments` is empty or if no prefix of `tail_segments` can be resolved
/// to a valid git ref and checked out.
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # // The example assumes a repository exists at `/tmp/repo` and appropriate git refs;
/// let repo = Path::new("/tmp/repo");
/// let tail = vec!["main".to_string(), "src".to_string()];
/// // On success this might return ("main".to_string(), Some("src".to_string()))
/// let result = resolve_tree_checkout(repo, &tail);
/// ```
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

/// Resolve a git commit-ish for a repository and check it out in detached HEAD mode.
///
/// Attempts to resolve `raw_ref` to a commit. If unresolved locally, also tries `origin/<raw_ref>`.
/// After successfully resolving a commit, performs `git checkout --detach <resolved_commit>`.
///
/// # Errors
///
/// Returns an error if `raw_ref` is empty, if the ref cannot be resolved to a commit, or if the
/// checkout operation fails.
///
/// # Returns
///
/// The trimmed `raw_ref` string that was requested.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// // Resolve and checkout "main" in the repository at "/tmp/repo"
/// let out = resolve_checkout_ref(Path::new("/tmp/repo"), " main ").unwrap();
/// assert_eq!(out, "main");
/// ```
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

/// Resolves a git commit-ish (branch, tag, ref, or commit) to a commit hash.
///
/// The function verifies that `candidate` refers to a commit and returns the resolved
/// commit identifier as a `String`. Returns an error if the candidate cannot be
/// resolved to a commit in the repository at `repo_root`.
///
/// # Parameters
///
/// - `repo_root`: filesystem path to the git repository root to query.
/// - `candidate`: commit-ish to resolve (e.g., branch name, tag, ref, or commit).
///
/// # Returns
///
/// `String` containing the resolved commit hash.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let sha = resolve_commitish(Path::new("."), "HEAD").unwrap();
/// assert!(!sha.is_empty());
/// ```
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

/// Parses a user-supplied remote repository or tree URL into a `ParsedRemoteSource`.
///
/// Accepts SCP-style git URLs (e.g. `git@host:owner/repo.git`), `http`/`https` URLs
/// (including GitHub/GitLab tree URL forms), and `ssh`/`file` URLs. Errors if the
/// input is empty, cannot be parsed, or has an unsupported scheme.
///
/// # Returns
///
/// A `ParsedRemoteSource` that describes how the URL should be cloned and whether
/// it encodes a repository root or a tree (ref + optional subpath).
///
/// # Examples
///
/// ```
/// let repo = parse_remote_source("git@github.com:owner/repo.git").unwrap();
/// match repo {
///     ParsedRemoteSource::Repo { clone_url, display_repo_root } => {
///         assert!(clone_url.contains("git@github.com:owner/repo.git"));
///         assert!(display_repo_root.contains("git@github.com:owner/repo"));
///     }
///     _ => panic!("expected Repo variant"),
/// }
/// ```
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

/// Parses an HTTP/HTTPS repository URL and returns a `ParsedRemoteSource` describing
/// either a repository clone URL or a tree URL (host+ref+optional subpath) for GitHub or GitLab.
///
/// This function:
/// - Requires at least two path segments (owner and repository) and errors otherwise.
/// - Detects GitLab tree URLs of the form `.../-/tree/<ref>/...` and GitHub tree URLs
///   of the form `.../tree/<ref>/...`, returning `ParsedRemoteSource::Tree` with
///   the repository clone root and the remaining tail segments.
/// - Rejects blob URLs (any path segment equal to `"blob"`).
/// - For plain repository URLs returns `ParsedRemoteSource::Repo` with `clone_url` and
///   a `display_repo_root` (trailing slashes and a trailing `.git` are stripped for display).
///
/// # Examples
///
/// ```
/// use url::Url;
/// use crate::parse_http_remote_source;
/// use crate::ParsedRemoteSource;
///
/// let url = Url::parse("https://github.com/owner/repo").unwrap();
/// let parsed = parse_http_remote_source(&url).unwrap();
/// assert!(matches!(parsed, ParsedRemoteSource::Repo { .. }));
/// ```
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

/// Construct the canonical "<scheme>://<host>[:port]/<repo_segments...>" repository root for an HTTP(S) URL.
///
/// `repo_segments` are the path segments that identify the repository (e.g., `["owner", "repo"]`).
///
/// # Examples
///
/// ```
/// use url::Url;
/// let url = Url::parse("https://example.com:8443/owner/repo/tree/main/path").unwrap();
/// let root = build_http_repo_root(&url, &["owner", "repo"]);
/// assert_eq!(root, "https://example.com:8443/owner/repo");
/// ```
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

/// Format a repository tree URL for display using the hosting style, git ref, and optional subpath.
///
/// The returned string combines the repository root with the appropriate tree path for the hosting
/// service and appends `sub_path` if provided and non-empty.
///
/// # Returns
///
/// A formatted display URL combining the repository root, the tree/ref segment for the given
/// hosting style, and the optional subpath.
///
/// # Examples
///
/// ```
/// let url = render_tree_display_url(
///     TreeHostingStyle::GitHub,
///     "https://github.com/example/repo",
///     "main",
///     Some("src/lib"),
/// );
/// assert_eq!(url, "https://github.com/example/repo/tree/main/src/lib");
///
/// let url2 = render_tree_display_url(
///     TreeHostingStyle::GitLab,
///     "https://gitlab.com/example/repo",
///     "feature/branch",
///     None,
/// );
/// assert_eq!(url2, "https://gitlab.com/example/repo/-/tree/feature/branch");
/// ```
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

/// Detects whether a string is an SCP-style Git URL (for example `git@host:owner/repo`).
///
/// Returns `true` if the input contains `@` and `:`, does not contain `://`, and has non-empty text on both sides of the first `:`.
///
/// # Examples
///
/// ```
/// assert!(is_scp_style_git_url("git@github.com:owner/repo.git"));
/// assert!(!is_scp_style_git_url("https://github.com/owner/repo"));
/// assert!(!is_scp_style_git_url("user@host:"));
/// ```
fn is_scp_style_git_url(raw_url: &str) -> bool {
    raw_url.contains('@')
        && raw_url.contains(':')
        && !raw_url.contains("://")
        && raw_url
            .split_once(':')
            .is_some_and(|(left, right)| !left.is_empty() && !right.is_empty())
}

/// Removes any trailing '/' characters from the given URL string.
///
/// Returns a `String` containing the input with trailing '/' characters removed.
///
/// # Examples
///
/// ```
/// assert_eq!(strip_trailing_slash("https://example.com/"), "https://example.com");
/// assert_eq!(strip_trailing_slash("https://example.com///"), "https://example.com");
/// assert_eq!(strip_trailing_slash("https://example.com/path"), "https://example.com/path");
/// ```
fn strip_trailing_slash(raw_url: &str) -> String {
    raw_url.trim_end_matches('/').to_owned()
}

/// Normalizes a repository URL or path by removing trailing slashes and a trailing `.git` suffix.
///
/// The returned string has any trailing `/` characters removed, and if the result ends with
/// `.git` that suffix is removed as well.
///
/// # Examples
///
/// ```
/// assert_eq!(
///     crate::strip_trailing_git_suffix("https://example.com/org/repo.git/"),
///     "https://example.com/org/repo"
/// );
/// assert_eq!(
///     crate::strip_trailing_git_suffix("git@github.com:org/repo.git"),
///     "git@github.com:org/repo"
/// );
/// assert_eq!(
///     crate::strip_trailing_git_suffix("https://example.com/org/repo/"),
///     "https://example.com/org/repo"
/// );
/// assert_eq!(
///     crate::strip_trailing_git_suffix("plain-repo"),
///     "plain-repo"
/// );
/// ```
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

    /// Runs the `git` command with the provided arguments in `repo_root`, panicking if the command cannot be started or if it exits with a non-zero status.
    ///
    /// # Examples
    ///
    /// ```
    /// let repo = std::path::Path::new(".");
    /// run_git(repo, &["--version"]);
    /// ```
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

    /// Initializes a new git repository at the given path and sets a default committer identity.
    
    ///
    
    /// Configures `user.name` to "Sephera Tests" and `user.email` to "tests@example.com" in the repository.
    
    ///
    
    /// # Examples
    
    ///
    
    /// ```
    
    /// let td = tempfile::tempdir().unwrap();
    
    /// let repo = td.path().join("repo");
    
    /// std::fs::create_dir_all(&repo).unwrap();
    
    /// init_repo(&repo);
    
    /// assert!(repo.join(".git").exists());
    
    /// ```
    fn init_repo(repo_root: &Path) {
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["config", "user.name", "Sephera Tests"]);
        run_git(repo_root, &["config", "user.email", "tests@example.com"]);
    }

    /// Stages all changes (including additions, modifications, and deletions) under `repo_root` and creates a commit with the given `message`.
    ///
    /// # Examples
    ///
    /// ```
    /// // Stage and commit all changes in the repository at `/path/to/repo`.
    /// // commit_all(Path::new("/path/to/repo"), "Save workspace changes");
    /// ```
    fn commit_all(repo_root: &Path, message: &str) {
        run_git(repo_root, &["add", "-A"]);
        run_git(repo_root, &["commit", "-m", message]);
    }

    /// Writes `contents` to a file located at `repo_root` joined with `relative_path`, creating any missing parent directories.
    ///
    /// `relative_path` is interpreted relative to `repo_root`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// // Writes "hello" to "<repo_root>/sub/dir/file.txt"
    /// write_file(Path::new("/tmp/myrepo"), "sub/dir/file.txt", "hello");
    /// ```
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
            "https://github.com/reim/sephera/tree/main/docs/src",
        )
        .unwrap();

        assert_eq!(
            parsed_source,
            ParsedRemoteSource::Tree {
                clone_url: String::from("https://github.com/reim/sephera"),
                display_repo_root: String::from(
                    "https://github.com/reim/sephera"
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
            "https://github.com/reim/sephera/blob/main/README.md",
        )
        .unwrap_err();

        assert!(error.to_string().contains("blob URLs are not supported"));
    }

    #[test]
    fn parses_scp_style_repo_url() {
        let parsed_source =
            parse_remote_source("git@github.com:reim/sephera.git").unwrap();

        assert_eq!(
            parsed_source,
            ParsedRemoteSource::Repo {
                clone_url: String::from("git@github.com:reim/sephera.git"),
                display_repo_root: String::from("git@github.com:reim/sephera"),
            }
        );
    }

    #[test]
    fn resolves_file_repo_url_and_selected_ref() {
        let temp_dir = tempdir().unwrap();
        init_repo(temp_dir.path());
        write_file(temp_dir.path(), "README.md", "# main\n");
        commit_all(temp_dir.path(), "main");
        run_git(temp_dir.path(), &["tag", "v1.0.0"]);

        let source = resolve_source(&SourceRequest {
            path: None,
            url: Some(format!("file://{}", temp_dir.path().display())),
            git_ref: Some(String::from("v1.0.0")),
        })
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

    #[test]
    fn resolves_tree_url_with_branch_names_that_contain_slashes() {
        let temp_dir = tempdir().unwrap();
        init_repo(temp_dir.path());
        write_file(temp_dir.path(), "src/lib.rs", "pub fn main() {}\n");
        commit_all(temp_dir.path(), "initial");
        run_git(temp_dir.path(), &["checkout", "-b", "feature/docs"]);
        write_file(temp_dir.path(), "docs/guide.md", "# guide\n");
        commit_all(temp_dir.path(), "docs");

        let source = resolve_source(&SourceRequest {
            path: None,
            url: Some(
                "https://github.com/reim/sephera/tree/feature/docs/docs"
                    .to_string(),
            ),
            git_ref: None,
        });

        assert!(source.is_err());

        let source = resolve_remote_source(
            ParsedRemoteSource::Tree {
                clone_url: format!("file://{}", temp_dir.path().display()),
                display_repo_root: String::from(
                    "https://github.com/reim/sephera",
                ),
                style: TreeHostingStyle::GitHub,
                tail_segments: vec![
                    String::from("feature"),
                    String::from("docs"),
                    String::from("docs"),
                ],
            },
            None,
        )
        .unwrap();

        assert!(
            source.analysis_path.join("guide.md").is_file(),
            "expected tree URL to resolve to the docs subdirectory"
        );
        assert_eq!(
            source.display_path.as_deref(),
            Some("https://github.com/reim/sephera/tree/feature/docs/docs")
        );
        assert_eq!(
            source.display_repo_root.as_deref(),
            Some("https://github.com/reim/sephera@feature/docs")
        );
    }

    #[test]
    fn rejects_ref_without_url() {
        let error = resolve_source(&SourceRequest {
            path: Some(PathBuf::from(".")),
            url: None,
            git_ref: Some(String::from("main")),
        })
        .unwrap_err();

        assert!(error.to_string().contains("`ref` requires `url`"));
    }
}
