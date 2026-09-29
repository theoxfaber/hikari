//! Multilingual demo target.
//!
//! Doubles as the visual check that the embedded font subset kept its layout
//! tables: the Arabic and Farsi lines below only shape into joined letters
//! because `GSUB` survived subsetting, and the headline only kerns because
//! `GPOS` did. See `crates/hikari-core/src/font_subset_tests.rs` for the
//! machine-checked version of the same claim.

mod common;

use hikari::{render_png, Align, Justify, Node, Style};

fn main() {
    let bg = Style::column()
        .with_size(1200.0, 630.0)
        .with_background("#0b1020")
        .with_justify(Justify::Center)
        .with_align(Align::Center)
        .with_gap(10.0);
    let tree = Node::container(
        bg,
        vec![
            Node::text("Waffle AVATAR office", Style::text(52.0, "#ffffff")),
            // Arabic: only joins into connected letterforms if the embedded
            // subset kept GSUB.
            Node::text(
                "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} \u{0628}\u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}",
                Style::text(52.0, "#93c5fd"),
            ),
            Node::text("\u{0641}\u{0627}\u{0631}\u{0633}\u{06CC}", Style::text(52.0, "#c4b5fd")),
            Node::text("\u{05E9}\u{05DC}\u{05D5}\u{05DD}", Style::text(52.0, "#fcd34d")),
            Node::text("\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{30BF}\u{30A4}\u{30C8}\u{30EB}", Style::text(48.0, "#6ee7b7")),
        ],
    );
    let png = render_png(&tree, 1200, 630).expect("render");
    common::write("i18n.png", &png);
}
