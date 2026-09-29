import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';

const require = createRequire(import.meta.url);
const hk = require('./hikari-node.node');

const C = (r, g, b) => ({ r, g, b, a: 255 });
const base = {
  width: null, height: null, display: 'Flex', dir: 'Row',
  justify: 'Start', align: 'Start', gap: 0, padding: 0, margin: 0,
  border: 0, border_color: null, absolute: false, left: null, top: null,
  background: null, color: null, font_size: null, radius: 0, grow: 0,
  grid_cols: null, max_width: null, aspect: null,
};
const S = (o) => ({ ...base, ...o });
const T = (text, size, hex) => ({
  Text: { text, style: S({ color: hex, font_size: size }) },
});
const white = C(255, 255, 255);

const tree = JSON.stringify({
  Container: {
    style: S({
      width: 1200, height: 630, align: 'Center', justify: 'Center',
      gap: 48, padding: 60, background: { Solid: C(11, 16, 32) },
    }),
    children: [
      {
        Container: {
          style: S({
            width: 480, height: 480, radius: 32, border: 4,
            border_color: C(56, 189, 248),
            background: {
              Linear: {
                angle_deg: 180,
                stops: [
                  { pos: 0, color: C(219, 234, 254) },
                  { pos: 1, color: C(254, 226, 226) },
                ],
              },
            },
          }),
          children: [],
        },
      },
      {
        Container: {
          style: S({ dir: 'Column', justify: 'Center', gap: 20 }),
          children: [
            T('Ship OG images', 72, white),
            {
              Text: {
                text: 'Grid layout, decoded images, borders and wrapped paragraphs. No browser, no screenshots, just pixels.',
                style: S({ color: C(148, 163, 184), font_size: 30, max_width: 480 }),
              },
            },
            T('hikari bench', 28, C(56, 189, 248)),
          ],
        },
      },
    ],
  },
});

const times = [];
let out;
for (let i = 0; i < 3; i++) {
  const t0 = performance.now();
  out = hk.renderPngSync(tree, 1200, 630);
  times.push(performance.now() - t0);
}
assert.ok(out.length > 10000);
writeFileSync('/tmp/bench-tree.json', tree);
console.log(JSON.stringify({ times_ms: times, bytes: out.length }));
