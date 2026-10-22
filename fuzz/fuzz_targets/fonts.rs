//! Fuzz the font registration surface that WASM and Node expose.
//!
//! # Why this target exists
//!
//! Every JS binding has `register_font(name: &str, bytes: &[u8])`, and bytes
//! from a caller this library has never met go further here than anywhere
//! else: through `read-fonts`, the build-time subset tables, shaping, glyph
//! rasterisation and (for PDF) the TrueType subsetter and CID ToUnicode CMap
//! generator. The `pdf` target already reaches the subsetter by registering
//! fonts; this one exists because the registration API itself -- and the
//! face-cache it feeds -- deserve their own, cheaper target, which means a
//! corpus can hammer the font parser thousands of times per second without
//! paying for a document render to get there.
//!
//! # Face-cache growth, and why leak detection is off for this class
//!
//! Every successfully parsed distinct byte string is cached for the life of
//! the process. A fuzz campaign can therefore grow the cache unboundedly;
//! this is true of the real API too (documented there), and bounded runs in
//! CI never reach it. A long campaign should run with `--rss_limit_mb` set,
//! like every other target.
//!
//! LeakSanitizer is disabled for CI fuzz runs, for all four targets, for one
//! reason: this cache (and `css`/`pdf`'s font registrations) are
//! process-lifetime by contract, so end-of-run leak reports on them are
//! guaranteed, and on the other targets they are on a stateful malloc/free
//! counter that made previously-green jobs fail intermittently with 1-3 byte
//! reports. ASAN's crash/overflow/OOM detection stays on: that is what the
//! fuzzers have actually found here (the tag-scanner slice panic). The full
//! reasoning, with the local evidence, is in the fuzz job comment in
//! `.github/workflows/ci.yml`.
//!
//! # What counts as a failure
//!
//! Anything that is not a returned `Err`. A font that cannot be parsed may be
//! rejected; one that parses must survive shaping and SVG rendering of text
//! set in it.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Hard cap on accepted byte length. libFuzzer bounds corpus members anyway;
/// the cap exists so this stays cheap to walk into, and so a single input
/// cannot dominate the run's time budget inside `read-fonts`.
const MAX_FONT_BYTES: usize = 1 << 20;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FONT_BYTES {
        return;
    }
    let Ok(id) = hikari::register_font("fz", data) else {
        return;
    };
    // Only the fonts that parsed get to shaping: a text run set explicitly
    // in the font just registered, rendered through the same SVG path a web
    // caller would use.
    let style = hikari::Style {
        font: Some(id),
        ..hikari::Style::text(24.0, "#334155")
    };
    let tree = hikari::Node::text("Shaped in the font just registered", style);
    let _ = hikari::render_svg(&tree, 240, 60);
});
