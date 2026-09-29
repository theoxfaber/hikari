# Changelog

All notable changes to Hikari. Versions match the workspace `version` and
the `hikari-rs` facade crate.

## v0.18.0 — correct text shaping, publish-ready, docs site

### Fixed: the embedded font had no shaping tables

The build-time font subset was produced by `allsorts`, which silently drops
`GSUB`, `GPOS` and `GDEF`. The embedded font rendered Latin perfectly while
losing **Arabic joining, kerning and every ligature** — no error, no warning.

`crates/hikari-core/build/subset.rs` replaces it with a layout-preserving
subsetter that keeps the original glyph ids, leaving the gaps empty, so the
layout tables are copied byte for byte and need no rewriting. `post` is
rewritten to format 3.0 (drops 62 KB of unread glyph names) and the legacy
`kern` table is dropped (16 KB, redundant with GPOS). Net 757 KB → 369 KB
*with* shaping intact, versus 289 KB without it.

### Fixed: the painter ignored the shaper

`shape_run` received the correct glyph id from `rustybuzz` and then threw it
away, re-deriving the id from the source character via cmap. The rasterizer
then looked glyphs up by character too. Anything `GSUB` rewrote was therefore
asked for by a character mapping to a different, empty glyph, and painted as a
blank gap. Measurement was correct throughout, which is why no existing test
caught it. The paint path is now keyed on the shaped glyph id; the fallback
font, which never goes through the shaper, is still addressed by character.

Net effect: `office`/`waffle` render their `ffi`/`ffl` ligatures, `AVATAR`
kerns, and Arabic and Persian join into connected letterforms.

### Fixed
- `npm/index.js` did not re-export `renderWebpSync`, so the function existed
  in the native binding but was unreachable through the package entry point.
  `npm/test.mjs` now asserts the wrapper exposes every binding entry point —
  it previously loaded `./hikari-node.node` directly, which is why the gap went
  unnoticed.
- `npm/package.json` declared `"types": "./index.d.ts"` for a file that did not
  exist, sat at version 0.7.0, and pointed `repository` at
  `github.com/example/hikari`. Added a hand-written `index.d.ts` describing the
  real wire format (externally tagged `Node`, `snake_case` style fields,
  `{r,g,b,a}` colours) and fixed the metadata.
- Examples hardcoded `/Users/apple/hikari/...`, so the documented
  `cargo run --example ...` invocations only worked on the original author's
  machine. Outputs now land in the workspace root, overridable with
  `HIKARI_EXAMPLE_OUT`.
- `compute_layout` panicked on taffy errors via `expect`. Layout failures now
  propagate as `Error` — this path runs on untrusted input from the Node and
  WASM bindings, where unwinding across FFI is undefined behaviour.
- Cache mutexes used `lock().expect(...)`, so one panic while the lock was held
  poisoned the cache and turned every later render into a panic. They now
  recover, which is safe for pure memoization.
- `cargo fmt --check` failed, so CI was red on its first step.

### Added
- `font_subset_tests`: shaping compared against the full source font for
  Arabic, Persian, Hebrew, kerning and ligatures, plus outline-presence checks
  over every covered codepoint. The failure this guards against is silent by
  construction.
- `hikari-raster`: a pixel-level guard asserting no hole appears inside
  rendered text. Measured, not guessed — correct rendering leaves at most a
  12 px side-bearing gap, the old code left 47 px.
- Determinism digests moved into `crates/hikari/tests/determinism.sha256` with
  a `--bless` flag, so re-baselining is one command instead of an edit to the
  workflow file.
- CI gates: fmt, clippy, tests, embedded-font shaping as its own job,
  determinism, benchmark smoke, `cargo deny`.

### Corrected
- The roadmap claimed "3x faster warm" than Takumi. That number came from a
  debug build and was wrong; `BENCHMARKS.md` already said Takumi leads. The
  claim is retracted in the README.
- The canonical determinism digests changed (`7f97ec62…` → `ec287845…`,
  21,628 B → 21,649 B) because kerning and ligatures now actually apply. The
  previously published Linux row is marked unverified rather than reprinted —
  we did not reproduce it, so we are not printing it again.
- The README claimed "snapshot + bench gates" while the roadmap listed them as
  unfinished. The README now describes what CI actually does.

### Known gaps
- **Hebrew does not join.** DejaVu Sans has no Hebrew presentation forms, so
  even the unshaded source font renders isolated letters. Font coverage, not a
  subsetting bug; a test asserts the subset matches the source so the two are
  not confused. Needs a bundled Noto Sans Hebrew.
- **CJK uses system fonts.** Explicitly not production-ready, as documented.
- Takumi still leads on warm render, cold start, peak RSS and PNG size.

## v0.17.0 — headlines + links + Linux
- `fit_font_size()` (text-fit) and `balance_text()` headline helpers.
- PDF link annotations with URI actions (`Style::with_link`).
- Full workspace `cargo check` clean for `x86_64-unknown-linux-gnu`.

## v0.16.0 — WebP + prebuilds
- Lossless WebP stills with decode round-trip test (`render_webp`).
- `napi.json` (7 triples) + `release.yml`; `napi build --platform` proven.

## v0.15.0 — nested outlines + attachments
- Outline hierarchy from bookmark levels; embedded file attachments
  (Factur-X-shaped e-invoice XML in the demo).

## v0.14.0 — pagination + outlines + metadata
- Flow pagination (`paginate`, `keep_together`, `repeat_header`);
  `PdfOptions{title, author, outline, paginate}`; XYZ-dest outline items.

## v0.13.0 — build-time font subset
- Embedded font subset at build time (757KB → 289KB, 2,464 glyphs);
  cold render 33ms → 14ms. **The "kerning/ligature tradeoff" this entry refers
  to was not a tradeoff — it was the subsetter dropping the layout tables. See
  v0.18.0.**

## v0.12.0 — APNG + faster encoding
- APNG encoder; glyph bitmap cache; zlib-rs SIMD encode path.

## v0.11.0 — box shadows
- Blurred raster silhouettes + SVG `feDropShadow`.

## v0.10.0 — pagination groundwork, outlines, WASM in Node
- `hikari-wasm` renders in Node via wasm-bindgen glue.

## v0.9.0 — PNG encoder overhaul
- Default + Adaptive + RGB-strip encoding (209KB → 47KB cards);
  trade-off curve measured and published.

## v0.8.0 — font subsetting for PDF
- allsorts subsetting: 387KB → 17KB invoices, spaces extract.

## v0.7.0 — Node.js bindings
- `hikari-node` cdylib: PNG/SVG/PDF from node-tree JSON.

## v0.6.0 — PDF backend
- Identity-CID text + ToUnicode, embedded + fallback fonts, Flate images.

## v0.5.0 — motion + licensing
- GIF encoder, offline `ed25519` Pro keys, pricing.

## v0.4.0 — gradients
- Linear/radial backgrounds in PNG + SVG.

## v0.3.0 — grid + images
- Grid layout, image nodes, borders, word wrap.

## v0.2.0 — shaping
- `rustybuzz` shaping, bidi, CJK fallback, multiline.

## v0.1.0 — MVP
- Flex layout, text, PNG/SVG, hash cache, parallel frames.
