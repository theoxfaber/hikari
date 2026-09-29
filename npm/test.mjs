import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';

const require = createRequire(import.meta.url);
const hk = require('./hikari-node.node');
const { mkdirSync } = require('node:fs');

const outDir = process.env.HIKARI_EXAMPLE_OUT
  ? (mkdirSync(process.env.HIKARI_EXAMPLE_OUT, { recursive: true }),
    process.env.HIKARI_EXAMPLE_OUT)
  : new URL('.', import.meta.url).pathname;

// Exercise the package entry point too, not just the raw binding. These were
// separate for a while and the wrapper quietly omitted renderWebpSync, which
// nothing caught because every test loaded ./hikari-node.node directly.
const wrapper = require('./index.js');
for (const name of [
  'renderPngSync',
  'renderSvgSync',
  'renderWebpSync',
  'renderPdfSync',
  'registerFont',
  'registeredFontCount',
  'version',
]) {
  assert.equal(typeof wrapper[name], 'function', `index.js must export ${name}`);
}
assert.equal(
  Object.keys(wrapper).length,
  Object.keys(hk).length,
  'index.js must re-export every binding entry point',
);

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
writeFileSync(`${outDir}/og-node.png`, png);

// Custom font registration: idempotent on content, and a registered font must
// actually change the render rather than being silently ignored.
const fontBytes = readFileSync('/System/Library/Fonts/Supplemental/Georgia.ttf');
const fontId = hk.registerFont('Brand', fontBytes);
assert.equal(typeof fontId, 'number', 'registerFont returns an id');
assert.equal(hk.registerFont('Brand again', fontBytes), fontId, 'same bytes -> same id');
assert.ok(hk.registeredFontCount() >= 2, 'embedded + registered');

const withFont = JSON.stringify({
  Container: {
    style: { width: 1200, height: 630, background: '#0b1020', font: fontId },
    children: [
      { Text: { text: 'Brand font from Node', style: { font_size: 72, color: '#ffffff' } } },
    ],
  },
});
const customPng = hk.renderPngSync(withFont, 1200, 630);
assert.ok(customPng.length > 2000, `custom-font png size ${customPng.length}`);
assert.notDeepEqual(
  Buffer.from(customPng),
  png,
  'a registered font produced identical output to the built-in',
);
const customSvg = hk.renderSvgSync(withFont, 1200, 630);
assert.ok(customSvg.includes('Brand'), `SVG names the family: ${customSvg}`);

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
