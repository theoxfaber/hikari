//! determinism.rs target: print content hashes of a canonical render.
//!
//! Used to prove byte-identical output across machines and platforms, and to
//! gate visual regressions in CI. The expected digests live in
//! `tests/determinism.sha256` next to this example's crate rather than being
//! pasted into the workflow, so re-baselining is one command and the digests
//! live next to the thing they describe.
//!
//! ```sh
//! cargo run --release -p hikari-rs --example determinism           # verify
//! cargo run --release -p hikari-rs --example determinism -- --bless # re-baseline
//! ```

use hikari::{hash_bytes, render_png, render_svg, Node, Style};

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

/// Where the blessed digests are stored.
fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/determinism.sha256")
}

fn main() {
    let bless = std::env::args().any(|a| a == "--bless");

    let t = tree();
    let png = render_png(&t, 1200, 630).expect("render");
    let svg = render_svg(&t, 1200, 630).expect("render");
    let png_hash = hash_bytes(&png);
    let svg_hash = hash_bytes(svg.as_bytes());

    println!("png {} {png_hash}", png.len());
    println!("svg {} {svg_hash}", svg.len());

    let body = format!(
        "# Blessed by `cargo run --release -p hikari-rs --example determinism -- --bless`.\n\
         # Any change here is a deliberate visual change; re-bless and say why in\n\
         # BENCHMARKS.md. Cross-platform byte determinism is what makes these\n\
         # digests meaningful.\n\
         png {} {png_hash}\n\
         svg {} {svg_hash}\n",
        png.len(),
        svg.len()
    );

    let path = golden_path();
    if bless {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create tests dir");
        }
        std::fs::write(&path, body).expect("write golden file");
        println!("blessed {}", path.display());
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun with --bless to create it",
            path.display()
        )
    });

    let mut mismatches = Vec::new();
    for (label, actual) in [("png", &png_hash), ("svg", &svg_hash)] {
        let line = expected
            .lines()
            .find(|l| l.starts_with(label) && !l.starts_with('#'))
            .unwrap_or_else(|| panic!("no blessed {label} digest in {}", path.display()));
        let blessed = line.split_whitespace().last().unwrap_or_default();
        if blessed != actual {
            mismatches.push(format!("  {label}: expected {blessed}, got {actual}"));
        }
    }

    if mismatches.is_empty() {
        println!("determinism: OK (digests match {})", path.display());
    } else {
        eprintln!("determinism: FAILED -- render output changed:");
        for m in &mismatches {
            eprintln!("{m}");
        }
        eprintln!(
            "\nIf this change is intended, re-bless with:\n  \
             cargo run --release -p hikari-rs --example determinism -- --bless"
        );
        std::process::exit(1);
    }
}
