# Construction plan

How this project gets built, and what "done" means at each step. Kept in the
repository so the reasoning is auditable rather than reconstructed from commit
messages later.

Objective: be the fastest, most correct and most predictable way to turn a
layout tree into pixels and documents, measured and published every release.

## What "good" means here

A step is done when all of the following are true. Not "it works on my machine":

- `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo deny check` are green.
- The capability has a test that fails when the capability regresses.
- Any number quoted about it is measured, and the machine is named.
- Known limitations are written down where a user will find them.

Anything short of that is a prototype. Several things in this project were
prototypes for a long time and said so.

## Status

Steps 1–5 and 7 are complete. Step 6 (PDF) is complete for its stated scope.
Step 9 (gates) is partially complete; the benchmark regression gate is
outstanding because shared CI runners are too noisy to assert on timings.

## Steps

### 1. Shaping-first text — done (v0.2)
Upstream `rustybuzz` + `unicode-bidi`, no forks. Single embedded font as the
source of truth for shaping, raster and PDF, so all three agree by
construction.

*Superseded in v0.18:* the embedded subset was dropping `GSUB`/`GPOS`/`GDEF`,
and the paint path was looking glyphs up by character rather than by the
shaper's glyph id. Both fixed; see the v0.18 changelog entry.

### 2. Layout coverage — done (v0.3)
Grid, block display, image nodes with aspect back-fill,
margin/border/inset/absolute, greedy word wrap.

### 3. Paint breadth — done (v0.4, v0.11, v0.12)
Gradients, box shadows, glyph bitmap cache, APNG.

### 4. Motion and licensing — done (v0.5)
GIF encoder, parallel RGBA frames via `rayon`, offline `ed25519` Pro keys.

### 5. Documents — done (v0.6 through v0.17)
Identity-CID PDF text with ToUnicode, embedded and fallback fonts, Flate
images, pagination, outlines, link annotations, file attachments, metadata.

Scope boundary: not PDF/A or PDF/UA conformant. No tagging, no accessibility
structure.

### 6. Bindings — done (v0.7, v0.10, v0.18)
Node.js via `napi`, WASM via `wasm-bindgen`, both free. Hand-written
`index.d.ts` describing the real wire format, and a test that the package entry
point re-exports every binding symbol.

### 7. Font subsetting — done (v0.8, v0.13), reworked (v0.18)
Per-document PDF subsetting, then a build-time embedded subset. The v0.18
rework replaced a generic subsetter with a layout-preserving one; the earlier
version silently dropped shaping.

### 8. Measurement — done, and treated as a feature
`BENCHMARKS.md` is published per release with a corrections log. A benchmark
document that has never been wrong has probably not been measuring anything,
and this one has been wrong twice in public.

### 9. Gates — partial
Done: formatting, lints, tests, embedded-font shaping, determinism digests,
benchmark smoke, supply chain, Node bindings.

Outstanding: a golden-image corpus beyond the single determinism card, a fuzz
corpus, and a criterion regression gate on a dedicated runner.

### 10. Unmaintained dependencies — open, highest priority
`rustybuzz` and `ttf-parser` are both declared unmaintained. Destination is
Google Fonts' `fontations` (`skrifa` / `write-fonts`). Until this is done, the
project's most critical path depends on abandoned code, which is the single
largest risk it carries.

## Sequencing

Serial by default. Shaping had to precede paint, layout had to precede
documents, and font correctness had to precede any performance claim — a
benchmark over incorrectly rendered text measures nothing.

No step gets its exit criteria relaxed to make a schedule. When a step cannot
be finished, it gets written down as a known limitation instead, which is why
that section of the README is longer than most projects'.
