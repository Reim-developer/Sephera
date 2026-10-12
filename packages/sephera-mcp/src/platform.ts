/**
 * Which Sephera build this machine needs, and where it goes.
 *
 * One place decides three things a launcher has to agree on: the Rust target
 * triple, the archive the release publishes for it, and the path the extracted
 * binary lands at. They are derived from the same tuple rather than computed
 * three times, because a mismatch between the triple used to download and the
 * one used to look for the result is a bug that only shows up on the one
 * platform nobody tested.
 *
 * **The table below is a copy of the release workflow's matrix.** Add a target to
 * `.github/workflows/release.yml` and this table knows nothing about it: the
 * installer reports "no published build" on a platform that has one, which is
 * worse than the wrong binary because it is a refusal nobody can diagnose.
 * `scripts/check_mcp_package.py` compares the two and fails when they drift --
 * which is the only reason this duplication is safe.
 */

/** The release's own naming, from `.github/workflows/release.yml`. */
export const RELEASE_BASE =
  'https://github.com/Reim-developer/Sephera/releases/download';

/** A platform the release publishes a build for. */
export interface Build {
  /** `process.platform` -- `win32`, `darwin`, `linux`. */
  readonly platform: NodeJS.Platform;
  /** `process.arch` -- `x64`, `arm64`. */
  readonly arch: string;
  /** The Rust target triple the release builds for it. */
  readonly target: string;
  /** The archive extension that release publishes. */
  readonly ext: 'zip' | 'tar.gz';
}

/**
 * The builds the release workflow produces.
 *
 * Written out rather than computed: a computed mapping is a function with four
 * correct branches and one arm nobody has a machine for, and a table fails loudly
 * at the first lookup instead.
 */
export const BUILDS: readonly Build[] = [
  { platform: 'win32', arch: 'x64', target: 'x86_64-pc-windows-msvc', ext: 'zip' },
  { platform: 'darwin', arch: 'x64', target: 'x86_64-apple-darwin', ext: 'tar.gz' },
  { platform: 'darwin', arch: 'arm64', target: 'aarch64-apple-darwin', ext: 'tar.gz' },
  { platform: 'linux', arch: 'x64', target: 'x86_64-unknown-linux-musl', ext: 'tar.gz' },
];

/** The binary's name inside the archive. */
export function binaryName(platform: NodeJS.Platform = process.platform): string {
  return platform === 'win32' ? 'sephera.exe' : 'sephera';
}

/**
 * The build for this machine, or `null` when the release does not make one.
 *
 * `null` rather than a throw, because the two callers need opposite things:
 * `postinstall` warns and lets the install finish, and the launcher refuses with
 * the reason. An exception cannot be both.
 */
export function buildForCurrentMachine(
  platform: NodeJS.Platform = process.platform,
  arch: string = process.arch,
): Build | null {
  return BUILDS.find((build) => build.platform === platform && build.arch === arch) ?? null;
}

/** The build for a given tuple, whether or not it is this machine's. */
export function buildFor(
  platform: NodeJS.Platform,
  arch: string,
): Build | null {
  return buildForCurrentMachine(platform, arch);
}

/** The archive a release publishes for a build: `sephera-v0.7.1-x86_64-...zip`. */
export function archiveName(build: Build, version: string): string {
  return `sephera-v${version}-${build.target}.${build.ext}`;
}

/** Where to fetch that archive. */
export function archiveUrl(build: Build, version: string): string {
  return `${RELEASE_BASE}/v${version}/${archiveName(build, version)}`;
}

/** Where the release publishes its checksums. */
export function checksumsUrl(version: string): string {
  return `${RELEASE_BASE}/v${version}/SHA256SUMS.txt`;
}

/**
 * Where the extracted binary lives inside the package.
 *
 * Under the package's own `bin/` rather than `node_modules/.bin/`, so a user who
 * wants to point an MCP client at a specific path finds it where they would look.
 */
export function binaryPath(root: string, build?: Build | null): string {
  const target = build ?? buildForCurrentMachine();
  return pathJoin(root, 'bin', binaryName(target?.platform ?? process.platform));
}

/** Whether the binary is already present. */
export function isInstalled(root: string): boolean {
  try {
    return statSync(binaryPath(root)).size > 0;
  } catch {
    return false;
  }
}

/** One line saying what is supported and what is not. */
export function supportedSummary(
  platform: NodeJS.Platform = process.platform,
  arch: string = process.arch,
): string {
  const supported = BUILDS.map((b) => `${b.platform}:${b.arch}`).join(', ');
  const current = `${platform}:${arch}`;
  return buildFor(platform, arch)
    ? `${current} is supported`
    : `${current} is not built by the release; supported: ${supported}`;
}

// `node:path` imported last so the module reads top-down: the domain, then the
// library it happens to need.
import { statSync } from 'node:fs';
import { join as pathJoin } from 'node:path';
