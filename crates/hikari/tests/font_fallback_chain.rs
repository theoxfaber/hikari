//! End-to-end proof that the font fallback chain reaches the rasterizer.
//!
//! The chain is only worth anything if the *pixels* change. A shaper that
//! correctly reports "this glyph belongs to font 2" while the painter keeps
//! drawing from font 0 would pass every unit test in `hikari-core` and still
//! render nothing. So these tests decode the PNG and count ink, which is the
//! only assertion that actually proves glyphs were drawn.
//!
//! The CJK half is gated on the `bundled-cjk` feature. Without it, CJK falls
//! back to a *system* font, so whether anything renders depends on the host —
//! which is the whole reason the feature exists.

use hikari::{render_png, Node, Style};

/// Count pixels that are neither the background nor transparent.
///
/// The background is opaque `#0b1020`, so anything that differs from it is
/// glyph coverage. Anti-aliased edges count too, which is what makes this
/// sensitive to a glyph being drawn at all rather than to its exact shape.
fn ink_pixels(png: &[u8]) -> usize {
    let img = image::load_from_memory(png).expect("decode png").to_rgba8();
    img.pixels()
        .filter(|p| p[0] > 24 || p[1] > 30 || p[2] > 48)
        .count()
}

fn render_text(text: &str) -> Vec<u8> {
    render_png(
        &Node::container(
            Style::row()
                .with_size(900.0, 200.0)
                .with_background("#0b1020"),
            vec![Node::text(text, Style::text(90.0, "#ffffff"))],
        ),
        900,
        200,
    )
    .expect("render")
}

#[test]
fn latin_ink_reaches_the_canvas() {
    // The floor every other case is measured against: if this is 0 the counting
    // itself is broken and a passing CJK test would mean nothing.
    // Measured: 5657 at 90 px. The floor is a quarter of that, which still fails
    // loudly if text stops being drawn while tolerating a font change.
    assert!(
        ink_pixels(&render_text("Hikari")) > 1_400,
        "Latin text drew almost nothing, so the ink metric is unreliable"
    );
}

#[test]
fn arabic_ink_reaches_the_canvas() {
    // Arabic is in the primary font, so this must keep working. It is the
    // regression guard for the segmenter: any run split or coverage check that
    // misroutes would empty this.
    // Measured: 3441.
    assert!(ink_pixels(&render_text("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}")) > 1_200);
}

#[test]
fn hebrew_ink_reaches_the_canvas() {
    // Measured: 3868.
    assert!(ink_pixels(&render_text("\u{05E9}\u{05DC}\u{05D5}\u{05DD}")) > 1_200);
}

#[cfg(not(feature = "bundled-cjk"))]
#[test]
fn cjk_without_the_feature_depends_on_the_host() {
    // Without a CJK face, CJK reaches the *system* fallback — a proprietary
    // macOS font on a Mac, nothing at all on a bare Linux CI box. So the honest
    // assertion is not "absent" but "not ours": either a system font drew it,
    // or nothing did, and both are correct. Pinning it to 0 would pass on CI and
    // fail on any developer Mac, which is exactly the machine-dependent
    // threshold mistake this project has already made once.
    //
    // What matters is that the bundled feature removes the host dependency,
    // which `cjk_ink_reaches_the_canvas_through_the_chain` covers.
    let ink = ink_pixels(&render_text("\u{65E5}\u{672C}\u{8A9E}"));
    assert!(
        ink == 0 || ink > 1_000,
        "CJK drew {ink} ink pixels, which is neither nothing nor a glyph"
    );
    eprintln!("cjk without bundled-cjk: {ink} ink pixels (host-dependent)");
}

#[cfg(feature = "bundled-cjk")]
#[test]
fn cjk_ink_reaches_the_canvas_through_the_chain() {
    // The payoff of the whole change: glyphs the primary font lacks still get
    // drawn, from a face the shaper selected.
    //
    // The threshold is measured, not guessed: at 90 px on a 900x200 card this
    // string inks 3940 pixels, against 5657 for `Hikari` and 3441 for Arabic.
    // The bar sits well under that but far above zero, so a missing glyph, a
    // blank fallback or a lost segment all fail. There is deliberately no upper
    // bound: ink can only be added by drawing something real, and a cap would
    // fail on a legitimate font change.
    let ink = ink_pixels(&render_text("\u{65E5}\u{672C}\u{8A9E}"));
    assert!(
        ink > 1_500,
        "CJK inked only {ink} pixels against a measured 3940; the fallback \
         chain did not reach the rasterizer"
    );
}

#[cfg(feature = "bundled-cjk")]
#[test]
fn mixed_script_run_draws_every_part() {
    // Latin, CJK and Arabic in one line. Each part must be drawn, which is only
    // true if per-segment font routing survives all the way to the painter.
    //
    // The parts do not sum: rendered separately, each string is centered in the
    // card and so spreads over the full width, while the mixed string packs
    // three scripts into one line and their ink overlaps. 9542 measured against
    // a floor of 7000, which catches a whole missing script without depending on
    // the exact spacing.
    let ink = ink_pixels(&render_text(
        "HI \u{65E5}\u{672C}\u{8A9E} \u{0645}\u{0631}\u{062D}\u{0628}\u{0627}",
    ));
    assert!(
        ink > 7_000,
        "mixed run inked only {ink} pixels; a script was dropped"
    );
}
