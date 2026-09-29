//! og.rs target: Takumi-style gradient banner.

mod common;

use hikari::{render_png, Node, Style};

fn main() {
    // Mirrors the classic `bg-linear-to-b from-blue-100 to-red-50` OG card.
    let tree = Node::container(
        Style::centered()
            .with_size(1200.0, 630.0)
            .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
        vec![Node::text(
            "Hello from Hikari",
            Style::text(84.0, "#0f172a"),
        )],
    );
    let png = render_png(&tree, 1200, 630).expect("render");
    common::write("og.png", &png);
    println!("wrote og.png ({} bytes)", png.len());
}
