//! Guards on the embedded font subset.
//!
//! These exist because the failure they catch is *silent*. A subsetter that
//! renumbers glyphs and drops `GSUB`/`GPOS`/`GDEF` still renders Latin
//! correctly, still passes a smoke test, and still produces a plausible PNG --
//! while quietly losing Arabic joining, kerning and ligatures. Every test here
//! compares against the full source font so a regression shows up as a failing
//! test rather than a subtly worse image.

use super::shape::font_bytes;

const SOURCE: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// Shape `text` with `rustybuzz` and return (glyph count, total advance).
fn shape(bytes: &[u8], text: &str) -> (usize, i32) {
    let face = rustybuzz::Face::from_slice(bytes, 0).expect("font loads");
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let out = rustybuzz::shape(&face, &[], buffer);
    let width = out.glyph_positions().iter().map(|p| p.x_advance).sum();
    (out.len(), width)
}

/// Reverse a glyph id back to a codepoint by scanning the covered ranges.
///
/// The subset ships `post` format 3.0 (no glyph names -- 62 KB of names for
/// nothing), and presentation forms reached only through GSUB have no direct
/// cmap entry under their own codepoint, so a scan is the honest inverse here.
fn glyph_to_codepoint(bytes: &[u8], gid: u16) -> Option<char> {
    let ttf = ttf_parser::Face::parse(bytes, 0).expect("ttf parses");
    for (lo, hi) in RANGES {
        for cp in *lo..=*hi {
            if let Some(ch) = char::from_u32(cp) {
                if ttf.glyph_index(ch).map(|g| g.0) == Some(gid) {
                    return Some(ch);
                }
            }
        }
    }
    None
}

fn has_table(bytes: &[u8], tag: &[u8; 4]) -> bool {
    let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    (0..count).any(|i| {
        let o = 12 + i * 16;
        &bytes[o..o + 4] == tag
    })
}

#[test]
fn embedded_font_is_smaller_than_source() {
    assert!(
        font_bytes().len() < SOURCE.len(),
        "subset ({} B) should be smaller than source ({} B)",
        font_bytes().len(),
        SOURCE.len()
    );
}

#[test]
fn embedded_font_keeps_layout_tables() {
    // GSUB carries substitutions (joining, ligatures), GPOS carries kerning
    // and mark positioning, GDEF carries glyph classes the other two need.
    for (tag, name) in [
        (&b"GSUB"[..], "GSUB"),
        (&b"GPOS"[..], "GPOS"),
        (&b"GDEF"[..], "GDEF"),
    ] {
        assert!(
            has_table(font_bytes(), tag.try_into().expect("4-byte tag")),
            "embedded font is missing {name}; text shaping is silently degraded"
        );
    }
}

#[test]
fn embedded_font_has_cmap() {
    // A PDF-oriented subsetter will happily emit a font with no cmap, because
    // PDF CID fonts carry their own encoding. That font cannot be shaped.
    assert!(
        has_table(font_bytes(), b"cmap"),
        "embedded font has no cmap"
    );
    let ttf = ttf_parser::Face::parse(font_bytes(), 0).expect("subset parses");
    assert!(ttf.glyph_index('A').is_some(), "'A' missing from cmap");
    assert!(ttf.glyph_index('\u{0645}').is_some(), "Arabic meem missing");
}

#[test]
fn arabic_letters_join() {
    // Without GSUB, each Arabic letter falls back to its isolated form and the
    // word renders as disconnected letters. Joining requires a glyph from the
    // Arabic Presentation Forms blocks, which only a substitution can produce.
    let text = "\u{0645}\u{062D}\u{0628}\u{0627}"; // محبا
    let bytes = font_bytes();
    let face = rustybuzz::Face::from_slice(bytes, 0).expect("font loads");
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let gids: Vec<u16> = rustybuzz::shape(&face, &[], buffer)
        .glyph_infos()
        .iter()
        .map(|g| g.glyph_id as u16)
        .collect();

    let joined = gids
        .iter()
        .filter_map(|g| glyph_to_codepoint(bytes, *g))
        .any(|c| {
            let v = c as u32;
            (0xFB50..=0xFDFF).contains(&v) || (0xFE70..=0xFEFF).contains(&v)
        });
    assert!(
        joined,
        "no Arabic presentation forms among glyph ids {gids:?} -- letters are not joining"
    );
}

#[test]
fn arabic_shaping_matches_source_font() {
    let text = "\u{0634}\u{064E}\u{0644}\u{0648}\u{0645} \u{0645}\u{062D}\u{064F}\u{0628}\u{0627}";
    assert_eq!(
        shape(font_bytes(), text),
        shape(SOURCE, text),
        "Arabic shaping diverges from the source font"
    );
}

#[test]
fn persian_shaping_matches_source_font() {
    // Farsi pulls in the presentation forms-B range via GSUB.
    let text = "\u{0641}\u{0627}\u{0631}\u{0633}\u{06CC}"; // فارسی
    assert_eq!(
        shape(font_bytes(), text),
        shape(SOURCE, text),
        "Persian shaping diverges from the source font"
    );
}

#[test]
fn hebrew_shaping_matches_source_font() {
    let text = "\u{05E9}\u{05DC}\u{05D5}\u{05DD} \u{05E2}\u{05D5}\u{05DC}\u{05DD}";
    assert_eq!(
        shape(font_bytes(), text),
        shape(SOURCE, text),
        "Hebrew shaping diverges from the source font"
    );
}

#[test]
fn kerning_matches_source_font() {
    // Kerning lives in GPOS as position deltas, so the glyph run is identical
    // either way -- only the advances differ. This is the test that catches a
    // dropped GPOS.
    let text = "AVATAR Wave To.";
    let (subset_glyphs, subset_width) = shape(font_bytes(), text);
    let (source_glyphs, source_width) = shape(SOURCE, text);
    assert_eq!(subset_glyphs, source_glyphs);
    assert_eq!(
        subset_width, source_width,
        "kerning lost: subset width {subset_width} != source {source_width}"
    );
}

#[test]
fn ligatures_match_source_font() {
    // fi/fl/ffi collapse to single glyphs when GSUB is intact.
    let text = "fi fl ffi office waffle";
    let (subset_glyphs, _) = shape(font_bytes(), text);
    let (source_glyphs, _) = shape(SOURCE, text);
    assert_eq!(subset_glyphs, source_glyphs);
    assert!(
        subset_glyphs < 20,
        "expected ligature substitution, got {subset_glyphs} glyphs"
    );
}

#[test]
fn shaped_glyphs_always_have_outlines() {
    // The failure mode this catches is nasty: if GSUB substitutes *to* a glyph
    // whose outline was not carried into the subset, shaping succeeds, the
    // glyph count and advance are all correct, and the character simply
    // renders as a blank gap. Nothing errors. The only way to see it is to
    // check that every glyph the shaper can emit actually has geometry.
    struct Counter(usize);
    impl ttf_parser::OutlineBuilder for Counter {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn close(&mut self) {}
    }

    let bytes = font_bytes();
    let ttf = ttf_parser::Face::parse(bytes, 0).expect("subset parses");
    let face = rustybuzz::Face::from_slice(bytes, 0).expect("font loads");

    let samples = [
        "office waffle flag affix",                 // fi/fl/ffi/f-l ligatures
        "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}", // Arabic joining
        "\u{0641}\u{0627}\u{0631}\u{0633}\u{06CC}", // Persian, presentation forms-B
        "\u{05E9}\u{05DC}\u{05D5}\u{05DD}",         // Hebrew
        "\u{0391}\u{03B2}\u{03B3}",                 // Greek
        "\u{0416}\u{0438}\u{0432}\u{043D}",         // Cyrillic
        "AVATAR Wave To. ffi",                      // kerning + ligature
    ];

    for text in samples {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let chars: Vec<char> = text.chars().collect();
        for info in rustybuzz::shape(&face, &[], buffer).glyph_infos() {
            // Whitespace legitimately maps to an empty glyph. Everything else
            // must have geometry or the character renders as a blank gap.
            let source = chars.get(info.cluster as usize).copied();
            if source.is_some_and(char::is_whitespace) {
                continue;
            }
            let gid = ttf_parser::GlyphId(info.glyph_id as u16);
            let mut counter = Counter(0);
            let drawn = ttf.outline_glyph(gid, &mut counter).is_some();
            assert!(
                drawn,
                "glyph {} for {:?} in {text:?} has no outline -- substitution target missing from subset",
                info.glyph_id,
                source
            );
            assert!(
                counter.0 > 0,
                "glyph {} for {:?} in {text:?} has an empty outline",
                info.glyph_id,
                source
            );
        }
    }
}

/// The codepoint ranges the embedded subset covers.
///
/// Mirrors `build/subset.rs::RANGES`. If you widen coverage there, widen it
/// here too, or this test stops checking the new codepoints.
const RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E),
    (0x00A0, 0x00FF),
    (0x0100, 0x024F),
    (0x0370, 0x03FF),
    (0x0400, 0x04FF),
    (0x0590, 0x05FF),
    (0x0600, 0x06FF),
    (0x2000, 0x206F),
    (0x2070, 0x209F),
    (0x20A0, 0x20CF),
    (0x2100, 0x214F),
    (0x2190, 0x21FF),
    (0x2200, 0x22FF),
    (0x25A0, 0x25FF),
    (0x2600, 0x26FF),
    (0x2700, 0x27BF),
    (0xFB00, 0xFB4F),
    (0xFB50, 0xFDFF),
    (0xFE70, 0xFEFF),
];

#[test]
fn every_covered_codepoint_has_an_outline() {
    // Guards the composite closure: a composite whose components were dropped
    // still has an entry in loca but renders truncated or blank.
    struct Counter(usize);
    impl ttf_parser::OutlineBuilder for Counter {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn close(&mut self) {}
    }

    let ttf = ttf_parser::Face::parse(font_bytes(), 0).expect("subset parses");
    let mut checked = 0;
    for (lo, hi) in RANGES {
        for cp in *lo..=*hi {
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            let Some(gid) = ttf.glyph_index(ch) else {
                continue;
            };
            // Whitespace, zero-width marks and format controls legitimately
            // carry no outline; what matters is that they still map to a glyph
            // so measurement and line breaking behave.
            // `char::is_control` covers the C0/C1 blocks and DEL; the explicit
            // list covers the invisible formatting characters in General
            // Punctuation that Unicode does not classify as control.
            let blank = ch.is_whitespace()
                || ch.is_control()
                || matches!(ch,
                    '\u{00AD}'               // soft hyphen
                    | '\u{200B}'..='\u{200F}' // zero width + bidi marks
                    | '\u{2028}'..='\u{2029}' // line/paragraph separators
                    | '\u{202A}'..='\u{202E}' // bidi embedding/override
                    | '\u{2060}'..='\u{206F}' // word joiner, invisible operators, isolates
                    | '\u{FEFF}'               // zero width no-break space
                );
            if blank {
                checked += 1;
                continue;
            }
            let mut counter = Counter(0);
            let has_outline = ttf.outline_glyph(gid, &mut counter).is_some();
            assert!(has_outline, "U+{cp:04X} '{ch}' has no outline");
            assert!(counter.0 > 0, "U+{cp:04X} '{ch}' produced an empty outline");
            checked += 1;
        }
    }
    assert!(checked > 2000, "expected broad coverage, checked {checked}");
}
