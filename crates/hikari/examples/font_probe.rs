fn main() {
    for s in [
        "H", "\u{f1}", "\u{5e9}", "\u{645}", "\u{2014}", "\u{20ac}", "\u{65e5}",
    ] {
        let (adv, w) = hikari::shape_text(s, 40.0);
        let missing = adv.first().map(|a| a.missing).unwrap_or(true);
        println!(
            "U+{:04X} w={w:.1} missing={missing}",
            s.chars().next().unwrap() as u32
        );
    }
    let (_, wav) = hikari::shape_text("AV", 100.0);
    let (_, wa) = hikari::shape_text("A", 100.0);
    let (_, wv) = hikari::shape_text("V", 100.0);
    println!(
        "AV={wav:.1} A+V={:.1} kerned={}",
        wa + wv,
        wav < wa + wv - 0.5
    );
}
