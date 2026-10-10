# Changelog

All notable changes to Hikari. Versions match the workspace `version` and
the `hikari-rs` facade crate.

## v0.21.2 — fontdue replaced by skrifa + tiny-skia

### Changed
- **Glyph rasterization no longer uses `fontdue`.** Outlines come from `skrifa`
  and coverage from `tiny-skia`, which was already a dependency for the rest of
  the raster backend. `fontdue` is gone from the tree entirely.
- This removes the last non-PDF path to `ttf-parser` (RUSTSEC-2026-0192), and as
  a side effect the tree now carries **one** font parser — `read-fonts`, reached
  through both `harfrust` for shaping and `skrifa` for outlines — where it
  previously carried `ttf-parser` twice, via `rustybuzz` and `fontdue`.

  `hikari-core` no longer touches `ttf-parser` at all; its subset tests moved to
  `skrifa`. One direct user remains, `hikari-pdf`, which is now the only thing
  standing between this project and a clean advisory report.

### The bounds convention, which is the whole subtlety

A rasterized glyph is an alpha bitmap plus metrics for placing it, and those
metrics are reported relative to the pen. The vertical one is counter-intuitive:
`ymin` is the glyph's **lowest** point, y-up from the baseline, and `draw_text`
places the top edge at `baseline - ymin - height`. Reporting the *top* edge as
`ymin` shifts every glyph down by exactly its own height — which still produces a
plausible image with a plausible ink count, and would have been re-blessed
happily as "expected antialiasing differences".

It is pinned by seven tests in `hikari-raster/src/glyph.rs` instead, including
one that asserts a glyph sitting on the baseline lands on the baseline rather
than one height below it.

### Verified equivalent, not assumed

A side-by-side comparison against `fontdue` before anything was re-blessed:

* **Bitmap dimensions identical** at 16, 32 and 72px for straight, round,
  descender and apex glyphs.
* **Bounds identical to within 0.01px** (`xmin 1.58` vs `1.57`, `ymin -0.23` vs
  `-0.23`, height `11.67` vs `11.66`).
* **Advances identical** to two decimals.
* **Coverage agreement 90–99%**, rising with size — the residue is antialiasing
  edge weighting, which is expected between two rasterizers.
* A rendered "Hi" at 48px inks **665 pixels in both**, with an identical bounding
  box of (180,77)–(220,113).

13 of 21 corpus digests changed, every one *smaller*, consistent with slightly
tighter edge coverage compressing better. All 20 ink floors still pass.

### Fixed
- **`png_signature_and_size` asserted the wrong thing.** It checked
  `bytes.len() > 1000` — a claim about the PNG encoder dressed up as a claim
  about the renderer. The rasterizer swap moved it to 971 bytes without changing
  a pixel, and the test failed on correct output. It now asserts ink count and
  ink bounding box, which is what would actually catch a glyph drawn in the wrong
  place.

### Performance
**3.52ms against 4.06ms** on the 1200x630 banner — faster than the `fontdue`
path it replaces, and 20% faster than the pre-`harfrust` baseline of 4.38ms.

### Known costs
- **WASM grew to 3.51 MB**, from 3.29 MB, as `skrifa` replaces `fontdue`. Net
  across both migrations that is 2.95 MB → 3.51 MB: +560 KB for a maintained
  shaper and a maintained rasterizer with no duplicated parser.
- `skrifa` 0.33 pins `read-fonts` 0.31 while `harfrust` pins 0.45, which put two
  copies of the font parser in the binary. Caught by `cargo deny check bans` and
  fixed by moving to `skrifa` 0.48, which aligns on `read-fonts` 0.45. Worth
  stating plainly: this was only caught because duplicate versions were already
  configured to be reported.

174 tests pass, clippy clean, fmt clean, `cargo deny` clean.

---

## v0.21.1 — shaper moved off rustybuzz onto harfrust

### Changed
- **`rustybuzz` → `harfrust` for all shaping.** `rustybuzz` is declared
  unmaintained (RUSTSEC-2026-0206) and sat in the most critical path in the
  project. That advisory is now gone from the dependency tree.
- Shaping now goes through `harfrust`, the HarfBuzz project's own maintained
  fork. It began as a fork of `rustybuzz` and swapped `ttf-parser` for
  `read-fonts`, which is why the two produce identical output for the same font
  while depending on maintained code.
- `ttf-parser` no longer reaches the shaping or coverage path. The cmap lookup
  moved to `read-fonts`, with a test sweeping the whole BMP to confirm it agrees
  with `ttf-parser` on every character except U+FFFF.

### Fixed
- **Script properties are now set explicitly before shaping.** The buffer was
  being given a `Direction` but never a `Script`, so the shaper picked a script
  set itself. `rustybuzz` happened to choose the Latin one, which gave correct
  ligatures and kerning; `harfrust` chooses differently, and with only a
  direction set it dropped `ffi` into three separate letters and lost kerning
  (a measured 390.9px → 424.8px on a kerning sample). Segment properties are now
  guessed first and only the direction overridden, which is both correct and
  independent of which shaper is underneath.

  This was a latent bug, not a migration artifact: the old code was relying on a
  shaper's guess rather than stating what it meant.

  Known limit: the script is inferred from a run's first strong character, and
  bidi runs are not guaranteed script-homogeneous, so a run mixing Latin into RTL
  text shapes under one script. Splitting runs by script as well as direction is
  the fix and needs script data this crate does not carry.

### Performance
- `harfrust::ShaperFont::new` re-resolves the shaper's layout tables, which cost
  **16% of the render benchmark** when constructed per shaped run (4.38ms →
  5.10ms on the 1200x630 banner). The shaper is now cached per thread per font.
  The cache cannot live in the registry — `ShaperFont` holds interior `OnceCell`s
  and a `&dyn FontFuncs`, so it is neither `Sync` nor meaningful across calls —
  and a thread-local is exactly its natural lifetime.

  Net effect: **4.06ms against a 4.38ms pre-migration baseline**, so the
  migration is slightly faster than what it replaced.

### Tests
- 7 new tests in `shaping_path_tests`, all going through `shape_text` — the
  entry point the renderer actually calls. This distinction is the point: the
  pre-existing Arabic joining test shaped with `guess_segment_properties`
  instead, a different path from production, which is why Arabic joining was
  never verified at all. Arabic joining, Latin ligatures, and a Hebrew control
  case are now checked against per-character shaping of the same text, which
  removes context by construction rather than by asking the shaper to disable
  features (an empty feature slice *adds* nothing, so that reference silently
  returned the joined answer and made the first version of the test vacuous).

- 167 tests pass, clippy clean, fmt clean. The determinism card and all 21
  corpus digests are **unchanged** — the migration is byte-identical in output,
  which is the strongest evidence available that `harfrust` and `rustybuzz`
  agree on this corpus.

### Still open
- **RUSTSEC-2026-0192 (`ttf-parser`) remains**, now reached only through
  `hikari-pdf` and `fontdue`, the glyph rasterizer. It cannot be cleared until
  `fontdue` is replaced. Ignored in `deny.toml` with that reason written down.
- WASM grew 2.95 MB → 3.29 MB, measured unoptimised. The trade is a maintained
  shaper for the size; `wasm-opt` should recover some. No size budget is
  enforced in CI, so the figure can drift unnoticed.

---

## v0.21.0 — CSS and HTML front end

### Added
- **`hikari::css`.** `html_to_tree` parses markup with an inline `<style>`
  block into a node tree; `Stylesheet` + `from_elements` do the same for a
  stylesheet and a declarative `Element` tree. A card no longer has to be
  hand-built as a `Node`.
- Supported CSS: box model, `display`, `flex-direction`, `justify-content`,
  `align-items`, `gap`, colours in every common form (`#rgb`/`#rgba`/
  `#rrggbb`/`#rrggbbaa`, `rgb()`, `rgba()` including the modern
  `rgb(1 2 3 / 50%)` form, and ~50 named colours), linear and radial
  gradients with degree/`grad`/`rad`/`turn` and `to <side>` forms,
  `box-shadow` including `inset`, `mix-blend-mode`, `background-clip: text`,
  position/left/top, `aspect-ratio`, `grid-template-columns: repeat(n, 1fr)`,
  and text properties with CSS inheritance.
- `ParseReport` lists every declaration that was skipped and why. Unsupported
  CSS is **never guessed**: `width:50` is rejected rather than read as 50px,
  because a plausible wrong value is worse than an absent one.

### Why not `lightningcss`
Measured, not assumed. It is a much better parser, but it pulls
`dashmap` → `ahash` → `getrandom` 0.3.4, and that version is a hard
`compile_error!` on `wasm32-unknown-unknown` unless both a `--cfg` and a feature
flag are set — neither reachable through its dependency graph. Adding it breaks
the WebAssembly build outright. Its default features also carry a bundler this
project has no use for. The replacement is ordinary Rust with no new
dependencies, which keeps the "boring dependencies" claim honest.

### Fixed
- **A `Node::Text` tree root rendered nothing.** Not a CSS bug: the layout
  produced a correct 306x90 box for a root text node and the painter emitted
  zero ink. Found while testing the CSS front end, and left fixed because it is
  a real bug for anyone passing a bare `Node::text(...)` to `render_png`.
- **CSS text inheritance.** `font-size` and `color` were applied to the
  containing box rather than the glyphs, so `<span class="t">Hello</span>`
  rendered at the default size. A boxless element now collapses into a text
  node; one with a box of its own keeps it.
- **Six parser bugs found by the parser's own tests**, each of which looked like
  working code: declaration values were concatenated with no separator, so every
  gradient failed to parse; the closing brace of a rule was consumed twice,
  which silently discarded every rule after the first; `to right` collapsed to
  `toright`; `a:hover` was reported as `a: hover`; `rgb()` discarded its alpha;
  and gradient stops split on spaces rather than commas.

### Tests
36 new: 29 unit tests asserting on decoded style values, and 7 end-to-end tests
that render and count ink. The headline one renders the same card from CSS and
by hand and asserts the bytes are **identical** — if they ever diverge, the CSS
layer has become a second renderer rather than a convenience. The corpus grew to
21 cases with a `css-html-card` digest, so a parser regression is gated in CI.

160 tests pass, clippy clean, fmt clean, the determinism card and all 21 corpus
digests unchanged except the one new case.

---

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
