import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { version, render_png, render_svg } from './hikari_wasm.js';

assert.match(version(), /^\d+\.\d+\.\d+$/);
const tree = readFileSync('/tmp/bench-tree.json', 'utf8');
const t0 = performance.now();
const png = render_png(tree, 1200, 630);
const ms = performance.now() - t0;
assert.equal(png[0], 0x89, 'PNG magic');
assert.ok(png.length > 10000, `png size ${png.length}`);
const svg = render_svg(tree, 1200, 630);
assert.ok(svg.startsWith('<svg'));
console.log(JSON.stringify({ version: version(), ms, bytes: png.length }));
