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

### Encoding is half the cost, and SVG is not a rasteriser

Measured on the same card, 1200x630, best of 20:

| Output | Time | Size |
|---|---|---|
| raw RGBA (no encode) | 1.89 ms | — |
| `render_svg` | **0.005 ms** | 309 B |
| `render_png` | 3.89 ms | 12,196 B |
| `render_webp` | 3.60 ms | 8,658 B |

The gap between the first and third rows is the whole encode cost: **51% of
`render_png` is zlib, not drawing.** Layout is 0.08 ms of it.

SVG is 780x faster than PNG because it does not rasterise anything — it
serialises. That is not a free lunch for everyone: a browser rasterises SVG for
free on the GPU, and a server-side rasteriser pays the same cost this library
avoided. It is the right output when the consumer can take vectors, and choosing
it is the single largest performance decision available to a caller.

`render_webp` is close in time but nowhere near the same in size — and which way
it wins depends entirely on the content:

| Workload | PNG | WebP | WebP advantage |
|---|---|---|---|
| flat card, text | 3.83 ms / 12,238 B | 3.50 ms / 8,662 B | **29% smaller** |
| 12 image tiles | 11.36 ms / 28,858 B | 10.52 ms / 178,902 B | **6.2x larger** |

The first version of this document said "29% size win" on the strength of the
card alone, which is the same error as publishing a card-shaped performance
claim in the first place. Lossless WebP wins on flat UI colour — flat fills,
large uniform areas, few distinct values — and loses badly on photographic or
high-frequency content, where PNG's predictive filters have far less to learn
against. A gradient tile is the worst case for WebP here and the best case for
PNG.

So the guidance is not "use WebP for smaller files". It is: **there is no
default that is right**, and the choice is worth a measurement on your own
content rather than a rule copied from this table.

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

## Workloads

Everything above renders roughly one shape: a few text runs and flat rectangles
on a card. That measures fill cost and says almost nothing about what people
actually render. These three are chosen to stress *different* stages, and each
reports the stages separately so the split can move between them.

1200x630, criterion 95% CI, M2/macOS ARM64. `cargo bench -p hikari-rs --bench render`.

| Workload | layout | paint_to_rgba | encode_png |
|---|---|---|---|
| `text_dense` — 40 sections, 80 runs | 0.751 ms | 4.238 ms | 5.221 ms |
| `images` — 12 decoded PNGs | **0.013 ms** | 8.901 ms | 12.182 ms |
| `document` — 150 rows | 0.442 ms | 2.536 ms | 3.624 ms |
| *(banner, for contrast)* | *0.004 ms* | *1.821 ms* | *3.532 ms* |

Reading the shares rather than the totals:

- **Encode is 40–56% everywhere.** It is consistently the largest single item,
  and it is not a constant.
- **Layout is 0.4% on a card and 0.1% on an image card.** The "layout is three
  orders of magnitude below the total, not worth optimising" line above is true
  of cards and says nothing else.
- **Layout scales with text, not area.** `document` has 150 nodes and costs
  0.44 ms; `text_dense` has 80 runs and costs 0.75 ms.

### What the image workload found

Adding `images` to this suite immediately found a defect the other benchmarks
could not see, because none of them contained an image.

`image_dimensions` **fully decoded the image** to read its width and height. The
header already contains them — two `u32`s at offset 16 in a PNG — so layout was
decompressing every pixel of every image to learn how large it was, and the
paint stage then decoded them all again.

```
images/layout       before  3.969 ms      after  0.013 ms     300x
images/paint        before  13.845 ms     after  8.901 ms      1.6x
```

Paint improved too, because the image was being decoded twice and now is decoded
once. Layout+paint for this workload went from 17.81 ms to 8.91 ms — **half the
time**, from a one-line change.

This is the argument for the workload benchmarks in one measurement: a suite
that varies only the content of a card will never find this, because the bug is
not about cards. The 300x figure was sitting in plain sight, unmeasured, for
the entire life of the project, and the card benchmarks were not wrong — they
were just not asking.

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

- Cold start is **not** dominated by glyph rasterization, as this file used to
  claim. Measured on the first render in a process: 10.4 ms total, first paint
  3.19 ms against a second paint of 2.26 ms, across 10 glyph misses. Glyph
  rasterization is ~0.9 ms of a one-time cost, so pre-warming the cache would
  be optimising the smaller half of it and is not worth doing. What remains is
  genuinely one-time setup — font face parse, pixmap allocation, encode — and
  there is no lever on it short of not starting a process.
- `card.png` at 58 KB looked too large for a mostly-flat image, and the note
  guessed that gradient banding was the cause. **Measured, and the guess was
  wrong.** The hero gradient is constant down every column (zero variation over
  87 samples) and varies smoothly across, by at most 1 unit per 5 px. There is
  no banding. Re-encoding the same pixels at maximum compression gives 55,776
  bytes against the encoder's 57,931 — 3.7% of headroom, which is not a win
  worth taking. The file is large because a full-canvas smooth gradient is
  genuinely expensive to compress losslessly, and the encoder was never the
  problem. Closed.
- Benchmarks are single-machine and single-card. The golden corpus added in
  v0.20 covers features, not workloads: it says nothing about throughput on
  text-heavy, image-heavy or document-shaped trees.
