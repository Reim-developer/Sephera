//! Fetch the release archive, check it, and unpack it.
//!
//! This downloads a binary and then runs it, so the checksum is not a nicety. The
//! release publishes `SHA256SUMS.txt` beside the archives, and a mismatch is a
//! refusal rather than a warning -- an unverified binary that turns out to be
//! wrong is the one failure mode of an installer that nobody survives.
//!
//! `postinstall` rather than a first-run download, because an MCP client starts
//! the server with a timeout and a download inside that window reads as a broken
//! server. The launcher still downloads on demand, for the case where the
//! lifecycle script was skipped -- npm lets `--ignore-scripts` do that, and some
//! CI images set it by default.

'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const https = require('node:https');
const { execFileSync } = require('node:child_process');
const crypto = require('node:crypto');

const {
  archiveUrl,
  archiveName,
  checksumsUrl,
  binaryPath,
  buildForCurrentMachine,
} = require('./platform');
const { extractZip } = require('./unzip');

/** Follow a redirect, up to a depth. A release asset is one hop, usually. */
function fetch(url, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { 'user-agent': 'sephera-mcp-installer' } }, (response) => {
        const status = response.statusCode;
        if (status >= 300 && status < 400 && response.headers.location) {
          if (redirectsLeft === 0) {
            reject(new Error(`too many redirects fetching ${url}`));
            return;
          }
          resolve(fetch(response.headers.location, redirectsLeft - 1));
          return;
        }
        if (status !== 200) {
          reject(new Error(`${url} returned ${status}`));
          return;
        }
        const chunks = [];
        response.on('data', (chunk) => chunks.push(chunk));
        response.on('end', () => resolve(Buffer.concat(chunks)));
      })
      .on('error', reject);
  });
}

function sha256(buffer) {
  return crypto.createHash('sha256').update(buffer).digest('hex');
}

/**
 * The expected digest for `name`, read out of `SHA256SUMS.txt`.
 *
 * Returns null when the file has no line for it rather than throwing: the sums
 * file is published per release and a missing entry is the same situation as a
 * missing file, which is a refusal either way and is reported once.
 */
function expectedDigest(sumsText, name) {
  for (const line of sumsText.split(/\r?\n/)) {
    const [digest, file] = line.trim().split(/\s+/);
    if (file && path.basename(file) === name) {
      return digest;
    }
  }
  return null;
}

/** Every file under `dir`, as absolute paths. */
function walk(dir) {
  const found = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) found.push(...walk(full));
    else found.push(full);
  }
  return found;
}

/**
 * Unpack `archive` and return the path to the binary inside it.
 *
 * The staging directory doubles as the working directory and the extraction
 * target, so no absolute path reaches `tar` -- see the note above on why.
 */
function unpack(archive, build) {
  const staging = fs.mkdtempSync(path.join(os.tmpdir(), 'sephera-mcp-'));
  const file = path.basename(archiveName(build, require('../package.json').version));
  fs.writeFileSync(path.join(staging, file), archive);

  if (build.ext === 'zip') {
    // Not `tar`. bsdtar on a stock Windows opens a zip; GNU tar on a machine with
    // Git installed does not, and says "This does not look like a tar archive",
    // which reads as a corrupt download rather than as the wrong tool.
    extractZip(archive, staging);
  } else {
    execFileSync('tar', ['-xzf', file], { cwd: staging, stdio: 'inherit' });
  }

  const found = walk(staging).find((full) => path.basename(full) === build.binary);
  if (!found) {
    throw new Error(`${file} does not contain ${build.binary}`);
  }
  return found;
}

/**
 * Download, verify, and install. Idempotent: does nothing when the binary is
 * already there, so a reinstall does not re-fetch a hundred megabytes.
 *
 * @returns {Promise<string|null>} the binary's path, or null when this machine
 *   has no published build.
 */
async function install(root) {
  const target = binaryPath(root);
  if (fs.existsSync(target) && fs.statSync(target).size > 0) {
    return target;
  }

  const build = buildForCurrentMachine();
  if (!build) {
    return null;
  }

  const version = require('../package.json').version;
  const [archive, sums] = await Promise.all([
    fetch(archiveUrl(build, version)),
    fetch(checksumsUrl(version)).catch(() => Buffer.from('')),
  ]);

  const expected = expectedDigest(sums.toString('utf8'), archiveName(build, version));
  if (!expected) {
    throw new Error(
      `SHA256SUMS.txt has no entry for ${archiveName(build, version)}; ` +
        'refusing to run an unverified binary'
    );
  }
  const actual = sha256(archive);
  if (actual !== expected) {
    throw new Error(
      `checksum mismatch for ${archiveName(build, version)}:\n` +
        `  expected ${expected}\n  actual   ${actual}`
    );
  }

  const extracted = unpack(archive, build);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(extracted, target);
  if (process.platform !== 'win32') {
    fs.chmodSync(target, 0o755);
  }
  return target;
}

module.exports = { install, fetch, sha256, expectedDigest };
