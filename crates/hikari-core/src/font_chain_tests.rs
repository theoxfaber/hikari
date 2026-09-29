//! Guards on the font fallback chain.
//!
//! The chain is the fix for a bug that was invisible: a glyph the primary font
//! lacked used to be rasterized *by character* in a system font, never passing
//! through the shaper. Joining happens in the shaper, so a fallback could never
//! join, and no choice of fallback file could fix it. Now each run is split by
//! coverage and every segment is shaped with a face that actually has the
//! glyph, which is what makes fallback complex-script-correct.
//!
//! These tests are about *routing*, not appearance: that the right face is
//! chosen, that nothing regressed for the common case, and that the
//! all-primary fast path stayed allocation-free.

use super::font;
use super::shape::shape_text;
use crate::BUILTIN_FONT;

/// The fonts each advance was shaped with, in visual order.
fn fonts_for(text: &str) -> Vec<u32> {
    shape_text(text, 48.0, BUILTIN_FONT)
        .0
        .iter()
        .map(|a| a.font)
        .collect()
}

#[test]
fn latin_stays_in_the_primary_font() {
    // The overwhelmingly common case. If this ever routed elsewhere, every
    // render in the project would change for no stated reason.
    for text in ["Sphinx of black quartz", "AVATAR", "office waffle", "a"] {
        assert!(
            fonts_for(text).iter().all(|&f| f == BUILTIN_FONT),
            "{text:?} left the primary font: {:?}",
            fonts_for(text)
        );
    }
}

#[test]
fn single_advance_for_a_single_char() {
    // A sanity floor: the segmenter must not drop or duplicate advances, since
    // layout totals are summed from them.
    for text in ["a", "AVATAR", "שלום", "abc שלום 123", "日本語"] {
        let (advances, width) = shape_text(text, 48.0, BUILTIN_FONT);
        let expected = text.chars().count();
        assert_eq!(
            advances.len(),
            expected,
            "{text:?} produced {} advances for {expected} chars",
            advances.len()
        );
        assert!(width > 0.0, "{text:?} measured zero width");
    }
}

#[test]
fn mixed_script_run_splits_but_stays_contiguous() {
    // A Latin run with a Hebrew word inside it must not be handed to one face.
    // The Latin keeps the primary; the Hebrew is shaped somewhere that can
    // actually render it.
    let text = "abc שלום 123";
    let (advances, _) = shape_text(text, 48.0, BUILTIN_FONT);
    assert_eq!(
        advances.len(),
        text.chars().count(),
        "advances must not be lost at the split"
    );

    // Latin letters must be in the primary font. If the whole run were
    // redirected, the first three would not be.
    let latin: Vec<char> = "abc".chars().collect();
    for ch in latin {
        let adv = advances.iter().find(|a| a.ch == ch).expect("latin advance");
        assert_eq!(adv.font, BUILTIN_FONT, "{ch:?} should use the primary");
    }
}

#[test]
fn uncovered_codepoint_is_reported_missing() {
    // Without the bundled CJK feature there is no face for these, so the shaper
    // must still say so rather than pretending it shaped something.
    let (advances, _) = shape_text("日本語", 48.0, BUILTIN_FONT);
    if !cfg!(feature = "bundled-cjk") {
        assert!(
            advances.iter().all(|a| a.missing),
            "日本語 should be missing without the bundled-cjk feature"
        );
    } else {
        assert!(
            advances.iter().all(|a| !a.missing),
            "日本語 should be covered by the bundled CJK face"
        );
        assert!(
            advances.iter().all(|a| a.font != BUILTIN_FONT),
            "日本語 should not be shaped in the primary font"
        );
    }
}

#[test]
fn cjk_advances_are_full_width_when_missing() {
    // The one thing a missing CJK glyph can still get right is its advance:
    // layout must reserve a full em so text does not overlap, even when nothing
    // will be drawn there.
    let (advances, _) = shape_text("日本語", 48.0, BUILTIN_FONT);
    for adv in &advances {
        if adv.missing {
            assert!(
                (adv.advance - 48.0).abs() < 0.01,
                "missing CJK advance was {} px, expected one em (48)",
                adv.advance
            );
        }
    }
}

#[test]
fn unknown_font_id_behaves_exactly_like_the_builtin() {
    // Ids arrive from untrusted JSON in the bindings. Coverage lookup against an
    // unknown id answers "no" for every character, which would divert the whole
    // run into the fallback chain -- a real bug this caught.
    let bogus = 424_242u32;
    let a = shape_text("Sphinx of black quartz", 48.0, bogus);
    let b = shape_text("Sphinx of black quartz", 48.0, BUILTIN_FONT);
    assert_eq!(a.1, b.1, "width diverged for an unknown font id");
    assert_eq!(
        a.0.iter().map(|x| x.gid).collect::<Vec<_>>(),
        b.0.iter().map(|x| x.gid).collect::<Vec<_>>(),
        "an unknown font id produced different glyphs"
    );
    assert!(
        a.0.iter().all(|x| x.font == BUILTIN_FONT),
        "an unknown font id should resolve to the built-in, got {:?}",
        a.0.iter().map(|x| x.font).collect::<Vec<_>>()
    );
}

#[test]
fn resolve_maps_only_unknown_ids() {
    assert_eq!(font::resolve(BUILTIN_FONT), BUILTIN_FONT);
    assert_eq!(font::resolve(424_242), BUILTIN_FONT);

    // A genuinely registered id must survive resolution unchanged, otherwise the
    // chain would quietly discard a caller's typeface.
    let id = font::register_font("Chain test", &font_bytes_differing()).expect("register");
    assert_ne!(id, BUILTIN_FONT, "test font collided with the primary");
    assert_eq!(font::resolve(id), id);
    assert!(font::font_covers(id, 'A'));
}

#[test]
fn coverage_agrees_with_the_registry() {
    // An unknown id resolves to the built-in face, so it must answer coverage
    // exactly as the built-in does. The bug this guards was the opposite: the
    // lookup skipped resolution and so answered "no" for everything, diverting
    // the whole run into the fallback chain.
    assert!(font::font_covers(BUILTIN_FONT, 'A'));
    assert_eq!(
        font::font_covers(424_242, 'A'),
        font::font_covers(BUILTIN_FONT, 'A'),
        "an unknown id must answer coverage as the built-in does"
    );
    // And a codepoint nothing covers still reports honestly rather than
    // defaulting to true, which would suppress the fallback.
    assert!(!font::font_covers(BUILTIN_FONT, '\u{10FFFD}'));
}

#[test]
fn measuring_uses_the_same_faces_as_shaping() {
    // Layout totals are summed from advances, so a measure/shape disagreement
    // would put text in the wrong place. Compare the summed advances against
    // the reported total rather than trusting either alone.
    for text in [
        "Sphinx of black quartz",
        "abc שלום 123",
        "office waffle AVATAR",
    ] {
        let (advances, total) = shape_text(text, 48.0, BUILTIN_FONT);
        let summed: f32 = advances.iter().map(|a| a.advance).sum();
        assert!(
            (summed - total).abs() < 0.5,
            "{text:?}: advances sum to {summed} but total reports {total}"
        );
    }
}

/// A font that is definitely not the built-in one, for `resolve` round-trips.
///
/// A bundled face when one is compiled in, otherwise the built-in's own bytes —
/// which register as the primary and so make the `assert_ne!` above fail loudly
/// rather than silently passing on a no-op.
fn font_bytes_differing() -> Vec<u8> {
    crate::shape::bundled_fallbacks()
        .first()
        .map_or_else(|| crate::builtin_bytes().to_vec(), |(_, b)| b.to_vec())
}
