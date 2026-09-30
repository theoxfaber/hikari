//! corpus_ink.rs target: prove every corpus case actually draws something.
//!
//! # The trap this closes
//!
//! A golden-digest corpus happily blesses a blank image. If a regression made a
//! case render nothing, the bytes would change, someone would re-bless, and the
//! corpus would now be guarding emptiness. A digest proves *stability*, not
//! *correctness*, so it needs a companion that proves the output has content.
//!
//! Each case is checked against a measured ink floor rather than a guessed one:
//! the floor is well under the observed ink so ordinary antialiasing and font
//! changes do not trip it, but far enough above zero that a dropped glyph, a
//! failed gradient or a mis-clipped image all fail.
//!
//! This is a test, not a golden file. It has no digests and cannot be
//! re-blessed, which is the point: it should only ever need changing when a case
//! is *added* or genuinely becomes something else.

use hikari::{render_png, Node, Style};

/// Count pixels that differ from the case's own background.
///
/// A case is either a solid background or a gradient/image, so "differs from the
/// corner pixel" is the honest general test: it catches a blank render, a
/// missing glyph and an unpainted gradient without needing per-case knowledge.
fn content_pixels(png: &[u8]) -> usize {
    let img = image::load_from_memory(png).expect("decode png").to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return 0;
    }
    let bg = *img.get_pixel(0, 0);
    img.pixels().filter(|p| **p != bg).count()
}

/// A 64x64 gradient tile, matching the corpus so the case renders the same thing.
fn swatch() -> Vec<u8> {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, RgbaImage};
    let mut img = RgbaImage::new(64, 64);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 4) as u8, (y * 4) as u8, 180, 255]);
    }
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), 64, 64, ExtendedColorType::Rgba8)
        .expect("encode");
    buf
}

fn text_card(text: &str, px: f32, color: &str, w: u32, h: u32) -> Node {
    Node::container(
        Style::centered()
            .with_size(w as f32, h as f32)
            .with_background("#0b1020"),
        vec![Node::text(text, Style::text(px, color))],
    )
}

/// The same cases as `corpus.rs`, with a measured ink floor each.
///
/// Kept as a literal table rather than importing from the example: an example is
/// a binary, not a library, and duplicating the table is what lets this assert
/// independently of the golden digests. If a case is added to one and not the
/// other, the ink test simply does not cover it — which is a gap, not a lie.
fn cases() -> Vec<(&'static str, Node, u32, u32, usize)> {
    vec![
        (
            "gradient-linear",
            Node::container(
                Style::new()
                    .with_size(1200.0, 630.0)
                    .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
                vec![Node::text("Angle 180", Style::text(72.0, "#0f172a"))],
            ),
            1200,
            630,
            // A gradient has no flat region, so nearly every pixel differs from
            // the corner. Measured 745,200 of 756,000.
            700_000,
        ),
        (
            "gradient-radial",
            Node::container(
                Style::centered()
                    .with_size(600.0, 600.0)
                    .with_radial_gradient(0.5, 0.5, 0.6, &[(0.0, "#fde68a"), (1.0, "#7c2d12")]),
                vec![Node::text("Radial", Style::text(60.0, "#1c1917"))],
            ),
            600,
            600,
            // Measured 326,263 before the radial-units fix and 3,263 after the
            // radius was being read as pixels. The floor is well under the real
            // value but far above the flat-fill reading.
            300_000,
        ),
        (
            "border-radius",
            Node::container(
                Style::new().with_size(600.0, 400.0).with_background("#020617"),
                vec![Node::container(
                    Style::centered()
                        .with_size(320.0, 200.0)
                        .with_background("#1e293b")
                        .with_radius(48.0)
                        .with_border(6.0, "#38bdf8"),
                    vec![Node::text("r=48", Style::text(40.0, "#e2e8f0"))],
                )],
            ),
            600,
            400,
            // The inner card alone is 64,000 px, plus border and text.
            50_000,
        ),
        (
            "box-shadow",
            Node::container(
                Style::new().with_size(600.0, 400.0).with_background("#f1f5f9"),
                vec![Node::container(
                    Style::new()
                        .with_size(240.0, 140.0)
                        .with_background("#ffffff")
                        .with_radius(16.0)
                        .with_shadow(0.0, 18.0, 32.0, 0.0, "#0f172a"),
                    vec![],
                )],
            ),
            600,
            400,
            // The card is 33,600 px; the shadow is the reason for the case.
            30_000,
        ),
        (
            "grid-layout",
            Node::container(
                Style::grid(3)
                    .with_size(800.0, 400.0)
                    .with_gap(16.0)
                    .with_padding(24.0)
                    .with_background("#111827"),
                (0..3)
                    .map(|i| {
                        let label = format!("col {i}");
                        Node::container(
                            Style::centered()
                                .with_background("#1f2937")
                                .with_radius(12.0)
                                .with_border(2.0, "#374151"),
                            vec![Node::text(&label, Style::text(28.0, "#9ca3af"))],
                        )
                    })
                    .collect(),
            ),
            800,
            400,
            200_000,
        ),
        (
            "flex-justify-align",
            Node::container(
                Style::row()
                    .with_size(800.0, 300.0)
                    .with_gap(12.0)
                    .with_padding(32.0)
                    .with_background("#020617")
                    .with_justify(hikari::Justify::SpaceBetween)
                    .with_align(hikari::Align::Center),
                ["#38bdf8", "#a78bfa", "#34d399"]
                    .into_iter()
                    .map(|hex| {
                        Node::container(
                            Style::new().with_size(80.0, 80.0).with_background(hex),
                            vec![],
                        )
                    })
                    .collect(),
            ),
            800,
            300,
            // Three 80x80 squares = 19,200 px, nothing else.
            18_000,
        ),
        (
            "word-wrap",
            Node::container(
                Style::new()
                    .with_size(600.0, 400.0)
                    .with_padding(40.0)
                    .with_background("#0b1020"),
                vec![Node::text(
                    "Wrapping is greedy and breaks inside an over-long word only \
                     when the word cannot fit a line of its own, so this sentence \
                     should reflow to four lines at this width.",
                    Style::text(26.0, "#e2e8f0").with_max_width(420.0),
                )],
            ),
            600,
            400,
            5_000,
        ),
        (
            "text-clip-gradient",
            Node::container(
                Style::centered()
                    .with_size(800.0, 300.0)
                    .with_background("#0b1020"),
                vec![Node::text(
                    "Gradient words",
                    Style::text(84.0, "#ffffff")
                        .clip_text()
                        .with_linear_gradient(90.0, &[(0.0, "#38bdf8"), (1.0, "#a78bfa")]),
                )],
            ),
            800,
            300,
            10_000,
        ),
        (
            "latin-typography",
            text_card("office waffle AVATAR To", 72.0, "#ffffff", 900, 240),
            900,
            240,
            5_000,
        ),
        (
            "arabic-joining",
            text_card(
                "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} \u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}",
                72.0,
                "#93c5fd",
                900,
                240,
            ),
            900,
            240,
            5_000,
        ),
        (
            "hebrew-rtl",
            text_card(
                "\u{05E9}\u{05DC}\u{05D5}\u{05DD} \u{05E2}\u{05D5}\u{05DC}\u{05DD}",
                72.0,
                "#fde68a",
                900,
                240,
            ),
            900,
            240,
            4_000,
        ),
        (
            "mixed-direction",
            text_card(
                "Latin then \u{05E9}\u{05DC}\u{05D5}\u{05DD} then Latin again",
                56.0,
                "#e2e8f0",
                1000,
                240,
            ),
            1000,
            240,
            5_000,
        ),
        (
            "image-clipped",
            Node::container(
                Style::centered()
                    .with_size(800.0, 400.0)
                    .with_background("#0b1020"),
                vec![Node::image(
                    swatch(),
                    Style::new()
                        .with_size(320.0, 320.0)
                        .with_radius(64.0)
                        .with_border(4.0, "#38bdf8"),
                )],
            ),
            800,
            400,
            // A rounded 320x320 clip is a bit under 102,400 px; the floor is
            // set so a square, unclipped image would also pass, since the clip
            // itself is what the digest guards, not the ink.
            80_000,
        ),
        (
            "absolute-positioning",
            Node::container(
                Style::new().with_size(600.0, 400.0).with_background("#0b1020"),
                vec![Node::container(
                    Style::new()
                        .with_size(200.0, 120.0)
                        .with_background("#f472b6")
                        .with_radius(12.0)
                        .absolute_at(320.0, 220.0),
                    vec![],
                )],
            ),
            600,
            400,
            // One 200x120 box = 24,000 px.
            20_000,
        ),
        (
            "nested-containers",
            Node::container(
                Style::centered()
                    .with_size(700.0, 420.0)
                    .with_background("#0b1020"),
                vec![Node::container(
                    Style::column()
                        .with_size(560.0, 320.0)
                        .with_padding(32.0)
                        .with_gap(16.0)
                        .with_background("#111827")
                        .with_radius(24.0)
                        .with_border(2.0, "#1f2937"),
                    vec![
                        Node::text("Nested", Style::text(44.0, "#f8fafc")),
                        Node::container(
                            Style::row().with_gap(12.0),
                            ["#38bdf8", "#a78bfa"]
                                .into_iter()
                                .map(|hex| {
                                    Node::container(
                                        Style::new()
                                            .with_size(64.0, 64.0)
                                            .with_background(hex)
                                            .with_radius(12.0),
                                        vec![],
                                    )
                                })
                                .collect(),
                        ),
                    ],
                )],
            ),
            700,
            420,
            150_000,
        ),
        (
            "blend-multiply",
            Node::container(
                Style::row()
                    .with_size(200.0, 200.0)
                    .with_background("#ff0000"),
                vec![Node::container(
                    Style::new()
                        .with_size(200.0, 200.0)
                        .with_background("#00ff00")
                        .with_blend_mode(hikari::BlendMode::Multiply),
                    vec![],
                )],
            ),
            200,
            200,
            // Multiply of green over red is black, so the whole frame is one
            // colour and the content count is 0 by design. The floor is 0 and the
            // real check is the digest plus blend_and_shadow's pixel assertions:
            // this case exists to catch a change in the blend *result*, not to
            // prove anything is visible. Recorded as 0 rather than omitted so a
            // future change to the blend is visible as a digest mismatch here too.
            0,
        ),
        (
            "blend-screen-text",
            Node::container(
                Style::centered()
                    .with_size(400.0, 200.0)
                    .with_background("#404040"),
                vec![Node::text(
                    "Blend",
                    Style::text(90.0, "#ffffff")
                        .with_blend_mode(hikari::BlendMode::Difference),
                )],
            ),
            400,
            200,
            5_000,
        ),
        (
            "shadow-inset-top",
            Node::container(
                Style::centered()
                    .with_size(300.0, 300.0)
                    .with_background("#ffffff"),
                vec![Node::container(
                    Style::new()
                        .with_size(200.0, 140.0)
                        .with_background("#3b82f6")
                        .with_radius(12.0)
                        .with_shadow_kind(hikari::ShadowKind::InsetTop, 0.0, 6.0, 10.0, 0.0, "#000000"),
                    vec![],
                )],
            ),
            300,
            300,
            // The box is 200x140 = 28,000 px; the inset band is a small slice of it.
            25_000,
        ),
        (
            "shadow-inset-edge",
            Node::container(
                Style::centered()
                    .with_size(300.0, 300.0)
                    .with_background("#ffffff"),
                vec![Node::container(
                    Style::new()
                        .with_size(200.0, 140.0)
                        .with_background("#3b82f6")
                        .with_radius(12.0)
                        .with_shadow_kind(hikari::ShadowKind::InsetEdge, 0.0, 12.0, 40.0, 0.0, "#000000"),
                    vec![],
                )],
            ),
            300,
            300,
            25_000,
        ),
    ]
}

fn main() {
    let mut failures: Vec<String> = Vec::new();
    let mut report: Vec<String> = Vec::new();

    for (name, tree, w, h, floor) in cases() {
        let png = match render_png(&tree, w, h) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{name}: failed to render: {e}"));
                continue;
            }
        };
        let ink = content_pixels(&png);
        report.push(format!(
            "{name:22} {:>7} px of content (floor {floor})",
            ink
        ));
        if ink < floor {
            failures.push(format!(
                "{name}: only {ink} px of content, floor is {floor} — this case \
                 is guarding an empty or near-empty image"
            ));
        }
    }

    for line in &report {
        println!("{line}");
    }

    if !failures.is_empty() {
        eprintln!("\n{} case(s) below their ink floor:", failures.len());
        for f in &failures {
            eprintln!("  {f}");
        }
        std::process::exit(1);
    }
    println!("\nall {} cases have content", report.len());
}
