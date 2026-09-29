//! End-to-end proof that a caller-supplied font reaches the renderer.
//!
//! The risk this guards against is silent: if a registered font were ignored
//! and the embedded font used instead, every image would still be a valid PNG,
//! still pass a smoke test, and simply be in the wrong typeface. So this
//! asserts the two renders differ *and* that the custom one has the registered
//! font's metrics, rather than just "it didn't panic".

use hikari::{register_font, render_png, Node, Style, BUILTIN_FONT};

/// A visually and metrically distinct font, if the host has one.
///
/// Registration is content-addressed, so using a system font here is fine: the
/// test only needs *a* second font, and the assertions compare against the
/// built-in rather than against hardcoded pixel values.
fn second_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Supplemental/Georgia.ttf",
        "/Library/Fonts/Georgia.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
    ];
    CANDIDATES
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .filter(|b| b.len() > 10_000)
}

fn card(font: Option<u32>) -> Node {
    let mut style = Style::centered()
        .with_size(900.0, 300.0)
        .with_background("#101010");
    style.font = font;
    Node::container(
        style,
        vec![Node::text(
            "Sphinx of black quartz",
            Style::text(64.0, "#ffffff"),
        )],
    )
}

#[test]
fn registered_font_changes_the_render() {
    let Some(bytes) = second_font() else {
        eprintln!("no second font on this host; skipping");
        return;
    };
    let id = register_font("Second", &bytes).expect("register");
    assert_ne!(id, BUILTIN_FONT);

    let builtin_png = render_png(&card(None), 900, 300).expect("builtin");
    let custom_png = render_png(&card(Some(id)), 900, 300).expect("custom");

    assert_ne!(
        builtin_png, custom_png,
        "a registered font produced byte-identical output to the built-in, so \
         it was not used"
    );
}

#[test]
fn registered_font_changes_measured_width() {
    // Layout and paint both go through the registry, so measurement must move
    // too. A font that only affected painting would mean text was positioned
    // for one typeface and drawn in another.
    let Some(bytes) = second_font() else {
        return;
    };
    let id = register_font("Second-width", &bytes).expect("register");

    // Measure through the same call layout uses, so this is a real check that
    // the registry is on the measurement path and not just the paint path.
    let width = |font: Option<u32>| {
        hikari::measure_text("Sphinx of black quartz", 64.0, font.unwrap_or(BUILTIN_FONT)).0
    };

    assert_ne!(
        width(None),
        width(Some(id)),
        "measurement ignored the registered font"
    );
}

#[test]
fn unknown_font_id_falls_back_instead_of_failing() {
    // Ids arrive from untrusted JSON in the bindings, so a bad one must degrade
    // to the built-in font rather than error or panic.
    let bogus = 424_242u32;
    let png = render_png(&card(Some(bogus)), 900, 300).expect("render with unknown font id");
    let builtin = render_png(&card(None), 900, 300).expect("render with built-in");
    assert_eq!(
        png, builtin,
        "an unknown font id should render as the built-in font"
    );
}

#[test]
fn svg_names_the_registered_family() {
    let Some(bytes) = second_font() else {
        return;
    };
    let id = register_font("Brand Sans", &bytes).expect("register");
    // Registration is content-addressed, so if another test already
    // registered these bytes the id comes back with the *first* name. Ask the
    // registry rather than assuming.
    let name = hikari::font_entry(id).expect("entry").name.clone();

    let svg = hikari::render_svg(&card(Some(id)), 900, 300).expect("svg");
    assert!(
        svg.contains(&name),
        "SVG did not name the registered family {name:?}: {svg}"
    );
    assert!(
        !svg.contains("DejaVu Sans"),
        "SVG still names the embedded family: {svg}"
    );
}
