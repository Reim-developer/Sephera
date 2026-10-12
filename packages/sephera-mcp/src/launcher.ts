/**
 * `npx -y sephera-mcp` -- start the MCP server over stdio.
 *
 * The launcher is three lines of work and most of this file is making sure it
 * fails well. An MCP client starts the server, waits, and reports "connection
 * refused" if nothing appears; the useful thing to do in that moment is say which
 * platform failed and what to do about it, on stderr, where it is not mixed into
 * the protocol stream.
 *
 * Nothing is written to stdout before the server starts. stdout *is* the MCP
 * transport, so a stray `console.log` here corrupts the session -- which is why
 * the download path reports through stderr too.
 */

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

import { binaryPath, buildForCurrentMachine, supportedSummary } from './platform.js';
import { install } from './install.js';

/** The package root, from `dist/launcher.js`. */
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** Read from the package's own manifest rather than a constant, so a version bump is one edit. */
async function version(): Promise<string> {
  const manifest = JSON.parse(
    await readFile(resolve(ROOT, 'package.json'), 'utf8'),
  ) as { version: string };
  return manifest.version;
}

async function main(): Promise<void> {
  let binary = binaryPath(ROOT);

  // The postinstall path is the normal one. This is the fallback for an install
  // that skipped lifecycle scripts -- `--ignore-scripts`, and some CI images set
  // it by default -- so the server still works without the user diagnosing why
  // it does not.
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

  // `mcp` is the subcommand that serves the protocol over stdio. Spawned with
  // stdio inherited so the client speaks to the process directly rather than
  // through this launcher -- a pipe between them would add a place for the
  // protocol to be corrupted.
  const child = spawn(binary, ['mcp'], { stdio: 'inherit' });
  child.on('error', (error) => {
    process.stderr.write(`sephera-mcp: failed to start ${binary}: ${message(error)}\n`);
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    process.exit(signal ? 1 : (code ?? 0));
  });
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

main().catch((error: unknown) => {
  process.stderr.write(`sephera-mcp: ${message(error)}\n`);
  process.exit(1);
});
