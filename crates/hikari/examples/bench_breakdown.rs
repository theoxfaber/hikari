//! bench_breakdown.rs target: release timing split (temporary diagnostic).

use hikari::{compute_layout, Node, Style};

fn card() -> Node {
    Node::container(
        Style::centered()
            .with_size(1200.0, 630.0)
            .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
        vec![
            Node::text("Hello from Hikari", Style::text(84.0, "#0f172a")),
            Node::text(
                "Grid layout, decoded images, borders and wrapped paragraphs. No browser.",
                Style::text(30.0, "#475569").with_max_width(800.0),
            ),
        ],
    )
}

fn main() {
    let tree = card();
    // Cold: very first layout + paint + encode in this process.
    let t = std::time::Instant::now();
    let p0 = compute_layout(&tree, 1200.0, 630.0).unwrap();
    let layout_cold = t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let _ = hikari::render_placed_to_png(&p0, 1200, 630).unwrap();
    let paint_cold = t.elapsed().as_secs_f64() * 1000.0;
    println!("cold: layout={layout_cold:.2}ms paint+encode={paint_cold:.2}ms");
    let n = 20u32;
    let t = std::time::Instant::now();
    let mut placed = None;
    for _ in 0..n {
        placed = Some(compute_layout(&tree, 1200.0, 630.0).unwrap());
    }
    let layout_ms = t.elapsed().as_secs_f64() * 1000.0 / f64::from(n);
    let placed = placed.unwrap();

    let t = std::time::Instant::now();
    for _ in 0..n {
        let _ = hikari_raster_lite(&placed);
    }
    let paint_ms = t.elapsed().as_secs_f64() * 1000.0 / f64::from(n);

    let t = std::time::Instant::now();
    for _ in 0..n {
        let _ = hikari::render_placed_to_png(&placed, 1200, 630).unwrap();
    }
    let png_ms = t.elapsed().as_secs_f64() * 1000.0 / f64::from(n);
    println!("layout={layout_ms:.2}ms paint(rgba)={paint_ms:.2}ms paint+encode={png_ms:.2}ms");
}

fn hikari_raster_lite(placed: &hikari::Placed) -> Vec<u8> {
    hikari_raster::render_to_rgba(placed, 1200, 630).unwrap()
}
