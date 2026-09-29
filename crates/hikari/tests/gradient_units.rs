//! Gradient unit consistency between the raster and SVG backends.
//!
//! # What this catches
//!
//! `Style::with_radial_gradient` documents its centre as a box *fraction*, and
//! `radius` follows the same convention. The rasterizer passed `radius` to
//! `tiny_skia::RadialGradient` as if it were pixels, so `0.6` became a
//! half-pixel gradient and every other pixel fell outside it and clamped to the
//! final stop. The render was a flat fill in the last colour — 5,280 bytes that
//! looked like a small, plausible image.
//!
//! Nothing caught it because the golden digest was blessed from the broken
//! output, and a flat gradient is a perfectly stable thing to hash. A digest
//! proves stability, not correctness, which is why these assertions check the
//! actual pixel values at specific points rather than a hash.

use hikari::{render_png, render_svg, Node, Style};

/// The lightest stop, placed at the centre.
const INNER: &str = "#fde68a";
/// The darkest stop, at the rim.
const OUTER: &str = "#7c2d12";

fn radial(radius: f32, w: u32, h: u32) -> Vec<u8> {
    let tree = Node::container(
        Style::new()
            .with_size(w as f32, h as f32)
            .with_radial_gradient(0.5, 0.5, radius, &[(0.0, INNER), (1.0, OUTER)]),
        vec![],
    );
    render_png(&tree, w, h).expect("render radial")
}

fn red_at(png: &[u8], x: u32, y: u32) -> u8 {
    let img = image::load_from_memory(png).expect("decode").to_rgba8();
    img.get_pixel(x, y).0[0]
}

#[test]
fn radial_gradient_actually_varies() {
    // The regression this whole file exists for: a 0.6 radius produced a
    // uniform image. Distinct red values across the box is the minimum proof
    // that the ramp is being evaluated at all.
    let png = radial(0.6, 600, 600);
    let img = image::load_from_memory(&png).expect("decode").to_rgba8();
    let distinct: std::collections::HashSet<u8> = img.pixels().map(|p| p.0[0]).collect();
    assert!(
        distinct.len() > 50,
        "radial gradient produced only {} distinct values; it is flat",
        distinct.len()
    );
}

#[test]
fn radial_gradient_is_lightest_at_the_centre() {
    // Not just "different values" but the right ones: the inner stop belongs at
    // the centre and the outer stop at the rim. A gradient with its stops
    // inverted would still vary and still pass a distinct-count check.
    let png = radial(0.6, 600, 600);
    let centre = red_at(&png, 300, 300);
    let corner = red_at(&png, 4, 4);
    assert!(
        centre > corner + 60,
        "centre {centre} should be much lighter than the corner {corner}"
    );
    // And the centre should be near the inner stop (253), not merely lighter.
    assert!(
        centre >= 230,
        "centre red was {centre}, expected near the inner stop 253"
    );
}

#[test]
fn radius_is_a_box_fraction_not_pixels() {
    // 0.6 of a 600px box is a 360px gradient, so the midpoint of the left edge
    // sits well inside it. If the radius were read as 0.6px, everything would
    // clamp to the outer stop and read 124.
    let png = radial(0.6, 600, 600);
    let mid_left = red_at(&png, 150, 300);
    assert!(
        mid_left > 160,
        "a point 150px from a 0.6-fraction centre read {mid_left}, which is the \
         clamped outer stop; the radius is being treated as pixels"
    );
}

#[test]
fn a_tiny_radius_is_still_monotonic() {
    // radius 0.01 is a deliberately small dot. It must still produce a gradient
    // in the middle rather than an inverted or empty one.
    let png = radial(0.01, 400, 400);
    let centre = red_at(&png, 200, 200);
    let corner = red_at(&png, 2, 2);
    assert!(
        centre > corner,
        "a small radial gradient was darker at the centre ({centre}) than the \
         corner ({corner})"
    );
}

#[test]
fn zero_radius_falls_back_to_covering_the_box() {
    // A zero radius is documented as "cover the box", and the diagonal half is
    // the right span. The centre must still be the light stop.
    let png = radial(0.0, 400, 400);
    let centre = red_at(&png, 200, 200);
    assert!(
        centre > 200,
        "a zero-radius radial gradient read {centre} at the centre"
    );
}

#[test]
fn svg_and_raster_agree_on_the_radius() {
    // `userSpaceOnUse` makes `r` a user-space length, so the SVG backend has to
    // scale the fraction too. A tree that is a gradient in PNG and a flat fill
    // in SVG is the divergence the shared `Style` type exists to prevent.
    let svg = render_svg(
        &Node::container(
            Style::new().with_size(600.0, 600.0).with_radial_gradient(
                0.5,
                0.5,
                0.6,
                &[(0.0, INNER), (1.0, OUTER)],
            ),
            vec![],
        ),
        600,
        600,
    )
    .expect("render svg");

    assert!(
        svg.contains("radialGradient"),
        "SVG has no radial gradient element"
    );
    // 0.6 of 600 is 360. The broken output emitted r="0.6".
    assert!(
        svg.contains(r#"r="360.0""#),
        "SVG radial radius is not scaled from a box fraction: {svg}"
    );
}

#[test]
fn linear_gradient_is_unaffected() {
    // The linear path was always correct. This guards against a fix to the
    // radial units regressing its neighbour.
    let tree = Node::container(
        Style::new()
            .with_size(400.0, 400.0)
            .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
        vec![],
    );
    let png = render_png(&tree, 400, 400).expect("render");
    let top = red_at(&png, 200, 2);
    let bottom = red_at(&png, 200, 397);
    assert!(
        top < bottom,
        "a 180-degree linear gradient should go light to dark downward, got \
         {top} at the top and {bottom} at the bottom"
    );
}
