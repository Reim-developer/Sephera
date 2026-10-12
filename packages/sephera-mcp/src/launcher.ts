/**
 * The pipe between the client and the Rust server.
 *
 * One writer, one reader, and the launcher owns both.
 *
 * The Rust server's transport ends the session on a line it cannot parse: rmcp's
 * stdio reads a frame, fails to parse it, and reports end-of-input. The process
 * exits 0 -- nothing crashed, the transport simply decided the conversation was
 * over. That is unreachable from here, because the client never talks to it
 * directly: a line has to pass through this file first.
 *
 * The launcher is the only writer to the client's stdout. The server's stdout is
 * read and re-emitted rather than inherited, so a parse error and the response
 * that follows it cannot interleave mid-frame -- two writers to one file
 * descriptor would eventually produce exactly that.
 */

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

import { binaryPath, buildForCurrentMachine, supportedSummary } from './platform.js';
import { install } from './install.js';

/** The package root, from `dist/launcher.js`. */
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** The protocol's own code for a frame that could not be parsed. */
const PARSE_ERROR = -327;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function version(): Promise<string> {
  const manifest = JSON.parse(
    await readFile(resolve(ROOT, 'package.json'), 'utf8'),
  ) as { version: string };
  return manifest.version;
}

/**
 * Answer a line that is not JSON, the way the protocol does.
 *
 * `id: null`, because a frame that could not be parsed carries no id to answer
 * to -- which is precisely the case the spec reserves this shape for.
 */
function parseErrorFrame(): string {
  return `${JSON.stringify({
    jsonrpc: '2.0',
    id: null,
    error: {
      code: PARSE_ERROR,
      message: 'Parse error: the line was not valid JSON',
    },
  })}\n`;
}

/**
 * Forward the server's stdout to the client, one line at a time.
 *
 * Re-emitted rather than inherited, so the launcher is the single writer to the
 * client's stdout. Two processes writing to one descriptor would let a parse
 * error land in the middle of a frame.
 */
function pipeOut(stream: NodeJS.ReadableStream, out: NodeJS.WritableStream): Promise<void> {
  const lines = createInterface({ input: stream, crlfDelay: Infinity });
  return new Promise((resolvePromise) => {
    lines.on('line', (line) => {
      // The newline is the frame delimiter and `readline` strips it.
      out.write(`${line}\n`);
    });
    lines.on('close', () => resolvePromise());
  });
}

async function main(): Promise<void> {
  let binary = binaryPath(ROOT);

  // The postinstall path is the normal one; this is the fallback for an install
  // that skipped it.
  if (!existsSync(binary)) {
    process.stderr.write('sephera-mcp: binary not found, downloading...\n');
    if (!buildForCurrentMachine()) {
      process.stderr.write(`sephera-mcp: ${supportedSummary()}\n`);
      process.stderr.write(
        'Install from a source build instead: cargo install --git ' +
          'https://github.com/Reim-developer/Sephera sephera\n',
      );
      process.exit(1);
    }
    try {
      const installed = await install(ROOT, await version());
      if (!installed) {
        process.stderr.write(`sephera-mcp: ${supportedSummary()}\n`);
        process.exit(1);
      }
      binary = installed;
    } catch (error) {
      process.stderr.write(`sephera-mcp: ${message(error)}\n`);
      process.exit(1);
    }
  }

  // Piped, not inherited, because this process is the single writer to the
  // client's stdout and the single reader of the client's stdin.
  const child = spawn(binary, ['mcp'], { stdio: ['pipe', 'pipe', 'inherit'] });

  // The server's half, re-emitted rather than inherited.
  const piped = pipeOut(child.stdout, process.stdout);

  // The client's half, one line at a time: a line that is not JSON is answered
  // here and never reaches the server, which is the whole point. A line that is
  // valid JSON but not valid JSON-RPC is the server's business, not this one's.
  const client = createInterface({ input: process.stdin, crlfDelay: Infinity });
  client.on('line', (line) => {
    if (line.trim() === '') {
      // A client padding its frames with whitespace is not making a mistake
      // worth reporting.
      return;
    }
    try {
      JSON.parse(line);
    } catch {
      process.stdout.write(parseErrorFrame());
      return;
    }
    child.stdin.write(`${line}\n`);
  });

  // The client going away is the normal end: nothing to forward, nothing to
  // answer, and the server exits on its own stdin closing.
  client.on('close', () => child.stdin.end());

  child.on('error', (error) => {
    process.stderr.write(`sephera-mcp: failed to start ${binary}: ${message(error)}\n`);
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    process.exit(signal ? 1 : (code ?? 0));
  });

  await piped;
}

main().catch((error: unknown) => {
  process.stderr.write(`sephera-mcp: ${message(error)}\n`);
  process.exit(1);
});
