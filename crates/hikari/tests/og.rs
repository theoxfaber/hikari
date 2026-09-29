//! og.rs target

use hikari::{render_png, render_svg, Node, Style};

#[test]
fn og_snapshot_png_and_svg() {
    let tree = Node::container(
        Style::centered()
            .with_size(1200.0, 630.0)
            .with_background("#0b1020"),
        vec![Node::text("Hikari 10/10", Style::text(96.0, "#ffffff"))],
    );
    let png = render_png(&tree, 1200, 630).unwrap();
    assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    // Snapshot: output must be non-trivial and deterministic in size band.
    assert!(png.len() > 5_000 && png.len() < 2_000_000);
    let svg = render_svg(&tree, 1200, 630).unwrap();
    assert!(svg.contains("Hikari 10/10"));
}

/// 32x16 gradient PNG bytes (self-contained fixture, no network).
fn gradient_png() -> Vec<u8> {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, RgbaImage};
    let mut img = RgbaImage::new(32, 16);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 8) as u8, (y * 16) as u8, 160, 255]);
    }
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), 32, 16, ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

#[test]
fn grid_image_card_renders() {
    let hero = Node::image(
        gradient_png(),
        Style::new()
            .with_size(560.0, 510.0)
            .with_radius(24.0)
            .with_border(4.0, "#38bdf8"),
    );
    let copy = Node::container(
        Style::column()
            .with_gap(16.0)
            .with_justify(hikari::Justify::Center),
        vec![
            Node::text("hikari motion", Style::text(64.0, "#ffffff")),
            Node::text(
                "Grid layout, real images, borders and wrapped text in one render with no browser.",
                Style::text(30.0, "#94a3b8").with_max_width(480.0),
            ),
        ],
    );
    let tree = Node::container(
        Style::grid(2)
            .with_size(1200.0, 630.0)
            .with_background("#0b1020")
            .with_padding(60.0)
            .with_gap(48.0),
        vec![hero, copy],
    );
    let png = render_png(&tree, 1200, 630).unwrap();
    assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    assert!(png.len() > 20_000 && png.len() < 2_000_000);
    let svg = render_svg(&tree, 1200, 630).unwrap();
    assert!(svg.contains("data:image/png;base64,"));
    assert!(svg.contains("hikari motion"));
}

#[test]
fn renders_are_byte_deterministic() {
    // Same tree twice must produce identical bytes: the foundation of
    // snapshot testing and the cross-platform determinism story.
    let tree = Node::banner(1200.0, 630.0, "#0b1020", "Deterministic", 72.0, "#ffffff");
    let (a, b) = (
        render_png(&tree, 1200, 630).unwrap(),
        render_png(&tree, 1200, 630).unwrap(),
    );
    assert_eq!(a, b);
    assert!(a.len() > 5_000);
    let (c, d) = (
        render_svg(&tree, 1200, 630).unwrap(),
        render_svg(&tree, 1200, 630).unwrap(),
    );
    assert_eq!(c, d);
}
