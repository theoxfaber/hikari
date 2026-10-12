# Changelog

All notable changes to Hikari. Versions match the workspace `version` and
the `hikari-rs` facade crate.

## v0.21.10 — PDF/A-1b conformance, verified rather than asserted

v0.21.9 shipped PDF/A-1b support with a CI job meant to gate it. **That job had
never produced a report.** Once veraPDF was made to run, it failed **twelve**
clauses on the project's own fixture.

All twelve are fixed. The fixture now validates clean: **129 rules pass, 0 fail,
523 checks pass, 0 fail** under veraPDF 1.30.3's PDF/A-1b profile. This is the
first point at which PDF/A-1b conformance can honestly be claimed, and the claim
is made here on the basis of that run rather than on the existence of the gate.

### The gate could never have gone green

Two separate faults, and the second one is why the first went unnoticed.

**It grepped for text veraPDF does not print.** The job looked for
`is in accordance with PDF/A-1`. veraPDF 1.30.3 prints:

```
PASS /work/pdfa-fixture.pdf 1b
```

That phrase appears zero times, even on a passing file. So the gate's failure
mode was indistinguishable from real non-conformance — it would have reported
"did not confirm" on a document that passed, forever.

**veraPDF was running out of memory.** The official image ships with `JAVA_OPTS`
empty. Loading a PDF/A validation profile in a default heap dies with
`OutOfMemory caught when validating item` and still exits non-zero, so an empty
`JAVA_OPTS` produced no report at all and the job reported the absence of a
report as a validation failure.

Both are now fixed (`-Xmx2g`; grep `^PASS[[:space:]]`), and the gate's failure
report extracts the failing clause numbers from an XML report, because the text
report says only `FAIL`, which is not actionable. The gate has been checked in
both directions against the real fixture: it passes the valid file, and rejects
a copy with `rdf:Seq` changed to `rdf:Bag` in `dc:creator` — a single-token
change that no structural test in this crate would notice.

### Fixed — the generated ICC profile had a misaligned header

`preferred CMM` was written as a single byte. It is a four-byte signature, so
every field from offset 4 onward sat three bytes early: the data colour space
landed at 13 instead of 16, `acsp` at 33 instead of 36, the PCS illuminant at
65 instead of 68.

The profile's declared size still matched its real size, so the writer's own
`debug_assert_eq!` passed. So did three unit tests, because those tests had been
written against the broken offsets and agreed with it. veraPDF read the header,
found `XYZ ` where the data colour space belonged, and reported an RGB profile
as `XYZ` — failing clauses 6.2.2/1, 6.2.3.2/2 and 6.2.3.3/1 together.

Those tests now assert the offsets ICC.1 actually specifies.

### Fixed — XMP metadata

- Clause 6.7.2/2: the packet went through `deflate`. PDF/A-1 forbids `/Filter`
  on the metadata stream outright, so compressing it was making the output
  non-conformant. It is written uncompressed now.
- Clauses 6.7.3/3 and /6: `/Author` and `/Creator` reached the document
  information dictionary with no XMP counterpart. Note the mapping is not
  mechanical — `/Creator` is the *tool* (`xmp:CreatorTool`) and `/Author` is a
  person (`dc:creator`); pairing them the other way satisfies neither rule.
- Clause 6.3.3.2/1: a Type 2 CIDFont now carries `/CIDToGIDMap /Identity`,
  which is correct rather than merely acceptable: the subsetter keeps original
  glyph ids, so CID and GID are the same number.
- Clause 6.3.5/3: the font descriptor carries a `/CIDSet` over the subset's
  CIDs, uncompressed because PDF/A forbids a filter there too.

### Fixed — `dc:creator`, which took ten attempts

This was the hard one, and the fix is worth recording precisely because it is
counter-intuitive.

The XMP specification declares `dc:creator` as a list of `Agent`, three levels
deep. **veraPDF cannot parse that shape at all** — not "rejects it", but stops
parsing the packet, after which `pdfaid:part` is no longer found,
`pdf:Producer` no longer matches, and every metadata rule fails at once. The
failure count goes 5 → 9 → 12 depending on the shape.

Among encodings that do parse, each was rejected for a different declared type:
`agent`, `agentname`, `propername`, `seq propername`. The only form that
satisfies both 6.7.9/3 and 6.7.3/3 is a flat `rdf:Seq`:

```xml
<dc:creator><rdf:Seq><rdf:li>Hikari Labs</rdf:li></rdf:Seq></dc:creator>
```

That is not the shape the specification describes. The discrepancy is pinned by
a test so a future reader does not "correct" it back into non-conformance.

The experiments were run by generating minimal PDFs around each candidate packet
and validating them directly, rather than by editing the writer and rebuilding
once per guess — the whole search was a single evening's work only because the
feedback loop was ~40 seconds instead of a full Rust rebuild.

### Fixed — two font bugs that drew wrong text without failing

- **Clause 6.3.6/1.** `/W` recorded the *shaped* advance, which includes
  kerning. Two glyphs were written as 620.12 and 561.52 where the font says
  684.08 and 579.10 — describing glyphs that do not exist. Widths now come from
  the font's own `hmtx`.
- **Clause 6.3.6/1 again.** `/DW` was hardcoded to 1000. The default width
  describes glyphs absent from `/W`, and the one always absent is glyph 0
  (`.notdef`), which has no character and so is never collected by the walk.
  The font says 600.10. `/DW` is now read from the font.
- **Clause 6.3.5/1.** When the subsetter renumbers glyphs, a referenced glyph
  it did not renumber had its *original* id written into a subset that no longer
  contained it. The file drew the wrong shape rather than failing, and the
  writer's own tests passed because every glyph they checked was one of the
  renumbered ones. The remap is now required to be total; if it is not, the
  subset is abandoned and the font is embedded whole.

That last one trades size for correctness: on the PDF/A fixture it costs the
subset entirely (14 KB → 205 KB), because exactly one glyph — a composite the
subsetter expands into components — is unrenumberable. A smaller file drawing
the wrong glyph is worse than a larger one that does not.

### Fixed — the fuzz jobs had never run, and then found a crash

`fuzz/Cargo.toml` declared no `[[bin]]` targets. `cargo fuzz run css` resolves
`css` as a bin target of the fuzz package and does not scan `fuzz_targets/` on
its own, so both fuzz jobs died immediately with:

```
error: no bin target named `css` in default-run packages
```

That is a build failure, not a validation failure, so it reads as "the fuzzer
found nothing" unless you read the log. The crate's lockfile was still pinned
at v0.21.8 while the workspace was at v0.21.9, which is the same evidence from a
different angle: the manifest had never been resolved since it was written.

With the targets declared, the fuzzer ran for the first time and found a real
bug within seconds.

### Fixed — a slice-index panic in the HTML tag scanner

Input `< R R E \x7f =` panicked:

```
thread panicked at crates/hikari-core/src/css.rs:
range start index 9 out of range for slice of length 8
```

The attribute scanner read the trailing `=` and defaulted the quote character to
`"` via `chars.get(i).copied().unwrap_or('"')`. With nothing after the `=`, that
fabricated a quote it then took the quoted-value branch on, advancing `i` past
the end of the input before slicing it.

Fixed by matching the character instead of defaulting it, and by stepping over
the closing quote only when there is one — an unterminated value leaves the
index at the end of the input, and incrementing unconditionally is what put it
out of range in the first place.

Every other malformed-tag test in this crate used a well-formed attribute
value, so none of them reached it. A tag scanner's job is surviving input that
is not like that, which is precisely the gap a fuzzer closes and a test suite
written alongside the parser does not.

The regression is pinned by a unit test carrying the exact bytes, plus the
twelve neighbouring shapes at end-of-input.

**A second panic, same root cause.** With the first fixed, the fuzzer found
another within seconds: `<sTyle ` again sliced past the end, at
`range start index 8 out of range for slice of length 7`.

The scanner advanced its cursor past the end of input when a tag ran out before
its `>`, and the next slice started there. The mixed-case spelling is what
exposed it: `<sTyle ` lowercases to `style`, one of the two tag names whose body
is skipped, and that is the only path that slices again immediately. `<style `
survives only by accident of the loop order — which is exactly why a
hand-written test would not have found it, and why the two bugs were worth
fixing as a class rather than one at a time.

Rather than patch the next crash when it arrived, every tail-slice in the HTML
scanner was made total. `i` is now clamped to `chars.len()` at each slice and
`chars[i..]` goes through `.get().unwrap_or_default()`. Six slice sites.

Regression tests cover the sixteen spellings the fuzzer actually produced and,
more usefully, **every byte prefix of a tag-laden document** at a character
boundary — so a future scanner change cannot reintroduce this on a spelling
nobody thought to list.

After both fixes: 484,052 CSS runs and 26,142 PDF runs, no crash.

### Fixed — the fuzz jobs compiled with `-D warnings` and one did not

`fuzz (pdf)` failed with `constant TREE is never used`. The selector byte picks
between the font surface and the writer, and `TREE` was declared as the writer's
value but the arm was a catch-all `_`, so nothing referenced it.

Neither fuzzing run noticed, because `cargo fuzz run` does not pass CI's
`-D warnings`. The difference between "the job is red" and "this crate cannot
compile under the flags the job uses" was invisible until someone read the log
instead of the icon. The README now carries the one-line command that checks it
locally, because the fuzz crate is not in the workspace and `cargo clippy
--workspace` will never see it.

Also dropped the `hikari-rs` dependency, which both targets do not use — they
drive `hikari-rs-core` and `hikari-rs-pdf` directly, since that is where the
parsers are and fuzzing the facade would spend its budget in argument
marshalling.

### Fixed — a per-thread font shaper leaked on every thread

With the fuzz targets building, LeakSanitizer reported 1040 bytes leaked in two
allocations on the first run of `fuzz (css)`.

`shaper_for` returned `&'static ShaperFont` from a `thread_local!` cache, and
the only way to produce a `&'static` into a thread-local without `unsafe` is to
`Box::leak` it. A thread-local holding leaked pointers frees none of them, so
every thread that ever shaped text leaked one `ShaperFont` per font, permanently.

This is the worst shape a leak can take: bounded, so nothing in ordinary use
notices, and small enough that only a leak checker sees it. `unsafe_code =
"forbid"` workspace-wide is why it was written that way — every trick for
handing out a stable reference into a growable thread-local needs `unsafe`.

Replaced with `with_shaper`, which hands the shaper to a closure. It is stored by
value in the thread-local and dropped when the thread exits; no `&'static`, no
`unsafe`, and the cache still works — the layout tables it avoids re-resolving
were measured at ~16% of the render benchmark.

Benchmarks are unchanged by this (layout 3.94 µs, `paint_to_rgba` 1.821 ms,
banner PNG 3.516 ms), which is the point: the performance argument for the cache
and the leak were the same line of code, and only one of them was justified.

LeakSanitizer does not run on the macOS development machine, which is why a
794,576-run local session reported nothing. The leak was only ever observable
on the Linux CI runner.

### Fixed — the fixture could never conform

The fixture embedded an invoice XML attachment to exercise that code path.
PDF/A-1 clause 6.1.11 forbids embedded files outright, both the `/EF` key on a
file specification and `EmbeddedFiles` in the name dictionary. A conformance
fixture that cannot conform tests nothing. The attachment moved to the
`invoice` example, which is what should have exercised it all along.

### Fixed — a test that asserted a bug

`xmp_with_no_title_still_has_the_element` required that an absent `/Title`
produce an *empty* `<dc:title>`. An empty element is a present value with an
empty string, which does not match an absent `/Title` — clause 6.7.3, the same
clause that later failed on `/Author`. The test was pinning the bug, and it
passed. It now asserts the property is omitted entirely.

### Known limitation — PDF/A output is not subset

As above: the PDF/A path embeds DejaVu Sans whole (368 KB, 205 KB compressed)
because one composite glyph cannot be renumbered. Non-PDF/A output is unaffected
and still subsets. Resolving this needs the subsetter to retain a glyph the
character map does not reach, which it does not currently offer.

A benign `Missing OutputConditionIdentifier` warning remains on stderr. PDF/A-1
clause 6.2.2 offers a choice between a registered `OutputConditionIdentifier`
and a `DestOutputProfile` with no identifier; this crate takes the second, which
is correct. Naming a condition the crate does not have a profile for would be a
false claim, so the warning is left standing.

## v0.21.9 — deterministic fallback, a frozen CSS spec, and a real CI matrix

This release is mostly about correcting things that were claimed rather than
things that were missing. Five of the six items were calls to stop overclaiming.

### Fixed — CJK output depended on the machine

The default path read a font from a hard-coded list of host paths when no bundled
face covered a character:

```
/Library/Fonts/Arial Unicode.ttf
/System/Library/Fonts/Supplemental/Arial Unicode.ttf
/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc
/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc
```

So the same tree produced different bytes on a developer Mac and a CI container —
while the crate advertised byte-identical output on every platform. Nothing
reported it: the render succeeded, the tests passed, and the only symptom was a
digest that matched on one machine and not another. CJK was the trigger in
practice, because that is what the embedded subset does not cover.

**`hikari::FallbackPolicy`**, defaulting to `Bundled`:

| Policy | A character no bundled face covers | Deterministic |
|---|---|---|
| `Bundled` | not drawn — `.notdef` | yes |
| `Strict` | an error naming the character | yes |
| `SystemFont` | read from a host path | **no** |

`Bundled` and `Strict` return without touching the filesystem, which is asserted
directly rather than inferred. `SystemFont` keeps the old capability available and
*chosen*, which is the difference between a feature and a defect.

CJK still renders only with the `bundled-cjk` feature (9,968,236 B; measured WASM
3,693,596 B without, 13,661,814 B with). Without it the output is deterministic
and shows `.notdef` boxes, which is the trade taken: a card that silently differs
between machines is worse than a card with a visible gap.

### Added — a frozen CSS spec with a conformance suite

`docs/css-spec.json`, generated from tables in `crates/hikari-core/src/spec.rs`,
with `spec::conformance` checking them against the parser.

**The first draft of the table listed 32 properties the parser does not support.**
`opacity`, `text-align`, `z-index`, `max-height`, `transform` and 27 others are
matched by the dispatcher only in order to *refuse* them with a specific reason
("use an rgba colour with alpha instead"), and the hand-written table listed them
as supported. The conformance test caught it on its first run, which is the
argument for having written it before publishing the table.

The spec is now two tables — `SUPPORTED` and `REPORTED` — and the suite asserts
every supported property is accepted, every reported one still refuses *with the
same reason*, the tables do not overlap, and both are sorted and unique.

The first attempt at generating the JSON scraped `css.rs` with a regular
expression. It produced 89 properties including `white`, `block` and `fixed`
(values of other properties, matched at the same indentation) while *missing*
`filter`, whose arm is written `"filter" | "backdrop-filter" =>`. A spec with those
errors is worse than none, so the extraction was replaced by Rust tables.

### CI — sixteen jobs, most of them new

- **`platform`** — the full test suite on Linux x86_64, Linux arm64, macOS arm64,
  macOS x86_64 and Windows.
- **`determinism`** — same tree, five targets, byte-identical output. This is the
  job that would have caught the CJK defect and could not, because it ran on one
  runner with the font installed. Those runners deliberately have no CJK font, so
  the difference cannot hide.
- **`node`** — Node 18/20/22/24 across Linux, macOS and Windows.
- **`verapdf`** — veraPDF 1.30.3 over a fixture built by the `pdfa_fixture`
  example. **The gate had not been observed green at the time of this release,
  so PDF/A conformance is still
  not claimed.** See below.
- **`fuzz`** — `cargo-fuzz` over the CSS/HTML parsers and the PDF writer,
  bounded to 90 s. The PDF target needed a public `subset_font_for_text` to reach
  the table-walking subsetter at all.
- **`bench-linux`** — a Linux measurement for BENCHMARKS.md, uploaded as an
  artifact. Asserts nothing: a shared runner is too noisy for a threshold.
- **`bench-gate`** — a real criterion gate, on self-hosted runners only. Shared
  runners vary by more than any threshold worth having.
- **`wasm-browser`** — Playwright/Chromium against the real web-target bundle.
- **`supply-chain`** — `cargo audit` alongside `cargo deny`, and a CycloneDX SBOM.

### Removed — `panic = "abort"` from the release profile

It was in the wrong place: a Cargo profile applies to every crate built from the
workspace, so it put abort-on-panic into the library rlibs. A library that
carries that into a link is a library deciding its consumers' panic behaviour.

Measuring it first changed the argument. **It is worth 264 bytes** — 3,693,596
unwind, 3,693,332 abort — and two claims I had written down about it were wrong:
it does *not* break `cargo test --release` (Cargo forces `unwind` for test
harnesses), and it is not worth 63 KB. Both are recorded in `Cargo.toml` so they
are not repeated.

### Fixed — documentation drift

`crates/hikari-raster/src/lib.rs` still said `rustybuzz` eleven releases after the
shaper moved to `harfrust`, as did the `hikari-core` shaping module doc. The
historical references to `rustybuzz` in the shaper's comments are *deliberate* —
they explain why the code is shaped the way it is — and stay.
`plans/construction-plan.md` now opens with a note that it is a dated record.

### Decided — PDF/UA is out of scope for 1.0

Not deferred. Tagged PDF requires a structure tree, a parent tree, marked content
on every text run, `/Alt`, `/Lang` and correct heading nesting — which changes
the shape of every paint operation in `hikari-pdf` rather than adding a parameter.

The deciding factor is that PDF/UA's purpose is WCAG conformance, and no
validator can check that. A checker confirms tags are present and nested; it
cannot tell whether an `alt` describes the figure or says "image". Building it
would mean emitting structure without being able to say whether the result is
accessible, and this project's rule is that a feature is either verified or not
claimed.

### Not done — a head-to-head benchmark

BENCHMARKS.md previously promised one and did not have it. It now says so
explicitly, with the three reasons a fair comparison has not been earned here:
equivalent input (HTML/CSS vs SVG vs a typed tree), equivalent output (needs a
pixel-diff harness), and a machine neither project controls.

What was fixed instead is the part that was rot: every stage quoted in the
document now corresponds to a `bench_function`, and the benchmark **refuses to
run under a debug build** — the failure that made the v0.7 comparison wrong.

---

## v0.21.8 — PDF/A-1b structural metadata

### Added
- **`PdfOptions::pdfa`**, off by default. Emits an output intent with a
  destination ICC profile, an XMP packet declaring `pdfaid:part` 1 and
  `pdfaid:conformance` B, a content-derived trailer `/ID`, and
  `/ViewerPreferences << /DisplayDocTitle true >>`.
- **`hikari_pdf::pdfa`** — the pieces, usable and testable on their own:
  `srgb_icc_profile`, `build_xmp`, `document_id`, `deflate`, `zlib_reader`.
- A generated **500-byte sRGB ICC v2 display profile**, rather than a new
  dependency or an embedded blob.

### What is claimed, and what is not
**This output has not been validated against a PDF/A conformance checker.** What
the tests establish is the narrower claim: the output intent, the XMP packet and
the document identifier are present, well-formed and reachable from the catalog.
A checker may still reject the file for something not visible in the byte stream
-- a font program missing a required table, an annotation flag, a colour space the
part does not permit. Producing the right metadata and claiming conformance are
different claims and only the first is supported. The README says so, and a test
asserts the caveat is still there.

### Cost
7,575 bytes plain, 8,952 with PDF/A on a one-page card: **+1,377 bytes**, almost
all of it the uncompressed ICC profile. Off by default for that reason.

### Why the ICC profile is generated
There is no sRGB profile in the crate and adding a dependency to ship ~3 KB of
colour data would be a poor trade. `srgb_icc_profile` builds a minimal v2
matrix/TRC display profile from Bradford-adapted sRGB colorants: `desc`, `wtpt`,
`rXYZ`/`gXYZ`/`bXYZ`, `rTRC`/`gTRC`/`bTRC`, `cprt` and `mntr`. The `mntr` tag is
what PDF/A checks for specifically, as distinct from the profile class in the
header. A caller rendering for a specific press should supply that press's
profile; this one exists because PDF/A requires *an* intent and refusing to write
one would make the option unavailable.

### Bugs the tests caught
- **Every tag was missing its type signature.** The tag *table* listed all the
  required fourccs with in-range offsets and sizes, so every table-level check
  passed, while the tags themselves carried no `XYZ `/`curv`/`text` signature and
  a reader could not interpret any of them. A lint reporting that a `kind` field
  was never read is what exposed it -- worth taking "this is set but nothing
  consumes it" seriously rather than deleting the field.
- **The sRGB primaries were wrong twice.** Stored as three bytes each instead of
  four-byte s15Fixed16 values, and with the green and blue rows mixed up, giving
  a profile whose colorants summed to 0.65 luminance -- a colour 35% too dark.
  Caught by a new test asserting the colorant Y values sum to the white point,
  which is the property that actually defines a set of colorants.
- **A trailer search found no trailer.** A test located `/ID` by searching for the
  literal word `trailer`, which this version of `pdf-writer` does not emit. It
  now anchors on `startxref` and looks backwards.
- `pdf-writer` 0.12 exposes `OutputIntent` as a writer but has no constructor, so
  it is written inline through the typed array. `Chunk::indirect` returns an
  `Obj`, not a `Dict`, so the raw-object route needs `.dict()` first.

### Why PDF/UA is absent
PDF/UA requires tagged PDF: a structure tree, marked content on every text run,
`/Alt` on figures, `/Lang`, correct heading nesting. That is a different
content-generation model, not a metadata addition -- every paint operation would
have to emit marked content paired with a tag. And the point of PDF/UA is WCAG
conformance, which no validator can check: one can confirm the tags are present
and correctly nested, not that an alternative text is any good. Shipping it would
mean claiming accessibility this renderer cannot verify.

### Tests
35 new: 25 unit tests on the ICC profile, XMP and identifier, and 10 reading the
generated PDF back to confirm the pieces are actually *in the document* -- a
correctly-built profile that is never referenced satisfies every unit test and
produces a file that is not PDF/A at all. One test asserts two different
documents get different identifiers and another that the output is byte-stable,
since the `/ID` is content-derived rather than clock-derived precisely so that
both hold.

301 tests pass, clippy clean, fmt clean, corpus and determinism unchanged.

---

## v0.21.7 — a typed builder for trees

### Added
- **`hikari::ui`** — `div()`, `text()`, `image()`, `image_ref()` returning an
  [`Elem`] builder whose `.child()` appends in the order written, so a deep card
  reads top-down instead of nesting inside-out.
- Five shorthands (`size`, `bg`, `pad`, `color`, `column`) and one escape hatch,
  `Elem::style`, which reaches every remaining `Style` field.
- `From<Elem>` and `From<&Node>` for `Elem`, so builders and hand-built nodes mix
  freely in the same tree.

### The problem
`Node::container(style, vec![...])` puts the opening bracket, the style and the
children before the thing they describe, so indentation nests the wrong way and
a three-level card is a wall of closing brackets.

### Why only five shorthands
The obvious design is one fluent method per `Style` field. That duplicates twenty
setters that then have to be kept in step with `Style`, and the copy is worse than
what it replaces — no documentation, no colour parsing, no defaults. `Elem::style`
takes the same `FnOnce(Style) -> Style` that `Style`'s own `with_*` chain takes,
so nothing in `Style` is unreachable and nothing is duplicated.

### Fixed, in the tests and examples written alongside it
- **`Elem::style` originally took `FnOnce(&mut Style)`.** That is the more natural
  signature to reach for and the wrong one: every `Style` method consumes and
  returns, so each call site needed a `mem::take` and a put-back. A doc example
  failed to compile because of it, which is how it was found. Now the signature
  mirrors `Style`'s own chain and the shorthands call it directly.
- **`row()` was a no-op.** `Style`'s default `dir` is already `Row`, so an
  end-to-end test comparing a "row" layout to a "column" layout found the two
  byte-identical and read it as the shorthand being broken. The default is now
  pinned by a test and the shorthand is `column()`, which does change something.
- **A doc example referenced `hikari::render_png_with_assets`, which does not
  exist.** The design deliberately has no such function -- `Assets::resolve`
  returns a resolved tree and the renderer takes no part -- so the example
  described an API that was never built. It now matches the real one, and every
  example in this module and in `assets` is compiled rather than `ignore`d, since
  an uncompiled example is exactly what let that one through.
- Two assertions guessed at pixel coordinates and were wrong: a "slate box at
  (120, 120)" outside a 100x40 box, and a "row layout puts the second box past
  x=60" when it starts *at* 60. Both now assert the measured relationship.

### Tests
16 new: 15 in `hikari-core`, including one that asserts the builder produces the
tree `Node::container(...)` would produce for the same card, and 5 end-to-end
through the renderer. The end-to-end comparison is a **byte-digest equality**
against the hand-built equivalent, because "both produce ink" would pass for a
builder that dropped a background colour -- and a paired assertion that the built
tree is not blank, so two matching blank renders cannot pass as agreement.

266 tests pass, clippy clean, fmt clean. Corpus, determinism and the WASM size
budget are unchanged.

---

## v0.21.6 — animated WebP

### Added
- **`render_animation_webp`** and `encode_webp_animation`, alongside the existing
  animated GIF and APNG paths. Lossless, full-canvas frames, `VP8X` + `ANIM` +
  `ANMF` container, animated WebP's usual "no cross-frame optimisation" cost --
  each frame stands alone, so a damaged frame costs one frame rather than the
  rest of the animation.
- `count_webp_frames` for the decoder round trip.
- `Loop`, `AnimBackground` and `MAX_CANVAS` (16383 px, the format's own limit).

### Why it is hand-written
`image-webp` has no animation support in either direction, so there is nothing to
call. The frame *bitstreams* are delegated to `image`'s still lossless encoder
and the container written here. That split is safe for one specific reason worth
stating: `WebPEncoder::new_lossless` emits exactly one `VP8L ` chunk after the
12-byte `RIFF`-size-`WEBP` prologue, so dropping the prologue leaves a valid
chunk. The code asserts that prologue's shape rather than assuming it, so a
future `image` release that starts adding an `EXIF` chunk turns into an error
instead of a file that decodes to one frame.

### Measured, 120x90, four flat-colour frames with text
| format | bytes |
| --- | --- |
| animated WebP | 2,974 |
| APNG | 3,636 |
| animated GIF | 1,901 |

WebP beats APNG by 18%. **GIF is smaller here**, which the README would rather
not leave out: GIF is palette-indexed, so flat colour is exactly the case it
wins, and the gap closes or reverses as soon as there are gradients or alpha.
A test asserts WebP beats APNG and nothing asserts it beats GIF.

### Two details of `ANMF` that are easy to get backwards
Both were written the other way round here first, and both produced a file that
hashed consistently, passed every structural check, and decoded to a single frame
of the wrong size:

1. **The 16 bytes of frame geometry come before the frame bitstream**, not after.
2. **`Frame X` and `Frame Y` are the coordinate divided by 2**, not by 3.

The specification's `ANMF` diagram is unambiguous about both. The decoder round
trip is what catches them; a byte-level test written against the writer agrees
with the writer either way, which is the whole reason every test in
`webp_anim_tests` decodes.

### Blend flag set, not clear
Every frame here is a whole canvas, so the `ANMF` B flag is set to "do not
blend". Alpha-blending would also be defensible, but it is not free: the
specification's blend formula recomputes colour as
`(src*A + dst*A*(1-A)) / blendA`, which loses a unit in integer division -- a
solid red frame came back as `254, 0, 0`. Overwriting preserves the frame byte
for byte, which is what a lossless encoder should do and what lets the round trip
be asserted exactly rather than to within a tolerance.

### Fixed, in tests written alongside it
A clamp test asserted that a one-hour duration was clamped to 24 bits. An hour
fits in 24 bits comfortably, so nothing clamped; the test used `u32::MAX`
instead, and a companion test now pins that an hour survives unchanged so the
clamp cannot become "clamp everything to something".

### Tests
20 new: 17 in `hikari-animate` covering the container, every chunk's word
alignment, the `VP8X` flags, odd canvas sizes, transparency, single-frame
degeneracy and rejected input; 3 end-to-end through the real renderer, because a
frame that renders through the pipeline but comes out empty is a failure no
hand-built frame buffer would ever produce.

242 tests pass, clippy clean, fmt clean, corpus and determinism unchanged.

---

## v0.21.5 — assets, a WASM size budget, benchmark visibility

### Added
- **`hikari::assets::Assets`** -- resolve named image references into bytes before
  a render. `Node::image_ref` declares a name; `Assets::resolve` fills it in.
- **`render_png_with_assets` is not needed, and that is the design.** The resolver
  returns a tree with bytes inline; the renderer is not involved at any point, so
  it cannot grow a fetch, a cache or a timeout by accident. The "a render makes no
  network calls" property is structural rather than a promise.
- **A 3.75 MB WASM size budget in CI**, against a measured 3.52 MB.
- **Criterion saves a baseline and reports changes in the job log** instead of
  discarding the run.

### Why no fetch inside the renderer
A render that can fail because a server is down is not deterministic, and the
determinism guarantee here is byte-identical output from the same input. A
renderer that blocks on I/O has no backpressure, so 200 cards would issue 200
uncoordinated fetches. And a fetch error has nowhere to go mid-paint, so it would
have to become a blank box -- the hardest kind of failure to diagnose. The caller
owns the HTTP client, the timeout, the retry policy and the cache.

### An unresolved reference is an error, not a blank box
A missing image that renders as an empty box is indistinguishable from one that is
genuinely transparent, and finding that out by looking at the output is the worst
way to find out. `resolve` fails naming the asset; `resolve_lenient` leaves the
node referenced and returns every missing name at once, which is what a preflight
wants. Its first implementation returned the original tree on any miss -- correct,
and the opposite of what "lenient" should mean, since it threw away every asset
that *was* found. A test now pins the useful behaviour.

### Fixed, found by the tests written alongside it
`resolve_lenient` discarded partial results, as above. Two test expectations were
wrong and the code was right: an inline three-byte image was asserted to equal a
one-byte slice.

### Notes on the CI work
- The size budget is on the **unoptimised** output on purpose. That is what the
  README quotes and what a reviewer can reproduce without installing `wasm-opt`;
  adding it would make the number better and less checkable.
- A first draft of the benchmark step globbed criterion's internal directory
  layout to build a summary table. That layout is not part of criterion's API and
  the paths were a guess, so it was replaced with a link to the log, which is the
  part that is actually reliable.
- The WASM figure had already drifted 2.95 MB to 3.51 MB across three dependency
  migrations. "It compiles" is not a statement about size, and this is the one
  place the number was written down -- which is exactly why it should not be a
  number only a human reads.

224 tests pass, clippy clean, fmt clean.

---

## v0.21.4 — colour filters

### Added
- **`Style::filter`, applied to a node and everything inside it.** Six colour
  operations — `brightness`, `contrast`, `saturate`, `grayscale`, `sepia`,
  `invert` — plus `opacity`, reachable from CSS as `filter: grayscale(1) invert(1)`.
- `Filter::color_matrix` folds all six into a single 4x5 affine transform, so any
  combination costs one pass over the pixels regardless of how many operations
  are set. `None` for no colour operation at all, which skips the pass entirely.
- CSS support for the above. `blur`, `drop-shadow`, `hue-rotate` and `url()` are
  **reported as skipped**, never silently dropped: a dropped blur is
  indistinguishable from a filter that does nothing.
- `Style::mask` and its types exist and round-trip through JSON, but **nothing
  renders through a mask yet**. A mask on a node with no image is inert, and a
  test says so explicitly rather than asserting a feature works.

### Filtered nodes render offscreen
A filter has to see a node *and* its children as one image, so the subtree is
painted into its own buffer and composited back. A per-element filter would make
`filter` on a card blur each child separately and leave the card's background
sharp. An identity filter is detected and skips the offscreen path entirely, so a
node with `filter: None` -- or one whose fields are all at their identity values
-- renders byte-identically to having no filter at all, which a test asserts.

### Fixed, all found by the tests written alongside it
- **Source-over had its coverage terms swapped**: `src*(1-da) + dst*sa` instead of
  `src + dst*(1-sa)`. Invisible against a transparent destination -- the case a
  naive unit test uses -- and it discards the source completely over anything
  opaque. An inverted card over a white page came out pure white.
- **The matrix composition was transposed.** Invisible whenever the incoming
  matrix is the identity, so every single-operation test passed, grayscale
  included; only `saturate(1.0)` exposed it. Two *luma* operations composed in a
  test now catch it, since `diag(a,b,c)` is its own transpose on the diagonal.
- **`saturate` and `grayscale` had the same parameterisation.** They are the same
  operation with the argument the other way round, so `saturate(1.0)` fully
  desaturated -- the exact inverse of what it documents.
- **Offsets were in 0..=1 units while the matrix works in 0..=255**, so
  `contrast(0)` produced black instead of flat mid grey.
- **`fold_luma` gave every row the same weights**, which is only correct at full
  desaturation; at `t = 0` it summed all three channels instead of being the
  identity.
- **The identity initialisation made all four rows identical**, so every colour
  row scaled red.

### Blur is not here
An earlier version implemented `blur` as three box passes. It was wrong in a way
that kept eating the image -- repeated passes darkened it rather than spreading
it -- and shipping a filter that quietly erases a picture is worse than shipping
no blur. The field was **removed** rather than left with a caveat, so no tree can
ask for it, and the roadmap says so. `hue-rotate` never existed.

### Known limitation
A filtered node composites with `Normal`, even when the node sets a blend mode.
`blend` still applies to everything the node paints *into* the layer -- each
element blends against its neighbours exactly as without a filter -- but the
filtered layer does not then blend as a unit against what is behind it.
`PixmapMut::draw_pixmap` accepts no blend mode in tiny-skia 0.11, and hand-rolling
all fifteen modes for a whole-image composite means a second implementation of
hue, saturation, colour and luminosity, which is how those four would drift out of
agreement with the per-glyph path. Pinned by a test so it cannot change silently.

### Tests
26 new: 15 arithmetic tests in `hikari-core` checked against hand-computed values
and a brute-force reference that never touches the matrix code, 6 pixel-level
tests in `hikari-raster`, and 5 end-to-end render tests. The corpus grew to 22
cases with a `filter-grayscale-invert` digest and a measured ink floor.

Two test expectations were wrong and the code was right: `brightness(2)` of a
`#4080c0` card puts red at exactly 128, not "well above 128"; and sepia of *white*
clips, because the standard matrix rows sum to more than 1. Both now assert the
value the arithmetic produces.

206 tests pass, clippy clean, fmt clean. The determinism card is unchanged.

---

## v0.21.3 — PDF font embedding moved off ttf-parser; no unmaintained deps remain

### Changed
- **PDF font embedding and metrics now read through `skrifa`.** This was the
  last direct user of `ttf-parser`, and the change that actually retired
  RUSTSEC-2026-0192.

  That advisory could not have been cleared by any partial swap, which is why it
  survived the previous two releases. `rustybuzz` and `fontdue` each depended on
  `ttf-parser` themselves, so replacing our own call sites left the crate in the
  tree regardless. It took all three replacements — `harfrust` for shaping,
  `skrifa` for glyph rasterization, and `skrifa` again here.

- `DocFont` now holds a parsed `FontRef` instead of reparsing on every lookup.
  `has`, `gid_of` and `exact_width` each called `ttf_parser::Face::parse`,
  which meant re-reading the whole cmap for every glyph of every page.
  `exact_width` runs per character, so a dense table paid for it hundreds of
  times. The parsed face borrows `bytes`, which is `'static`, so this costs a
  pointer.

### `ttf-parser` is gone from the tree entirely

Zero occurrences in `Cargo.lock` and zero in any manifest. `deny.toml` now
ignores **no** advisories, and `unmaintained = "all"` stays on as a gate rather
than an exception: the next crate that declares itself unmaintained fails the
build instead of needing a written justification.

### Verified byte-identical, not assumed

Nothing in the suite would have noticed if PDF metrics had drifted. The corpus
digests cover PNG and SVG; the determinism card does not cover PDF; and a wrong
advance width does not look wrong — the text renders, just spaced slightly off.

So the `/W` array and the `/FontDescriptor` were extracted from a real PDF and
compared across the migration. **Identical**: every width to the last decimal
place, `/Ascent 1901`, `/Descent -483`, `/CapHeight 1493`, `/FontBBox
[-2090 -948 3673 2524]`, and a byte-identical 201,711-byte file.

### Tests
Five new in `crates/hikari/tests/pdf_font_metrics.rs`, pinning the width array,
the descriptor fields, and the invariant that widths do not move with page size.

One of them asserts the *opposite* of what it first claimed. It assumed an
unmapped character would be skipped gracefully; the renderer actually fails with
`no glyph for ... (install a CJK fallback font)`. That is the better contract — a
PDF with one silently mis-spaced glyph among a page of correct ones is far worse
to debug than a clear error — so the test now pins the error.

179 tests pass, clippy clean, fmt clean, `cargo deny` clean with no advisories.

### Where the unmaintained-dependency work landed

| | before | after |
|---|---|---|
| advisories ignored | 2 | 0 |
| font parsers in the tree | `ttf-parser` + `read-fonts` | `read-fonts` |
| `rustybuzz` / `ttf-parser` / `fontdue` | present | all gone |
| render, 1200x630 banner | 4.38 ms | 3.52 ms |
| WASM, unoptimised | 2.95 MB | 3.51 MB |

The WebAssembly cost is real and stated plainly: 560 KB for a maintained shaper,
a maintained rasterizer, and no duplicated parser. `wasm-opt` should recover some
of it, but no size budget is enforced in CI yet.

---

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
