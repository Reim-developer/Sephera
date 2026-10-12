/**
 * A minimal zip reader, so installing needs nothing from the host.
 *
 * The Windows archive is a zip, and there are three ways to open one: `tar`
 * (bsdtar on a stock Windows, but GNU tar on a machine with Git for Windows --
 * which fails with "This does not look like a tar archive", an error that reads
 * as a corrupt download), PowerShell's `Expand-Archive` (always present, and a
 * process spawn per install), or the format itself.
 *
 * The format is the only option with no host dependency, and Node already has
 * the one primitive it needs: `zlib.inflateRawSync`. A zip stores each entry
 * uncompressed or deflated, and the central directory at the end says which and
 * where.
 *
 * Every offset here is verified once, against the buffer's length, rather than
 * defensively at each read -- which is the whole reason this file is written in
 * TypeScript. The arithmetic is the part that can be wrong, and a reader that
 * silently reads past the end of a buffer produces a corrupt binary that fails
 * later and somewhere else.
 */

import { mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve, sep } from 'node:path';
import { inflateRawSync } from 'node:zlib';

const EOCD_SIGNATURE = 0x06054b50;
const CENTRAL_SIGNATURE = 0x02014b50;
const LOCAL_SIGNATURE = 0x04034b50;
const STORED = 0;
const DEFLATED = 8;

/** The central directory's fixed header size: signature through disk numbers. */
const CENTRAL_HEADER = 46;
/** The end-of-central-directory record, excluding its variable-length comment. */
const EOCD_SIZE = 22;
/**
 * How far back to look for that record.
 *
 * The comment may be up to 64 KB, and a release archive is tens of megabytes, so
 * the search is bounded at both ends.
 */
const MAX_COMMENT = 0x10000;

/** One entry, as the central directory describes it. */
export interface ZipEntry {
  readonly name: string;
  readonly method: number;
  readonly compressedSize: number;
  readonly localOffset: number;
}

function require_(buffer: Buffer, at: number, what: string): void {
  if (at < 0 || at >= buffer.length) {
    throw new Error(`corrupt zip: ${what} points past the end of the file`);
  }
}

/**
 * Locate the end-of-central-directory record.
 *
 * Searched backwards rather than read from the front, because the record is at
 * the end and a zip can carry a comment of arbitrary length after it.
 */
export function findEndOfCentralDirectory(buffer: Buffer): number {
  const earliest = Math.max(0, buffer.length - MAX_COMMENT - EOCD_SIZE);
  for (let at = buffer.length - EOCD_SIZE; at >= earliest; at -= 1) {
    if (buffer.readUInt32LE(at) === EOCD_SIGNATURE) {
      return at;
    }
  }
  throw new Error('not a zip archive: no end-of-central-directory record');
}

/** Every entry the central directory lists. */
export function centralDirectory(buffer: Buffer): ZipEntry[] {
  const eocd = findEndOfCentralDirectory(buffer);
  const count = buffer.readUInt16LE(eocd + 10);
  const start = buffer.readUInt32LE(eocd + 16);
  require_(buffer, start, 'the central directory');
  require_(buffer, start + count * CENTRAL_HEADER, 'the last entry');

  const entries: ZipEntry[] = [];
  let at = start;
  for (let index = 0; index < count; index += 1) {
    if (buffer.readUInt32LE(at) !== CENTRAL_SIGNATURE) {
      throw new Error(`corrupt zip: entry ${index} has a bad signature`);
    }
    const nameLength = buffer.readUInt16LE(at + 28);
    const extraLength = buffer.readUInt16LE(at + 30);
    const commentLength = buffer.readUInt16LE(at + 32);
    entries.push({
      name: buffer.toString('utf8', at + CENTRAL_HEADER, at + CENTRAL_HEADER + nameLength),
      method: buffer.readUInt16LE(at + 10),
      compressedSize: buffer.readUInt32LE(at + 20),
      localOffset: buffer.readUInt32LE(at + 42),
    });
    at += CENTRAL_HEADER + nameLength + extraLength + commentLength;
  }
  return entries;
}

/** The bytes of one entry, inflated when the archive deflated it. */
export function readEntry(buffer: Buffer, entry: ZipEntry): Buffer {
  const at = entry.localOffset;
  require_(buffer, at, `the local header of ${entry.name}`);
  if (buffer.readUInt32LE(at) !== LOCAL_SIGNATURE) {
    throw new Error(`corrupt zip: ${entry.name} has a bad local header`);
  }
  // The local header repeats the name and carries its own extra field, whose
  // length can differ from the central one. Both must be skipped to reach data --
  // and this is the off-by-one that a careful reading of the spec turns into a
  // garbage binary.
  const nameLength = buffer.readUInt16LE(at + 26);
  const extraLength = buffer.readUInt16LE(at + 28);
  const dataAt = at + 30 + nameLength + extraLength;
  require_(buffer, dataAt + entry.compressedSize, `the data of ${entry.name}`);
  const raw = buffer.subarray(dataAt, dataAt + entry.compressedSize);

  if (entry.method === DEFLATED) return inflateRawSync(raw);
  if (entry.method === STORED) return Buffer.from(raw);
  throw new Error(`unsupported compression method ${entry.method} for ${entry.name}`);
}

/** Extract a zip into `dest`, returning every file written. */
export function extractZip(buffer: Buffer, dest: string): string[] {
  const root = resolve(dest);
  const written: string[] = [];
  for (const entry of centralDirectory(buffer)) {
    // Directory entries are names ending in a slash with no content.
    if (entry.name.endsWith('/')) continue;
    // An archive from a third party is not trusted input, and `../` in a zip has
    // been used to write outside the directory it named.
    const target = resolve(root, entry.name);
    if (target !== root && !target.startsWith(root + sep)) {
      throw new Error(`refusing to extract ${entry.name}: it escapes the destination`);
    }
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, readEntry(buffer, entry));
    written.push(target);
  }
  return written;
}

/** Exported for the tests: the signature each header must carry. */
export const SIGNATURES = {
  eocd: EOCD_SIGNATURE,
  central: CENTRAL_SIGNATURE,
  local: LOCAL_SIGNATURE,
} as const;
