//! Blend modes and shadow kinds.
//!
//! Two things are being guarded, and they are different in kind.
//!
//! For blend modes the risk is *silent divergence*: `Normal` has to stay
//! byte-identical to the pre-blend-mode path, because every golden digest in the
//! project was blessed against it. A rounding change in the "normal" case would
//! move 16 corpus digests and the determinism card without anything being
//! visually wrong, and the natural response would be to re-bless. So the
//! compatibility assertion comes first and is deliberately the strictest test in
//! the file.
//!
//! For shadow kinds the risk is *plausible-looking wrongness*. An inset shadow
//! built as a dark overlay rather than a subtraction looks like a shadow at a
//! glance, so the tests assert geometry — that the band is where it should be and
//! the interior is untouched.

use hikari::{render_png, BlendMode, Node, ShadowKind, Style};

/// Read one pixel as `[r, g, b, a]`.
fn at(png: &[u8], x: u32, y: u32) -> [u8; 4] {
    let img = image::load_from_memory(png).expect("decode").to_rgba8();
    img.get_pixel(x, y).0
}

/// A child box over a full-bleed backdrop, so the blend has a painted parent to
/// act on. Both boxes are the full canvas, so the sample point is unambiguous:
/// `Style::row` does not centre, and a 100px child in a 200px row is easy to
/// misread as "the blend did nothing" when it is actually "the geometry was
/// wrong".
fn layered(blend: BlendMode) -> Vec<u8> {
    render_png(
        &Node::container(
            Style::row()
                .with_size(100.0, 100.0)
                .with_background("#ff0000"),
            vec![Node::container(
                Style::new()
                    .with_size(100.0, 100.0)
                    .with_background("#00ff00")
                    .with_blend_mode(blend),
                vec![],
            )],
        ),
        100,
        100,
    )
    .expect("render")
}

#[test]
fn normal_is_byte_identical_to_naming_nothing() {
    // The compatibility contract. `Some(Normal)` and `None` must produce the
    // same bytes, and both must match the digest corpus. If this fails, a
    // pre-existing render changed and the fix is in the code, not in --bless.
    let explicit = layered(BlendMode::Normal);
    let implicit = render_png(
        &Node::container(
            Style::row()
                .with_size(100.0, 100.0)
                .with_background("#ff0000"),
            vec![Node::container(
                Style::new()
                    .with_size(100.0, 100.0)
                    .with_background("#00ff00"),
                vec![],
            )],
        ),
        100,
        100,
    )
    .expect("render");
    assert_eq!(
        explicit, implicit,
        "BlendMode::Normal changed the render; every existing digest is now stale"
    );
}

#[test]
fn multiply_darkens_where_it_overlaps() {
    // Multiply of white over grey is grey; the result must be no lighter than the
    // backdrop. The key assertion is that the overlap differs from `Normal`,
    // since "looks plausible" is the failure mode here.
    let multiplied = layered(BlendMode::Multiply);
    let normal = layered(BlendMode::Normal);
    assert_ne!(
        multiplied, normal,
        "multiply produced the same pixels as normal"
    );

    // Green multiplied by red is black. Anything else means the mode is not
    // being applied at all — which is the bug this caught, where the solid-fill
    // path silently ignored the blend.
    let px = at(&multiplied, 50, 50);
    assert!(
        px[0] < 40 && px[1] < 40 && px[2] < 40,
        "green over red with multiply should be near black, got {px:?}"
    );
    // And the unblended render is plain green, so the assertion above is
    // measuring the blend rather than the geometry.
    let plain = at(&normal, 50, 50);
    assert_eq!(
        plain,
        [0, 255, 0, 255],
        "unblended backdrop changed: {plain:?}"
    );
}

#[test]
fn screen_lightens_where_it_overlaps() {
    // Screen is the inverse of multiply: green over red goes to yellow.
    let screened = at(&layered(BlendMode::Screen), 50, 50);
    let normal = at(&layered(BlendMode::Normal), 50, 50);
    assert_eq!(normal, [0, 255, 0, 255], "unblended backdrop changed");
    assert!(
        screened[0] > 200 && screened[1] > 200 && screened[2] < 60,
        "green over red with screen should be yellow, got {screened:?}"
    );
}

#[test]
fn every_mode_renders_without_erroring() {
    // A smoke floor across the whole enum: a match arm that panics or produces
    // NaN would otherwise only surface for whichever mode a user picked.
    for mode in [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ] {
        let png = layered(mode);
        assert!(
            png.len() > 100,
            "{mode:?} produced a suspiciously small render"
        );
        // A blend must never punch a transparent hole where a box was drawn, and
        // must never produce a channel outside 0..=255 by overflowing.
        let px = at(&png, 50, 50);
        assert_eq!(px[3], 255, "{mode:?} left the box area transparent: {px:?}");
    }
}

#[test]
fn non_separable_modes_degrade_to_normal_for_text() {
    // The non-separable four mix whole-colour functions, not per channel, so the
    // text path routes them to `Normal`. It must degrade *quietly and identically*
    // rather than producing a wrong colour: text is where a caller is most likely
    // to set a mode, so a wrong result would be glaring.
    let tree = |mode: BlendMode| {
        Node::container(
            Style::centered()
                .with_size(400.0, 200.0)
                .with_background("#808080"),
            vec![Node::text(
                "Hue",
                Style::text(60.0, "#ffffff").with_blend_mode(mode),
            )],
        )
    };
    let hue = render_png(&tree(BlendMode::Hue), 400, 200).expect("render");
    let normal = render_png(&tree(BlendMode::Normal), 400, 200).expect("render");
    assert_eq!(
        hue, normal,
        "a non-separable mode should degrade to Normal on text, not blend wrongly"
    );
}

#[test]
fn blend_applies_to_text_and_images_not_just_fills() {
    // A blend mode that only reached fills would be a trap: the caller sets it on
    // a box expecting the whole box to blend, and the text on it stays opaque.
    // Rendering white text over a mid-grey box with `difference` should invert the
    // backdrop where the glyphs are.
    let plain = render_png(
        &Node::container(
            Style::centered()
                .with_size(400.0, 200.0)
                .with_background("#404040"),
            vec![Node::text("X", Style::text(120.0, "#ffffff"))],
        ),
        400,
        200,
    )
    .expect("render");
    let differenced = render_png(
        &Node::container(
            Style::centered()
                .with_size(400.0, 200.0)
                .with_background("#404040"),
            vec![Node::text(
                "X",
                Style::text(120.0, "#ffffff").with_blend_mode(BlendMode::Difference),
            )],
        ),
        400,
        200,
    )
    .expect("render");
    assert_ne!(
        plain, differenced,
        "a blend mode on text changed nothing; the blend did not reach the glyph path"
    );
}

// --- shadow kinds -----------------------------------------------------------

/// A light box with a shadow of `kind`, so the shadow is visible as a change.
fn shadowed(kind: ShadowKind, dx: f32, dy: f32, blur: f32) -> Vec<u8> {
    render_png(
        &Node::container(
            Style::centered()
                .with_size(300.0, 300.0)
                .with_background("#ffffff"),
            vec![Node::container(
                Style::new()
                    .with_size(200.0, 140.0)
                    .with_background("#3b82f6")
                    .with_radius(12.0)
                    .with_shadow_kind(kind, dx, dy, blur, 0.0, "#000000"),
                vec![],
            )],
        ),
        300,
        300,
    )
    .expect("render")
}

#[test]
fn drop_shadow_falls_outside_the_box() {
    let png = shadowed(ShadowKind::Drop, 0.0, 14.0, 12.0);
    // The box is centered, so it spans x 50..250, y 80..220. A drop shadow
    // offset downward must darken pixels *below* the bottom edge.
    let below = at(&png, 150, 232);
    assert!(
        below[0] < 250,
        "a drop shadow offset 14px down did not darken below the box: {below:?}"
    );
}

#[test]
fn inset_shadow_stays_inside_the_box() {
    let png = shadowed(ShadowKind::InsetTop, 0.0, 4.0, 10.0);
    // Below the box must be untouched: an inset cannot paint outside the silhouette.
    let below = at(&png, 150, 232);
    assert_eq!(
        below,
        [255, 255, 255, 255],
        "an inset shadow leaked outside the box: {below:?}"
    );
}

#[test]
fn inset_shadow_darkens_the_top_edge_not_the_whole_interior() {
    // The defining property of an inset. If it were a dark overlay the centre
    // would darken too, which is exactly the "looks like a shadow but is not one"
    // bug this asserts against.
    let png = shadowed(ShadowKind::InsetTop, 0.0, 4.0, 10.0);
    let near_top = at(&png, 150, 82);
    let centre = at(&png, 150, 150);
    assert!(
        near_top[2] < centre[2] - 30,
        "an inset top shadow should darken the top edge ({near_top:?}) far more \
         than the centre ({centre:?})"
    );
    // The interior is the box's own colour, undimmed.
    assert_eq!(
        centre,
        [59, 130, 246, 255],
        "an inset shadow dimmed the whole interior: {centre:?}"
    );
}

#[test]
fn inset_edge_paints_a_hard_band() {
    // The band is exactly as thick as the offset, with no falloff. Probed with a
    // 12px offset because a 3px band is easy to sample past by accident.
    let png = shadowed(ShadowKind::InsetEdge, 0.0, 12.0, 0.0);
    let in_band = at(&png, 150, 86);
    let below_band = at(&png, 150, 100);
    let box_colour = [59u8, 130, 246, 255];
    assert_eq!(
        below_band, box_colour,
        "the interior was altered: {below_band:?}"
    );
    assert!(
        in_band[2] < box_colour[2] - 60,
        "a 12px inset band should be strongly darkened at 6px in, got {in_band:?}"
    );
}

#[test]
fn inset_edge_has_no_blur() {
    // `InsetEdge` is the hard-edged variant. A blur radius must be ignored, so
    // the band is the same whether blur is 0 or a large value.
    let sharp = shadowed(ShadowKind::InsetEdge, 0.0, 12.0, 0.0);
    let supposedly_ignored = shadowed(ShadowKind::InsetEdge, 0.0, 12.0, 40.0);
    assert_eq!(
        sharp, supposedly_ignored,
        "InsetEdge applied a blur; it is meant to be the hard-edged variant"
    );
}

#[test]
fn inset_and_drop_differ() {
    assert_ne!(
        shadowed(ShadowKind::Drop, 0.0, 10.0, 10.0),
        shadowed(ShadowKind::InsetTop, 0.0, 10.0, 10.0),
        "the two shadow kinds rendered identically"
    );
}

#[test]
fn shadow_kind_round_trips_through_json() {
    // A tree comes from JSON in the bindings. An absent or unknown `kind` must
    // still deserialize to `Drop`, because that is what a pre-v0.20 tree meant and
    // silently defaulting to something else would change old documents' output.
    let tree = r#"{"Container":{"style":{"width":10,"height":10,"display":"Block",
        "shadow":{"dx":0,"dy":2,"blur":4,"spread":0,"color":{"r":0,"g":0,"b":0,"a":128}}},"children":[]}}"#;
    let node: Node = serde_json::from_str(tree).expect("parse tree without a shadow kind");
    let style = match &node {
        Node::Container { style, .. } => style,
        _ => panic!("expected a container"),
    };
    let shadow = style.shadow.expect("shadow parsed");
    assert_eq!(
        shadow.kind,
        ShadowKind::Drop,
        "a shadow with no `kind` must default to Drop, not something that \
         changes the render of an older document"
    );
}

#[test]
fn dodge_blend_survives_a_fully_white_source() {
    // A regression guard for a process abort, not a wrong pixel.
    //
    // The color-dodge formula divides by `255 - blend`, which is zero when the
    // source is pure white. A `u32` division by zero aborts rather than panics,
    // so this took down the entire test binary — and it only reproduced on CI,
    // because it needs a glyph pixel with full coverage. The bug was live for
    // the whole of the blend-mode change and only a Linux aarch64 runner
    // produced the input.
    //
    // White text on a mid-grey box is the input. Asserting that the render
    // *succeeds* is the whole test: there is no pixel value to check, because
    // the failure mode was the process dying.
    for mode in [
        BlendMode::ColorDodge,
        BlendMode::HardLight,
        BlendMode::ColorBurn,
    ] {
        let png = render_png(
            &Node::container(
                Style::centered()
                    .with_size(400.0, 200.0)
                    .with_background("#404040"),
                vec![Node::text(
                    "White",
                    Style::text(120.0, "#ffffff").with_blend_mode(mode),
                )],
            ),
            400,
            200,
        )
        .unwrap_or_else(|e| panic!("{mode:?} failed to render: {e}"));
        assert!(png.len() > 500, "{mode:?} produced almost nothing");
    }
}

#[test]
fn every_mode_over_a_full_range_of_backdrops() {
    // The abort above needed a specific backdrop. Sweeping the backdrops means
    // the next degenerate input in a blend formula surfaces here rather than on
    // someone else's machine.
    let backdrops = [
        "#000000", "#404040", "#808080", "#c0c0c0", "#ffffff", "#ff0000",
    ];
    for mode in [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ] {
        for bg in backdrops {
            let png = render_png(
                &Node::container(
                    Style::centered()
                        .with_size(300.0, 120.0)
                        .with_background(bg),
                    vec![Node::text(
                        "Ag",
                        Style::text(64.0, "#ffffff").with_blend_mode(mode),
                    )],
                ),
                300,
                120,
            )
            .unwrap_or_else(|e| panic!("{mode:?} over {bg} failed to render: {e}"));
            assert!(png.len() > 200, "{mode:?} over {bg} produced nothing");
        }
    }
}

/// Every separable mode, i.e. the ones the integer path implements.
const MODES: [BlendMode; 11] = [
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Lighten,
    BlendMode::Darken,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Normal,
];

/// Mid-grey text over a dark backdrop.
fn img(mode: BlendMode) -> image::RgbaImage {
    let png = render_png(
        &Node::container(
            Style::centered()
                .with_size(400.0, 200.0)
                .with_background("#202020"),
            vec![Node::text(
                "IIII",
                Style::text(150.0, "#808080").with_blend_mode(mode),
            )],
        ),
        400,
        200,
    )
    .expect("render");
    image::load_from_memory(&png).expect("decode").to_rgba8()
}

/// The most-covered glyph pixel.
///
/// Found by scanning rather than hardcoding a coordinate, which would depend on
/// hinting and exact metrics and would silently test background instead of a
/// glyph stem the moment either changed.
fn peak(mode: BlendMode) -> u8 {
    img(mode)
        .pixels()
        .map(|p| p.0[0])
        .max()
        .expect("non-empty image")
}

#[test]
fn normal_text_is_unchanged() {
    // The reference: grey text on a dark backdrop. Every other mode is compared
    // against this, so a regression in the blend shows as a *difference* rather
    // than needing a hardcoded pixel value.
    assert_eq!(peak(BlendMode::Normal), 0x80);
}

#[test]
fn every_separable_mode_changes_the_glyph_colour() {
    // The core assertion. With the saturation bug all eleven read 255; with a
    // mid-grey source over a dark backdrop, none of them should.
    for mode in MODES {
        let v = peak(mode);
        assert_ne!(
            v, 255,
            "{mode:?} peaked at 255 — saturated, so the blend did nothing"
        );
    }
}

#[test]
fn multiply_text_is_darker_than_normal() {
    // 0x80 * 0x20 / 255 ≈ 0x10, so multiply darkens sharply against a 0x80 normal.
    let mul = peak(BlendMode::Multiply);
    assert!(
        mul < 0x40,
        "multiply of grey over near-black should be much darker, got {mul}"
    );
}

#[test]
fn screen_text_is_lighter_than_normal() {
    // screen of 0x80 over 0x20 ≈ 0x88, lighter than the 0x80 normal.
    let screen = peak(BlendMode::Screen);
    assert!(
        screen > 0x80,
        "screen of grey over near-black should lighten, got {screen}"
    );
}

#[test]
fn difference_text_inverts_the_gap() {
    // |0x80 - 0x20| = 0x60.
    let dif = peak(BlendMode::Difference);
    assert!(
        (0x40..0x80).contains(&dif),
        "difference should land near 0x60, got {dif}"
    );
}

#[test]
fn hardlight_is_not_color_dodge() {
    // A dark source takes hard-light's multiply branch, so it darkens; color-dodge
    // with a dark source barely moves. This is the assertion that caught hard-light
    // being aliased to dodge.
    let hard = peak(BlendMode::HardLight);
    let dodge = peak(BlendMode::ColorDodge);
    assert_ne!(
        hard, dodge,
        "hard-light behaved identically to color-dodge: {hard}"
    );
}

#[test]
fn the_non_separable_modes_degrade_to_normal_on_text() {
    // Documented behaviour, pinned: these four mix whole-colour functions, so the
    // integer path routes them to `Normal` rather than producing a wrong colour.
    for mode in [
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ] {
        assert_eq!(
            peak(mode),
            peak(BlendMode::Normal),
            "{mode:?} should degrade to Normal on the glyph path"
        );
    }
}
