//! Resolve which Sephera build this machine needs, and where it goes.
//!
//! One place decides three things a launcher has to agree on: the Rust target
//! triple, the archive the release publishes for it, and the path the extracted
//! binary lands at. They are derived from the same tuple rather than computed
//! three times, because a mismatch between the triple used to download and the
//! one used to look for the result is the kind of bug that only shows up on the
//! one platform nobody tested.

'use strict';

const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');

/** The release's own naming, from `.github/workflows/release.yml`. */
const RELEASE = 'https://github.com/Reim-developer/Sephera/releases/download';

/**
 * The builds the release workflow produces, keyed by `process.platform` and
 * `process.arch`. Written out rather than computed: a computed mapping is a
 * function with four correct branches and one arm nobody has a machine for, and
 * a table fails loudly at the first lookup instead.
 */
const BUILDS = {
  'win32:x64': { target: 'x86_64-pc-windows-msvc', ext: 'zip' },
  'darwin:x64': { target: 'x86_64-apple-darwin', ext: 'tar.gz' },
  'darwin:arm64': { target: 'aarch64-apple-darwin', ext: 'tar.gz' },
  'linux:x64': { target: 'x86_64-unknown-linux-musl', ext: 'tar.gz' },
};

/**
 * The build for this machine, or null when the release does not make one.
 *
 * Returning null rather than throwing is deliberate: `postinstall` uses it to
 * *warn*, because a failure there fails the install of a package the user may
 * only be trying. The launcher uses it to *refuse*, because at that point
 * nothing works anyway and a clear message beats a download of something that
 * will not run.
 */
function buildForCurrentMachine() {
  const key = `${process.platform}:${process.arch}`;
  const build = BUILDS[key];
  if (!build) {
    return null;
  }
  return { ...build, key, binary: binaryName(build.target) };
}

function binaryName(target) {
  return process.platform === 'win32' ? 'sephera.exe' : 'sephera';
}

/** The archive for a build at a tag: `sephera-v0.7.1-x86_64-pc-windows-msvc.zip`. */
function archiveName(build, version) {
  return `sephera-v${version}-${build.target}.${build.ext}`;
}

function archiveUrl(build, version) {
  return `${RELEASE}/v${version}/${archiveName(build, version)}`;
}

function checksumsUrl(version) {
  return `${RELEASE}/v${version}/SHA256SUMS.txt`;
}

/**
 * Where the extracted binary lives inside the package.
 *
 * Under `node_modules/.bin/` rather than somewhere else of our own, so a user
 * who wants to run it by hand -- or point an MCP client at a specific path --
 * finds it where they would expect.
 */
function binaryPath(root) {
  const build = buildForCurrentMachine();
  return path.join(root, 'bin', binaryName(build.target));
}

/** Whether the binary has already been extracted. */
function isInstalled(root) {
  try {
    return fs.statSync(binaryPath(root)).size > 0;
  } catch {
    return false;
  }
}

/** A one-line statement of what is supported and what is not. */
function supportedSummary() {
  const supported = Object.keys(BUILDS).join(', ');
  const current = `${process.platform}:${process.arch}`;
  return BUILDS[current]
    ? `${current} is supported`
    : `${current} is not built by the release; supported: ${supported}`;
}

module.exports = {
  RELEASE,
  BUILDS,
  buildForCurrentMachine,
  binaryName,
  archiveName,
  archiveUrl,
  checksumsUrl,
  binaryPath,
  isInstalled,
  supportedSummary,
  os,
};
