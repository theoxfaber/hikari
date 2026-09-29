//! determinism.rs target: print content hashes of a canonical render.
//!
//! Used to prove byte-identical output across machines and platforms:
//! run here and anywhere else, compare the printed hashes.

use hikari::{Node, Style, hash_bytes, render_png, render_svg};

fn tree() -> Node {
    Node::container(
        Style::centered()
            .with_size(1200.0, 630.0)
            .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
        vec![
            Node::text("Deterministic Hikari", Style::text(84.0, "#0f172a")),
            Node::text("same bytes everywhere", Style::text(30.0, "#475569")),
        ],
    )
}

fn main() {
    let t = tree();
    let png = render_png(&t, 1200, 630).expect("render");
    let svg = render_svg(&t, 1200, 630).expect("render");
    println!("png {} {}", png.len(), hash_bytes(&png));
    println!("svg {} {}", svg.len(), hash_bytes(svg.as_bytes()));
}
