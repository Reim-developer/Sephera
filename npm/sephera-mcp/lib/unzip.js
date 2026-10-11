//! A minimal zip reader, so installing needs nothing from the host.
//!
//! The Windows archive is a zip, and there are three ways to open one: `tar`
//! (bsdtar on a stock Windows, but GNU tar on a machine with Git for Windows --
//! which fails with "This does not look like a tar archive", an error that reads
//! as a corrupt download), PowerShell's `Expand-Archive` (always present, and a
//! process spawn per install), or the format itself.
//!
//! The format is the only option with no host dependency, and Node already has
//! the one primitive it needs: `zlib.inflateRawSync`. Zip stores each entry
//! uncompressed or deflated, and the central directory at the end of the file
//! says which and where.

'use strict';

const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');

const EOCD_SIGNATURE = 0x06054b50;
const CENTRAL_SIGNATURE = 0x02014b50;
const LOCAL_SIGNATURE = 0x04034b50;
const DEFLATED = 8;

/**
 * Locate the end-of-central-directory record.
 *
 * Searched backwards over a bounded window rather than the whole file: the record
 * is at the end, but up to 64 KB of comment may follow it, and a release archive
 * is tens of megabytes.
 */
function findEndOfCentralDirectory(buffer) {
  const earliest = Math.max(0, buffer.length - 0x10000 - 22);
  for (let at = buffer.length - 22; at >= earliest; at -= 1) {
    if (buffer.readUInt32LE(at) === EOCD_SIGNATURE) {
      return at;
    }
  }
  throw new Error('not a zip archive: no end-of-central-directory record');
}

/** Every entry in the archive, as `{ name, method, compressedSize, localOffset }`. */
function centralDirectory(buffer) {
  const eocd = findEndOfCentralDirectory(buffer);
  const count = buffer.readUInt16LE(eocd + 10);
  let at = buffer.readUInt32LE(eocd + 16);
  const entries = [];

  for (let index = 0; index < count; index += 1) {
    if (buffer.readUInt32LE(at) !== CENTRAL_SIGNATURE) {
      throw new Error(`corrupt zip: entry ${index} has a bad signature`);
    }
    const nameLength = buffer.readUInt16LE(at + 28);
    const extraLength = buffer.readUInt16LE(at + 30);
    const commentLength = buffer.readUInt16LE(at + 32);
    entries.push({
      name: buffer.toString('utf8', at + 46, at + 46 + nameLength),
      method: buffer.readUInt16LE(at + 10),
      compressedSize: buffer.readUInt32LE(at + 20),
      localOffset: buffer.readUInt32LE(at + 42),
    });
    at += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}

/** The bytes of one entry, inflated when the archive deflated it. */
function readEntry(buffer, entry) {
  const at = entry.localOffset;
  if (buffer.readUInt32LE(at) !== LOCAL_SIGNATURE) {
    throw new Error(`corrupt zip: ${entry.name} has a bad local header`);
  }
  // The local header repeats the name and may carry its own extra field, whose
  // length can differ from the central one. Both must be skipped to reach data.
  const nameLength = buffer.readUInt16LE(at + 26);
  const extraLength = buffer.readUInt16LE(at + 28);
  const start = at + 30 + nameLength + extraLength;
  const raw = buffer.subarray(start, start + entry.compressedSize);

  if (entry.method === DEFLATED) return zlib.inflateRawSync(raw);
  if (entry.method === 0) return Buffer.from(raw);
  throw new Error(`unsupported compression method ${entry.method} for ${entry.name}`);
}

/** Extract a zip buffer into `dest`, returning every file written. */
function extractZip(buffer, dest) {
  const written = [];
  for (const entry of centralDirectory(buffer)) {
    // Skip directories, which are entries with a trailing slash and no content.
    if (entry.name.endsWith('/')) continue;
    // Refuse an entry that escapes the destination. An archive from a third party
    // is not trusted input, and `../` in a zip has been used to write outside the
    // directory it named.
    const target = path.resolve(dest, entry.name);
    if (!target.startsWith(path.resolve(dest) + path.sep)) {
      throw new Error(`refusing to extract ${entry.name}: it escapes the destination`);
    }
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, readEntry(buffer, entry));
    written.push(target);
  }
  return written;
}

module.exports = { extractZip, centralDirectory, readEntry };
