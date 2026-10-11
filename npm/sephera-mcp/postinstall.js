//! Install the binary at `npm install`, so the first MCP call is not a download.
//!
//! A warning, never a failure. `postinstall` failing fails the install of the
//! whole tree, and the user who just typed `npx -y sephera-mcp` may only be
//! trying it -- they can still run it, because the launcher downloads on demand.
//! The failure that matters is the launcher refusing, which happens with the
//! actual reason on stderr.

'use strict';

const path = require('node:path');
const { install } = require('./lib/install');
const { buildForCurrentMachine, supportedSummary } = require('./lib/platform');

const ROOT = path.resolve(__dirname);

install(ROOT).then(
  (binary) => {
    if (binary) {
      process.stderr.write(`sephera-mcp: installed ${binary}\n`);
    } else {
      process.stderr.write(
        `sephera-mcp: no published build for this machine (${supportedSummary()}).\n`
      );
    }
  },
  (error) => {
    process.stderr.write(
      `sephera-mcp: could not download the binary (${error.message}).\n` +
        'The package still installed; running it will try again.\n'
    );
  }
);
