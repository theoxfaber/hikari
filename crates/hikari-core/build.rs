//! Build-time subset of the embedded primary font.
//!
//! The subset is produced by [`subset`], a layout-preserving subsetter that
//! keeps the source glyph ids so `GSUB`/`GPOS`/`GDEF` can be copied verbatim.
//! That is what makes Arabic joining, kerning and ligatures work in the
//! embedded font; a generic subsetter that renumbers glyphs drops all three
//! silently.
//!
//! `allsorts` is still used for one thing only: the per-document PDF subset in
//! `hikari-pdf`, where the outline is already shaped and the PDF profile is
//! the correct choice.

#[path = "build/subset.rs"]
mod subset;

use std::path::PathBuf;

fn main() {
    let src = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("assets/DejaVuSans.ttf");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=build/subset.rs");

    let bytes = std::fs::read(&src).expect("read embedded font");
    let (data, stats) = subset::subset(&bytes).expect("subset embedded font");

    if !stats.layout_preserved {
        panic!("source font is missing GSUB/GPOS/GDEF; shaping would be silently broken");
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("dejavu-subset.ttf");
    std::fs::write(&out, &data).expect("write subset font");

    println!(
        "cargo:warning=hikari font subset: {} glyphs ({} codepoints), {} -> {} bytes ({:.0}%), layout tables preserved",
        stats.glyphs,
        stats.codepoints,
        stats.source_bytes,
        stats.subset_bytes,
        stats.subset_bytes as f64 * 100.0 / stats.source_bytes as f64,
    );
}
