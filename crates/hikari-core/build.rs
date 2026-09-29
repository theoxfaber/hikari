//! Build-time subset of the embedded primary font.
//!
//! `fontdue` parses the whole font eagerly (~14ms cold for full DejaVu),
//! so we ship a subset covering Latin, Greek, Cyrillic, Hebrew, Arabic,
//! punctuation, currency, arrows, math, and symbols. CJK stays on the
//! runtime system fallback (unchanged behavior).
//!
//! Tradeoff, stated openly: `allsorts` cannot subset GSUB/GPOS/GDEF, so the
//! subset drops kerning and ligatures (layout, paint, and PDF all consume
//! the same bytes, so they stay mutually consistent). Arabic joining was
//! already isolated-form through `fontdue` and is unchanged.

use std::collections::BTreeSet;
use std::path::PathBuf;

const RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E), // ASCII
    (0x00A0, 0x00FF), // Latin-1
    (0x0100, 0x024F), // Latin Ext A/B
    (0x0370, 0x03FF), // Greek
    (0x0400, 0x04FF), // Cyrillic
    (0x0590, 0x05FF), // Hebrew
    (0x0600, 0x06FF), // Arabic
    (0x2000, 0x206F), // Punctuation
    (0x2070, 0x209F), // Super/subscripts
    (0x20A0, 0x20CF), // Currency
    (0x2100, 0x214F), // Letterlike
    (0x2190, 0x21FF), // Arrows
    (0x2200, 0x22FF), // Math operators
    (0x25A0, 0x25FF), // Geometric shapes
    (0x2600, 0x26FF), // Misc symbols
    (0x2700, 0x27BF), // Dingbats
    (0xFB50, 0xFDFF), // Arabic presentation A
    (0xFE70, 0xFEFF), // Arabic presentation B
];

fn main() {
    let src =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/DejaVuSans.ttf");
    println!("cargo:rerun-if-changed={}", src.display());
    let bytes = std::fs::read(&src).expect("read embedded font");

    let scope = allsorts::binary::read::ReadScope::new(&bytes);
    let font_file = scope
        .read::<allsorts::font_data::FontData<'_>>()
        .expect("parse font for subsetting");
    let provider = font_file.table_provider(0).expect("font provider");
    let probe = ttf_parser::Face::parse(&bytes, 0).expect("probe face");

    let mut gids: BTreeSet<u16> = BTreeSet::from([0]);
    for (lo, hi) in RANGES {
        for cp in *lo..=*hi {
            if let Some(ch) = char::from_u32(cp) {
                if let Some(g) = probe.glyph_index(ch) {
                    gids.insert(g.0);
                }
            }
        }
    }
    let ids: Vec<u16> = gids.into_iter().collect();
    println!("cargo:warning=subset keeps {} glyphs", ids.len());

    let subset = allsorts::subset::subset(
        &provider,
        &ids,
        &allsorts::subset::SubsetProfile::Custom(vec![
            allsorts::tag::HEAD,
            allsorts::tag::HHEA,
            allsorts::tag::MAXP,
            allsorts::tag::CMAP,
            allsorts::tag::HMTX,
            allsorts::tag::LOCA,
            allsorts::tag::GLYF,
            allsorts::tag::NAME,
            allsorts::tag::POST,
            allsorts::tag::OS_2,
            allsorts::tag::GSUB,
            allsorts::tag::GPOS,
            allsorts::tag::GDEF,
        ]),
        allsorts::subset::CmapTarget::Unicode,
    )
    .expect("subset embedded font");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("dejavu-subset.ttf");
    std::fs::write(&out, &subset).expect("write subset font");
    println!("cargo:warning=subset font {} bytes", subset.len());
}
