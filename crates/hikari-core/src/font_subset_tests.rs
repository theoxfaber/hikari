//! Guards on the embedded font subset.
//!
//! These exist because the failure they catch is *silent*. A subsetter that
//! renumbers glyphs and drops `GSUB`/`GPOS`/`GDEF` still renders Latin
//! correctly, still passes a smoke test, and still produces a plausible PNG --
//! while quietly losing Arabic joining, kerning and ligatures. Every test here
//! compares against the full source font so a regression shows up as a failing
//! test rather than a subtly worse image.

use super::shape::font_bytes;
use skrifa::MetadataProvider;

const SOURCE: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// Counts path operations, to answer "does this glyph have any geometry?".
///
/// This replaced a `ttf_parser::OutlineBuilder`, which was the last place the
/// core crate reached the unmaintained parser. Note it counts *operations*, not
/// points: the old builder counted one per callback too, so the threshold below
/// means the same thing it always did.
#[derive(Default)]
struct Counter(usize);

impl skrifa::outline::OutlinePen for Counter {
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
    fn close(&mut self) {
        self.0 += 1;
    }
}

/// Does `gid` have a non-empty outline in `bytes`?
fn glyph_is_drawn(bytes: &[u8], gid: u16) -> bool {
    let font = skrifa::FontRef::new(bytes).expect("font parses");
    let Some(outline) = font.outline_glyphs().get(skrifa::GlyphId::new(gid as u32)) else {
        return false;
    };
    let mut counter = Counter::default();
    let settings = skrifa::outline::DrawSettings::unhinted(
        skrifa::instance::Size::new(16.0),
        skrifa::instance::LocationRef::default(),
    );
    let _ = outline.draw(settings, &mut counter);
    counter.0 > 0
}

/// The `'static` requirement on the helpers below is not incidental:
/// `read-fonts` stores borrowed font data as a `Blob::Static` slice, so a font
/// parsed from a non-`'static` slice has no `From` impl and cannot be built at
/// all. Production gets this for free because the registry leaks font bytes once
/// at registration; the tests below are handed `include_bytes!` output and the
/// embedded subset, which are already `'static`.
///
/// One shaped glyph: its id and the byte offset it came from.
type Shaped = (u16, u32);

/// Shape `text` and return `(glyph count, total advance, glyphs)`.
///
/// One helper for all three, because the buffer is owned by the shaper and
/// cannot outlive it: `harfrust::shape` fills a caller-held `&mut Buffer`
/// rather than returning one. Building the buffer inside a function that also
/// reports from it is the only way to read it back out.
///
/// The cluster is carried alongside the glyph id because these tests map a glyph
/// back to the source character that produced it, which a ligature or a
/// contextual form has no other way to do.
fn shape_glyphs(bytes: &'static [u8], text: &str) -> (usize, i32, Vec<Shaped>) {
    let font = harfrust::Font::new(bytes, 0).expect("font loads");
    let shaper = harfrust::ShaperFont::new(&font);
    let mut buffer = harfrust::Buffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    harfrust::shape(&shaper, &mut buffer, harfrust::ShapeOptions::default())
        .expect("shaping succeeds");
    let width = buffer.glyph_positions().iter().map(|p| p.x_advance).sum();
    let glyphs = buffer
        .glyph_infos()
        .iter()
        .map(|g| (g.glyph_id as u16, g.cluster))
        .collect();
    (buffer.len(), width, glyphs)
}

/// Shape `text` and return (glyph count, total advance).
fn shape(bytes: &'static [u8], text: &str) -> (usize, i32) {
    let (count, width, _) = shape_glyphs(bytes, text);
    (count, width)
}

/// Reverse a glyph id back to a codepoint by scanning the covered ranges.
///
/// The subset ships `post` format 3.0 (no glyph names -- 62 KB of names for
/// nothing), and presentation forms reached only through GSUB have no direct
/// cmap entry under their own codepoint, so a scan is the honest inverse here.
fn glyph_to_codepoint(bytes: &[u8], gid: u16) -> Option<char> {
    let font = skrifa::FontRef::new(bytes).expect("font parses");
    for (lo, hi) in RANGES {
        for cp in *lo..=*hi {
            if let Some(ch) = char::from_u32(cp) {
                if font.charmap().map(ch).map(|g| g.to_u32() as u16) == Some(gid) {
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
    let font = skrifa::FontRef::new(font_bytes()).expect("subset parses");
    assert!(font.charmap().map('A').is_some(), "'A' missing from cmap");
    assert!(
        font.charmap().map('\u{0645}').is_some(),
        "Arabic meem missing"
    );
}

#[test]
fn arabic_letters_join() {
    // Without GSUB, each Arabic letter falls back to its isolated form and the
    // word renders as disconnected letters. Joining requires a glyph from the
    // Arabic Presentation Forms blocks, which only a substitution can produce.
    let text = "\u{0645}\u{062D}\u{0628}\u{0627}"; // محبا
    let bytes = font_bytes();
    let (_, _, glyphs) = shape_glyphs(bytes, text);
    let gids: Vec<u16> = glyphs.iter().map(|(gid, _)| *gid).collect();

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
    let bytes = font_bytes();

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
        let chars: Vec<char> = text.chars().collect();
        for (gidx, cluster) in shape_glyphs(bytes, text).2 {
            // Whitespace legitimately maps to an empty glyph. Everything else
            // must have geometry or the character renders as a blank gap.
            let source = chars.get(cluster as usize).copied();
            if source.is_some_and(char::is_whitespace) {
                continue;
            }
            let drawn = glyph_is_drawn(bytes, gidx);
            assert!(
                drawn,
                "glyph {gidx} for {source:?} in {text:?} has no outline -- substitution target missing from subset"
            );
            assert!(
                drawn,
                "glyph {} for {:?} in {text:?} has an empty outline",
                gidx, source
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
    let font = skrifa::FontRef::new(font_bytes()).expect("subset parses");
    let mut checked = 0;
    for (lo, hi) in RANGES {
        for cp in *lo..=*hi {
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            let Some(gid) = font.charmap().map(ch) else {
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
            let gid = gid.to_u32() as u16;
            assert!(
                glyph_is_drawn(font_bytes(), gid),
                "U+{cp:04X} '{ch}' has no outline or an empty one"
            );
            checked += 1;
        }
    }
    assert!(checked > 2000, "expected broad coverage, checked {checked}");
}
