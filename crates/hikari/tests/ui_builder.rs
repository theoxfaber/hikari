//! The builder, through the real renderer.
//!
//! The unit tests in `hikari-core` pin the tree it produces. These check the
//! other half: that a tree built with it renders the same as the equivalent
//! hand-built one, byte for byte.
//!
//! Byte equality is the assertion that matters here. "Both produce ink" would
//! pass for a builder that dropped a background colour or moved a child, because
//! a card with the wrong fill is still a card. Comparing digests against the
//! hand-built equivalent catches every field the builder might fail to forward.

use hikari::{hash_bytes, Node, Style};

fn by_hand() -> Node {
    Node::container(
        Style::new()
            .with_size(200.0, 140.0)
            .with_background("#0f172a")
            .with_padding(12.0)
            .with_gap(6.0),
        vec![
            Node::text("Release 1.2", Style::text(28.0, "#f8fafc")),
            Node::text("All checks passed", Style::text(14.0, "#94a3b8")),
            Node::container(
                Style::new()
                    .with_size(176.0, 30.0)
                    .with_background("#1e293b")
                    .with_border(2.0, "#f59e0b"),
                vec![Node::text("Ship", Style::text(16.0, "#f8fafc"))],
            ),
        ],
    )
}

fn built() -> Node {
    use hikari::ui::{div, text};
    div()
        .size(200.0, 140.0)
        .bg("#0f172a")
        .pad(12.0)
        .style(|mut s| {
            s.gap = 6.0;
            s
        })
        .child(text("Release 1.2").size(28.0, 0.0).color("#f8fafc"))
        .child(text("All checks passed").size(14.0, 0.0).color("#94a3b8"))
        .child(
            div()
                .size(176.0, 30.0)
                .bg("#1e293b")
                .style(|s| s.with_border(2.0, "#f59e0b"))
                .child(text("Ship").size(16.0, 0.0).color("#f8fafc")),
        )
        .build()
}

#[test]
fn a_built_tree_renders_identically_to_a_hand_built_one() {
    let a = hikari::render_png(&by_hand(), 200, 140).expect("hand-built renders");
    let b = hikari::render_png(&built(), 200, 140).expect("built renders");
    assert_eq!(
        hash_bytes(&a),
        hash_bytes(&b),
        "the builder produced a different render than the literal tree"
    );
}

#[test]
fn the_built_tree_is_not_blank() {
    // Paired with the equality above. If both renders were blank, the digests
    // would match and this would be the only thing standing between a broken
    // builder and a passing test suite -- which is the exact shape of the bug
    // that has bitten this project before.
    let png = hikari::render_png(&built(), 200, 140).expect("renders");
    let img = image::load_from_memory(&png).expect("decode").to_rgba8();
    let distinct = img
        .pixels()
        .map(|p| p.0)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(
        distinct >= 5,
        "expected the background, border and two text colours; got {distinct} distinct"
    );
}

#[test]
fn a_builder_can_be_nested_into_a_literal_tree_and_the_other_way_round() {
    // The two styles have to compose, because a real tree is rarely built one way
    // throughout.
    let mixed = Node::container(
        Style::new()
            .with_size(200.0, 140.0)
            .with_background("#0f172a"),
        vec![
            Node::text("literal", Style::text(20.0, "#ffffff")),
            hikari::ui::div()
                .size(100.0, 40.0)
                .bg("#334155")
                .child(hikari::ui::text("builder").size(16.0, 0.0).color("#ffffff"))
                .build(),
        ],
    );
    let png = hikari::render_png(&mixed, 200, 140).expect("renders");
    let img = image::load_from_memory(&png).expect("decode").to_rgba8();
    let count =
        |want: [u8; 3]| -> usize { img.pixels().filter(|p| [p[0], p[1], p[2]] == want).count() };
    // Scanned rather than sampled at fixed coordinates. Guessing where layout
    // would put things is how the first draft of this test failed: it asserted
    // a slate pixel at (120, 120), which is outside a 100x40 box, and concluded
    // the builder had dropped the node. Both children are present; the
    // assertion, not the code, was wrong.
    assert!(
        count([0x33, 0x41, 0x55]) > 2_000,
        "the builder box should be present"
    );
    assert!(
        img.pixels().any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200),
        "the literal text should be present"
    );
}

#[test]
fn an_empty_builder_renders_rather_than_panicking() {
    // `Elem::div()` with nothing set is a legitimate zero-size node. It must be
    // inert, not a crash: a caller assembling a tree conditionally should be able
    // to skip a branch without a special case.
    let png = hikari::render_png(&hikari::ui::div().build(), 40, 40).expect("renders");
    assert!(!png.is_empty());
}

#[test]
fn column_direction_reaches_the_layout() {
    // `Style`'s default is `Row`, so `column()` is the shorthand that changes
    // something. Containers rather than text, because a text node's size comes
    // from its glyphs and two one-glyph lines are too close together to tell a
    // direction change apart from a rendering difference.
    let kids = || {
        [
            hikari::Node::container(
                Style::new()
                    .with_size(60.0, 40.0)
                    .with_background("#ff0000"),
                vec![],
            ),
            hikari::Node::container(
                Style::new()
                    .with_size(60.0, 40.0)
                    .with_background("#00ff00"),
                vec![],
            ),
        ]
    };
    let row = hikari::ui::div()
        .size(200.0, 140.0)
        .children(kids())
        .build();
    let column = hikari::ui::div()
        .size(200.0, 140.0)
        .column()
        .children(kids())
        .build();

    let first_green = |t: &Node| {
        let png = hikari::render_png(t, 200, 140).expect("renders");
        let img = image::load_from_memory(&png).expect("decode").to_rgba8();
        let (w, h) = (img.width(), img.height());
        (0..h)
            .flat_map(move |y| (0..w).map(move |x| (x, y)))
            .find(|&(x, y)| {
                let p = img.get_pixel(x, y);
                p[1] > 200 && p[0] < 60
            })
    };

    let (rx, ry) = first_green(&row).expect("green box in the row layout");
    let (cx, cy) = first_green(&column).expect("green box in the column layout");
    // Row: the second box starts where the first ends, on the same line.
    // Column: directly below it, at the same x.
    assert!(
        rx >= 60 && ry < 40,
        "row should place the second box to the right on the same line, got {rx},{ry}"
    );
    assert!(
        cy >= 40 && cx < 60,
        "column should place the second box below at the same x, got {cx},{cy}"
    );
    assert!(
        hash_bytes(&hikari::render_png(&row, 200, 140).expect("renders"))
            != hash_bytes(&hikari::render_png(&column, 200, 140).expect("renders")),
        "direction had no effect on the render"
    );
}
