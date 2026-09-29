import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';

const require = createRequire(import.meta.url);
const hk = require('./hikari-node.node');

assert.match(hk.version(), /^\d+\.\d+\.\d+$/, 'version looks semver');

const tree = JSON.stringify({
  Container: {
    style: {
      width: 1200.0,
      height: 630.0,
      display: 'Flex',
      dir: 'Row',
      justify: 'Center',
      align: 'Center',
      gap: 0.0,
      padding: 0.0,
      margin: 0.0,
      border: 0.0,
      border_color: null,
      absolute: false,
      left: null,
      top: null,
      background: { Solid: { r: 11, g: 16, b: 32, a: 255 } },
      color: null,
      font_size: null,
      radius: 0.0,
      grow: 0.0,
      grid_cols: null,
      max_width: null,
      aspect: null,
    },
    children: [
      {
        Text: {
          text: 'Hello from Node',
          style: {
            width: null,
            height: null,
            display: 'Flex',
            dir: 'Row',
            justify: 'Start',
            align: 'Start',
            gap: 0.0,
            padding: 0.0,
            margin: 0.0,
            border: 0.0,
            border_color: null,
            absolute: false,
            left: null,
            top: null,
            background: null,
            color: { r: 255, g: 255, b: 255, a: 255 },
            font_size: 72.0,
            radius: 0.0,
            grow: 0.0,
            grid_cols: null,
            max_width: null,
            aspect: null,
          },
        },
      },
    ],
  },
});

const png = hk.renderPngSync(tree, 1200, 630);
assert.equal(png[0], 0x89, 'PNG magic');
assert.equal(png[1], 0x50, 'PNG magic');
assert.ok(png.length > 5000, `png size ${png.length}`);
writeFileSync(process.env.HIKARI_EXAMPLE_OUT
  ? `${process.env.HIKARI_EXAMPLE_OUT}/og-node.png`
  : new URL('./og-node.png', import.meta.url).pathname, png);

const svg = hk.renderSvgSync(tree, 1200, 630);
assert.ok(svg.startsWith('<svg'), 'svg root');
assert.ok(svg.includes('Hello from Node'), 'svg text');

const webp = hk.renderWebpSync(tree, 1200, 630);
assert.equal(webp[0], 0x52, 'RIFF magic'); // R
assert.equal(webp[1], 0x49); // I
assert.ok(
  webp[8] === 0x57 && webp[9] === 0x45 && webp[10] === 0x42 && webp[11] === 0x50,
  'WEBP marker'
);

console.log(`node OK: version=${hk.version()} png=${png.length}B svg=${svg.length} chars webp=${webp.length}B`);
