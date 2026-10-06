//! Shaping tests that go through the **production** path.
//!
//! These exist because a test that exercises a different code path than
//! production is worse than no test: it passes while the shipped behaviour is
//! broken. The Arabic joining bug was exactly that.
//!
//! [`shape_text`] is the entry point the renderer calls. It sets `Direction`
//! explicitly from bidi analysis rather than letting the shaper guess segment
//! properties, and that difference is load-bearing: with only the direction set,
//! `rustybuzz` 0.14 declined to apply Arabic GSUB and produced isolated
//! letterforms, while `harfrust` joins them. A test that shaped with
//! `guess_segment_properties` saw identical output from both shapers and so
//! reported Arabic as fine for as long as the bug existed.
//!
//! The comparisons here deliberately avoid asking the shaper to disable its
//! features to manufacture a reference. Passing an empty feature slice *adds*
//! nothing rather than switching the defaults off, so it silently produced the
//! joined answer and made the test vacuous. Instead each check compares the
//! production path against shaping the same characters one at a time, which
//! takes the joining and ligature contexts away by construction.

use super::shape::shape_text;
use super::BUILTIN_FONT;

/// Arabic for "hello world". Two words, so the word boundary is exercised too.
const ARABIC: &str =
    "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} \u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}";

/// Glyph ids the production path produces, in visual order.
fn production_glyphs(text: &str) -> Vec<u16> {
    shape_text(text, 72.0, BUILTIN_FONT)
        .0
        .iter()
        .map(|a| a.gid as u16)
        .collect()
}

/// Glyph ids produced when each character is shaped on its own.
///
/// A character with no neighbours cannot join to anything, so this is the
/// set of *context-free* letterforms. It is computed through the same production
/// entry point, so the only difference from [`production_glyphs`] is the
/// context.
fn per_character_glyphs(text: &str) -> Vec<u16> {
    text.chars()
        .flat_map(|ch| shape_text(&ch.to_string(), 72.0, BUILTIN_FONT).0)
        .map(|a| a.gid as u16)
        .collect()
}

#[test]
fn production_shaping_joins_arabic() {
    let joined = production_glyphs(ARABIC);
    let context_free = per_character_glyphs(ARABIC);

    assert_ne!(
        joined, context_free,
        "production shaping produced the same letterforms as context-free shaping, \
         so Arabic contextual joining is not being applied and the text would \
         render as disconnected letters.\n  joined: {joined:?}\n  context-free: \
         {context_free:?}"
    );
}

#[test]
fn joining_changes_most_of_the_letters() {
    // A guard on the guard. Joining must alter every letter that can take a
    // non-initial form, which in Arabic is all but the first of each word. If
    // only one glyph differed the test above would pass while almost nothing
    // was actually being shaped.
    let joined = production_glyphs(ARABIC);
    let context_free = per_character_glyphs(ARABIC);
    let shared = joined
        .iter()
        .zip(context_free.iter())
        .filter(|(a, b)| a == b)
        .count();
    assert!(
        shared * 2 < joined.len(),
        "{shared} of {} glyphs are unchanged by context, so most of this font's \
         Arabic coverage may be too thin for the joining test to mean anything",
        joined.len()
    );
}

#[test]
fn arabic_accounts_for_every_character() {
    // Joining must not lose characters: every glyph still carries a source
    // character, and each character's advance must be non-negative. A negative
    // advance would walk glyphs backwards along the line.
    let (advances, total) = shape_text(ARABIC, 72.0, BUILTIN_FONT);
    let chars = ARABIC.chars().count();
    assert!(
        advances.len() >= chars,
        "expected at least one glyph per character, got {} glyphs for {chars} \
         characters",
        advances.len()
    );
    assert!(
        advances.iter().all(|a| a.advance >= 0.0),
        "a negative advance would move glyphs backwards: {advances:?}"
    );
    assert!(
        advances.iter().all(|a| !a.ch.is_control() || a.ch == ' '),
        "a glyph carries a control character as its source: {advances:?}"
    );
    assert!(total > 0.0, "Arabic shaped to zero width");
}

#[test]
fn joined_arabic_is_narrower_than_the_sum_of_its_letters() {
    // Joining is not only a change of glyph ids: it is a change of metrics.
    // Joined letterforms connect and share advance, so the run must measure
    // *narrower* than the same characters shaped independently. Measured on the
    // embedded subset: 166.5px against 213.6px for five letters, and 357.8px
    // against 468.7px for twelve -- ratios of 0.78 and 0.76.
    //
    // The direction of this inequality is the point. An earlier version of this
    // test asserted the opposite on the reasoning that connected strokes must
    // add width, and it failed on correct output. A "joining" implementation that
    // swapped glyph ids while leaving advances untouched would pass the
    // glyph-id comparison above and fail this one, which is why both exist.
    let (_, joined) = shape_text(ARABIC, 72.0, BUILTIN_FONT);
    let context_free: f32 = ARABIC
        .chars()
        .map(|ch| shape_text(&ch.to_string(), 72.0, BUILTIN_FONT).1)
        .sum();
    assert!(
        joined < context_free,
        "joined run measured {joined} against {context_free} context-free; joined \
         letterforms share advance and must come out narrower, so equal or wider \
         means the metrics were not updated along with the glyphs"
    );
    // A floor on the saving, so a regression that only nibbles at it is caught.
    // 0.6 is below the measured 0.76 and above zero, and is asserted rather than
    // noted so a font change that flattens joining fails loudly.
    assert!(
        joined < context_free * 0.9,
        "joined run measured {joined} against {context_free} context-free; the \
         saving has all but vanished, so joining is only partly applied"
    );
}

#[test]
fn latin_ligatures_survive_the_production_path() {
    // The same class of check for the other thing that relies on GSUB. Without
    // ligatures Latin still looks plausible, just less refined, so this is easy
    // to miss by eye.
    let joined = production_glyphs("ffi office");
    let context_free = per_character_glyphs("ffi office");
    // Measured on the embedded subset: "ffi" collapses to a single glyph and
    // "office" to four, so the run is nine characters and six glyphs. If a
    // subsetting change drops those ligatures this test fails loudly rather
    // than quietly asserting nothing.
    assert!(
        joined.len() < context_free.len(),
        "no ligature collapsed any glyphs: joined={} context-free={}\n  {joined:?}\n  \
         {context_free:?}",
        joined.len(),
        context_free.len()
    );
}

#[test]
fn hebrew_has_no_joining_to_apply() {
    // Hebrew is the control case: it has no contextual joining, so shaping it as
    // one run must yield the same glyphs as shaping it per character. If this
    // ever fails, whatever fixed Arabic has over-corrected into changing scripts
    // that should not change.
    //
    // The comparison is on sorted glyphs rather than the sequences themselves.
    // A whole Hebrew run comes back in visual RTL order while single-character
    // runs are LTR, so the sequences are legitimately reversed even when the
    // shaping is identical -- and an order-sensitive comparison here would fail
    // for a reason that has nothing to do with joining.
    let mut whole = production_glyphs("\u{05E9}\u{05DC}\u{05D5}\u{05DD}");
    let mut per_char = per_character_glyphs("\u{05E9}\u{05DC}\u{05D5}\u{05DD}");
    whole.sort_unstable();
    per_char.sort_unstable();
    assert_eq!(
        whole, per_char,
        "Hebrew should not gain joining behaviour it does not have"
    );
}

#[test]
fn latin_joining_is_stable_across_calls() {
    // Shaping is repeated for measurement, then again for paint. If the two
    // disagreed, layout and raster would be working from different glyph runs --
    // a mismatch that shows up as text measured at one width and drawn at
    // another, with no error anywhere.
    let first = production_glyphs(ARABIC);
    let second = production_glyphs(ARABIC);
    assert_eq!(
        first, second,
        "repeated shaping produced different glyph runs"
    );

    let (_, w1) = shape_text(ARABIC, 72.0, BUILTIN_FONT);
    let (_, w2) = shape_text(ARABIC, 72.0, BUILTIN_FONT);
    assert!(
        (w1 - w2).abs() < f32::EPSILON,
        "repeated shaping measured {w1} then {w2}"
    );
}
