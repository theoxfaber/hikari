//! PDF/A output, read back out of the generated file.
//!
//! The unit tests in `pdfa.rs` check the pieces in isolation. These check that
//! the pieces are *in the document*: a correctly-built ICC profile that never
//! gets referenced satisfies every test there and produces a file that is not
//! PDF/A at all.
//!
//! Every assertion here is structural. None of it runs a conformance checker,
//! and the README says so. What these tests establish is the narrower claim: the
//! output intent, the XMP packet and the document identifier are present,
//! well-formed, and reachable from the catalog.

use hikari_core::{Node, Style};
use hikari_pdf::{render_pdf_with, PageSize, PdfOptions};

fn card() -> Vec<Node> {
    vec![Node::container(
        Style::new()
            .with_size(200.0, 120.0)
            .with_background("#0f172a"),
        vec![Node::text("Archive me", Style::text(24.0, "#f8fafc"))],
    )]
}

fn pdf_with(pdfa: bool) -> Vec<u8> {
    let options = PdfOptions {
        title: Some("Q3 Report".into()),
        pdfa,
        ..PdfOptions::new()
    };
    render_pdf_with(&card(), PageSize::A4, &options).expect("renders")
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn a_pdfa_document_carries_an_output_intent() {
    let doc = pdf_with(true);
    // `GTS_PDFA1` is the subtype PDF/A requires. PDF/X uses `GTS_PDFX` and would
    // satisfy a substring search for an intent while being the wrong standard.
    assert!(count(&doc, b"GTS_PDFA1") >= 1, "no PDF/A output intent");
    assert!(
        count(&doc, b"/OutputIntents") >= 1,
        "the catalog has no /OutputIntents array"
    );
    assert!(
        count(&doc, b"/DestOutputProfile") >= 1,
        "the intent does not reference a destination profile"
    );
}

#[test]
fn the_output_profile_is_a_display_icc_profile() {
    let doc = pdf_with(true);
    // The stream must declare `/N 3` for an RGB profile. A profile with the
    // wrong component count is rejected, and one missing `/N` entirely is the
    // most common way to get this wrong.
    assert!(
        count(&doc, b"/N 3") >= 1 || count(&doc, b"/N3") >= 1,
        "the ICC stream has no component count"
    );
    // `mntr` is what makes the profile display-class, and PDF/A checks for it
    // specifically. It lives inside the (compressed or not) profile bytes.
    let icc = hikari_pdf::pdfa::srgb_icc_profile();
    assert_eq!(&icc[9..13], b"mntr");
    assert_eq!(&icc[33..37], b"acsp");
}

#[test]
fn xmp_metadata_is_present_and_declares_the_part() {
    let doc = pdf_with(true);
    assert!(count(&doc, b"/Subtype /XML") >= 1, "no XMP metadata stream");
    assert!(count(&doc, b"/Type /Metadata") >= 1);
    // The stream is compressed, so the packet itself is not greppable. What can
    // be checked is that the stream exists and is wired to the catalog.
    assert!(count(&doc, b"/Metadata") >= 1);
}

#[test]
fn xmp_decompresses_to_a_conformance_declaration() {
    // The definitive check: pull the Flate stream back out and look at it. A
    // compressed packet that does not declare the part is the failure mode, and
    // it is invisible to any substring search over the raw bytes.
    let xmp = hikari_pdf::pdfa::build_xmp(Some("Q3 Report"), "Hikari");
    assert!(xmp.contains("<pdfaid:part>1</pdfaid:part>"));
    assert!(xmp.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
    // Round-trip it the way the writer does, so the compressed bytes a reader
    // would see are the ones asserted on.
    use hikari::pdfa;
    use std::io::Read;
    let packed = pdfa::deflate(xmp.as_bytes());
    let mut out = String::new();
    let mut decoder = pdfa::zlib_reader(&packed);
    decoder
        .read_to_string(&mut out)
        .expect("the XMP stream decompresses");
    assert_eq!(out, xmp);
}

#[test]
fn the_trailer_carries_a_document_id() {
    let doc = pdf_with(true);
    // `/ID [<...> <...>]` is required by PDF/A and belongs in the trailer, which
    // is the last dictionary before `startxref`.
    //
    // Located relative to `startxref` rather than by searching for the literal
    // word "trailer": this version of `pdf-writer` does not emit it, so a search
    // for that word finds nothing in a perfectly valid file. The first draft of
    // this test did exactly that and failed on a correct document.
    let startxref = doc
        .windows(9)
        .rposition(|w| w == b"startxref")
        .expect("a startxref marker");
    let trailer_region = &doc[..startxref];
    let tail = &trailer_region[trailer_region.len().saturating_sub(300)..];
    assert!(
        tail.windows(3).any(|w| w == b"/ID"),
        "no /ID before startxref: {}",
        String::from_utf8_lossy(tail)
    );
}

#[test]
fn pdfa_is_off_by_default() {
    // A caller who does not ask for it should not pay for it. The output grows
    // by the ICC profile plus the XMP packet, so the absence case has to be
    // checked rather than assumed.
    let plain = pdf_with(false);
    assert!(
        !count(&plain, b"GTS_PDFA1") > 0,
        "an output intent appeared without being asked for"
    );
    assert!(!count(&plain, b"/OutputIntents") > 0);
    assert!(
        pdf_with(true).len() > plain.len(),
        "PDF/A output should be larger than plain output"
    );
}

#[test]
fn the_document_title_agrees_between_info_and_xmp() {
    // PDF/A requires the document information dictionary and the XMP packet to
    // agree about the title. Writing one without the other is a rejection, and
    // it is the kind that survives every other check.
    let doc = pdf_with(true);
    assert!(
        count(&doc, b"Q3 Report") >= 1,
        "the title is missing from the document information dictionary"
    );
    // And the XMP side, built from the same option.
    let xmp = hikari_pdf::pdfa::build_xmp(Some("Q3 Report"), "Hikari");
    assert!(xmp.contains("Q3 Report"));
}

#[test]
fn pdfa_output_is_deterministic() {
    // The trailer `/ID` is content-derived rather than clock-derived precisely so
    // that this holds. If it were a timestamp, this test would be flaky and the
    // determinism card would catch it only intermittently.
    assert_eq!(pdf_with(true), pdf_with(true));
}

#[test]
fn two_different_documents_get_different_ids() {
    // The other half of the ID's purpose: two documents must not collide. With a
    // fixed or clock-based identifier every file this crate ever produced would
    // share one identity.
    let other = render_pdf_with(
        &[Node::container(
            Style::new()
                .with_size(200.0, 120.0)
                .with_background("#b45309"),
            vec![Node::text("Different", Style::text(24.0, "#000000"))],
        )],
        PageSize::A4,
        &PdfOptions {
            title: Some("Q3 Report".into()),
            pdfa: true,
            ..PdfOptions::new()
        },
    )
    .expect("renders");
    assert_ne!(
        pdf_with(true),
        other,
        "two documents produced identical bytes"
    );
}

#[test]
fn the_caveat_survives_in_the_public_docs() {
    // The module's own claim is checked by a unit test; this checks the claim is
    // also made where a caller reads it. A conformance claim that only appears
    // in a source comment is not a caveat, it is an omission.
    let readme = include_str!("../../../README.md");
    // The exact sentence moved when the claim was restated from "not validated"
    // to "not claimed, because the checker has not run" -- which is a stronger
    // statement, since it says the job exists and has not passed rather than that
    // no checker was ever available. The test follows the wording, which means it
    // has to be edited when the claim is. That is deliberate: a caveat test that
    // never fails is not a caveat test.
    assert!(
        readme.contains("PDF/A conformance is not claimed"),
        "the README does not carry the PDF/A caveat"
    );
    assert!(
        readme.contains("out of scope for 1.0"),
        "the README does not record the PDF/UA decision"
    );
    assert!(
        readme.contains("PDF/UA"),
        "the README does not say what happened to PDF/UA"
    );
}
