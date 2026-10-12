//! Render an HTML/CSS file to PNG, for the cross-renderer comparison.
//!
//! Deliberately does the minimum: parse the document, render it once, write the
//! PNG. No warm-up loop, no averaging, no internal counters -- the JS harness
//! does the timing so that both sides are measured by the same code.

use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: html_bench <in.html> <out.png> [runs]")?;
    let output = args
        .next()
        .ok_or("usage: html_bench <in.html> <out.png> [runs]")?;
    let runs: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

    let html = std::fs::read_to_string(&input)?;

    // Parse outside the timed region. Satori's `html_to_tree` equivalent is
    // also parse-then-render, and timing the parse against its render would
    // compare two different pipelines rather than two renderers.
    let mut report = hikari::css::ParseReport::default();
    let tree = hikari::css::html_to_tree(&html, &mut report)?;
    let skipped = report.skipped.len();

    // Two numbers, because the JS side has two too and the comparison is only
    // meaningful if both sides are measured the same way:
    //   - render only, for the renderers' own number;
    //   - parse + render, for the end-to-end number a caller actually pays.
    let mut render: Vec<f64> = Vec::with_capacity(runs as usize);
    let mut end_to_end: Vec<f64> = Vec::with_capacity(runs as usize);
    let mut png = Vec::new();
    for _ in 0..runs {
        let t = Instant::now();
        png = hikari::render_png(&tree.clone(), 1200, 630)?;
        render.push(t.elapsed().as_secs_f64() * 1000.0);

        let t = Instant::now();
        let mut r2 = hikari::css::ParseReport::default();
        let t2 = hikari::css::html_to_tree(&html, &mut r2)?;
        let out = hikari::render_png(&t2, 1200, 630)?;
        end_to_end.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(&out);
    }
    std::fs::write(&output, &png)?;

    // Also emit raw RGBA so the pixel diff does not have to decode a PNG on the
    // other side. resvg rasterises from SVG and cannot read a PNG, and pulling
    // in a second decoder just to compare two buffers would put one more thing
    // between the measurement and the thing measured.
    let placed = hikari::compute_layout(&tree, 1200.0, 630.0)?;
    let rgba = hikari::render_placed_to_rgba(&placed, 1200, 630)?;
    std::fs::write(output.replace(".png", ".rgba"), &rgba)?;

    let med = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    println!(
        "hikari\ttree->png\tbest={:.3}ms\tmedian={:.3}ms",
        render.iter().cloned().fold(f64::MAX, f64::min),
        med(render)
    );
    println!(
        "hikari\thtml->png\tbest={:.3}ms\tmedian={:.3}ms",
        end_to_end.iter().cloned().fold(f64::MAX, f64::min),
        med(end_to_end)
    );
    println!("# png={} skipped_properties={skipped}", png.len());
    Ok(())
}
