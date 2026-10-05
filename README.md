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

A card can be written as markup and CSS instead of a hand-built tree:

```rust
let html = r#"
<style>
  .card { display: flex; flex-direction: column; justify-content: center;
          width: 1200px; height: 630px; padding: 80px;
          background: linear-gradient(180deg, #dbeafe, #fee2e2); }
  .title { font-size: 84px; color: #0f172a; }
</style>
<div class="card"><span class="title">Hikari</span></div>
"#;
let tree = hikari::css::html_to_tree(html, &mut hikari::css::ParseReport::default())?;
std::fs::write("og.png", hikari::render_png(&tree, 1200, 630)?)?;
```

Unsupported CSS is reported rather than guessed, so a declaration this subset
does not implement can never quietly render as the wrong thing:

```rust
let mut report = hikari::css::ParseReport::default();
let tree = hikari::css::html_to_tree(html, &mut report)?;
println!("{report}");   // lists every declaration it skipped, and why
```

One API produces PNG, SVG, animated GIF/APNG, lossless WebP, and PDF with
selectable text. Text is shaped with real OpenType layout tables, so kerning,
ligatures and Arabic joining work. Output is byte-identical across platforms
for every script the embedded font covers — which makes it snapshot-testable in
CI. CJK currently breaks that guarantee; see
[Determinism](#determinism).

```sh
cargo add hikari-rs
```

---

## The bar

Be the fastest, most correct and most predictable way to turn a layout tree into
pixels and documents — and publish the numbers either way, including the ones
that are unflattering. That standard is why the interesting parts of this
codebase are the unglamorous ones.

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
- [Licence](#licence)
- [Known limitations](#known-limitations)
- [Roadmap](#roadmap)
- [Development](#development)
- [Licence](#licence)

---

## What it does

| Area | Capability |
|---|---|
| **Layout** | Flexbox, CSS grid, block flow, absolute positioning, margins, borders, gaps |
| **Text** | Shaping, bidi/RTL, word wrap, balanced headlines, fit-to-width, `background-clip: text` |
| **Fonts** | Embedded layout-preserving subset plus a bundled OFL Hebrew face; caller-supplied fonts via `register_font`, inherited through the tree |
| **Scripts** | Latin, Greek, Cyrillic, Hebrew, Arabic with contextual joining, Persian. CJK bundled behind an opt-in feature, else system fallback |
| **Images** | PNG/JPEG/GIF/WebP decode, `cover`/`contain`/`fill`, rounded clipping, decode cache |
| **Paint** | Solid, linear and radial gradients, drop and inset box shadows, 15 blend modes, `background-clip: text` |
| **Fallbacks** | A shaped font chain: runs are split by glyph coverage and shaped per segment, so fallback text joins |
| **Output** | PNG, SVG, animated GIF, animated APNG, lossless WebP, PDF with selectable text |
| **Documents** | PDF outlines, link annotations, file attachments, pagination, repeating headers, metadata |
| **Bindings** | Rust, Node.js (napi), WebAssembly (wasm-bindgen) |

Every backend above is free under MIT/Apache-2.0. There is no paid tier, no key
material and no feature flag — see [Licence](#licence).

```sh
cargo run -p hikari-rs --example og           # gradient banner
cargo run -p hikari-rs --example card         # grid + image + rounded clip
cargo run -p hikari-rs --example multilingual # Arabic, Persian, Hebrew, CJK
cargo run -p hikari-rs --example cliptext     # background-clip: text
cargo run -p hikari-rs --example motion       # animated GIF + APNG
cargo run -p hikari-rs --example invoice      # flowing multi-page PDF
cargo run -p hikari-rs --example determinism  # cross-platform digests
```

Every example writes to the workspace root (override with `HIKARI_EXAMPLE_OUT`).

---

## Determinism

**Scope: every script the embedded font covers.** Latin, Greek, Cyrillic,
Hebrew, Arabic, Persian and the symbol blocks render byte-identically on every
platform because the font is compiled into the binary. CJK is the exception and
is called out below rather than glossed over.

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

### The feature corpus

One reference card cannot cover an engine. `cargo run --release -p hikari-rs
--example corpus` checks 16 cases — one per capability, each hashed separately,
so a regression names the feature that moved instead of hiding in one opaque
image. Mismatches print the byte delta, because "the hash differs" does not tell
you whether the change was intended.

A digest proves *stability*, not *correctness*: a blank render is exactly as
stable as a right one and is blessed just as happily. So
`cargo run --release -p hikari-rs --example corpus_ink` additionally asserts
each case contains a measured amount of real content. That is what caught a
radial gradient that shipped as a flat fill.

Re-baseline after an intentional visual change:

```sh
cargo run --release -p hikari-rs --example determinism -- --bless
cargo run --release -p hikari-rs --example corpus -- --bless
```

### Where determinism does not hold

CJK glyphs come from a **system font** by default, so their outlines differ by
platform and the guarantee does not apply to them. Enable the `bundled-cjk`
feature to embed an OFL CJK face and the guarantee extends to it; the cost is a
9,968,236-byte subset, which is why it is opt-in. The determinism card
deliberately covers only the embedded font's scripts for this reason. A committed card with
CJK in it would be green on one machine and red on another, which is worse than
not testing it.

Closing this means bundling a subsetted OFL CJK font so those glyphs come from
the binary too. It is on the [roadmap](#roadmap) and is the honest reason the
headline claim above is scoped rather than absolute.

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

The embedded DejaVu subset is the default, and you can register your own.
Rendering in a brand typeface is the normal case for social images, so
`register_font` is a first-class API rather than a patch.

```rust
use hikari::{register_font, render_png, Node, Style};

let brand = register_font("Inter", &std::fs::read("Inter-Regular.ttf")?)?;
let tree = Node::container(
    Style::row()
        .with_size(1200.0, 630.0)
        .with_background("#0b1020")
        .with_font(brand),
    vec![Node::text("Shipped in your typeface", Style::text(72.0, "#ffffff"))],
);
render_png(&tree, 1200, 630)?;
```

`Style::with_font` takes a `FontId`, is **inherited by descendants**, and
applies to layout, measurement, raster, SVG and PDF alike — so a font set once
on a card root is the font every glyph in it is shaped, measured and drawn with.
An unregistered id degrades to the embedded font rather than failing, since ids
arrive from untrusted JSON in the bindings.

Registration is keyed on the SHA-256 of the font bytes, so registering the same
font twice returns the same id and does no work. That is what makes the lifetime
strategy safe: a server re-registering its brand font on every request leaks
nothing, because the second request hits the content hash.
`registered_font_count()` is exposed for callers generating unbounded distinct
fonts.

In Node: `registerFont(name, bytes)`. In WASM: `register_font(name, bytes)`.

```sh
cargo run -p hikari-rs --example custom_font   # same card, two typefaces
```

`crates/hikari-core/assets/DejaVuSans.ttf` (DejaVu Fonts License, permissive) is
subset at build time and covers Latin, Latin Extended A/B, Greek, Cyrillic,
Hebrew, Arabic, Arabic Presentation Forms A and B, General Punctuation,
super/subscripts, currency, letterlike symbols, arrows, mathematical operators,
geometric shapes, miscellaneous symbols and dingbats. CJK falls back to a system
font at runtime — see [Known limitations](#known-limitations).

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
| `renderPdfSync(pagesJson, w, h)` | `Buffer` |

The tree is the node tree serialized as JSON. Node is externally tagged
(`{ "Text": { … } }`), style fields are `snake_case`, and every field of
`Style` is optional:

```js
const hikari = require('@hikari-rs/node');

const tree = JSON.stringify({
  Container: {
    style: { width: 1200, height: 630, background: '#0b1020' },
    children: [
      { Text: { text: 'Hello from Node', style: { font_size: 72, color: '#ffffff' } } },
    ],
  },
});

const png = hikari.renderPngSync(tree, 1200, 630);
```

Colours accept CSS strings (`"#rrggbb"`, `"#rgb"`, `"#rrggbbaa"`) or an
`{ r, g, b, a }` object, and `background` accepts a bare colour string as a
shorthand for `{ "Solid": … }`. Gradient forms are unchanged. Full types ship
in `npm/index.d.ts`.

PDF is free and needs no key material or environment variable.

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

## Licence

MIT or Apache-2.0, at your option. Every backend — stills, animation and PDF —
is available with no key material, no environment variable and no feature flag.
There is no paid tier.

There was one until v0.20. PDF and animation were gated behind an `ed25519` key
checked offline. That was a commercial-terms mechanism rather than a technical
one, so anyone could remove it in a fork, and selling features behind it cost
more credibility than it earned. The gate and the licence crate are deleted; the
CHANGELOG records the change.

The embedded fonts are OFL-1.1, which is MIT-compatible and permits embedding in
a closed-source product. Licence text ships beside each face in
`crates/hikari-core/assets/fonts/`.

---

## Known limitations

Stated plainly, because a limitations section that hedges is worse than none.

- **Hebrew coverage now comes from a bundled face.** Previously this file
  claimed "Hebrew does not join" and that was **wrong**: modern Hebrew does not
  join — isolated forms are correct typography, and both DejaVu Sans and Noto
  Sans Hebrew cover all 43 Hebrew codepoints including niqqud. A subsetted
  [Noto Sans Hebrew](https://github.com/notofonts/hebrew) is now bundled (57 KB
  after subsetting, OFL) so the coverage is ours rather than the host's.
- **CJK depends on system fonts by default.** Enable the `bundled-cjk` cargo
  feature to embed a subsetted OFL CJK face and make output deterministic
  without a system font. It is opt-in because the subset is 9,968,236 bytes,
  which would dwarf the 2.8 MB WebAssembly build; the source is fetched by
  `scripts/fetch-bundled-fonts.sh cjk` rather than committed. Without it, CJK
  reaches a *system* font, so a render may differ between machines — the
  determinism guarantee below covers every script the embedded faces cover, and
  CJK only when this feature is on.
- **`rustybuzz` and `ttf-parser` are declared unmaintained**
  (RUSTSEC-2026-0206, RUSTSEC-2026-0192). The shaper sitting at the centre of
  this project being unmaintained is the largest outstanding risk. Both
  advisories are ignored in `deny.toml` with written reasons rather than
  tolerated silently. See [Roadmap](#roadmap).
- **CSS coverage is narrow by design.** The `Style` surface is about two dozen
  fields: box model, `display` (`Flex`/`Grid`/`Block`), `dir`, `justify`,
  `align`, `gap`, `grow`, `absolute` with `left`/`top`, `radius`, `border`,
  `background`, `color`, `font_size`, `max_width`, `aspect`, `shadow` (with a
  `kind`), `blend`, `clip_text` and `link`. Absent: `float`,
  `position: fixed`/`sticky`, `transform`, `z-index`, custom properties,
  `@media`, masks, filters, and shorthand properties. This is a layout tree, not
  a CSS engine.
- **PDF is not PDF/A or PDF/UA conformant.** No tagging, no accessibility
  structure, no archival validation.
- **No ecosystem yet.** No star history, no adopters, no battle-tested corpus.
  The determinism work exists precisely to make that corpus possible.
- **The project is days old.** Every commit was made between 2026-09-29 and
  2026-10-02. There are no crates.io or npm downloads, no external
  contributors and no adopters, so there is no track record to judge this by.
  The release timeline in [CHANGELOG.md](CHANGELOG.md) is a claim about what
  changed, not something derivable from history — with a history this short,
  that distinction matters, and a reviewer who counted the commits is right to
  distrust the version numbers.

---

## Roadmap

Ordered by what most limits the project, not by what is easiest.

- [x] **Caller-supplied fonts** via `register_font`, inherited through the tree
      and honoured by layout, raster, SVG and PDF (v0.19)
- [ ] **Typed builder / JSX-style front end.** Today callers hand-build node
      trees or externally tagged JSON. Colour strings and custom fonts both work
      in JSON, but the ergonomics of building a tree by hand are still the
      weakest part of the API.
- [ ] **Migrate off `rustybuzz` and `ttf-parser` to `fontations`**
      (`skrifa` / `write-fonts`). Both crates are unmaintained. This is the
      single largest piece of technical debt and the only reason abandoned code
      sits in the most critical path.
- [x] **Bundle OFL Hebrew and CJK-covering fonts.** A layout-preserving subset
      of Noto Sans Hebrew is always bundled (57 KB), and Noto Sans SC is
      available behind the opt-in `bundled-cjk` feature (9,968,236 B, fetched by
      `scripts/fetch-bundled-fonts.sh`). The larger architectural part was a
      *shaped* font chain: fallback text used to be rasterized by character,
      bypassing the shaper, so no fallback could ever join. Each run is now split
      by coverage and shaped with a face that has the glyph. Found and fixed a
      bug where an unregistered `FontId` rerouted the whole run into the chain.
- [x] **Blend modes and shadow kinds.** Fifteen blend modes (the eleven
      separable ones plus `hue`, `saturation`, `color`, `luminosity`) applied to
      a node's border, background, image, text *and* shadow, plus `InsetTop` and
      `InsetEdge` shadows built by silhouette subtraction rather than as a dark
      overlay. `Normal` is byte-identical to the pre-blend-mode path, verified
      against the determinism card and every corpus digest. Masks and filters
      remain open.
- [x] **Golden-image corpus** — 21 cases, one per capability, in
      `crates/hikari/tests/corpus.sha256`, diffed in CI, plus an ink assertion
      per case so a blank render cannot be blessed. Found a radial gradient that
      shipped as a flat fill because its radius was read as pixels.
- [x] **A CSS and HTML front end**, so a card no longer has to be hand-built as
      a node tree. `css::html_to_tree` takes markup with an inline `<style>`
      block; `css::Stylesheet` plus `css::from_elements` takes a stylesheet and a
      declarative element tree for callers who would rather build elements than
      write markup. Unsupported declarations are **reported, never guessed**, so
      `width:50` is skipped rather than read as 50px. Cascade is source order,
      documented rather than half-implemented. `lightningcss` was measured and
      rejected: it reaches `getrandom` 0.3.4, which is a hard `compile_error!` on
      `wasm32-unknown-unknown`, so it breaks the WebAssembly build outright.
- [ ] **A remote-asset preload helper.** Fetching images is a network call, and
      this library makes none, so this has to be opt-in and explicit rather than
      something a render triggers on its own.
- [ ] **Criterion regression gates.** CI currently smoke-runs benchmarks;
      shared runners are too noisy to assert on timings, so this needs a
      dedicated runner.
- [ ] **Animated WebP** encoding, and PDF/A + PDF/UA conformance.

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
cargo test --workspace --exclude hikari-rs-node   # 87 tests
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

## Licence

MIT or Apache-2.0, at your option — either one is sufficient. Use it
commercially, modify it, redistribute it, embed it in a closed-source product.
The only requirements are the licence text and the notice.

The embedded DejaVu Sans and Noto faces are under OFL-1.1 / the DejaVu Fonts
License, both permissive and both compatible with closed-source redistribution.

