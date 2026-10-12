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

## Comparison — Satori + resvg

This section did not exist for six releases, and the reason given was that a
fair comparison had to be earned. That was a fair position and an unhelpful one:
it declined the work rather than doing it. The harness now exists
(`scripts/compare/`), so this section is measured rather than asserted.

**Same document, same machine, same measurement code.** 1200x630, M2/macOS
ARM64, 50 iterations, best and median reported.

| Stage | Best | Median |
|---|---|---|
| `satori` HTML tree -> SVG | 0.42 ms | 0.55 ms |
| `resvg` SVG -> PNG | 7.96 ms | 8.37 ms |
| **satori + resvg, end to end** | **8.38 ms** | **8.92 ms** |
| `hikari` HTML -> PNG | **3.47 ms** | **3.59 ms** |

**hikari is 2.5x faster end to end.** 94% of the Satori pipeline is resvg's
rasterisation; Satori's own layout pass is 0.55 ms, close to hikari's whole
pipeline.

### Output equivalence, measured

Not eyeballed. Both images are compared pixel by pixel, and because the two
renderers place this document differently, the search reports the vertical
offset that minimises the difference and the residual at that offset:

| | differing pixels | after best vertical shift |
|---|---|---|
| as rendered | 10.16% | 1.23% at dy=68px |
| with `padding` removed | 1.69% | **1.23% at dy=4px** |

The 1.23% residual is constant across every variant tested and is antialiasing
on glyph edges — mean channel delta 0.55/255. The two rasterisers agree on the
image. What they do not agree on is layout: hikari places this document 68px
from where Satori does, and removing `padding` collapses that to 4px, so the
disagreement is in how padding and `justify-content` compose in a column flex
container. That is a real behavioural difference and it is stated here rather
than smoothed over.

### What these numbers do not say

- **`resvg` is `@resvg/resvg-js`**, the Node binding, not the Rust crate. The
  two are the same rasteriser but not the same overhead.
- **hikari skips three properties in this document** — `font-family`,
  `font-weight`, `margin-top` — so it does strictly less work. Removing them
  from the input to make the comparison flattering would have been dishonest;
  leaving them in and reporting them is the point.
- **A `resvg` figure of 117 ms also exists** and is not reported above. It is
  what you get if the `Resvg` instance is constructed inside the timing loop,
  which re-parses the SVG and reloads the font every iteration. The first
  version of this harness did exactly that and appeared to show a 15x lead.
  A benchmark that measures construction rather than rendering is worse than no
  benchmark, because it is plausible.
- **One machine, one run.** No cross-machine figures, no CI comparison job.
- **Two runtimes.** Rust native against Node. Some of the difference is the
  runtime boundary, and this cannot be separated from the rasterisers without a
  third implementation.

### The thing worth keeping

Handing Satori the HTML string directly does not fail loudly. It renders the
markup as literal text, emits an SVG full of `x="NaN"`, produces a blank white
page — and then benchmarks as the fastest renderer in the comparison. That is
what an unfair comparison looks like from the inside: not a wrong number, but a
number describing a different computation than the one claimed.
Output size

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
- **v0.7–v0.21.9 published no comparison at all**, on the grounds that a fair
  one could not be earned without a pixel-diff harness and a port. That was
  correct about the difficulty and wrong about the response: it declined the
  work for six releases instead of building the harness, which took an
  afternoon and made the comparison possible. The withdrawal was honest and it
  was also a way of not doing the thing. v0.21.10 publishes a measured
  comparison with its caveats attached.

  Building it also produced the numbers this file would otherwise have had to
  take on trust: a `resvg` figure of 117 ms that is really SVG parsing and font
  loading, and a Satori pipeline that renders an HTML string as literal text and
  benchmarks as the fastest thing in the room. Neither is in the comparison
  table. Both are here because a benchmark document that has never been wrong
  has probably not been measuring anything.
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
