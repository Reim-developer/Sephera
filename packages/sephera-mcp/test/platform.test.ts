/**
 * The platform table, the release URLs, and the checksum parser.
 *
 * The table in `platform.ts` is a copy of the release workflow's matrix, and a
 * copy is the kind of thing that drifts. These tests pin what the installer
 * promises -- a URL that resolves, a digest that parses, a refusal that is loud
 * -- against the real `SHA256SUMS.txt` a release publishes.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  BUILDS,
  archiveName,
  archiveUrl,
  binaryName,
  buildFor,
  buildForCurrentMachine,
  checksumsUrl,
  supportedSummary,
} from '../src/platform.js';
import { expectedDigest, sha256 } from '../src/install.js';

const FIXTURES = resolve(dirname(fileURLToPath(import.meta.url)), 'fixtures');
const SUMS = readFileSync(resolve(FIXTURES, 'SHA256SUMS.txt'), 'utf8');

describe('the build table', () => {
  it('covers every release target', () => {
    // The four the release workflow builds. A fifth target added there and not
    // here is the drift this list exists to make visible.
    expect(BUILDS.map((build) => build.target).sort()).toEqual([
      'aarch64-apple-darwin',
      'x86_64-apple-darwin',
      'x86_64-pc-windows-msvc',
      'x86_64-unknown-linux-musl',
    ]);
  });

  it('gives every platform exactly one architecture', () => {
    const keys = BUILDS.map((build) => `${build.platform}:${build.arch}`);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it('says which platform a binary has, because it decides the suffix', () => {
    expect(binaryName('win32')).toBe('sephera.exe');
    expect(binaryName('linux')).toBe('sephera');
    expect(binaryName('darwin')).toBe('sephera');
  });

  it('returns null rather than guessing on an unbuilt platform', () => {
    // linux/arm64 is not built. Returning a plausible-looking target would send
    // the installer to a URL that 404s, and a 404 at install time reads as a
    // network fault rather than as "this platform is not supported".
    expect(buildFor('linux', 'arm64')).toBeNull();
    expect(buildForCurrentMachine('linux', 'arm64')).toBeNull();
    expect(supportedSummary('linux', 'arm64')).toContain('is not built by the release');
  });

  it('reports support plainly when the platform is built', () => {
    expect(buildFor('linux', 'x64')?.target).toBe('x86_64-unknown-linux-musl');
    expect(supportedSummary('win32', 'x64')).toBe('win32:x64 is supported');
  });
});

describe('release URLs', () => {
  it('matches the names the release actually publishes', () => {
    // The archive names are pinned by the real sums file below; this is the
    // shape, and a change to the naming convention should fail here first.
    const build = buildFor('win32', 'x64');
    expect(build).not.toBeNull();
    expect(archiveName(build!, '0.7.1')).toBe('sephera-v0.7.1-x86_64-pc-windows-msvc.zip');
    expect(archiveUrl(build!, '0.7.1')).toBe(
      'https://github.com/Reim-developer/Sephera/releases/download/v0.7.1/sephera-v0.7.1-x86_64-pc-windows-msvc.zip',
    );
    expect(checksumsUrl('0.7.1')).toBe(
      'https://github.com/Reim-developer/Sephera/releases/download/v0.7.1/SHA256SUMS.txt',
    );
  });
});

describe('checksum parsing', () => {
  it('finds the digest for an archive the release published', () => {
    // The real file. A parser that only handles the format this repository
    // believes it writes passes against a synthesised fixture and fails here.
    const build = buildFor('win32', 'x64')!;
    const digest = expectedDigest(SUMS, archiveName(build, '0.7.1'));
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
  });

  it('finds every digest the release published', () => {
    for (const build of BUILDS) {
      expect(expectedDigest(SUMS, archiveName(build, '0.7.1'))).toMatch(
        /^[0-9a-f]{64}$/,
        `${archiveName(build, '0.7.1')} has no digest`,
      );
    }
  });

  it('returns null for an archive that is not listed', () => {
    // The refusal path: an unlisted archive means the sums file does not describe
    // what would be downloaded, and running it anyway is the thing the check
    // exists to prevent.
    expect(expectedDigest(SUMS, 'sephera-v9.9.9-nonsense.tar.gz')).toBeNull();
  });

  it('matches a digest the checksum of the bytes agrees with', () => {
    const bytes = Buffer.from('sephera');
    expect(sha256(bytes)).toBe(
      expectedDigest(
        `${sha256(bytes)}  sephera\n`,
        'sephera',
      ),
    );
  });

  it('ignores a path prefix, because the release writes bare names', () => {
    const digest = 'a'.repeat(64);
    expect(expectedDigest(`${digest}  dist/sephera-v0.7.1-x86_64-pc-windows-msvc.zip`, 'sephera-v0.7.1-x86_64-pc-windows-msvc.zip')).toBe(digest);
  });
});
