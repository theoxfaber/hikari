# Hikari — images from layout trees, no browser

`hikari` is a minimal, fast alternative to `takumi` for the 90% case:
OG/social images (1200x630) from a JSON-serializable node tree.

## Why better than Takumi (v0.1 wedge)

- **One API**, not 7 packages: `hikari::render_png()` / `render_svg()`.
- **Upstream deps only**: `taffy` + `cosmic-text` + `tiny-skia`. No font forks.
- **Content-hash cache**: identical subtrees skip layout+paint (`sha2`).
- **Parallel animations**: frames render with `rayon`.
- **Size-first**: single `[profile.wasm-release] opt-level=s`, feature flags. No per-crate opt-level whack-a-mole.
- **Strict by default**: `clippy::all = deny`, `unsafe_code = forbid`, and CI
  gates on fmt, clippy, the full test suite, embedded-font shaping, benchmark
  compilation, `cargo deny`, and cross-platform byte determinism.
- **Correct shaping**: the embedded font keeps `GSUB`/`GPOS`/`GDEF`, so Arabic
  and Persian join, kerning applies and ligatures form. A conventional
  subsetter drops all three and renders Latin while silently losing the rest.

## Scope (v0.18)

Flex/grid/block containers, shaped + wrapped + balanced + fitted text,
decoded images (cover/contain/fill, rounded clip, decode cache), solid +
linear + radial backgrounds, background-clip:text, box shadows, borders,
margins, absolute positioning, animated GIF/APNG (Pro), lossless WebP,
PDF documents with selectable text, outlines, links, attachments and
pagination (Pro), Node.js + WASM bindings (free).
[Docs + live playground](https://theoxfaber.github.io/hikari/) ·
[Benchmarks](BENCHMARKS.md) · [Competition](COMPETITION.md) ·
[Changelog](CHANGELOG.md).

Run `cargo run -p hikari-rs --example og` (gradient banner),
`--example card` (grid + image), `--example motion` (Pro GIF/APNG),
`--example invoice` (Pro flowing PDF) and `--example determinism`
(cross-platform byte hashes) for proof; `node npm/test.mjs` exercises
the Node binding.

## Quick start

```sh
cargo add hikari-rs
```

```rust
use hikari::{Node, Style, render_png};

let tree = Node::container(
    Style::row().with_size(1200.0, 630.0).with_background("#0b1020"),
    vec![Node::text("Hello from Hikari", Style::text(72.0, "#ffffff"))],
);
let png = render_png(&tree, 1200, 630)?;
std::fs::write("og.png", png)?;
```

## Layout

`hikari-core` converts `Node` -> `taffy::TaffyTree`, computes layout once,
then `hikari-raster` paints with `tiny-skia` and encodes PNG via `image`.
`hikari-svg` emits equivalent SVG without rasterization.

## Test / bench

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo bench
```

## Fonts

`hikari-core/assets/DejaVuSans.ttf` (DejaVu Fonts License, permissive) is
subset at build time to Latin, Greek, Cyrillic, Hebrew, Arabic, punctuation,
currency, arrows, math and symbols — the single source of truth for shaping,
raster and PDF, so all three agree. CJK ideographs fall back at runtime to a
system font (Arial Unicode on macOS, Noto CJK on Linux); production
deployments should ship a subsetted OFL CJK font instead of relying on system
paths.

### The subset preserves shaping

Most subsetters renumber glyphs into a dense range. That silently invalidates
every glyph id inside `GSUB`/`GPOS`/`GDEF`, and most of them then drop those
tables outright. The result is a font that renders Latin perfectly and quietly
loses Arabic joining, kerning and ligatures — no error, no warning, just
slightly wrong text.

So `crates/hikari-core/build/subset.rs` keeps the **original glyph ids** and
leaves the gaps empty. `GSUB`, `GPOS` and `GDEF` are then copied byte for byte
and need no rewriting at all, which means there is no field that can be
forgotten. The cost is a sparse id space: about 49 KB of `loca`/`hmtx`
padding. The legacy `kern` table is dropped (16 KB) because every modern
shaper prefers `GPOS`, and `post` is rewritten to format 3.0, discarding 62 KB
of glyph names nothing reads.

Net: 757 KB → 369 KB, with kerning, ligatures and Arabic joining intact.

Two things this does **not** fix, stated plainly:

- **Hebrew does not join.** DejaVu Sans itself has no Hebrew presentation
  forms, so even the unshaded source font renders `שלום` with isolated letters.
  This is font coverage, not a subsetting bug; a test asserts the subset
  matches the source exactly so the two are never confused again. Fixing it
  means bundling a font with Hebrew coverage (Noto Sans Hebrew).
- **CJK depends on system fonts**, as above.

`crates/hikari-core/src/font_subset_tests.rs` compares shaping against the
full source font for Arabic, Persian, Hebrew, kerning and ligatures, and
`hikari-raster` has a pixel-level guard that fails if a shaped glyph fails to
reach the rasterizer. Both exist because this failure mode is invisible.

## Node.js

```sh
cargo build --release -p hikari-rs-node
cp target/release/libhikari_node.dylib npm/hikari-node.node
node npm/test.mjs
```

`renderPngSync(treeJson, w, h)`, `renderSvgSync(treeJson, w, h)`,
`renderPdfSync(pagesJson, w, h)` (Pro — reads `HIKARI_LICENSE` /
`HIKARI_PUBKEY`). Styles accept partial JSON (every field defaults).

## WebAssembly

```sh
rustup target add wasm32-unknown-unknown
cargo build -p hikari-rs-wasm --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/debug/hikari_wasm.wasm \
  --out-dir wasm --target nodejs
node wasm/test.mjs
```

`render_png(treeJson, w, h)`, `render_svg(treeJson, w, h)` — PNG/SVG for
edge runtimes and browsers. Pure-Rust dependency tree (no C zlib anywhere:
`flate2/rust_backend`, allsorts without `flate2_zlib`). Debug build renders
the reference card in ~630ms; release + `wasm-opt` is the tracked follow-up
with a size budget.

## Monetization

Open core stays MIT/Apache-2.0. Pro (motion now, PDF next) is
source-available under `LICENSE-COMMERCIAL.md`, keyed by offline
`ed25519` licenses (`hikari-license`). See `PRICING.md` for tiers.
Enforcement is honest: a key check plus commercial terms, no DRM theater.

## Roadmap to beat Takumi fully

- [x] `rustybuzz` shaping + bidi + CJK fallback (v0.2: runtime system fallback)
- [x] grid + image nodes + borders/margins/absolute + word wrap (v0.3)
- [x] solid + linear + radial backgrounds, PNG + SVG (v0.4)
- [x] animated GIF, parallel RGBA frames, offline license keys (v0.5 Pro)
- [x] PDF backend: selectable text, embedded fonts, images, 2-page proof (v0.6 Pro)
- [x] Measured bake-off vs Takumi, honest gaps logged (v0.7). Correction: an
      earlier version of this line claimed "3x faster warm". That number came
      from a debug build and was wrong. Takumi leads on warm render, cold start,
      peak RSS and PNG size; see [BENCHMARKS.md](BENCHMARKS.md) for the
      measurements and what we still lose.
- [x] Font subsetting: 387KB → 17KB invoices, spaces extract (v0.8 Pro)
- [x] PNG encoder overhaul: 118KB → 43KB cards, trade-off curve measured (v0.9)
- [x] Competition mapped (COMPETITION.md), bake-off published (BENCHMARKS.md)
- [x] Node.js bindings: PNG/SVG/PDF from JSON, partial styles, npm scaffold (v0.7)
- [x] Pagination, outlines, metadata; WASM renders in Node (v0.10)
- [x] Box shadows: blurred raster silhouettes + SVG feDropShadow (v0.11)
- [x] APNG encoder, glyph cache, zlib-rs encode path (v0.12)
- [x] Build-time font subset: cold 33→14ms, binary/WASM −650KB (v0.13)
- [x] text-fit + balanced headlines, PDF link annotations (v0.14)
- [x] Nested outlines, file attachments incl. e-invoice XML (v0.15)
- [x] Lossless WebP stills + napi-CLI prebuild matrix proven (v0.16)
- [x] text-fit + balance, clip:text, Linux target check (v0.17)
- [x] Cross-platform byte determinism proven, `hikari-rs-*` publish names, docs site (v0.18)
- [x] Layout-preserving font subset (`GSUB`/`GPOS`/`GDEF` retained) and paint
      path keyed on shaped glyph ids — Arabic joining, kerning and ligatures
      actually render (v0.18)
- [x] CI gates: fmt, clippy, tests, embedded-font shaping, determinism digests,
      bench smoke, `cargo deny` (v0.18)
- [ ] masks/filters/shadows/blend
- [ ] `lightningcss` style parsing, remote asset preload helper
- [ ] WebP/GIF animation encoding, NAPI + WASM bindings
- [ ] **Migrate off `rustybuzz` and `ttf-parser`** — both are now declared
      unmaintained (RUSTSEC-2026-0206, RUSTSEC-2026-0192). The destination is
      Google Fonts' `fontations` (`skrifa` / `write-fonts`). This is the largest
      outstanding piece of technical debt and the only reason the project
      depends on abandoned code in its most critical path.
- [ ] Bundled OFL CJK + Hebrew-covering fonts, replacing system-font fallback
- [ ] Golden-image corpus beyond the single determinism card
- [ ] `criterion` regression *gates* (CI currently smoke-runs benches; shared
      runners are too noisy to assert on timings)
