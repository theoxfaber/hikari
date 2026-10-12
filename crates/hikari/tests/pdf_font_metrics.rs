//! PDF font metrics, pinned.
//!
//! These assertions exist because of a gap this file was written to close. When
//! PDF font embedding moved from `ttf-parser` to `skrifa`, the numbers a PDF
//! reader uses to place glyphs -- the `/W` width array and the `/FontDescriptor`
//! fields -- changed from one font backend to another. Every other test in the
//! suite passed, the corpus digests matched, and the PDF was simply
//! wrong-in-a-way-nobody-looked-at: characters silently mis-spaced by a fraction
//! of a point per glyph.
//!
//! A side-by-side comparison showed the two backends agree exactly, which is why
//! the migration was safe. That agreement is worth keeping. The values below are
//! the measured ones from the embedded DejaVu Sans subset, expressed in
//! thousandths of an em as the PDF spec requires.

use hikari::{render_pdf, Node, PageSize, Style};

/// Render a one-page PDF and return its bytes as lossy UTF-8, which is enough
/// to read the uncompressed dictionary entries out of.
fn pdf_for(text: &str) -> String {
    let tree = Node::container(
        Style::new()
            .with_size(600.0, 200.0)
            .with_background("#ffffff"),
        vec![Node::text(text, Style::text(32.0, "#000000"))],
    );
    let pdf = render_pdf(&[tree], PageSize::Custom { w: 600.0, h: 200.0 }).expect("pdf renders");
    assert_eq!(&pdf[..8], b"%PDF-1.7", "output must be a PDF");
    String::from_utf8_lossy(&pdf).into_owned()
}

/// The `/W` array, which is the compact run-length form: `gid [w w w]`.
fn widths(pdf: &str) -> String {
    let start = pdf.find("/W [").expect("pdf carries a /W array");
    // Scan for the matching bracket rather than searching for a literal `] >>`.
    // The array is doubly nested -- `gid [w w w]` -- so a flat search either
    // stops at the inner bracket or, when it fails to match at all, runs to the
    // end of the file and returns the embedded font program as well. That
    // produced a multi-megabyte assertion message on the first attempt.
    let bytes = pdf.as_bytes();
    let mut depth = 0usize;
    let mut end = start;
    for (i, b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    pdf[start..end].to_owned()
}

/// A single `/Key value` pair from the font descriptor.
fn descriptor(pdf: &str, key: &str) -> String {
    let i = pdf.find(key).unwrap_or_else(|| panic!("pdf has no {key}"));
    pdf[i + key.len()..]
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn glyph_widths_are_exact_and_pinned() {
    // These are the embedded DejaVu Sans `hmtx` advances at 1000 units/em. A
    // wrong width here does not look wrong: the text still renders, it is just
    // spaced slightly off, which no digest catches and no ink count notices.
    //
    // Gids 36 and 78 changed when the writer stopped recording the *shaped*
    // advance and started recording the font's own. The shaped advance carries
    // kerning, so those two glyphs were written as 620.12 and 561.52 when the
    // font says 684.08 and 579.10 -- describing glyphs that do not exist.
    // PDF/A-1 clause 6.3.6 requires `/W` to agree with the embedded program and
    // veraPDF compares them numerically, so this was a conformance failure as
    // well as a spacing one.
    // The glyphs are numbered 1..13 because the font is now subset on this
    // path. The same thirteen advances appear either way -- 317.8711, four
    // times 636.23047, 684.08203, 751.9531, 684.08203, 612.79297, 277.83203,
    // 579.10156, 411.1328, 629.8828 -- so only the numbering changed, not the
    // metrics. It was previously a sparse array reaching gid 5042 because the
    // subset had been abandoned, which cost the whole font.
    //
    // 629.8828 is the `fi` ligature. It was present before too, at gid 5042,
    // but under the wrong *character*: the shaper reports the first character
    // of a cluster, so it was recorded as `f` and remapped onto the plain `f`.
    let pdf = pdf_for("Hikari AVfi 0123");
    assert_eq!(
        widths(&pdf),
        "/W [1 [317.8711 636.23047 636.23047 636.23047 636.23047 684.08203 \
         751.9531 684.08203 612.79297 277.83203 579.10156 411.1328 629.8828]]",
        "glyph advance widths changed; characters would mis-space"
    );
}

#[test]
fn a_ligature_is_not_remapped_onto_its_component() {
    // The bug this pins produced no failure at all. It drew the wrong shape.
    //
    // The shaper reports one character per glyph, and for a ligature that
    // character is the first of the cluster -- so the `fi` ligature (gid 5042
    // in DejaVu Sans) was recorded as `f`. The ligature and the plain `f` then
    // both claimed to be `f`, and the ligature was remapped onto the subset's
    // plain `f`. The document rendered "office" with a real `f` where the `fi`
    // should have been, and nothing here could see it: the corpus digests
    // stayed stable (a wrong-but-consistent glyph is still deterministic), and
    // only veraPDF noticed, by cross-reading the content stream against the
    // embedded font program.
    //
    // The assertion is on **ToUnicode**, not on `/W`. An earlier version of
    // this test checked widths and passed against the broken code, because the
    // width array kept the *ligature's* advance while the glyph underneath was
    // the plain `f`. The number was right and the shape was wrong -- which is
    // exactly why the mismatch only surfaced as a conformance failure.
    // `profile`, not `office`. Both contain "ffi", but only the first ligates:
    // DejaVu has no U+FB03 triple ligature, so the shaper treats the two
    // spellings differently and `office` never produces a standalone `fi`
    // glyph at all. A test written against text that happens not to exercise
    // the path is a test that passes for the wrong reason -- which is how the
    // first version of this one did.
    let ligated = pdf_for("profile");

    // `fi` must extract as the precomposed ligature, not as `f`.
    assert!(
        ligated.contains("FB01"),
        "the fi ligature must map to U+FB01 in ToUnicode"
    );
    assert!(
        ligated.contains("0066"),
        "a plain f must still map to U+0066"
    );
    // Both present: the document really does contain both characters, so the
    // presence of FB01 is not an accident of the fixture text. If the ligature
    // had been remapped onto `f` these would collapse to one gid and `f` would
    // map to exactly one code point.

    // And the ligature must be billed at the ligature's own advance. This is
    // the half that veraPDF checks; the ToUnicode half is the one that proves
    // the *glyph* is right.
    let lig = single_width(&pdf_for("fi"));
    let f = single_width(&pdf_for("f"));
    assert!(
        lig > f * 1.5,
        "the fi ligature must be billed wider than one f: {lig} vs {f}"
    );
}

/// The first glyph's advance in a `/W` array.
///
/// `/W` is the compact run-length form: `[<first-gid> [<w> <w> ...] ...]`, so
/// the widths are in the *inner* array. Reading the outer one yields the
/// starting gid, which is how the first version of this helper compared 1
/// against 1 and passed a meaningless assertion.
fn single_width(pdf: &str) -> f32 {
    let w = widths(pdf);
    let run = w.find('[').expect("/W run");
    let inner = w[run + 1..].find('[').expect("/W value array") + run + 1;
    let body = &w[inner + 1..];
    let close = body.find(']').expect("/W value array close");
    body[..close]
        .split_whitespace()
        .next()
        .expect("a width")
        .parse()
        .expect("numeric width")
}

#[test]
fn font_descriptor_fields_are_pinned() {
    let pdf = pdf_for("Hikari AVfi 0123");
    // /Ascent and /Descent come from the font's own line metrics, /CapHeight
    // from the OS/2 table, /FontBBox from `head`.
    assert_eq!(descriptor(&pdf, "/Ascent"), "1901");
    assert_eq!(descriptor(&pdf, "/Descent"), "-483");
    assert_eq!(descriptor(&pdf, "/CapHeight"), "1493");
    assert_eq!(descriptor(&pdf, "/ItalicAngle"), "0");
    assert_eq!(descriptor(&pdf, "/FontBBox"), "[-2090");
}

#[test]
fn widths_scale_with_the_font_not_the_page_size() {
    // Widths are in thousandths of an em, so they must not move when the page
    // does. If they did, the same string would space differently in an A4
    // invoice and a 1200x630 card, which is the kind of thing that only shows up
    // in print.
    let a = widths(&pdf_for("Hamburgefonstiv"));
    let tree = Node::container(
        Style::new()
            .with_size(1200.0, 630.0)
            .with_background("#ffffff"),
        vec![Node::text("Hamburgefonstiv", Style::text(32.0, "#000000"))],
    );
    let pdf = render_pdf(
        &[tree],
        PageSize::Custom {
            w: 1200.0,
            h: 630.0,
        },
    )
    .expect("pdf");
    let b = widths(&String::from_utf8_lossy(&pdf));
    assert_eq!(a, b, "glyph widths must not depend on page dimensions");
}

#[test]
fn a_registered_font_gets_its_own_widths() {
    // A second face must not silently reuse the built-in font's advances. This
    // is the case that would break if a backend change ever collapsed every
    // `DocFont` onto one shared parsed face.
    let id = hikari::register_font("probe", hikari::builtin_bytes()).expect("register");
    let tree = Node::container(
        Style::new()
            .with_size(600.0, 200.0)
            .with_background("#ffffff"),
        vec![Node::text(
            "Hikari AVfi 0123",
            Style::text(32.0, "#000000").with_font(id),
        )],
    );
    let pdf = render_pdf(&[tree], PageSize::Custom { w: 600.0, h: 200.0 }).expect("pdf");
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/FontFile2"),
        "a registered font must still be embedded in the PDF"
    );
    // Same bytes as the built-in, so the same widths; the point is that the
    // descriptor exists and is populated rather than falling back.
    assert_eq!(descriptor(&text, "/Ascent"), "1901");
}

#[test]
fn a_character_with_no_glyph_is_an_error_not_a_wrong_width() {
    // The width array is indexed by glyph id, so a character the embedded font
    // cannot map has no width to write. This asserts the renderer refuses rather
    // than emitting something plausible.
    //
    // It originally asserted graceful degradation instead, on the assumption
    // that an unmapped character would be skipped. It is not: the render fails
    // with `no glyph for ... (install a CJK fallback font)`. That is the better
    // contract, and a PDF with one silently mis-spaced glyph in a page of
    // correct ones is far worse to debug than a clear error, so the test now
    // pins the error.
    let tree = Node::container(
        Style::new()
            .with_size(600.0, 200.0)
            .with_background("#ffffff"),
        vec![Node::text("A\u{10FFFD} B", Style::text(32.0, "#000000"))],
    );
    let err = render_pdf(&[tree], PageSize::Custom { w: 600.0, h: 200.0 })
        .expect_err("an unmapped character must fail rather than mis-space");
    let message = format!("{err:?}");
    assert!(
        message.contains("no glyph"),
        "expected a missing-glyph error, got {message}"
    );

    // And the supported half still works, so this is not a blanket rejection of
    // the codepoint range.
    let ok = widths(&pdf_for("AB"));
    assert!(
        ok.starts_with("/W ["),
        "a supported string should still produce widths"
    );
}
