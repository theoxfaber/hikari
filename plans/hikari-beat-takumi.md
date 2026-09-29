# Hikari — beat Takumi in every field (construction plan)

Objective: match + exceed Takumi (images, SVG, animations, PDF, CSS, runtimes, DX, docs, perf) with a 10/10 repo.

Status: v0.1 done (flex containers + Latin text -> PNG/SVG, hash cache, parallel frames, clippy/test green).
v0.1 does NOT beat Takumi everywhere. Gaps below are load-bearing.

## Exit criteria for 10/10

- Feature parity matrix (images/SVG/anim/PDF/CSS/runtimes) all checked with snapshot tests.
- `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` green.
- `cargo deny check`, bench regression gate, fuzz corpus builds.
- WASM size budget + Node prebuild matrix documented with measured numbers, not claims.

## Steps (serial unless noted)

### 1. Shaping-first text — DONE in v0.2 (runtime system CJK fallback; bundled subset + text-fit/balance remain)
- Upstream `rustybuzz` + `unicode-bidi`, no forks. Single embedded font source of truth; measured layout.
- Verified: `cargo test -p hikari-core` (7 tests) + Arabic/Hebrew/CJK/multiline PNG tests + `i18n.png` visual proof.

### 2. Layout coverage — DONE in v0.3
- Grid (`Style::grid`), block display, image nodes with aspect back-fill,
  margin/border/inset/absolute, greedy word wrap (`max_width`).
- Verified: 12 core tests (grid tracks, wrap, intrinsic, absolute) + card proof.

### 3. Asset pipeline — DONE in v0.3 (local decode)
- PNG/JPEG decode with content-hash `Pixmap` cache (`image_cache_stats`),
  SVG `<image>` data-URI embedding. Remote preload helper still open.

### 4. Paint coverage — v0.11 (gradients + shadows done)
- Linear/radial gradients with CSS angles, PNG + SVG, tested + visual proof.
- Box shadows: triple-box-blur silhouettes in raster, `feDropShadow` in SVG
  (sharp-offset fallback without a background). PDF skips soft shadows
  (transparency groups; consistent with the PDF/A story).

### 5. Animations — v0.16 (GIF + APNG + WebP stills)
- Parallel RGBA frames, infinite-loop GIF + APNG encoders, lossless WebP
  stills with decode round-trip test, decoder checks,
  `motion.gif` + `motion.apng.png` proofs (3 frames each, verified externally).
- Glyph bitmap cache (bounded, hit-counted) + zlib-rs SIMD encode path.
- Pro-gated via offline `ed25519` keys; `PRICING.md` + `LICENSE-COMMERCIAL.md` ship.
- Still open: animated WebP (needs libwebp C, breaks pure-Rust/WASM story).

### 6. PDF backend — v0.15 (flow + outline + metadata + links + files)
- Identity-CID text + ToUnicode (pypdf extracts every word, spaces intact),
  embedded primary + on-demand fallback fonts, Flate images with soft masks,
  borders, rounded rects, link annotations with URI actions, embedded file
  attachments (Factur-X-shaped e-invoice XML), nested outlines from
  bookmark levels, `invoice.pdf` proof (30-row flowing invoice, repeating
  header, nested outline, metadata, pay link, XML attachment — **18KB**).
- Flow pagination (`paginate` + `keep_together` + `repeat_header`),
  `PdfOptions{title, author, outline, paginate, attachments}`,
  outline items with XYZ dests, Info dict always stamped.
- Still open: PDF/A validation.

### 7. Runtimes — v0.17 (Node.js + WASM + prebuild matrix + Linux)
- `hikari-node` cdylib: PNG/SVG/WebP/PDF from node-tree JSON, partial styles
  via serde defaults, env-key Pro gating, `npm/` scaffold + passing `test.mjs`
  with pixel-verified output.
- Prebuild matrix wired: `napi.json` (7 triples) + `release.yml` CI;
  `napi build --platform` proven locally producing a loadable
  `hikari-node.darwin-arm64.node` with all exports. Generated JS
  loader/d.ts emission still to verify (CLI emitted empty d.ts locally).
- `hikari-wasm`: full facade compiles for `wasm32-unknown-unknown`
  (pure-Rust tree forced: `flate2/rust_backend`, allsorts without C zlib),
  renders the reference card in Node via wasm-bindgen glue (debug: ~630ms).
- Full workspace `cargo check` clean for `x86_64-unknown-linux-gnu`;
  no Unix-only APIs (platform font paths degrade gracefully).
- Still open: release + `wasm-opt` size budget, CI runs on win/linux.

### 8. DX + docs (parallel with 7)
- Single JS API, `ImageResponse` compat, `Renderer` reuse, Tailwind v4 subset, migration guides (satori/next-og, Puppeteer, react-pdf, pdfkit), invoice benchmarks with versions + hardware.
- Verify: example apps build; docs link-check.
- Exit: docs + examples green.

### 9. 10/10 gates
- Snapshot corpus expansion, `cargo-fuzz` harness, criterion regression in CI, `cargo deny`, SBOM/attestations.
- Verify: full CI green on clean checkout.
- Exit: all gates enforced, no manual steps.

## Research log (2026-09-29)

- Competition mapped in `COMPETITION.md` (typst 56k★, satori 14k★,
  resvg 4k★, takumi 3k★, printpdf 1k★). Positioning: single engine for
  PNG+SVG+GIF+PDF in pure Rust; do not fight Typst on typesetting.
- Bake-off in `BENCHMARKS.md`: release-vs-release, corrected an earlier
  debug-build comparison error openly.
- PNG encoder: PIL-reference experiment proved settings dominate size
  (209KB→48KB on the same pixels). Shipped Default+Adaptive+RGB-strip;
  full filter search (oxipng `-o2` style) is the tracked follow-up with a
  measured 27KB target. `Best` rejected by measurement (300ms).
- Subsetting: allsorts works but needs `CmapTarget::Unicode`
  (`Unrestricted` emits a cmap ttf-parser can't read back).

## Ordering
1 -> (2, 3 parallel) -> 4 -> 5 -> 6 -> (7, 8 parallel) -> 9.

## Honest estimate
Steps 1-5: weeks. Step 6 (PDF): 4-6 weeks alone. Total to credible "beats everywhere": 5-8 engineer-months vs Takumi's 3,275-commit head start. Any 10/10 claim before step 9 is unverified.
