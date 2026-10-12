/**
 * The zip reader, against a real archive's layout.
 *
 * This is the part of the installer most likely to be quietly wrong: a central
 * directory parsed at the wrong offset, a local header's extra-field length
 * skipped wrongly, a stored entry read as a deflated one. Each of those produces
 * a binary that exists, has a plausible size, and fails later somewhere else.
 *
 * The fixture is a real zip built from the release's own directory layout, so it
 * carries the two things that broke the first version: a **top-level directory**
 * and **deflate compression**.
 */

import { readFileSync, mkdirSync, readFileSync as read, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { centralDirectory, extractZip, findEndOfCentralDirectory, readEntry } from '../src/unzip.js';
import { unpack } from '../src/install.js';
import { buildFor } from '../src/platform.js';

const FIXTURES = resolve(dirname(fileURLToPath(import.meta.url)), 'fixtures');
const SAMPLE = readFileSync(resolve(FIXTURES, 'sample.zip'));

let work: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'sephera-unzip-test-'));
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

describe('the central directory', () => {
  it('reads every entry, with the release layout intact', () => {
    const entries = centralDirectory(SAMPLE);
    expect(entries.map((entry) => entry.name)).toEqual([
      'sephera-v0.7.1-x86_64-pc-windows-msvc/sephera.exe',
      'sephera-v0.7.1-x86_64-pc-windows-msvc/LICENSE',
    ]);
  });

  it('records the compression method, so a stored entry is not inflated', () => {
    for (const entry of centralDirectory(SAMPLE)) {
      expect(entry.method).toBe(8); // deflate
    }
  });

  it('finds the end-of-central-directory record near the end', () => {
    const at = findEndOfCentralDirectory(SAMPLE);
    expect(at).toBeGreaterThan(SAMPLE.length - 100);
    expect(SAMPLE.readUInt32LE(at)).toBe(0x06054b50);
  });

  it('rejects a file that is not a zip, rather than returning nothing', () => {
    // A silent empty list would make the installer report "the archive does not
    // contain the binary" for a download that never was one.
    expect(() => centralDirectory(Buffer.from('not a zip at all'))).toThrow(
      /not a zip archive/,
    );
  });
});

describe('reading an entry', () => {
  it('inflates a deflated entry to the bytes that went in', () => {
    const entry = centralDirectory(SAMPLE)[0];
    const bytes = readEntry(SAMPLE, entry);
    expect(bytes.length).toBeGreaterThan(0);
    expect(bytes.subarray(0, 2).toString()).toBe('#!');
  });

  it('refuses an unsupported compression method rather than returning garbage', () => {
    const entry = { ...centralDirectory(SAMPLE)[0], method: 99 };
    expect(() => readEntry(SAMPLE, entry)).toThrow(/unsupported compression method/);
  });
});

describe('extraction', () => {
  it('writes both files, creating the directory the archive names', () => {
    const written = extractZip(SAMPLE, work);
    expect(written).toHaveLength(2);
    const binary = resolve(work, 'sephera-v0.7.1-x86_64-pc-windows-msvc/sephera.exe');
    expect(existsSync(binary)).toBe(true);
    expect(readFileSync(binary).length).toBeGreaterThan(0);
  });

  it('refuses an entry whose path escapes the destination', () => {
    // A zip from a third party is not trusted input, and `../` has been used to
    // write outside the directory the extraction named.
    const hostile = buildZip([{ name: '../escaped.txt', body: 'x' }]);
    expect(() => extractZip(hostile, work)).toThrow(/escapes the destination/);
  });
});

describe('unpack', () => {
  it('finds the binary inside the top-level directory', () => {
    // The bug this exists for: the release archive holds
    // `sephera-v0.7.1-<target>/sephera.exe`, so looking at the extraction root
    // finds nothing -- and reporting success at that point ships a package that
    // fails on the user's first run.
    const build = buildFor('win32', 'x64')!;
    const found = unpack(SAMPLE, build, '0.7.1');
    expect(found.endsWith('sephera.exe')).toBe(true);
    expect(existsSync(found)).toBe(true);
  });

  it('names the archive it cannot find rather than returning nothing', () => {
    const build = buildFor('win32', 'x64')!;
    const empty = buildZip([{ name: 'sephera-v0.7.1-x86_64-pc-windows-msvc/LICENSE', body: 'MIT' }]);
    expect(() => unpack(empty, build, '0.7.1')).toThrow(/does not contain sephera\.exe/);
  });
});

/** A tiny zip writer, for the cases no fixture should be built to express. */
function buildZip(files: { name: string; body: string }[]): Buffer {
  // Stored, not deflated: the hostile-path case is about the name, not the
  // compression, and this keeps the writer to a dozen lines.
  const locals: Buffer[] = [];
  const centrals: Buffer[] = [];
  let offset = 0;

  for (const file of files) {
    const name = Buffer.from(file.name, 'utf8');
    const body = Buffer.from(file.body, 'utf8');
    const local = Buffer.alloc(30 + name.length + body.length);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 8);
    local.writeUInt32LE(0, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(name.length, 26);
    name.copy(local, 30);
    body.copy(local, 30 + name.length);
    locals.push(local);

    const central = Buffer.alloc(46 + name.length);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(name.length, 28);
    name.copy(central, 46);
    centrals.push(central);

    offset += local.length;
  }

  const centralStart = offset;
  const centralSize = centrals.reduce((sum, part) => sum + part.length, 0);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(files.length, 8);
  eocd.writeUInt16LE(files.length, 10);
  eocd.writeUInt32LE(centralSize, 12);
  eocd.writeUInt32LE(centralStart, 16);

  return Buffer.concat([...locals, ...centrals, eocd]);
}
