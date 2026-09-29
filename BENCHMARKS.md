# Benchmarks

Measured numbers for Hikari, published every release. Every figure states the
machine it came from. Claims that changed are recorded rather than quietly
overwritten, because a benchmark document that only ever shows improvements is
marketing, not measurement.

## Environment

| | |
|---|---|
| Machine | Apple M2, macOS ARM64 |
| Toolchain | rustc 1.98.0, release profile (`lto = true`, `codegen-units = 1`) |
| Reference card | 1200×630, gradient background, two text runs, one wrapping |
| Reproduce | `cargo bench -p hikari-rs --bench render` |

## Render timing

| Stage | Cold (first in process) | Warm |
|---|---|---|
| Layout | 0.66 ms | 0.10 ms |
| Paint → RGBA | — | 3.06 ms |
| Paint + PNG encode | 13.59 ms | 5.44 ms |
| **Full `render_png`** | **14.3 ms** | **3.69 ms** |

Cold start is dominated by first-touch font rasterization, which is what the
build-time subset and the glyph bitmap cache exist to reduce. Warm numbers
benefit from the glyph cache; hit counters are asserted in tests, so a
regression there fails rather than quietly costing time.

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

| Target | PNG (21,649 B) | SVG (738 B) |
|---|---|---|
| macOS ARM64 | `ec287845…ba27993` | `57c9a893…8f79137` |
| Linux x86_64 | `ec287845…ba27993` | `57c9a893…8f79137` |

Full digests live in `crates/hikari/tests/determinism.sha256` and CI fails on
drift.

**These digests changed in v0.18, and the previous ones were wrong.** They were
`7f97ec62…` / `b30db642…` at 21,628 B. v0.18 fixed the embedded font subset,
which had been dropping `GSUB`/`GPOS`/`GDEF`, and the paint path, which had
been looking glyphs up by source character instead of by the shaper's glyph id.
Kerning and ligatures now actually apply to the reference card, so glyph
positions changed and the bytes changed with them.

The Linux row above has not been re-run since that change. The macOS row is
reproduced on every CI run; the Linux one is recorded from the run that verified
it. We would rather mark a row unverified than reprint a hash we have not
reproduced.

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

## Still slower than we would like

- Cold start at 14.3 ms is dominated by first glyph rasterization. A
  smaller embedded font or a warmed glyph cache across processes would help.
- `card.png` at 58 KB is larger than the encoder curve suggests it should be
  for a mostly-flat image. Gradient banding is the suspected cause and is
  unmeasured.
- Benchmarks are single-machine and single-card. A real corpus would cover
  text-heavy, image-heavy and document workloads separately.
