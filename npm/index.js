const { createRequire } = require('node:module');
const fs = require('node:fs');
const path = require('node:path');

// One package, one prebuilt binary per platform target, resolved at require
// time from process.platform/arch. release.yml builds exactly the targets
// listed below and suffixes each file with the cargo target triple; if that
// mapping ever drifts from the manifest, the require fails loudly and names
// both the host pair and the file it looked for -- a silent "bindings exists
// but nothing loads" from a stale map is worse than an explicit error.

const HOST = `${process.platform}-${process.arch}`;

// The first list entry is the ordinary case for the host libc; the musl
// fallback covers a statically-linked-musl binary loading under glibc, which
// is legal, so a musl-only mirror of a glibc host still loads rather than
// dying on the first candidate.
const TARGETS_BY_HOST = {
  'darwin-x64': ['x86_64-apple-darwin'],
  'darwin-arm64': ['aarch64-apple-darwin'],
  'linux-x64': ['x86_64-unknown-linux-gnu', 'x86_64-unknown-linux-musl'],
  'linux-arm64': ['aarch64-unknown-linux-gnu', 'aarch64-unknown-linux-musl'],
  'win32-x64': ['x86_64-pc-windows-msvc'],
};

const targets = TARGETS_BY_HOST[HOST];
if (!targets) {
  throw new Error(
    `hikari-node: no prebuilt binding for host '${HOST}'. ` +
    `Supported: ${Object.keys(TARGETS_BY_HOST).join(', ')}. ` +
    `Build from source: cargo build --release -p hikari-rs-node`,
  );
}

const requireCjs = createRequire(__filename);
let binding = null;
const tried = [];
for (const target of targets) {
  const file = path.join(__dirname, `hikari-node.${target}.node`);
  tried.push(path.basename(file));
  if (fs.existsSync(file)) {
    binding = requireCjs(file);
    break;
  }
}

if (!binding) {
  throw new Error(
    `hikari-node: package installed without a binding for host '${HOST}' ` +
    `(looked for: ${tried.join(', ')}). This usually means the npm package ` +
    `was published from an incomplete artifact merge.`,
  );
}

// Re-export every entry point the native binding provides. `renderWebpSync`
// was previously missing here, so it existed in the binding but was
// unreachable for anyone using the package entry point.
module.exports = {
  version: binding.version,
  registerFont: binding.registerFont,
  registeredFontCount: binding.registeredFontCount,
  renderPngSync: binding.renderPngSync,
  renderSvgSync: binding.renderSvgSync,
  renderWebpSync: binding.renderWebpSync,
  renderPdfSync: binding.renderPdfSync,
};
