//! The render benchmark.
//!
//! # Why each stage is separate
//!
//! `BENCHMARKS.md` quotes layout, paint and encode separately. Quoting numbers
//! that `cargo bench` cannot reproduce is how the v0.7 comparison happened: the
//! figures in the document came from a one-off measurement that was never wired
//! to anything, and were later found to be from a debug build. Every row in that
//! document now corresponds to a `bench_function` here, so a reader can run the
//! same command and get the same shape of answer.
//!
//! # Release-only, and it checks
//!
//! There is a real hazard here and it has bitten this project once. A benchmark
//! run under a debug profile is meaningless for these numbers, and nothing about
//! running it *looks* wrong -- it produces a plausible number, just one two
//! orders of magnitude off. So `assert_release_build()` below refuses to produce
//! a benchmark under a non-optimised profile rather than emitting a figure that
//! will be quoted somewhere.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use hikari::{Node, Style};

/// Refuse to benchmark a debug build.
///
/// Checks `cfg!(debug_assertions)`, which is the flag that actually indicates
/// an unoptimised build, rather than inferring it from timing -- a slow machine
/// and a debug build look identical from the outside, and confusing them would
/// be the exact failure this guards against.
fn assert_release_build() {
    // `assert!` on a `cfg!` is a constant expression to clippy and is denied
    // workspace-wide. The condition is still a compile-time constant, so the
    // guard costs nothing when it passes and the message is still there for a
    // human who runs `cargo bench --profile dev` and gets it.
    if cfg!(debug_assertions) {
        panic!(
            "refusing to benchmark: this is a debug build.\n\
             Run `cargo bench`, which uses the release profile, or the numbers \
             in BENCHMARKS.md will be meaningless."
        );
    }
}

fn banner() -> Node {
    Node::banner(
        1200.0,
        630.0,
        "#0b1020",
        "Hello from Hikari",
        72.0,
        "#ffffff",
    )
}

/// A text-heavy card: the workload most likely to differ between engines,
/// since it exercises shaping, caching and glyph rasterization rather than
/// large flat fills.
fn text_card() -> Node {
    let mut body = Node::container(
        Style::new()
            .with_size(560.0, 400.0)
            .with_background("#ffffff")
            .with_padding(24.0)
            .with_gap(12.0),
        Vec::new(),
    );
    for (heading, text) in [
        ("Throughput", "Layout, paint and encode are measured separately so a regression names the stage that moved."),
        ("Determinism", "The same tree produces the same bytes on every platform. That is what makes a golden corpus possible."),
        ("Coverage", "One golden case per capability, so a visual regression names a feature instead of hiding in one opaque image."),
    ] {
        body = Node::container(
            Style::new().with_size(512.0, 100.0).with_background("#f1f5f9"),
            vec![
                Node::text(heading, Style::text(22.0, "#0f172a")),
                Node::text(text, Style::text(14.0, "#334155")),
            ],
        );
    }
    Node::container(
        Style::new()
            .with_size(1200.0, 630.0)
            .with_background("#0b1020")
            .with_padding(40.0),
        vec![body],
    )
}

fn bench_banner(c: &mut Criterion) {
    assert_release_build();
    let tree = banner();
    let mut group = c.benchmark_group("banner_1200x630");
    group.throughput(Throughput::Bytes(1200 * 630 * 4));

    group.bench_function("layout", |b| {
        b.iter(|| hikari::compute_layout(&tree, 1200.0, 630.0).expect("layout"));
    });
    // Paint to raw RGBA, not to PNG. The first version of this file called
    // `render_placed_to_png` here and labelled it "paint", which made paint and
    // encode the same measurement -- and the two rows in BENCHMARKS.md that were
    // supposed to differ came out identical to four decimal places, which is
    // what finally made it obvious rather than a subtle mistake nobody noticed.
    group.bench_function("paint_to_rgba", |b| {
        b.iter(|| {
            let placed = hikari::compute_layout(&tree, 1200.0, 630.0).expect("layout");
            hikari::render_placed_to_rgba(&placed, 1200, 630).expect("paint")
        });
    });
    group.bench_function("png", |b| {
        b.iter(|| hikari::render_png(&tree, 1200, 630).expect("render"));
    });
    group.finish();
}

fn bench_text_card(c: &mut Criterion) {
    assert_release_build();
    let tree = text_card();
    let mut group = c.benchmark_group("text_card_1200x630");
    group.throughput(Throughput::Bytes(1200 * 630 * 4));

    group.bench_function("png", |b| {
        b.iter(|| hikari::render_png(&tree, 1200, 630).expect("render"));
    });
    group.bench_function("svg", |b| {
        b.iter(|| hikari::render_svg(&tree, 1200, 630).expect("render"));
    });
    group.finish();
}

/// Cold start, measured as a single iteration.
///
/// Deliberately not a `bench_function`: criterion takes many samples and reports
/// a distribution, which is exactly the wrong shape for "what does the first
/// render in a fresh process cost". Reported rather than asserted, and the
/// README quotes it as the figure it is.
fn bench_cold_start(c: &mut Criterion) {
    assert_release_build();
    // Reported as a distribution like everything else, but the *interpretation*
    // differs: after the first sample the glyph cache is warm, so this measures
    // a warm steady state. The genuinely cold figure is the first render in a
    // fresh process, which no in-process benchmark can take honestly -- and
    // BENCHMARKS.md says so rather than quoting a number that pretends to.
    c.bench_function("banner_1200x630_first_and_warm", |b| {
        b.iter(|| hikari::render_png(&banner(), 1200, 630).expect("render"));
    });
}

// ---------------------------------------------------------------------------
// Workloads
//
// The other benchmarks in this file all render roughly the same shape: a few
// text runs and some flat rectangles on a 1200x630 canvas. That measures fill
// cost and says almost nothing about the workloads people actually run.
//
// This group exists because a benchmark suite that varies only the content and
// reports no difference is measuring one thing. Each workload here is chosen to
// stress a *different* stage, and each reports layout, paint and encode
// separately -- so the split can move between workloads instead of being a
// single number that happens to be true of one card.
// ---------------------------------------------------------------------------

/// Dense text: many runs, many glyphs, little area.
///
/// Stresses shaping and glyph rasterisation. Layout should stop being
/// negligible here, which is the point of including it.
fn workload_text_dense() -> Node {
    let mut children = Vec::new();
    for i in 0..40 {
        children.push(Node::container(
            Style::new().with_max_width(1100.0),
            vec![
                Node::text(
                    format!("Section {i}: layout, paint and encode are separate stages").as_str(),
                    Style::text(20.0, "#0f172a"),
                ),
                Node::text(
                    "Glyph rasterization dominates a text-heavy card, and the cache is per thread and per size.",
                    Style::text(15.0, "#475569"),
                ),
            ],
        ));
    }
    Node::container(
        Style::new()
            .with_size(1200.0, 630.0)
            .with_background("#ffffff")
            .with_padding(16.0),
        children,
    )
}

/// Several large decoded images composited.
///
/// Stresses decode-once-then-blit and the alpha path, and it is the workload
/// most likely to move if the image path changes.
fn workload_images() -> Node {
    // A deterministic gradient, generated rather than loaded so the benchmark
    // does not depend on an asset being present or on a decoder being warm.
    let mut raw = Vec::with_capacity(256 * 256 * 3);
    for y in 0..256u32 {
        for x in 0..256u32 {
            raw.push((x * 255 / 255) as u8);
            raw.push((y * 255 / 255) as u8);
            raw.push(((x ^ y) * 255 / 255) as u8);
        }
    }
    // Wrap in a PNG so the benchmark measures the real image path, including
    // decode, rather than a pre-decoded buffer nobody would hand the library.
    let mut png = Vec::new();
    {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};
        PngEncoder::new_with_quality(&mut png, CompressionType::Default, FilterType::Adaptive)
            .write_image(&raw, 256, 256, ExtendedColorType::Rgb8)
            .expect("encode fixture");
    }

    let mut grid = Vec::new();
    for i in 0..12 {
        grid.push(Node::image(
            png.clone(),
            Style::new().with_size(370.0, 180.0).with_radius(8.0),
        ));
        let _ = i;
    }
    Node::container(
        Style::new()
            .with_size(1200.0, 630.0)
            .with_background("#0f172a")
            .with_padding(20.0)
            .with_gap(10.0),
        grid,
    )
}

/// Many small nodes: a document-shaped page rather than a card.
///
/// Stresses layout and the node tree rather than the raster target, and is the
/// shape whose cost is least likely to be predicted from the card benchmarks.
fn workload_document() -> Node {
    let mut rows = Vec::new();
    for i in 0..150 {
        rows.push(Node::container(
            Style::new().with_size(1000.0, 14.0),
            vec![Node::text(
                format!("Row {i}").as_str(),
                Style::text(11.0, "#334155"),
            )],
        ));
    }
    Node::container(
        Style::new()
            .with_size(1200.0, 630.0)
            .with_background("#ffffff")
            .with_padding(12.0),
        rows,
    )
}

/// Layout, paint and encode, for one workload.
///
/// Shared by all three so the split is comparable between them. Measuring the
/// stages separately is the only way to tell "slower" from "slower at a
/// different thing".
fn bench_workload_stages(c: &mut Criterion, name: &str, tree: &Node, area: u64) {
    let mut group = c.benchmark_group(name);
    group.throughput(Throughput::Bytes(area));

    group.bench_function("layout", |b| {
        b.iter(|| hikari::compute_layout(tree, 1200.0, 630.0).expect("layout"));
    });
    group.bench_function("paint_to_rgba", |b| {
        b.iter(|| {
            let placed = hikari::compute_layout(tree, 1200.0, 630.0).expect("layout");
            hikari::render_placed_to_rgba(&placed, 1200, 630).expect("paint")
        });
    });
    // Encode only: paint once, then time the encode. Re-painting inside the
    // encode case would report the sum of the two, which is the mistake this
    // file's header describes at length.
    let placed = hikari::compute_layout(tree, 1200.0, 630.0).expect("layout");
    group.bench_function("encode_png", |b| {
        b.iter(|| hikari::render_placed_to_png(&placed, 1200, 630).expect("encode"));
    });
    group.finish();
}

fn bench_workloads(c: &mut Criterion) {
    assert_release_build();
    let area = 1200 * 630 * 4;
    bench_workload_stages(c, "workload_text_dense", &workload_text_dense(), area);
    bench_workload_stages(c, "workload_images", &workload_images(), area);
    bench_workload_stages(c, "workload_document", &workload_document(), area);
}

criterion_group!(
    benches,
    bench_banner,
    bench_text_card,
    bench_cold_start,
    bench_workloads
);
criterion_main!(benches);
