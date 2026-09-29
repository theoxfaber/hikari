# Competition (researched 2026-09-29, stars live from GitHub API)

The field splits three ways: document compilers, web renderers, and
building blocks. Hikari's wedge is the intersection: one Rust engine for
stills + motion + documents with a single API.

## Direct competitors

| Project | Stars | Lang | What it is | Hikari vs it |
|---|---|---|---|---|
| `kane50613/takumi` | 3,041 | Rust+JS | OG images + PDFs + animations from JSX/CSS, no browser | Feature superset today; Hikari is faster nowhere measured — Takumi leads PNG (27KB@7ms vs 43KB@10.5ms). Our edges: single API, no forked deps, offline Pro licensing |
| `vercel/satori` | 13,989 | TS | HTML/CSS → SVG (needs a rasterizer after) | JS-only, SVG-only output; Hikari renders PNG/GIF/PDF natively in Rust |
| `og_image_writer` (crates.io) | — | Rust | CSS-like OG image API | Closest Rust niche rival; narrower scope (images only, no PDF/motion) |
| Chromium/Puppeteer screenshots | — | — | Full browser rendering | Pixel-perfect web fidelity; 100x heavier, needs a browser fleet |

## Adjacent giants (learn from, don't fight head-on)

| Project | Stars | Lesson for Hikari |
|---|---|---|
| `typst/typst` | 56,309 | World-class Rust document compiler. Sets the bar for text quality, scripting, and packaging. Hikari is not a typesetting system and should not pretend to be one |
| `linebender/resvg` | 4,096 | Reference SVG correctness. Candidate backend if our SVG fidelity ever needs to go beyond hand-rolled output |
| `fschutt/printpdf` | 1,117 | Pure-Rust PDF read/write/render, WASM-ready. Overlaps our PDF backend; watch its WASM story |

## Building blocks we stand on (upstream-only policy)

`taffy` (layout) · `rustybuzz` (shaping) · `tiny-skia` (raster) ·
`allsorts` (font subsetting) · `pdf-writer` (PDF objects) ·
`image` (codecs) · `fontdue` (glyph raster) · `napi` (Node bindings)

## Where Hikari can be best in the world (honest version)

1. **Single engine, four outputs.** Nobody else does PNG + SVG + GIF +
   selectable-text PDF from one tree with one API in pure Rust.
2. **Measured hot path.** Publish `BENCHMARKS.md` numbers every release;
   never claim without measuring (we were wrong about our 2.3ms).
3. **Boring dependencies.** Zero forks, zero system deps, offline everything.
4. **Priced to fund the unsexy parts.** Subsetting, PDF conformance,
   prebuilds — the work stars don't pay for.

## Where we are not best (do not claim)

CSS breadth, PDF/A-UA conformance, pagination, WASM/edge, docs, users.
Each has a plan step; none is done.
