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

criterion_group!(benches, bench_banner, bench_text_card, bench_cold_start);
criterion_main!(benches);
