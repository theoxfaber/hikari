//! custom_font.rs target: the same card in two typefaces.
//!
//! Side-by-side proof that a registered font reaches layout, paint and SVG.
//! The left card uses the embedded subset, the right one a registered font.

mod common;

use hikari::{register_font, render_png, Align, Justify, Node, Style, BUILTIN_FONT};

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

fn panel(label: &str, body: &str, font: Option<u32>) -> Node {
    let mut panel = Style::column()
        .with_size(600.0, 630.0)
        .with_background("#0b1020")
        .with_padding(48.0)
        .with_gap(18.0);
    panel.font = font;
    Node::container(
        panel,
        vec![
            Node::text(label, Style::text(24.0, "#64748b")),
            Node::text(body, Style::text(40.0, "#ffffff")),
            Node::text("Waffle office AVATAR", Style::text(28.0, "#93c5fd")),
        ],
    )
}

fn main() {
    let Some(bytes) = second_font() else {
        eprintln!("no second font on this host; rendering the built-in only");
        let root = Style::row()
            .with_size(600.0, 630.0)
            .with_background("#000000")
            .with_justify(Justify::Center)
            .with_align(Align::Center)
            .with_padding(32.0)
            .with_gap(16.0);
        let tree = Node::container(
            root,
            vec![panel("embedded subset", "Sphinx of black quartz", None)],
        );
        let png = render_png(&tree, 600, 630).expect("render");
        common::write("custom-font.png", &png);
        return;
    };

    let id = register_font("Georgia", &bytes).expect("register");
    let name = hikari::font_entry(id).expect("entry").name.clone();
    println!("font {id} = {name}");

    let root = Style::row()
        .with_size(1200.0, 630.0)
        .with_background("#000000")
        .with_padding(24.0)
        .with_gap(16.0);
    let tree = Node::container(
        root,
        vec![
            panel("embedded subset", "Sphinx of black quartz", None),
            panel(
                &format!("registered: {name}"),
                "Sphinx of black quartz",
                Some(id),
            ),
        ],
    );

    let png = render_png(&tree, 1200, 630).expect("render");
    common::write("custom-font.png", &png);
    let svg = hikari::render_svg(&tree, 1200, 630).expect("svg");
    common::write("custom-font.svg", svg.as_bytes());
    assert_ne!(id, BUILTIN_FONT);
}
