import { readFileSync } from 'node:fs';
import { Resvg } from '@resvg/resvg-js';
import satori from 'satori';
import { htmlToSatori } from './conv.mjs';

const [, , htmlPath, rgbaPath, label] = process.argv;
const FONT = '/Users/apple/hikari/crates/hikari-core/assets/DejaVuSans.ttf';
const W = 1200, H = 630;
const html = readFileSync(htmlPath, 'utf8');
const font = readFileSync(FONT);
const svg = await satori(htmlToSatori(html), { width: W, height: H,
  fonts: [{ name: 'DejaVu Sans', data: font, weight: 400, style: 'normal' }] });
const b = Buffer.from(new Resvg(svg, { background: 'white' }).render().pixels);
const a = readFileSync(rgbaPath);
if (a.length !== b.length) { console.log(label, 'size mismatch'); process.exit(1); }

const n0 = (dy) => { let n = 0;
  for (let y = 0; y < H; y++) { const y2 = y + dy; if (y2 < 0 || y2 >= H) continue;
    for (let x = 0; x < W; x++) { const i = (y*W+x)*4, j = (y2*W+x)*4;
      if (Math.max(Math.abs(a[i]-b[j]), Math.abs(a[i+1]-b[j+1]), Math.abs(a[i+2]-b[j+2])) > 8) n++; } }
  return n; };
let best = { n: Infinity, dy: 0 };
for (let dy = -140; dy <= 140; dy++) { const n = n0(dy); if (n < best.n) best = { n, dy }; }
console.log(`${label.padEnd(12)} raw ${(n0(0)/(W*H)*100).toFixed(2).padStart(5)}%   shift dy=${String(best.dy).padStart(4)}   residual ${(best.n/(W*H)*100).toFixed(2).padStart(5)}%`);
