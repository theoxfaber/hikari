const { createRequire } = require('node:module');
const binding = createRequire(__filename)('./hikari-node.node');

// Re-export every entry point the native binding provides. `renderWebpSync`
// was previously missing here, so it existed in the binding but was
// unreachable for anyone using the package entry point.
module.exports = {
  version: binding.version,
  renderPngSync: binding.renderPngSync,
  renderSvgSync: binding.renderSvgSync,
  renderWebpSync: binding.renderWebpSync,
  renderPdfSync: binding.renderPdfSync,
};
