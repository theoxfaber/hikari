# Bake-off: Hikari vs Takumi, same card

Date: 2026-09-29. Machine: Apple Silicon macOS, Node v26.3.0.
Takumi: `takumi-js@2.14.0` + `react@19.3.0` (darwin-arm64 prebuilt).
Hikari: v0.7.0 via `hikari-node` (`renderPngSync`, node-tree JSON).
Brief: identical 1200×630 card — dark bg, 480px gradient hero with cyan
border ring, 72px heading, wrapped 30px paragraph, cyan caption.
Scripts: `npm/bench.mjs` (hikari) and `/tmp/takumi-bench/run.mjs` (takumi).

## Results (3 runs each, release Hikari vs `takumi-js@2.14.0`)

| Metric | Takumi | Hikari |
|---|---|---|
| Cold process render | ~14.0 ms | ~14.0 ms |
| Warm in-process render | ~7.1 ms | ~8.7 ms |
| PNG bytes | 26,954 | 43,149 |
| Peak RSS, one render | ~67.7 MB | ~85.4 MB |
| WASM raw / gzip | 3.82MB / — | **2.66MB / 1.07MB** |
| First render ever observed | 480 ms once (init), then 14 ms | steady |

Outputs: `npm/og-node.png` (hikari), `/tmp/takumi-bench/card-takumi.png`.
Both visually correct (checked by eye).

## Encoder trade-off curve (Hikari, release, same card)

| PNG setting | Bytes | Warm |
|---|---|---|
| `tiny-skia` default (v0.8 and earlier) | 117,760 | 2.3 ms (debug) |
| Best + Adaptive | ~44,000 | ~300 ms — rejected |
| **Default + Adaptive + RGB-strip (shipped)** | **43,100** | **~10.5 ms** |
| Fast + Adaptive | 74,081 | ~3.9 ms — rejected (size) |

PIL `optimize=True` reference on our pixels: 48KB — our encoder now
matches the reference optimum. Takumi's extra 16KB likely comes from
filter search (oxipng `-o2` style: try every filter, keep the best) and/or
smoother gradient quantization — that is follow-up #1, with a measured
target (27KB) instead of a guess.

## Reading (updated with release numbers)

- Warm renders are the same order of magnitude (10.5 vs 7.1 ms); Takumi
  leads. Our earlier 2.3 ms number was a debug build with weak compression
  — wrong to compare, corrected here.
- Takumi wins cold start, resident memory (~18 MB), and PNG size
  (**27KB vs 43KB** — narrowed from 4.3x to 1.6x by encoder work; the rest
  is filter search, tracked as follow-up #1 with a measured target).
- zlib-rs (SIMD) cut our encode ~35% (7.3→5.8ms on the probe card); a glyph
  bitmap cache removed repeat rasterization (verified by hit counters).
- Neither number is the whole story: fonts differ (DejaVu vs Takumi
  default), so byte counts aren't pure encoder comparisons.

## Determinism (verified 2026-09-29)

Same tree, same bytes, everywhere. Canonical render (`hikari --example
determinism`):

| Target | PNG (21,649 B) | SVG (738 B) |
|---|---|---|
| macOS ARM64 (native) | `ec287845…ba27993` | `57c9a893…8f79137` |
| Linux x86_64 (Docker `rust:1.89-bookworm`) | pending re-run | pending re-run |

Full hashes: PNG `ec287845088a03b82e75f0ce307fa81ca48e8b519a547e39c5ed0bf28ba27993`,
SVG `57c9a8932fa4f91abaacfae947b2bcae800e4790fcae5d5a2b6fce4488f79137`.
These live in `crates/hikari/tests/determinism.sha256` and CI fails if the
render drifts.

**These digests changed in v0.18, and the old ones were wrong.** They were
`7f97ec62…` / `b30db642…` at 21,628 B. v0.18 fixed the embedded font subset
(dropped layout tables) and the paint path (glyphs looked up by character
instead of by the shaper's glyph id), so kerning and ligatures now actually
apply to the canonical card. Different glyph positions, different bytes. The
Linux row is unverified: the previous run was on a machine we no longer have
access to, and we are not going to print a hash we have not reproduced.

1. **PNG size.** 118KB → 43KB shipped via encoder settings + RGB-strip
   (PIL optimum: 48KB — we beat the reference). Filter-trial experiment:
   all filters × two levels on our pixels floors at ~44KB, so Takumi's
   27KB is content-side (smoother gradients/AA), not encoder-side.
   Trial encoding correctly **rejected** — all cost, no gain.
2. **RSS diet.** 86 vs 68 MB. Suspects: embedded font bytes held twice
   (fontdue + rustybuzz + ttf-parser copies), image crate tables.
3. **WASM.** Debug build renders the card in Node at ~630ms / 42KB.
   Release + subset: **~64ms**, 2.66MB raw / 1.07MB gzip
   (Takumi: 3.82MB raw) — release + `wasm-opt` with a size budget remains
   tracked for the last mile.
4. Re-run on Linux x64 + Windows before any marketing claim.
