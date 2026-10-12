/**
 * Fetch the release archive, check it, and unpack it.
 *
 * This downloads a binary and then runs it, so the checksum is not a nicety. The
 * release publishes `SHA256SUMS.txt` beside the archives and a mismatch is a
 * refusal rather than a warning: an unverified binary that turns out to be wrong
 * is the one failure mode of an installer nobody survives.
 *
 * `postinstall` rather than a first-run download, because an MCP client starts
 * the server with a timeout and a download inside that window reads as a broken
 * server. The launcher still downloads on demand, for the case where the
 * lifecycle script was skipped -- npm lets `--ignore-scripts` do that, and some
 * CI images set it by default.
 */

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdtempSync, readdirSync, statSync, writeFileSync, chmodSync, mkdirSync } from 'node:fs';
import { basename, join } from 'node:path';
import { tmpdir } from 'node:os';
import { get as httpsGet } from 'node:https';

import { archiveName, archiveUrl, binaryName, binaryPath, buildForCurrentMachine, checksumsUrl, type Build } from './platform.js';
import { extractZip } from './unzip.js';

/** How many redirects to follow. A release asset is usually one hop. */
const MAX_REDIRECTS = 5;

/** Fetch a URL, following redirects. */
export function fetch(url: string, redirectsLeft = MAX_REDIRECTS): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    httpsGet(
      url,
      { headers: { 'user-agent': 'sephera-mcp-installer' } },
      (response) => {
        const status = response.statusCode ?? 0;
        const location = response.headers.location;
        if (status >= 300 && status < 400 && location) {
          if (redirectsLeft === 0) {
            reject(new Error(`too many redirects fetching ${url}`));
            return;
          }
          resolve(fetch(location, redirectsLeft - 1));
          return;
        }
        if (status !== 200) {
          reject(new Error(`${url} returned ${status}`));
          return;
        }
        const chunks: Buffer[] = [];
        response.on('data', (chunk: Buffer) => chunks.push(chunk));
        response.on('end', () => resolve(Buffer.concat(chunks)));
      },
    ).on('error', reject);
  });
}

/** The hex SHA-256 of some bytes. */
export function sha256(buffer: Buffer): string {
  return createHash('sha256').update(buffer).digest('hex');
}

/**
 * The expected digest for `name`, read out of a `SHA256SUMS.txt` body.
 *
 * `null` when the file has no line for it, rather than throwing: the sums file is
 * published per release, and a missing entry is the same situation as a missing
 * file -- a refusal either way, reported once.
 *
 * The format is `<digest>  <name>`, and the name is compared by its basename
 * because the release writes bare filenames while `sha256sum -c` output can carry
 * a path.
 */
export function expectedDigest(sumsText: string, name: string): string | null {
  for (const line of sumsText.split(/\r?\n/)) {
    const [digest, file] = line.trim().split(/\s+/);
    if (file && basename(file) === name) {
      return digest;
    }
  }
  return null;
}

/** Every file under `dir`, as absolute paths. */
export function walk(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) found.push(...walk(full));
    else found.push(full);
  }
  return found;
}

/**
 * Unpack an archive and return the path to the binary inside it.
 *
 * The staging directory is both the working directory and the extraction target,
 * so no absolute path reaches `tar` -- on Windows it reads `C:\...` as a remote
 * host spec and fails with "Cannot connect to C: resolve failed", an error that
 * names a network problem when the archive is on the disk.
 *
 * The binary is *searched for* rather than assumed to be at the root: the release
 * archive has a top-level directory named after itself, so `sephera.exe` is at
 * `sephera-v0.7.1-x86_64-pc-windows-msvc/sephera.exe`. That directory is a
 * release-workflow detail which can change without breaking what a user cares
 * about, which is that the binary is in there somewhere.
 */
export function unpack(archive: Buffer, build: Build, version: string): string {
  const staging = mkdtempSync(join(tmpdir(), 'sephera-mcp-'));
  const file = archiveName(build, version);
  writeFileSync(join(staging, file), archive);

  if (build.ext === 'zip') {
    // Not `tar`: bsdtar on a stock Windows opens a zip, GNU tar on a machine with
    // Git installed does not, and says "This does not look like a tar archive".
    extractZip(archive, staging);
  } else {
    execFileSync('tar', ['-xzf', file], { cwd: staging, stdio: 'inherit' });
  }

  const wanted = binaryName(build.platform);
  const found = walk(staging).find((full) => basename(full) === wanted);
  if (!found) {
    throw new Error(`${file} does not contain ${wanted}`);
  }
  return found;
}

/**
 * Download, verify, and install. Idempotent: does nothing when the binary is
 * already there, so a reinstall does not re-fetch it.
 *
 * @returns the binary's path, or `null` when this machine has no published build.
 */
export async function install(root: string, version: string): Promise<string | null> {
  const target = binaryPath(root);
  if (existsSync(target) && statSync(target).size > 0) {
    return target;
  }

  const build = buildForCurrentMachine();
  if (!build) {
    return null;
  }

  const [archive, sums] = await Promise.all([
    fetch(archiveUrl(build, version)),
    fetch(checksumsUrl(version)).catch(() => Buffer.from('')),
  ]);

  const expected = expectedDigest(sums.toString('utf8'), archiveName(build, version));
  if (!expected) {
    throw new Error(
      `SHA256SUMS.txt has no entry for ${archiveName(build, version)}; ` +
        'refusing to run an unverified binary',
    );
  }
  const actual = sha256(archive);
  if (actual !== expected) {
    throw new Error(
      `checksum mismatch for ${archiveName(build, version)}:\n` +
        `  expected ${expected}\n  actual   ${actual}`,
    );
  }

  const extracted = unpack(archive, build, version);
  mkdirSync(join(root, 'bin'), { recursive: true });
  copyFileSync(extracted, target);
  if (process.platform !== 'win32') {
    chmodSync(target, 0o755);
  }
  return target;
}
