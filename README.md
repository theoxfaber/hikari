# Hikari

Render OG images, SVG, animations and documents from layout trees. Pure Rust, no browser, no forked dependencies.

```rust
use hikari::{render_png, Node, Style};

let tree = Node::container(
    Style::row().with_size(1200.0, 630.0).with_background("#0b1020"),
    vec![Node::text("Hello from Hikari", Style::text(72.0, "#ffffff"))],
);
std::fs::write("og.png", render_png(&tree, 1200, 630)?)?;
```

One API produces PNG, SVG, animated GIF/APNG, lossless WebP, and PDF with
selectable text. Text is shaped with real OpenType layout tables, so kerning,
ligatures and Arabic joining work. Output is byte-identical across platforms,
which is what makes it snapshot-testable in CI.

```sh
cargo add hikari-rs
```

---

## The bar

Hikari is held to one standard: **be the fastest, most correct and most
predictable way to turn a layout tree into pixels and documents.**

Not "fast for a Rust library". Not "good enough". The best number we can
measure, on every axis, with the unflattering ones published too. Every figure
in this README was measured on the machine named beside it. Where we are
weak, [Known limitations](#known-limitations) says so without hedging.

That standard is why the interesting parts of this codebase are the unglamorous
ones: font subsetting that does not silently break shaping, a paint path that
renders the glyph the shaper actually chose, a cache that survives a panic, and
CI that fails when any of that regresses.

---

## Contents

- [What it does](#what-it-does)
- [Determinism](#determinism)
- [Performance](#performance)
- [Text shaping](#text-shaping)
- [Fonts](#fonts)
- [Node.js](#nodejs)
- [WebAssembly](#webassembly)
- [Architecture](#architecture)
- [Open core and Pro](#open-core-and-pro)
- [Known limitations](#known-limitations)
- [Roadmap](#roadmap)
- [Development](#development)
- [License](#license)

---

## What it does

| Area | Capability |
|---|---|
| **Layout** | Flexbox, CSS grid, block flow, absolute positioning, margins, borders, gaps |
| **Text** | Shaping, bidi/RTL, word wrap, balanced headlines, fit-to-width, `background-clip: text` |
| **Scripts** | Latin, Greek, Cyrillic, Hebrew, Arabic with contextual joining, Persian. CJK via system fallback |
| **Images** | PNG/JPEG/GIF/WebP decode, `cover`/`contain`/`fill`, rounded clipping, decode cache |
| **Paint** | Solid, linear and radial gradients, box shadows (blurred raster + SVG `feDropShadow`) |
| **Output** | PNG, SVG, animated GIF, animated APNG, lossless WebP, PDF with selectable text |
| **Documents** | PDF outlines, link annotations, file attachments, pagination, repeating headers, metadata |
| **Bindings** | Rust, Node.js (napi), WebAssembly (wasm-bindgen) |

Bindings for PNG, SVG and WebP are free and MIT/Apache-2.0. Motion and PDF are
Pro — see [Open core and Pro](#open-core-and-pro).

```sh
cargo run -p hikari-rs --example og           # gradient banner
cargo run -p hikari-rs --example card         # grid + image + rounded clip
cargo run -p hikari-rs --example multilingual # Arabic, Persian, Hebrew, CJK
cargo run -p hikari-rs --example cliptext     # background-clip: text
cargo run -p hikari-rs --example motion       # animated GIF + APNG (Pro)
cargo run -p hikari-rs --example invoice      # flowing multi-page PDF (Pro)
cargo run -p hikari-rs --example determinism  # cross-platform digests
```

Every example writes to the workspace root (override with `HIKARI_EXAMPLE_OUT`).

---

## Determinism

The same tree produces the same bytes on every platform and architecture. Not
"visually equivalent" — the same SHA-256.

`cargo run --release -p hikari-rs --example determinism` renders a fixed card
and checks it against `crates/hikari/tests/determinism.sha256`:

| | Bytes | SHA-256 |
|---|---|---|
| PNG | 21,649 | `ec287845…ba27993` |
| SVG | 738 | `57c9a893…8f79137` |

CI fails if the render drifts, and prints the re-bless command. This is what
makes a golden-image test suite possible in the first place — a renderer whose
output moves with the host cannot be regression-tested by anyone.

Re-baseline after an intentional visual change:

```sh
cargo run --release -p hikari-rs --example determinism -- --bless
```

---

## Performance

Measured on **Apple M2, macOS ARM64, rustc 1.98.0**, release profile
(`lto = true`, `codegen-units = 1`). Reference card: 1200×630, gradient
background, two text runs, one with wrapping.

| | Time |
|---|---|
| First render in a process (cold) | 14.3 ms — layout 0.66 ms, paint + encode 13.6 ms |
| Warm render, full PNG | **3.69 ms** |
| Warm layout only | 0.10 ms |
| Warm paint → RGBA | 3.06 ms |

Cold start is dominated by font rasterization, which is why the glyph bitmap
cache and build-time subset exist. Reproduce with:

```sh
cargo bench -p hikari-rs --bench render
cargo run --release -p hikari-rs --example bench_breakdown
```

A correction worth keeping in the record: an earlier version of this project
claimed a large speed advantage over other renderers. That number came from a
debug build with weak compression, was wrong, and has been retracted. The
measurements above are the ones we can stand behind.

Output sizes for the examples above: `og.png` 13 KB, `i18n.png` 30 KB,
`card.png` 58 KB. The PNG encoder's size/speed trade-off curve is measured and
published in [BENCHMARKS.md](BENCHMARKS.md).

---

## Text shaping

Text is the part of a renderer most likely to be quietly wrong, so it gets the
most scrutiny here.

**The shaper's choice is the renderer's choice.** After OpenType substitution a
glyph is frequently *not* the glyph the source character maps to: ligatures
collapse several characters into one glyph, and Arabic letters are rewritten
into contextual presentation forms. Re-deriving a glyph id from the source
character throws that away, and the character paints as a blank gap — with
measurement, layout and advance widths all still correct, so nothing fails.

`hikari-core` carries the glyph id `rustybuzz` produced, and `hikari-raster`
paints that id. The fallback font, which never passes through the shaper, is
still addressed by character.

`crates/hikari-core/src/font_subset_tests.rs` shapes Arabic, Persian, Hebrew,
Latin kerning and Latin ligatures and compares the result against the full
source font — glyph count *and* advance widths. `hikari-raster` adds a
pixel-level guard asserting that no hole appears inside rendered text. The
thresholds in both are measured, not guessed: correct rendering leaves at most
a 12 px side-bearing gap, the character-keyed path left 47 px where a ligature
belonged. Both tests are verified to fail against the broken behaviour.

---

## Fonts

`crates/hikari-core/assets/DejaVuSans.ttf` (DejaVu Fonts License, permissive) is
subset at build time and is the single source of truth for shaping,
rasterization and PDF, so all three agree by construction.

### The subset preserves shaping

Most subsetters renumber glyphs into a dense range. That invalidates every
glyph id referenced inside `GSUB`, `GPOS` and `GDEF`, and most of them then
drop those tables entirely. The result is a font that renders Latin perfectly
and quietly loses Arabic joining, kerning and every ligature — no error, no
warning, just slightly wrong text.

So `crates/hikari-core/build/subset.rs` does the opposite: it **keeps the
original glyph ids** and leaves the gaps empty. The layout tables are then
copied byte for byte and need no rewriting at all, which means there is no
field that can be forgotten to remap — the class of bug that makes hand-rolled
layout subsetters fragile.

| | Bytes |
|---|---|
| Source font | 757,076 |
| Embedded subset | 368,724 (49%) |

The subset covers 2,521 codepoints across 2,596 glyphs (including composite
components, which reference further glyphs and must be pulled in or the outline
renders truncated). What that buys back versus the 288 KB layout-less subset:

- Arabic and Persian contextual joining, via the presentation-forms blocks
- kerning, via `GPOS`
- `fi` / `fl` / `ffi` / `ffl` ligatures, via the alphabetic presentation forms

The legacy `kern` table is dropped (16 KB) because every modern shaper prefers
`GPOS` — verified to change no shaped output. `post` is rewritten to format
3.0, discarding 62 KB of glyph names that nothing in the render path reads. The
id space is deliberately sparse, which costs about 49 KB of `loca`/`hmtx`
padding and buys the verbatim layout tables.

A build failure is raised if the source font lacks `GSUB`/`GPOS`/`GDEF`, so
this can never regress quietly again.

### Coverage

Latin, Latin Extended A/B, Greek, Cyrillic, Hebrew, Arabic, Arabic
Presentation Forms A and B, General Punctuation, super/subscripts, currency,
letterlike symbols, arrows, mathematical operators, geometric shapes,
miscellaneous symbols and dingbats. CJK falls back to a system font at runtime
(Arial Unicode on macOS, Noto CJK on Linux) — see
[Known limitations](#known-limitations).

---

## Node.js

```sh
cargo build --release -p hikari-rs-node
cp target/release/libhikari_node.dylib npm/hikari-node.node   # .so on Linux
node npm/test.mjs
```

| Export | Returns |
|---|---|
| `version()` | semver string |
| `renderPngSync(treeJson, w, h)` | `Buffer` |
| `renderSvgSync(treeJson, w, h)` | `string` |
| `renderWebpSync(treeJson, w, h)` | `Buffer`, lossless |
| `renderPdfSync(pagesJson, w, h)` | `Buffer` (Pro) |

The tree is the node tree serialized as JSON. Node is externally tagged
(`{ "Text": { … } }`), style fields are `snake_case`, and every field of
`Style` is optional:

```js
const hikari = require('@hikari-rs/node');

const tree = JSON.stringify({
  Container: {
    style: { width: 1200, height: 630, background: { Solid: { r: 11, g: 16, b: 32, a: 255 } } },
    children: [{ Text: { text: 'Hello from Node', style: { font_size: 72, color: { r: 255, g: 255, b: 255, a: 255 } } } }],
  },
});

const png = hikari.renderPngSync(tree, 1200, 630);
```

`color` is an `{ r, g, b, a }` struct, not a CSS string — `"#ffffff"` is
rejected. Full types ship in `npm/index.d.ts`.

PDF is a Pro feature and reads `HIKARI_LICENSE` and `HIKARI_PUBKEY` from the
environment, so operators keep their own signing infrastructure.

---

## WebAssembly

```sh
rustup target add wasm32-unknown-unknown
cargo build --release -p hikari-rs-wasm --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/release/hikari_wasm.wasm \
  --out-dir wasm --target nodejs
```

`render_png(treeJson, w, h)` and `render_svg(treeJson, w, h)`. The dependency
tree is pure Rust with no C zlib anywhere (`flate2/rust_backend`, `zlib-rs`),
so nothing needs a cross-compiled sysroot.

Release build is ~2.8 MB. `wasm-opt` with a size budget is tracked, and the
playground bundle is rebuilt in CI rather than committed so it can never drift
from the library it demonstrates.

---

## Architecture

```
Node  ──▶  hikari-core   layout, shaping, measurement, flow
              │              │
              │              ├──▶  hikari-raster   tiny-skia paint, PNG/WebP encode
              │              ├──▶  hikari-svg      vector output
              │              ├──▶  hikari-animate  GIF / APNG
              │              ├──▶  hikari-pdf     selectable text, outlines, attachments
              │              └──▶  hikari-license  offline ed25519 keys
              │
              └──▶  hikari (facade)  ──▶  hikari-node  ·  hikari-wasm
```

Layout runs once and the result is consumed by every backend, so the four
output formats cannot disagree with each other or with the measured text
metrics. Wrapping and line breaking are pure functions in `hikari-core`, called
by both layout and paint, so they agree exactly by construction.

Upstream dependencies only, no forks: `taffy` (layout), `rustybuzz` (shaping),
`tiny-skia` (raster), `pdf-writer` (PDF objects), `image` (codecs), `fontdue`
(glyph raster), `napi` (Node), `wasm-bindgen` (WASM).

Zero `unsafe` blocks in the workspace, enforced by `unsafe_code = "forbid"`.

---

## Open core and Pro

Stills are free forever under MIT/Apache-2.0. Pro adds animated GIF and APNG,
lossless WebP, and PDF documents, and funds the unglamorous work: font
subsetting, PDF conformance, prebuilt binaries for every OS and architecture,
and the WASM diet.

Pro keys are `ed25519`-signed strings (`hk1.…`) verified offline. No network
calls, no telemetry, no render metering, no seat counting. The enforcement code
is in the repository and readable, which is a deliberate choice: the check plus
commercial terms, rather than obfuscation. See [PRICING.md](PRICING.md).

---

## Known limitations

Stated plainly, because a limitations section that hedges is worse than none.

- **Hebrew does not join.** DejaVu Sans contains no Hebrew presentation forms,
  so even the unshaded source font renders `שלום` with isolated letters. This is
  font coverage, not a subsetting defect — a test asserts the subset matches
  the source exactly so the two are never confused. Closing it means bundling a
  font with Hebrew coverage.
- **CJK depends on system fonts.** Not bundled, because the permissive options
  are large and the proprietary system ones are not redistributable.
  Production deployments should ship a subsetted OFL CJK font instead of
  relying on system paths. The fallback path is exercised in CI with
  `fonts-noto-cjk` installed.
- **`rustybuzz` and `ttf-parser` are declared unmaintained**
  (RUSTSEC-2026-0206, RUSTSEC-2026-0192). The shaper sitting at the centre of
  this project being unmaintained is the largest outstanding risk. Both
  advisories are ignored in `deny.toml` with written reasons rather than
  tolerated silently. See [Roadmap](#roadmap).
- **CSS coverage is narrow by design.** The `Style` surface is about two dozen
  fields: box model, `display` (`Flex`/`Grid`/`Block`), `dir`, `justify`,
  `align`, `gap`, `grow`, `absolute` with `left`/`top`, `radius`, `border`,
  `background`, `color`, `font_size`, `max_width`, `aspect`, `shadow`,
  `clip_text` and `link`. Absent: `float`, `position: fixed`/`sticky`,
  `transform`, `z-index`, custom properties, `@media`, and shorthand
  properties. This is a layout tree, not a CSS engine.
- **PDF is not PDF/A or PDF/UA conformant.** No tagging, no accessibility
  structure, no archival validation.
- **No ecosystem yet.** No star history, no adopters, no battle-tested corpus.
  The determinism work exists precisely to make that corpus possible.
- **The git history is one commit.** The project's provenance is a single
  squashed commit, so its release timeline is documented in
  [CHANGELOG.md](CHANGELOG.md) rather than derived from history.

---

## Roadmap

Ordered by what most limits the project, not by what is easiest.

- [ ] **Migrate off `rustybuzz` and `ttf-parser` to `fontations`**
      (`skrifa` / `write-fonts`). Both crates are unmaintained. This is the
      single largest piece of technical debt and the only reason abandoned code
      sits in the most critical path.
- [ ] **Bundle OFL CJK and Hebrew-covering fonts**, ending the system-font
      fallback and fixing Hebrew joining.
- [ ] **Masks, filters, blend modes and shadow kinds.**
- [ ] **Golden-image corpus** beyond the single determinism card — every
      feature combination as a committed reference image, diffed in CI.
- [ ] **`lightningcss` style parsing** and a remote-asset preload helper.
- [ ] **Criterion regression gates.** CI currently smoke-runs benchmarks;
      shared runners are too noisy to assert on timings, so this needs a
      dedicated runner.
- [ ] **Animated WebP** encoding, and PDF/A + PDF/UA conformance.
- [ ] **Typed style builder** shared across Rust, Node and WASM, generated
      from one definition.

Shipped in v0.18: shaping and bidi with CJK fallback, grid and image nodes,
gradients, PNG and SVG, animated GIF and APNG, lossless WebP, the full PDF
backend with outlines/links/attachments/pagination, box shadows, PDF link
annotations, Node.js and WASM bindings, build-time font subsetting, text-fit
and headline balancing, cross-platform byte determinism, a layout-preserving
font subset, a paint path keyed on shaped glyph ids, and CI gates on
formatting, lints, tests, embedded-font shaping, determinism digests,
benchmarks and supply chain.

---

## Development

```sh
cargo test --workspace --exclude hikari-rs-node   # 73 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

`hikari-rs-node` is excluded because its test binary links N-API symbols that
only exist inside a Node process; it is tested by running `node npm/test.mjs`,
which CI does.

CI runs six gates, and each exists because something specific got past the
previous ones:

| Job | Catches |
|---|---|
| `fmt / clippy / test` | ordinary regressions |
| `embedded font shaping` | a subset that dropped `GSUB`/`GPOS`/`GDEF` — silent by construction |
| `cross-platform byte determinism` | any unintended change in rendered output |
| `benchmark smoke` | benches that stop compiling or hang |
| `supply chain` | unmaintained or vulnerably-licensed dependencies |
| `node bindings` | native binding and package metadata regressions |

`unsafe_code = "forbid"` and `clippy::all = "deny"` are workspace-wide.

Documentation and a live playground are at
[theoxfaber.github.io/hikari](https://theoxfaber.github.io/hikari), built in CI
from the current source.

---

## License

MIT or Apache-2.0, at your option. Pro features are source-available under
[LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md). The embedded DejaVu Sans is
under the DejaVu Fonts License.
