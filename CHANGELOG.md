# Changelog

All notable changes to Hikari. Versions match the workspace `version` and
the `hikari-rs` facade crate.

## v0.20.0 — everything free

### Removed
- **The licence gate.** `render_pdf`, `render_pdf_with`, `render_animation_gif`
  and `render_animation_apng` no longer take a `&License`. The `hikari-license`
  crate (`License`, `Feature`, `Plan`, `LicenseError`) is deleted, along with
  `now_unix`, the `Error::License` variant, `HIKARI_LICENSE` and
  `HIKARI_PUBKEY`. `LICENSE-COMMERCIAL.md` and `PRICING.md` are deleted.
- `docs/pricing.html`, replaced by `docs/licence.html`.

### Why
The gate was an `ed25519` check in readable source, so it was a
commercial-terms mechanism and not a technical one — anyone could delete it in a
fork, and pretending otherwise cost more credibility than the £ it could earn.
For a project whose pitch is that all of it is readable, that was the wrong
trade. Everything is now MIT/Apache-2.0 with no terms attached.

If you bought a Pro key: it is no longer needed and never will be.

### Also in this release
- **A shaped font fallback chain.** Runs are split by glyph coverage and each
  segment is shaped with a face that has the glyph, so fallback text joins
  instead of drawing isolated letters. A bundled OFL Hebrew face ships by
  default; an opt-in `bundled-cjk` feature bundles one for CJK.
- **A golden image corpus** of 20 cases, one per capability, diffed in CI.
- **Blend modes** (15) and **inset shadow kinds**.
- **Three rendering bugs fixed**, all found by the new tests: a radial gradient
  that shipped as a flat fill because its radius was read as pixels; blend modes
  that were silent no-ops for solid fills; inset shadows painted beneath their
  own background. See BENCHMARKS.md for the corrections log.

---

## v0.19.0 — caller-supplied fonts

### Added
- **`register_font(name, bytes) -> FontId`.** Rendering in a brand typeface is
  the normal case for social images, and until now the embedded DejaVu subset was
  the only font the Rust, PDF, SVG, Node and WASM paths would use. Exposed as
  `registerFont` in Node and `register_font` in WASM.
- `Style::with_font` / `Style::font`, **inherited by descendants**. Setting a
  font once on a card root applies to every text run inside it; paint never
  walks ancestors because layout writes the resolved id onto each node.
- `font_entry`, `registered_font_count`, `builtin_bytes` and `BUILTIN_FONT`
  re-exported from the facade.
- `cargo run --example custom_font` renders the same card in two typefaces.

### Design notes
- Registration is keyed on the SHA-256 of the font bytes, so the same font
  registered twice returns the same id and does no work. That is what makes the
  leak-once-per-distinct-font lifetime strategy safe for a server that
  re-registers its brand font on every request.
- The glyph cache is keyed on `(font id, glyph id)` rather than the source
  character. The same glyph id means different glyphs in different fonts, so the
  character was never a sufficient key once more than one font could be used.
- `shape_text`, `measure_text`, `wrap_text`, `fit_font_size` and `balance_text`
  all take a `FontId`. This is a breaking API change, and deliberately so: a
  default argument would have let the paint path and the measure path disagree
  about the typeface, which is the exact class of bug the glyph-id fix in v0.18
  removed.
- The PDF backend holds a dynamic font list keyed by `FontId` and emits one
  resource block per font used, instead of a fixed primary/fallback pair.
  Caller fonts take resource tags from F3, leaving F1 for the embedded font and
  F2 for the system fallback, so a caller font can never collide with the
  fallback. Covered by three tests: a single registered font, two fonts in one
  document, and an unregistered id (which errors rather than silently emitting
  the wrong typeface — the PDF writer is the one backend that must not degrade,
  because a document in the wrong font is a correctness bug someone ships).

### Behaviour worth knowing
- An unregistered font id degrades to the embedded font rather than failing.
  Font ids arrive from untrusted JSON in the bindings, and a tree naming a font
  the caller forgot to register should still produce an image.

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
- The CJK paint test asserted a pixel budget that only holds when a system CJK
  font is installed, so it failed on a bare Linux CI runner while passing on
  any Mac. The assertion is now conditional on a fallback font actually being
  present, and CI installs `fonts-noto-cjk` so the fallback path stays covered
  instead of being skipped everywhere.
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
- Colours now deserialize from CSS strings as well as `{r,g,b,a}` objects, and
  `background` accepts a bare colour string as shorthand for `{"Solid": …}`.
  The Node and WASM APIs previously required a four-key object literal for
  every colour, which made the most common field in the API the most tedious
  one to write. Gradient forms and the existing tagged solid form are
  unchanged, so existing input still parses.
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
- The roadmap claimed a large warm-render speed advantage. That number came
  from a debug build and was wrong — this project's own `BENCHMARKS.md`
  already contradicted it. The claim is retracted.
- The canonical determinism digests changed (`7f97ec62…` → `ec287845…`,
  21,628 B → 21,649 B) because kerning and ligatures now actually apply. The
  previously published Linux row is marked unverified rather than reprinted —
  we did not reproduce it, so we are not printing it again.
- The README claimed "snapshot + bench gates" while the roadmap listed them as
  unfinished. The README now describes what CI actually does.

### Known gaps
- **`rustybuzz` and `ttf-parser` are both declared unmaintained**
  (RUSTSEC-2026-0206, RUSTSEC-2026-0192). The shaper sits in the most critical
  path in the project, so this is the top item on the roadmap. `cargo deny`
  ignores both advisories with a written reason rather than tolerating them
  silently; the destination is Google Fonts' `fontations`.
- **Hebrew does not join.** DejaVu Sans has no Hebrew presentation forms, so
  even the unshaded source font renders isolated letters. Font coverage, not a
  subsetting bug; a test asserts the subset matches the source so the two are
  not confused. Needs a bundled Noto Sans Hebrew.
- **CJK uses system fonts.** Explicitly not production-ready, as documented.
- Cold start (14.3 ms) is well above warm render (3.69 ms) and is dominated by
  first glyph rasterization. Unaddressed.

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
