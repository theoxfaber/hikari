import { readFileSync, writeFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import satori from 'satori';
import { Resvg } from '@resvg/resvg-js';
import { htmlToSatori } from './conv.mjs';

const [, , inPath, outPng, runsArg, fontPath] = process.argv;
const runs = Number(runsArg || 50);
const html = readFileSync(inPath, 'utf8');
const font = readFileSync(fontPath);
const opts = { width: 1200, height: 630,
  fonts: [{ name: 'DejaVu Sans', data: font, weight: 400, style: 'normal' }] };

const stat = (a) => { const s=[...a].sort((x,y)=>x-y);
  return { best: s[0], med: s[s.length>>1] }; };

// --- Satori: tree -> SVG
const tree = htmlToSatori(html);
let svg, sB = [], sA = [];
for (let i = 0; i < runs; i++) {
  const t = performance.now(); svg = await satori(tree, opts);
  const d = performance.now() - t; sB.push(d); sA.push(d);
}
writeFileSync('satori.svg', svg);

// --- resvg: SVG -> PNG. The Resvg instance is built ONCE, outside the loop:
// the first version constructed it per iteration and was timing SVG parsing
// and font loading rather than rasterisation, which is a 40x overstatement.
const r = new Resvg(svg, { font: { fontFiles: [fontPath], defaultFontFamily: 'DejaVu Sans' } });
let png, rB = [], rA = [];
for (let i = 0; i < runs; i++) {
  const t = performance.now(); png = r.render().asPng();
  const d = performance.now() - t; rB.push(d); rA.push(d);
}
writeFileSync(outPng, png);

const S = stat(sA), R = stat(rA);
console.log(`satori\thtml->svg\tbest=${S.best.toFixed(3)}ms\tmedian=${S.med.toFixed(3)}ms`);
console.log(`resvg\tsvg->png\tbest=${R.best.toFixed(3)}ms\tmedian=${R.med.toFixed(3)}ms`);
console.log(`combined\t\thtml->png\tbest=${(S.best+R.best).toFixed(3)}ms\tmedian=${(S.med+R.med).toFixed(3)}ms`);
