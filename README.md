# Hikari — images from layout trees, no browser

`hikari` is a minimal, fast alternative to `takumi` for the 90% case:
OG/social images (1200x630) from a JSON-serializable node tree.

## Why better than Takumi (v0.1 wedge)

- **One API**, not 7 packages: `hikari::render_png()` / `render_svg()`.
- **Upstream deps only**: `taffy` + `cosmic-text` + `tiny-skia`. No font forks.
- **Content-hash cache**: identical subtrees skip layout+paint (`sha2`).
- **Parallel animations**: frames render with `rayon`.
- **Size-first**: single `[profile.wasm-release] opt-level=s`, feature flags. No per-crate opt-level whack-a-mole.
- **Strict by default**: `clippy::all = deny`, `unsafe_code = forbid`, snapshot + bench gates.

## Scope (intentional)

v0.4 renders: flex/grid/block containers, shaped + wrapped text, decoded
images (cover/contain/fill, rounded clip, decode cache), solid + linear +
radial backgrounds, background-clip:text, box shadows, borders, margins, absolute positioning, animated GIF
(Pro), PDF documents with selectable text (Pro), and Node.js bindings
(free). No masks/filters/shadows yet. Run `cargo run -p hikari
--example og` (gradient banner), `--example card` (grid + image),
`--example motion` (Pro GIF), and `--example invoice` (Pro 2-page PDF)
for proof; `node npm/test.mjs` exercises the Node binding.
Field guide: `COMPETITION.md` (rivals + positioning),
`BENCHMARKS.md` (measured numbers, updated per release).

## Quick start

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
subset at build time (`build.rs`, allsorts) to Latin, Greek, Cyrillic,
Hebrew, Arabic, punctuation, currency, arrows, math, and symbols — the
single source of truth for shaping, raster, and PDF, so all three agree.
CJK ideographs fall back at runtime to a system font (Arial Unicode on
macOS, Noto CJK on Linux). Tradeoff: the subset drops GSUB/GPOS/GDEF
(kerning, ligatures, Arabic joining — joining was already isolated-form
through the rasterizer); production deployments should ship a subsetted
OFL CJK font instead of relying on system paths.

## Node.js

```sh
cargo build --release -p hikari-node
cp target/release/libhikari_node.dylib npm/hikari-node.node
node npm/test.mjs
```

`renderPngSync(treeJson, w, h)`, `renderSvgSync(treeJson, w, h)`,
`renderPdfSync(pagesJson, w, h)` (Pro — reads `HIKARI_LICENSE` /
`HIKARI_PUBKEY`). Styles accept partial JSON (every field defaults).

## WebAssembly

```sh
rustup target add wasm32-unknown-unknown
cargo build -p hikari-wasm --target wasm32-unknown-unknown
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
- [x] Measured bake-off vs Takumi: 3x faster warm, honest gaps logged (v0.7)
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
- [ ] masks/filters/shadows/blend, `text-fit`, `balance`/`pretty`
- [ ] `lightningcss` style parsing, remote asset preload helper
- [ ] WebP/GIF animation encoding, NAPI + WASM bindings
- [ ] snapshot corpus + `criterion` regression gates in CI
