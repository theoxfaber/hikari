//! The CSS front end, end to end, through the real renderer.
//!
//! The unit tests in `css_tests` assert on decoded style values, which is the
//! right level for a parser. But a parser can produce a perfectly correct
//! [`Style`] and still render nothing, and only pixels settle that. So these
//! tests render and count ink, which is the same technique the corpus uses for
//! the same reason: a digest proves stability, not correctness.
//!
//! The headline case is the one the reviewer asked for — a card written in HTML
//! and CSS, the way a caller would actually write one, rather than as a
//! hand-built node tree.

use hikari::css::{self, Element, ParseReport, Stylesheet};
use hikari::{render_png, Node};

/// Pixels that differ from the pixel at the top-left corner.
///
/// Each case sets an opaque background there, so this counts "anything drawn"
/// without each case needing to know its own background colour.
fn ink(png: &[u8]) -> usize {
    let img = image::load_from_memory(png).expect("decode").to_rgba8();
    let bg = *img.get_pixel(0, 0);
    img.pixels().filter(|p| **p != bg).count()
}

fn render(tree: &Node, w: u32, h: u32) -> Vec<u8> {
    render_png(tree, w, h).expect("render")
}

#[test]
fn an_html_and_css_card_renders() {
    // The shape a caller would actually write.
    let html = r#"
    <style>
      .card {
        display: flex;
        flex-direction: column;
        justify-content: center;
        width: 1200px;
        height: 630px;
        padding: 60px;
        background: linear-gradient(180deg, #dbeafe, #fee2e2);
      }
      .title { font-size: 84px; color: #0f172a; }
      .sub   { font-size: 30px; color: #475569; }
    </style>
    <div class="card">
      <span class="title">Hikari</span>
      <span class="sub">deterministic output</span>
    </div>
    "#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    assert!(report.is_clean(), "the card should parse cleanly: {report}");

    let png = render(&tree, 1200, 630);
    assert!(
        ink(&png) > 5_000,
        "the card drew almost nothing: {} ink pixels",
        ink(&png)
    );
}

#[test]
fn the_same_card_written_by_hand_and_by_css_agree() {
    // The claim the CSS front end makes is that it removes the need to hand-build
    // a tree. This is the assertion: the CSS path must produce the *same pixels*
    // as the equivalent hand-built one. Anything less and the CSS layer is a
    // second, subtly different renderer rather than a convenience.
    let html = r#"
    <style>
      .card { width: 800px; height: 400px; padding: 40px; background: #0b1020; }
      .text { font-size: 56px; color: #ffffff; }
    </style>
    <div class="card"><span class="text">Same</span></div>
    "#;
    let mut report = ParseReport::default();
    let from_css = css::html_to_tree(html, &mut report).expect("parse");
    assert!(report.is_clean(), "unexpected skips: {report}");

    let mut inner = hikari::Style::text(56.0, "#ffffff");
    // `.text` also inherits the parent's padding unless overridden; the CSS
    // version only puts padding on `.card`, so this must match that.
    inner.grow = 0.0;
    let by_hand = Node::container(
        hikari::Style::new()
            .with_size(800.0, 400.0)
            .with_padding(40.0)
            .with_background("#0b1020"),
        vec![Node::text("Same", inner)],
    );

    let a = render(&from_css, 800, 400);
    let b = render(&by_hand, 800, 400);
    assert_eq!(
        hikari::hash_bytes(&a),
        hikari::hash_bytes(&b),
        "the CSS and hand-built trees rendered differently ({} vs {} bytes of ink), \
         so the CSS path is not merely a convenience but a second renderer",
        ink(&a),
        ink(&b)
    );
}

#[test]
fn an_unsupported_property_does_not_stop_the_rest() {
    // A partial stylesheet must still render what it *can*, and report what it
    // could not. Failing the whole document over `transform` would make the
    // parser useless against real-world CSS, where one unknown property is
    // normal.
    let html = r#"
    <style>
      .card { width: 600px; height: 300px; background: #102040; }
      .card { transform: translateX(10px); }
    </style>
    <div class="card"><span>text</span></div>
    "#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    assert!(
        report
            .skipped
            .iter()
            .any(|(d, _)| d.starts_with("transform")),
        "the unsupported property must be reported: {report}"
    );
    // Measured: one word at the 32px default inks 619 pixels on this card, so
    // the floor sits under that rather than at a round number that was never
    // checked against a real render.
    assert!(
        ink(&render(&tree, 600, 300)) > 400,
        "the supported declarations still had to render"
    );
}

#[test]
fn a_declaration_that_would_be_guessed_is_rejected_not_applied() {
    // `width: 50` is invalid CSS for a length. Silently reading it as 50px would
    // produce a card half the intended size that looks entirely plausible.
    let mut report = ParseReport::default();
    let sheet = Stylesheet::parse(".card{width:50;height:200px}", &mut report).expect("parse");
    let el = Element::new("div", vec![Element::text("x")]).class("card");
    let tree = css::from_elements(&sheet, &el, &mut report);
    assert!(
        ink(&render(&tree, 200, 300)) > 100,
        "the valid half still renders"
    );
    assert!(
        report.skipped.iter().any(|(d, _)| d.starts_with("width")),
        "the invalid declaration must be reported, not guessed: {report}"
    );
}

/// Render `text` at `size` on a dark card, and count the ink.
///
/// The dark card matters: the renderer's default canvas is white, so white text
/// on it is invisible and every measurement reads zero. That is correct
/// behaviour, and it is exactly the kind of thing that makes a test lie.
fn ink_of_text_at(size: &str) -> usize {
    let mut report = ParseReport::default();
    let sheet = Stylesheet::parse(
        &format!(".card{{width:600px;height:200px;background:#101010}} .t{{font-size:{size};color:#ffffff}}"),
        &mut report,
    )
    .expect("parse");
    let el = Element::new(
        "div",
        vec![Element::new("span", vec![Element::text("Measure")]).class("t")],
    )
    .class("card");
    ink(&render(
        &css::from_elements(&sheet, &el, &mut report),
        600,
        200,
    ))
}

#[test]
fn css_text_renders_with_the_right_metrics() {
    // The parser must reach `font-size`, because a text node with no size falls
    // back to a default and the layout is then wrong rather than absent.
    let small = ink_of_text_at("12px");
    let large = ink_of_text_at("72px");
    assert!(
        small > 200,
        "even a 12px run should ink something on a dark card, got {small}"
    );
    assert!(
        large > small * 4,
        "a 72px run should ink far more than a 12px one: {large} vs {small}"
    );
}

#[test]
fn grid_and_flex_from_css_reach_the_layout() {
    // `display:grid` with `repeat(3,1fr)` must produce three side-by-side
    // columns, not a stack. Ink alone cannot tell the difference between the two
    // layouts, so this asserts on the horizontal spread of the painted cells:
    // a column layout would put all the ink in one x-range.
    let html = r#"
    <style>
      .g { display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px;
           width: 600px; height: 200px; background: #101010; }
      .c { background: #38bdf8; }
    </style>
    <div class="g">
      <div class="c"></div><div class="c"></div><div class="c"></div>
    </div>
    "#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    assert!(report.is_clean(), "unexpected skips: {report}");

    let png = render(&tree, 600, 200);
    let img = image::load_from_memory(&png).expect("decode").to_rgba8();
    let bg = *img.get_pixel(0, 0);
    // For each row that has any cell pixel, record which thirds of the width are
    // covered. Three covered thirds means a row, one means a column.
    let mut rows_with_three = 0usize;
    let mut rows_with_one = 0usize;
    for y in 0..img.height() {
        let mut thirds = [false; 3];
        for x in 0..img.width() {
            if *img.get_pixel(x, y) != bg {
                thirds[(x * 3 / img.width() as u32) as usize] = true;
            }
        }
        match thirds.iter().filter(|x| **x).count() {
            3 => rows_with_three += 1,
            1 => rows_with_one += 1,
            _ => {}
        }
    }
    assert!(
        rows_with_three > rows_with_one,
        "expected side-by-side columns: {rows_with_three} rows spanned three thirds          of the width but {rows_with_one} rows spanned one"
    );
}

#[test]
fn a_gradient_from_css_is_not_a_flat_fill() {
    // The exact bug the golden corpus caught in the renderer: a gradient that
    // parses correctly but paints flat. Counting distinct colours catches it,
    // which a digest alone would not have.
    let mut report = ParseReport::default();
    let sheet = Stylesheet::parse(
        ".g{width:200px;height:200px;background:linear-gradient(90deg,#000000,#ffffff)}",
        &mut report,
    )
    .expect("parse");
    let tree = css::from_elements(&sheet, &Element::new("div", vec![]).class("g"), &mut report);
    let png = render(&tree, 200, 200);
    let img = image::load_from_memory(&png).expect("decode").to_rgba8();
    let distinct: std::collections::HashSet<u8> = img.pixels().map(|p| p.0[0]).collect();
    assert!(
        distinct.len() > 20,
        "a linear gradient painted only {} distinct values; it is flat",
        distinct.len()
    );
}
