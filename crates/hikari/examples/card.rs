//! card.rs target: grid + image + border + wrapped text demo.

use hikari::{render_png, Justify, Node, Style};

/// Sky-gradient hero image generated in code (no network, no assets).
fn hero_png() -> Vec<u8> {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, RgbaImage};
    let mut img = RgbaImage::new(480, 480);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let t = y as f32 / 480.0;
        *p = image::Rgba([
            (56.0 + 120.0 * t) as u8,
            (130.0 + 90.0 * (x as f32 / 480.0)) as u8,
            (240.0 - 80.0 * t) as u8,
            255,
        ]);
    }
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), 480, 480, ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

fn main() {
    let hero = Node::image(
        hero_png(),
        Style::new()
            .with_size(480.0, 480.0)
            .with_radius(32.0)
            .with_border(4.0, "#38bdf8")
            .with_shadow(0.0, 24.0, 48.0, 0.0, "#020617"),
    );
    let copy = Node::container(
        Style::column().with_gap(20.0).with_justify(Justify::Center),
        vec![
            Node::text("Ship OG images", Style::text(72.0, "#ffffff")),
            Node::text(
                "Grid layout, decoded images, borders and wrapped paragraphs. No browser, no screenshots, just pixels.",
                Style::text(30.0, "#94a3b8").with_max_width(480.0),
            ),
            Node::text("hikari", Style::text(28.0, "#38bdf8")),
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
    let png = render_png(&tree, 1200, 630).expect("render");
    std::fs::write("/Users/apple/hikari/card.png", &png).expect("write");
    println!("wrote card.png ({} bytes)", png.len());
}
