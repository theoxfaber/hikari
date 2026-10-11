# Benchmarks

Measured numbers for Hikari, published every release. Every figure states the
machine it came from. Claims that changed are recorded rather than quietly
overwritten, because a benchmark document that only ever shows improvements is
marketing, not measurement.

## Environment

| | |
|---|---|
| Machine | Apple M2, macOS ARM64, 8 GB |
| Toolchain | rustc 1.98.0, release profile (`lto = true`, `codegen-units = 1`) |
| Reproduce | `cargo bench -p hikari-rs --bench render` |

**Every number below is a criterion run on the machine above.** The benchmark
refuses to run under a debug build — `assert_release_build()` in
`crates/hikari/benches/render.rs` checks `cfg!(debug_assertions)` and panics with
an explanation. That check exists because v0.7 published a comparison taken from
a debug build; a debug benchmark produces a plausible number rather than an
obvious failure, which is the worst failure mode available for a measurement.

## Render timing — 1200×630

| Case | Time (criterion 95% CI) |
|---|---|
| `banner/layout` | 3.92 – 3.97 µs |
| `banner/paint_to_rgba` | 1.821 – 1.825 ms |
| `banner/png` (layout + paint + encode) | 3.532 – 3.537 ms |
| `text_card/png` | 3.547 – 3.642 ms |
| `text_card/svg` | 18.91 – 19.03 µs |

PNG encoding is therefore about 1.71 ms of the 3.53 ms — roughly half. Paint is
about 1.82 ms and layout is under 4 µs, which is three orders of magnitude below
the total and not worth optimising in isolation.

`banner` is a flat banner with one text run. `text_card` is three heading/body
pairs on a light panel. They render in the same time to within about 1%, and
their outputs differ (8,117 B vs 8,250 B, different digests). That is worth
stating rather than hiding: at this canvas size the cost is dominated by touching
every pixel, not by how much text is on it. A benchmark that varied only the text
and reported no difference is measuring the fill, and pretending otherwise would
be the kind of "optimisation" that lands on the wrong part of the pipeline.

## What this document does not contain

**No comparison against any other renderer.** v0.7 published one, it was taken
from a debug build, and it was withdrawn. This document does not replace it with
a fairer one.

The reason is not that the comparison would be unflattering. It is that a fair
one has to be *earned*, and this document has no budget to earn it:

- **Equivalent input.** Satori takes an HTML/CSS document, resvg takes SVG. This
  library takes a typed node tree. A benchmark that ports the tree into their
  input format is measuring the port as much as the renderer, and a benchmark
  that skips the port is measuring something neither of them does.
- **Equivalent output.** Any comparison needs both sides producing the same
  image. Matching that closely enough to attribute a time difference to the
  renderer rather than to the pixels needs a pixel-diff harness, which is its own
  piece of work and its own source of disagreement about who won.
- **A machine neither of us controls.** Numbers on a shared CI runner vary by
  more than the differences being claimed.

All three can be done. None is done here, and until one is, a number in this
document comparing against anything else would be an assertion rather than a
measurement — which is the thing this document exists to avoid.

## Output size

| Artifact | Bytes |
|---|---|
| `og.png` (gradient banner) | 13,077 |
| `i18n.png` (multilingual) | 29,788 |
| `card.png` (grid + image + rounded clip) | 57,931 |
| Determinism reference card | 21,649 |

## Embedded font

| | Bytes |
|---|---|
| `DejaVuSans.ttf` source | 757,076 |
| Embedded subset | 368,724 (49%) |
| Codepoints covered | 2,521 |
| Glyphs retained (incl. composite components) | 2,596 |

Shrinking the subset further means giving up shaping tables. The 288 KB
layout-less variant is smaller and renders Arabic, Persian, kerning and
ligatures incorrectly; the 369 KB variant is the smallest one measured that
still renders them correctly.

What was reclaimed to keep the tables:

| Change | Saved |
|---|---|
| `post` → format 3.0 (no glyph names) | 62 KB |
| Legacy `kern` dropped, `GPOS` retained | 16 KB |

Both were verified not to change shaped output. What it costs: a deliberately
sparse glyph id space, about 49 KB of `loca`/`hmtx` padding, in exchange for
copying `GSUB`/`GPOS`/`GDEF` byte for byte.

## PNG encoder trade-off curve

Encoder settings trade bytes against time. The defaults shipped are the point
where both are defensible; the rejected rows are recorded so the choice is
reproducible rather than asserted.

| Setting | Bytes | Time | |
|---|---|---|---|
| `tiny-skia` default | 117,760 | 2.3 ms | superseded |
| Best + Adaptive | ~44,000 | ~300 ms | rejected — 100× slower for 1 KB |
| Fast + Adaptive | 74,081 | ~3.9 ms | rejected — size |
| **Default + Adaptive + RGB-strip** | **43,100** | **~10.5 ms** | **shipped** |

A filter-trial experiment (every filter at two levels) floored out around
44 KB, so the remaining gap to the reference optimum is content-side — gradient
smoothness and anti-aliasing — not encoder-side. The trial was correctly
rejected: all cost, no gain.

Reference cross-check: PIL with `optimize=True` on the same pixels produced
48 KB, so the shipped encoder is already at or below the reference optimum.

## Determinism

Same tree, same bytes, on every platform. This is the property that makes a
golden-image suite possible for anyone downstream.

**There are no digest rows in this table any more, deliberately.** Every digest
is in `crates/hikari/tests/determinism.sha256`, and the `determinism` CI job runs
across five targets — Linux x86_64, Linux arm64, macOS arm64, macOS x86_64,
Windows x86_64 — failing on any mismatch. A digest reprinted here would be a
second copy that can drift from the first.

That job is also what would have caught the CJK defect fixed in v0.21.9, where
output depended on whether a CJK font happened to be installed on the machine.
It could not, because it only ran on one runner *with* the font installed. Five
targets across three operating systems and two architectures is the minimum for
"everywhere" to mean anything, and the runner in this matrix deliberately has no
CJK font installed so the difference cannot hide.

**These digests changed in v0.18, and the previous ones were wrong.** They were
`7f97ec62…` / `b30db642…` at 21,628 B. v0.18 fixed the embedded font subset,
which had been dropping `GSUB`/`GPOS`/`GDEF`, and the paint path, which had
been looking glyphs up by source character instead of by the shaper's glyph id.
Kerning and ligatures now actually apply to the reference card, so glyph
positions changed and the bytes changed with them.

## Golden image corpus

Determinism alone is not coverage. The reference card exercises a fraction of the
engine, so a regression in wrapping, radii, shadows or gradients would not move
its bytes and CI would stay green while the library rendered something worse.

So there is one case per capability, in `crates/hikari/tests/corpus.sha256`, each
hashed independently. A change names the feature that moved rather than hiding
inside one opaque image.

| Case | Bytes | Guards |
|---|---|---|
| `gradient-linear` | 13,666 | linear angle and stop interpolation |
| `gradient-radial` | 100,475 | radial centre, radius and stops |
| `border-radius` | 6,567 | corner rounding and border width |
| `box-shadow` | 14,058 | shadow offset, blur and spread |
| `grid-layout` | 5,393 | equal-column grid, gap, padding |
| `flex-justify-align` | 1,503 | main and cross axis distribution |
| `word-wrap` | 25,446 | greedy wrapping at `max_width` |
| `text-clip-gradient` | 15,928 | `background-clip: text` |
| `latin-typography` | 12,905 | kerning, ligatures, Latin metrics |
| `arabic-joining` | 6,838 | contextual joining through GSUB |
| `hebrew-rtl` | 5,413 | RTL ordering and Hebrew coverage |
| `mixed-direction` | 8,576 | bidi run splitting |
| `image-clipped` | 6,686 | image decode, sizing, rounded clip |
| `absolute-positioning` | 2,096 | absolute offsets independent of flow |
| `nested-containers` | 7,727 | nested layout and inherited padding |
| `opaque-alpha-strip` | 595 | alpha stripping on a fully opaque frame |
| `blend-multiply` | 584 | multiply compositing of a fill against its parent |
| `blend-screen-text` | 1,286 | a blend mode reaching the glyph path, not just fills |
| `shadow-inset-top` | 2,744 | an inset shadow staying inside the silhouette |
| `shadow-inset-edge` | 1,877 | the hard-edged inset variant ignoring any blur radius |
| `filter-grayscale-invert` | 3,366 | colour filters applied to a whole subtree, not per element |

**A digest proves stability, not correctness.** A blank render is exactly as
stable as a correct one, and gets blessed just as happily. So each case is also
asserted to contain a measured amount of content by
`cargo run -p hikari-rs --example corpus_ink`, which checks real pixel values
against per-case ink floors. That check is what caught the radial gradient bug
below; the digest alone blessed the broken output without complaint.

## Corrections log

Kept deliberately. A benchmark document that has never been wrong has probably
not been measuring anything.

- **v0.7 claimed a large speed advantage over other renderers, from a debug
  build.** The comparison was withdrawn: it measured a debug build with weak
  compression. The warm-render numbers in this file are release numbers.
- **v0.13 recorded a "kerning/ligature tradeoff" as an accepted cost of font
  subsetting.** It was not a tradeoff. The subsetter was dropping the layout
  tables, so kerning and ligatures were silently absent. Fixed in v0.18.
- **v0.18 corrected the determinism digests** for the same reason, as above.
- **v0.20 shipped a radial gradient that never rendered as a gradient.**
  `with_radial_gradient` documents its centre as a box fraction, and `radius`
  follows that convention, but the rasterizer passed the radius to
  `tiny_skia` as if it were pixels. A `0.6` radius therefore became a 0.6-pixel
  gradient; every other pixel fell outside it and `SpreadMode::Pad` clamped
  them all to the final stop. The output was a flat fill in the last colour —
  5,280 bytes of nothing that looked like a small, plausible image. The SVG
  backend had the identical bug, so the two backends agreed with each other and
  disagreed with the API.

  Nothing caught it because the corpus digest was blessed from the broken
  output, and a flat gradient is perfectly stable to hash. It took an
  assertion about *ink* rather than *stability* to surface it: 3,263 content
  pixels out of 360,000, where a real gradient gives 326,263. After the fix the
  same case is 100,475 bytes, +95 KB of actual gradient, and the centre reads
  the inner stop as it should.

## Still slower than we would like

- Cold start at 14.3 ms is dominated by first glyph rasterization. A
  smaller embedded font or a warmed glyph cache across processes would help.
- `card.png` at 58 KB is larger than the encoder curve suggests it should be
  for a mostly-flat image. Gradient banding is the suspected cause and is
  unmeasured.
- Benchmarks are single-machine and single-card. The golden corpus added in
  v0.20 covers features, not workloads: it says nothing about throughput on
  text-heavy, image-heavy or document-shaped trees.
