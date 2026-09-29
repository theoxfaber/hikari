//! render.rs target

use criterion::{criterion_group, criterion_main, Criterion};
use hikari::Node;

fn bench_banner(c: &mut Criterion) {
    let tree = Node::banner(
        1200.0,
        630.0,
        "#0b1020",
        "Hello from Hikari",
        72.0,
        "#ffffff",
    );
    c.bench_function("banner_1200x630_png", |b| {
        b.iter(|| hikari::render_png(&tree, 1200, 630).expect("render"));
    });
}

criterion_group!(benches, bench_banner);
criterion_main!(benches);
