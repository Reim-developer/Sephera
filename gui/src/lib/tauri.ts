/**
 * The host commands the file viewer uses.
 *
 * Layer 0, like `lib/ipc.ts`: the only module that speaks to the host, and the
 * only one a service imports. It is a separate file because the viewer is not
 * a *view* in the sense the analysis commands are -- it is a way of reading a
 * file, and the rest of the client never calls it.
 */

import { invoke } from "@tauri-apps/api/core";

/** One chunk of a file, and how much of it there is. */
export interface FileChunk {
  /** The decoded text of this chunk. */
  text: string;
  /** How many bytes have been read so far, including this chunk. */
  bytes: number;
  /** The file's size in bytes. */
  total: number;
}

/**
 * Read `length` bytes from `offset` in one file.
 *
 * The host returns bytes, and they are decoded here, because the boundary is
 * where a UTF-8 character may be split -- and a split that both sides would
 * have to agree on is a split that is better made once, on this side, than
 * agreed on twice.
 */
export async function readFileChunk(
  root: string,
  path: string,
  offset: number,
  length: number,
): Promise<FileChunk> {
  return invoke<FileChunk>("read_file_chunk", { root, path, offset, length });
}
