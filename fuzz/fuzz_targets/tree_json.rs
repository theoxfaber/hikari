//! Fuzz the tree JSON that the WASM and Node bindings parse.
//!
//! # Why this target exists
//!
//! Both JS entry points -- `hikari-wasm` and the napi bindings -- take a
//! `&str` / `String` of JSON and hand it to `serde_json::from_str::<Node>`
//! before anything else happens. Until this target existed, that entire surface
//! was un-fuzzed: the `css` target reaches the stylesheet parser and the `pdf`
//! target the PDF writer, but every wasm and Node render from a client starts
//! here, with bytes from a caller this library has never met.
//!
//! Deserialisation is not the only interest. A tree that parses successfully
//! then goes through layout and the SVG writer with every field left at its
//! default, which is exactly the path a web frontend drives. Malformed
//! numbers, negative sizes, enormous percentages, `NaN` where a px should be,
//! container loops that nest to thousands deep -- all of it is reachable from
//! untrusted JSON alone.
//!
//! # What counts as a failure
//!
//! Anything that is not a returned `Err`. Parsing, layout and SVG rendering
//! may reject a tree; they must never panic, recurse the stack away, or
//! allocate unboundedly relative to the input.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Canvas the render path is exercised against: small, fixed, and unrelated
/// to any dimension the input can influence, so per-input work is bounded by
/// the input alone, not by a size the fuzzer picks.
const CANVAS: (f32, f32, u32, u32) = (97.0, 65.0, 97, 65);

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(tree) = serde_json::from_str::<hikari::Node>(text) else {
        return;
    };
    let (w, h, w_px, h_px) = CANVAS;
    let _ = hikari::compute_layout(&tree, w, h);
    let _ = hikari::render_svg(&tree, w_px, h_px);
});
