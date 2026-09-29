//! cliptext.rs target: gradient-clipped headline demo.

mod common;

use hikari::{render_png, Node, Style};

fn main() {
    let tree = Node::container(
        Style::centered()
            .with_size(1200.0, 630.0)
            .with_background("#0b1020"),
        vec![Node::text(
            "Gradient words",
            Style::text(120.0, "#ffffff")
                .clip_text()
                .with_linear_gradient(90.0, &[(0.0, "#38bdf8"), (1.0, "#a78bfa")]),
        )],
    );
    let png = render_png(&tree, 1200, 630).expect("render");
    common::write("cliptext.png", &png);
    println!("wrote cliptext.png ({} bytes)", png.len());
}
