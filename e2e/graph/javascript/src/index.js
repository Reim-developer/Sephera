const sibling = require('./sibling');
const nested = require('../javascript/lib/entry');
const bare = require('react');
const deep = require('lodash/fp');
const pkgRoot = require('../javascript');
const dynamic = import('./lazy');

module.exports = { sibling, nested, bare, deep, pkgRoot, dynamic };
