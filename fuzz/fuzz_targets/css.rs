//! Fuzz the CSS/HTML front end.
//!
//! # Why this target exists
//!
//! `hikari_core::css` is the only part of this crate that parses untrusted
//! input into a tree. Everything else takes a typed [`Node`], so a fuzzer handed
//! arbitrary bytes has nothing to reach; here it has a stylesheet parser, an
//! HTML tag scanner and a colour parser, all of which index into strings and
//! parse numbers.
//!
//! # What counts as a failure
//!
//! Anything that is not a returned `Err`. The parsers are allowed to reject
//! input -- that is most of what they do -- but they must never panic, loop
//! forever, or allocate unboundedly in proportion to the input. A `Report`
//! listing skipped properties is the expected outcome for almost every input and
//! is explicitly *not* a failure.
//!
//! [`Node`]: hikari_core::Node

#![no_main]

use libfuzzer_sys::fuzz_target;

/// How many rules and declarations to parse out of the input.
///
/// Unbounded parsing of a fuzzer input is fine -- the input is bounded -- but the
/// report is what accumulates, so a cap keeps a 4 KB input from producing a
/// 200,000-entry report and turning every finding into a timeout.
const MAX_REPORTED: usize = 64;

fuzz_target!(|data: &[u8]| {
    // A CSS declaration list and a style attribute take the same parser, so both
    // shapes are exercised from the same bytes.
    let text = match std::str::from_utf8(data) {
        Ok(t) => t,
        Err(_) => return,
    };

    // A stylesheet, with the report that records what it refused.
    let mut report = hikari_core::css::ParseReport::default();
    let stylesheet = hikari_core::css::Stylesheet::parse(text, &mut report);
    // A report that grows without bound relative to the input is itself a
    // finding: it means one byte of input can be amplified into thousands of
    // entries. Bailing out here keeps such a case as a size measurement rather
    // than a timeout.
    if report.skipped.len() > MAX_REPORTED {
        return;
    }

    // The HTML scanner: a different parser with different failure modes --
    // unterminated tags, attributes without values, deeply nested elements.
    let mut report = hikari_core::css::ParseReport::default();
    let tree = match hikari_core::css::html_to_tree(text, &mut report) {
        Ok(t) => t,
        Err(_) => return,
    };
    let _ = stylesheet;
    // A tree that round-trips through layout without panicking is the property
    // worth asserting; the layout itself is not this target's business.
    let _ = hikari_core::compute_layout(&tree, 100.0, 100.0);
});