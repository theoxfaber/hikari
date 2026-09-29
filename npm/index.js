const { createRequire } = require('node:module');
const binding = createRequire(__filename)('./hikari-node.node');

module.exports = {
  version: binding.version,
  renderPngSync: binding.renderPngSync,
  renderSvgSync: binding.renderSvgSync,
  renderPdfSync: binding.renderPdfSync,
};
