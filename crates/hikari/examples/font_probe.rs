//! font_probe.rs target: report glyph coverage and kerning for the built-in
//! font, so a change to the embedded subset is visible as a coverage diff.
//!
//! For probing a caller-registered font, register it first and pass the id:
//! `FONT=<id> cargo run --example font_probe`.

use hikari::{shape_text, BUILTIN_FONT};

fn main() {
    let font: u32 = std::env::var("FONT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(BUILTIN_FONT);

    for s in [
        "H", "\u{f1}", "\u{5e9}", "\u{645}", "\u{2014}", "\u{20ac}", "\u{65e5}",
    ] {
        let (adv, w) = shape_text(s, 40.0, font);
        let missing = adv.first().map(|a| a.missing).unwrap_or(true);
        println!(
            "U+{:04X} w={w:.1} missing={missing}",
            s.chars().next().unwrap() as u32
        );
    }
    let (_, wav) = shape_text("AV", 100.0, font);
    let (_, wa) = shape_text("A", 100.0, font);
    let (_, wv) = shape_text("V", 100.0, font);
    println!(
        "AV={wav:.1} A+V={:.1} kerned={}",
        wa + wv,
        wav < wa + wv - 0.5
    );
}
