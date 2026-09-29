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

/// A bundled face: source file, output name, and the codepoint ranges to keep.
struct Face {
    src: &'static str,
    out: &'static str,
    ranges: &'static [(u32, u32)],
    /// Emit only when this env var is set. `None` means always.
    feature: Option<&'static str>,
    /// Family name recorded in PDF/SVG output.
    family: &'static str,
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=build/subset.rs");

    let faces = [
        Face {
            src: "assets/DejaVuSans.ttf",
            out: "dejavu-subset.ttf",
            ranges: subset::RANGES,
            feature: None,
            family: "DejaVu Sans",
        },
        // Hebrew. DejaVu has no Hebrew presentation forms, so Hebrew drew as
        // isolated letters. 57 KB after subsetting, small enough to always ship.
        Face {
            src: "assets/fonts/NotoSansHebrew.ttf",
            out: "noto-hebrew-subset.ttf",
            ranges: subset::RANGES,
            feature: Some("BUNDLED_HEBREW"),
            family: "Noto Sans Hebrew",
        },
        // CJK. 9.9 MB subsetted, which would dwarf a 2.8 MB WASM build, so it is
        // opt-in. See `docs/BENCHMARKS.md` for the tier byte counts.
        Face {
            src: "assets/fonts/NotoSansSC.ttf",
            out: "noto-cjk-subset.ttf",
            ranges: subset::CJK_RANGES,
            feature: Some("BUNDLED_CJK"),
            family: "Noto Sans SC",
        },
    ];

    // Keep in sync with `font::bundled_families`, which reports these names for
    // PDF and SVG output. A face missing here means PDF would name a font it
    // never embeds, so it is a build error rather than a warning.
    let mut expected: Vec<&str> = Vec::new();

    for face in &faces {
        println!("cargo:rerun-if-changed={}", face.src);

        if let Some(feature) = face.feature {
            let enabled = std::env::var_os(format!("CARGO_FEATURE_{feature}")).is_some();
            if !enabled {
                continue;
            }
            let src = manifest.join(face.src);
            if !src.exists() {
                panic!(
                    "{} is enabled but {} is missing. Fetch it with \
                     `scripts/fetch-bundled-fonts.sh` or disable the feature.",
                    feature, face.src
                );
            }
        }

        let src = manifest.join(face.src);
        let bytes = std::fs::read(&src).unwrap_or_else(|e| panic!("read {}: {e}", src.display()));
        let (data, stats) = subset::subset_ranges(&bytes, face.ranges)
            .unwrap_or_else(|e| panic!("subset {}: {e}", face.src));

        if !stats.layout_preserved {
            panic!(
                "{} is missing GSUB/GPOS/GDEF; shaping would be silently broken",
                face.src
            );
        }

        std::fs::write(out_dir.join(face.out), &data).expect("write subset font");
        expected.push(face.family);

        println!(
            "cargo:warning=hikari font subset [{}]: {} glyphs ({} codepoints), {} -> {} bytes ({:.0}%), layout preserved",
            face.family,
            stats.glyphs,
            stats.codepoints,
            stats.source_bytes,
            stats.subset_bytes,
            stats.subset_bytes as f64 * 100.0 / stats.source_bytes as f64,
        );
    }

    println!("cargo:bundled-families={}", expected.join(","));
}
