//! Multilingual demo target.

use hikari::{render_png, Node, Style};

fn main() {
    let bg = Style::column()
        .with_size(1200.0, 630.0)
        .with_background("#0b1020")
        .with_justify(hikari::Justify::Center)
        .with_align(hikari::Align::Center)
        .with_gap(12.0);
    let tree = Node::container(
        bg,
        vec![
            Node::text("Hello from Hikari", Style::text(64.0, "#ffffff")),
            Node::text("שלום مرحبا", Style::text(56.0, "#93c5fd")),
            Node::text("日本語のタイトル", Style::text(56.0, "#6ee7b7")),
            Node::text("line one\nline two", Style::text(40.0, "#fca5a5")),
        ],
    );
    let png = render_png(&tree, 1200, 630).expect("render");
    std::fs::write("/Users/apple/hikari/i18n.png", &png).expect("write");
    println!("wrote i18n.png ({} bytes)", png.len());
}
