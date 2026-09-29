//! corpus.rs target: hash one canonical render per feature and diff the lot.
//!
//! # Why a corpus and not just the determinism card
//!
//! The determinism example proves *byte-identical* output, which is the property
//! that makes visual regression testing possible. But it covers one card, and
//! one card exercises a fraction of the engine. A regression in wrapping, border
//! radii, shadows or gradients would not move that card's bytes at all, so CI
//! would stay green while the library quietly rendered something worse.
//!
//! This covers the feature matrix instead: one case per capability, so a change
//! to any of them shows up as exactly one named line of diff. The cases are
//! independent images rather than one combined sheet, because a combined image
//! cannot tell you *which* feature moved — you would be re-blessing blind.
//!
//! ```sh
//! cargo run --release -p hikari-rs --example corpus           # verify
//! cargo run --release -p hikari-rs --example corpus -- --bless # re-baseline
//! ```
//!
//! On a mismatch it prints the byte delta and the re-bless command, because a
//! bare "hash mismatch" says nothing about whether the change was intended.

use std::collections::HashSet;

use hikari::{hash_bytes, render_png, render_svg, Align, Justify, Node, Style};

/// One corpus case: a name, the tree, and the size to render it at.
struct Case {
    name: &'static str,
    /// What this case exists to catch. Travels with the digest so a reviewer
    /// reading a diff learns what the case was for.
    covers: &'static str,
    w: u32,
    h: u32,
    tree: Node,
}

impl Case {
    fn new(name: &'static str, covers: &'static str, w: u32, h: u32, tree: Node) -> Self {
        Self {
            name,
            covers,
            w,
            h,
            tree,
        }
    }
}

/// A 64x64 gradient tile, self-contained so the corpus needs no asset files.
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
        .expect("encode swatch");
    buf
}

/// A dark card with a single centered run of text, the common case.
fn text_card(text: &str, px: f32, color: &str, w: u32, h: u32) -> Node {
    Node::container(
        Style::centered()
            .with_size(w as f32, h as f32)
            .with_background("#0b1020"),
        vec![Node::text(text, Style::text(px, color))],
    )
}

fn cases() -> Vec<Case> {
    vec![
        Case::new(
            "gradient-linear",
            "linear gradient angle and stop interpolation",
            1200,
            630,
            Node::container(
                Style::new()
                    .with_size(1200.0, 630.0)
                    .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
                vec![Node::text("Angle 180", Style::text(72.0, "#0f172a"))],
            ),
        ),
        Case::new(
            "gradient-radial",
            "radial gradient center, radius and stops",
            600,
            600,
            Node::container(
                Style::centered()
                    .with_size(600.0, 600.0)
                    .with_radial_gradient(0.5, 0.5, 0.6, &[(0.0, "#fde68a"), (1.0, "#7c2d12")]),
                vec![Node::text("Radial", Style::text(60.0, "#1c1917"))],
            ),
        ),
        Case::new(
            "border-radius",
            "corner rounding and border width",
            600,
            400,
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
        ),
        Case::new(
            "box-shadow",
            "shadow offset, blur radius and spread",
            600,
            400,
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
        ),
        Case::new(
            "grid-layout",
            "equal-column grid, gap and padding",
            800,
            400,
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
        ),
        Case::new(
            "flex-justify-align",
            "main and cross axis distribution",
            800,
            300,
            Node::container(
                Style::row()
                    .with_size(800.0, 300.0)
                    .with_gap(12.0)
                    .with_padding(32.0)
                    .with_background("#020617")
                    .with_justify(Justify::SpaceBetween)
                    .with_align(Align::Center),
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
        ),
        Case::new(
            "word-wrap",
            "greedy wrapping at max_width",
            600,
            400,
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
        ),
        Case::new(
            "text-clip-gradient",
            "background-clip: text through glyphs",
            800,
            300,
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
        ),
        Case::new(
            "latin-typography",
            "kerning, ligatures and Latin metrics",
            900,
            240,
            text_card("office waffle AVATAR To", 72.0, "#ffffff", 900, 240),
        ),
        Case::new(
            "arabic-joining",
            "Arabic contextual joining through GSUB",
            900,
            240,
            // Two words, so the joining at the word boundary is exercised too.
            text_card(
                "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} \u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}",
                72.0,
                "#93c5fd",
                900,
                240,
            ),
        ),
        Case::new(
            "hebrew-rtl",
            "right-to-left ordering and Hebrew coverage",
            900,
            240,
            text_card(
                "\u{05E9}\u{05DC}\u{05D5}\u{05DD} \u{05E2}\u{05D5}\u{05DC}\u{05DD}",
                72.0,
                "#fde68a",
                900,
                240,
            ),
        ),
        Case::new(
            "mixed-direction",
            "bidi run splitting across a directional boundary",
            1000,
            240,
            text_card(
                "Latin then \u{05E9}\u{05DC}\u{05D5}\u{05DD} then Latin again",
                56.0,
                "#e2e8f0",
                1000,
                240,
            ),
        ),
        Case::new(
            "image-clipped",
            "image decode, sizing and rounded clip",
            800,
            400,
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
        ),
        Case::new(
            "absolute-positioning",
            "absolute offsets independent of flow",
            600,
            400,
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
        ),
        Case::new(
            "nested-containers",
            "nested layout, margins and inherited padding",
            700,
            420,
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
        ),
        Case::new(
            "opaque-alpha-strip",
            "alpha channel stripping when the frame is fully opaque",
            200,
            200,
            // A flat, fully opaque image: the encoder must drop alpha, which
            // changes the bytes without changing a single pixel.
            Node::container(
                Style::new().with_size(200.0, 200.0).with_background("#123456"),
                vec![],
            ),
        ),
    ]
}

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus.sha256")
}

fn main() {
    let bless = std::env::args().any(|a| a == "--bless");
    let all = cases();

    // Duplicate names would silently overwrite each other in the golden file,
    // leaving one case unchecked while the file still looked complete.
    let names: HashSet<&str> = all.iter().map(|c| c.name).collect();
    assert_eq!(
        names.len(),
        all.len(),
        "corpus case names must be unique, or the golden file silently loses one"
    );

    let mut rows: Vec<(&'static str, &'static str, usize, String)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for c in &all {
        match render_png(&c.tree, c.w, c.h) {
            Ok(png) => rows.push((c.name, c.covers, png.len(), hash_bytes(&png))),
            Err(e) => failures.push(format!("{}: {e}", c.name)),
        }
    }
    if !failures.is_empty() {
        eprintln!("corpus cases failed to render:");
        for f in &failures {
            eprintln!("  {f}");
        }
        std::process::exit(1);
    }

    // SVG is hashed for one reference case only. The PNG is the real artifact;
    // SVG text ordering is a formatting concern, and hashing it everywhere would
    // make ordinary refactors look like visual regressions.
    let reference = all[0].tree.clone();
    let svg = render_svg(&reference, all[0].w, all[0].h).expect("svg renders");
    let svg_hash = hash_bytes(svg.as_bytes());

    let mut body = String::from(
        "# Golden digests for the feature corpus. Blessed by\n\
         #   cargo run --release -p hikari-rs --example corpus -- --bless\n\
         # One case per capability, so a regression names the feature that moved\n\
         # rather than hiding inside one opaque image. Cross-platform byte\n\
         # determinism is what makes these meaningful; see BENCHMARKS.md.\n",
    );
    for (name, covers, len, hash) in &rows {
        body.push_str(&format!("{name} {len} {hash} # {covers}\n"));
    }
    body.push_str(&format!(
        "svg-reference {} {svg_hash} # text output shape and glyph ids\n",
        svg.len()
    ));

    let path = golden_path();
    if bless {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create tests dir");
        }
        std::fs::write(&path, &body).expect("write golden file");
        println!("blessed {} ({} cases)", path.display(), rows.len());
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun with --bless to create it",
            path.display()
        )
    });

    let mut problems: Vec<String> = Vec::new();
    for (name, _, len, hash) in &rows {
        let Some(line) = expected
            .lines()
            .find(|l| l.starts_with(&format!("{name} ")))
        else {
            problems.push(format!("{name}: no blessed digest (a new case?)"));
            continue;
        };
        let mut it = line.split_whitespace();
        it.next();
        let blessed_len: usize = it.next().unwrap_or_default().parse().unwrap_or(0);
        let blessed = it.next().unwrap_or_default();
        if blessed != hash {
            problems.push(format!(
                "{name}: {len} B ({:+} B), digest changed\n      expected {blessed}\n      actual   {hash}",
                *len as i64 - blessed_len as i64
            ));
        }
    }

    // A case removed from the corpus must be removed from the golden file too,
    // or the file accumulates entries nothing verifies.
    for line in expected
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let name = line.split_whitespace().next().unwrap_or_default();
        if name != "svg-reference" && !names.contains(name) {
            problems.push(format!(
                "{name}: blessed but no longer a case (stale entry, re-bless)"
            ));
        }
    }

    if problems.is_empty() {
        println!("corpus ok: {} cases match", rows.len());
        return;
    }

    eprintln!("corpus mismatch in {} case(s):", problems.len());
    for p in &problems {
        eprintln!("  {p}");
    }
    eprintln!("\nif these changes are intended:");
    eprintln!("  cargo run --release -p hikari-rs --example corpus -- --bless");
    std::process::exit(1);
}
