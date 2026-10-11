//! `npx -y sephera-mcp` -- start the MCP server over stdio.
//!
//! The launcher is three lines of work and most of the file is making sure it
//! fails well. An MCP client starts the server, waits, and reports "connection
//! refused" if nothing appears; the useful thing to do in that moment is say
//! which platform failed and what to do about it, on stderr, where it is not
//! mixed into the protocol stream.
//!
//! Everything goes to stdout only once the server is starting. stdout *is* the
//! MCP transport, so a stray `console.log` here corrupts the session -- which is
//! why the download path in `lib/install.js` reports through `stderr` too.

'use strict';

const { spawn } = require('node:child_process');
const path = require('node:path');

// `../lib`, not `./lib`: this file lives in `bin/` and the modules are beside it
// in the package root. `./lib` resolves to `bin/lib`, which does not exist, and
// the error names a module that is plainly right there in the repository -- which
// is how the end-to-end check found it.
const { binaryPath, buildForCurrentMachine, supportedSummary } = require('../lib/platform');
const { install } = require('../lib/install');

const ROOT = path.resolve(__dirname, '..');

async function main() {
  let binary = binaryPath(ROOT);

  // The postinstall path is the normal one. This is the fallback for an install
  // that skipped lifecycle scripts -- `--ignore-scripts`, and some CI images set
  // it by default -- so the server still works without the user diagnosing why
  // it does not.
  if (!require('node:fs').existsSync(binary)) {
    process.stderr.write('sephera-mcp: binary not found, downloading...\n');
    const build = buildForCurrentMachine();
    if (!build) {
      process.stderr.write(`sephera-mcp: ${supportedSummary()}\n`);
      process.stderr.write(
        'Install from a source build instead: cargo install --git ' +
          'https://github.com/Reim-developer/Sephera sephera\n'
      );
      process.exit(1);
    }
    try {
      binary = await install(ROOT);
    } catch (error) {
      process.stderr.write(`sephera-mcp: ${error.message}\n`);
      process.exit(1);
    }
  }

  // `mcp` is the subcommand that serves the protocol over stdio. Spawned with
  // stdio inherited so the client speaks to the process directly rather than
  // through this launcher -- a pipe between them would add a place for the
  // protocol to be corrupted.
  const child = spawn(binary, ['mcp'], { stdio: 'inherit' });
  child.on('error', (error) => {
    process.stderr.write(`sephera-mcp: failed to start ${binary}: ${error.message}\n`);
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    process.exit(signal ? 1 : (code ?? 0));
  });
}

main().catch((error) => {
  process.stderr.write(`sephera-mcp: ${error.stack ?? error.message}\n`);
  process.exit(1);
});
