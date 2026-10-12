//! PDF/A support, tested at the byte level.
//!
//! # Why these tests read bytes rather than call a checker
//!
//! The claim being made is narrow and structural: this output *contains* an
//! output intent with a well-formed ICC profile, an XMP packet declaring
//! conformance level B, and a document identifier. That claim is checkable by
//! reading the bytes, so it is checked that way.
//!
//! The claim is **not** "this file passes a PDF/A validator", and no test here
//! pretends to make it. A conformance checker would need to be run over the
//! output; until one is, every assertion below is about structure and none is
//! about conformance. The ICC profile tests are the sharpest version of that
//! line: a profile can satisfy every structural check below and still be wrong in
//! a way only a colour-management application would notice.

use super::pdfa::{build_xmp, deflate, document_id, srgb_icc_profile, zlib_reader, ICC_VERSION};

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// Read a big-endian u32.
fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Find the ICC tag table and return `(signature, offset, size)` for `sig`.
fn tag(profile: &[u8], sig: &[u8; 4]) -> Option<(usize, usize)> {
    let n = be32(profile, 128) as usize;
    for i in 0..n {
        let at = 132 + i * 12;
        if &profile[at..at + 4] == sig {
            return Some((
                be32(profile, at + 4) as usize,
                be32(profile, at + 8) as usize,
            ));
        }
    }
    None
}

#[test]
fn the_icc_header_is_the_size_the_header_says() {
    // The first field of an ICC profile is its total length, and a profile whose
    // header disagrees with its bytes is rejected before anything else is looked
    // at. This is the single most basic structural property and the easiest to
    // get wrong when building a profile by hand.
    let icc = srgb_icc_profile();
    assert_eq!(be32(&icc, 0) as usize, icc.len());
    assert!(
        icc.len() >= 132,
        "profile is too small to hold a header and a table"
    );
}

#[test]
fn the_icc_signature_and_version_are_right() {
    // Offsets are the ICC header layout. Every field there is four bytes,
    // including `preferred CMM` at 4..8 -- getting that wrong shifts everything
    // after it three bytes early, which is how a profile ends up
    // self-consistent (its declared size still matched its real size) and
    // unreadable to any other implementation.
    //
    // These offsets were written against that broken layout and so agreed with
    // it. veraPDF read the shifted header, found `XYZ ` where the data colour
    // space should be, and failed PDF/A clauses 6.2.2/1, 6.2.3.2/2 and
    // 6.2.3.3/1.
    let icc = srgb_icc_profile();
    assert_eq!(&icc[8..12], &ICC_VERSION, "expected an ICC 2.1 profile");
    assert_eq!(&icc[36..40], b"acsp", "ICC file signature");
    assert_eq!(&icc[12..16], b"mntr", "device class must be display");
}

#[test]
fn the_preferred_cmm_is_absent() {
    // Bytes 4..8 are the preferred CMM manufacturer signature; all zero means
    // "none", which is the honest value for a profile this crate builds. It is
    // a four-byte signature, not a single byte -- see the note on the offsets
    // above for what getting that wrong cost.
    let icc = srgb_icc_profile();
    assert_eq!(&icc[4..8], b"\0\0\0\0");
}

#[test]
fn the_icc_colour_spaces_are_srgb_to_xyz() {
    let icc = srgb_icc_profile();
    // 16..20 data colour space, 20..24 PCS.
    assert_eq!(&icc[16..20], b"RGB ", "data colour space");
    assert_eq!(&icc[20..24], b"XYZ ", "PCS");
    // The PCS illuminant is a `XYZNumber` at 68..80: X, then Y, then Z, each
    // s15Fixed16. D50 is X=0, Y=1.0, Z=0. These offsets were three bytes
    // early for the same reason the colour-space offsets were; Y read as 0.0
    // instead of 1.0, so the profile declared a black PCS illuminant.
    assert_eq!(be32(&icc, 68), 0, "PCS illuminant X should be 0");
    assert_eq!(
        be32(&icc, 72),
        0x0001_0000,
        "PCS illuminant Y should be 1.0"
    );
    assert_eq!(be32(&icc, 76), 0, "PCS illuminant Z should be 0");
}

#[test]
fn the_required_tags_are_present() {
    // PDF/A requires the destination profile to be a display profile carrying
    // `mntr` or `prtr`, and to describe its colourants. Missing `mntr` is the
    // specific rejection that matters; the colorant tags are what make the
    // profile describe anything at all.
    let icc = srgb_icc_profile();
    for sig in [
        b"mntr", b"desc", b"wtpt", b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC",
    ] {
        assert!(
            tag(&icc, sig).is_some(),
            "missing required tag {}",
            String::from_utf8_lossy(sig)
        );
    }
}

#[test]
fn every_tag_offset_and_size_is_inside_the_profile() {
    // A tag table with an out-of-range offset reads whatever happens to be
    // there, and the profile still looks structurally valid to a shallow check.
    // Walking every entry is the only way to catch it.
    let icc = srgb_icc_profile();
    let n = be32(&icc, 128) as usize;
    assert!(n > 0);
    for i in 0..n {
        let at = 132 + i * 12;
        let offset = be32(&icc, at + 4) as usize;
        let size = be32(&icc, at + 8) as usize;
        assert!(
            offset + size <= icc.len(),
            "tag {} runs past the end: {offset}+{size} > {}",
            String::from_utf8_lossy(&icc[at..at + 4]),
            icc.len()
        );
        // Tags start after the table, and an offset pointing into the table
        // would be a self-referential profile.
        assert!(
            offset >= 128 + 4 + n * 12,
            "tag {} overlaps the header or tag table",
            String::from_utf8_lossy(&icc[at..at + 4])
        );
    }
}

#[test]
fn every_tag_is_four_byte_aligned() {
    // ICC requires tag data to start on a 4-byte boundary. The profile is
    // written with padding to achieve this, and this is what verifies it rather
    // than assuming the padding loop ran.
    let icc = srgb_icc_profile();
    let n = be32(&icc, 128) as usize;
    for i in 0..n {
        let at = 132 + i * 12;
        assert_eq!(
            be32(&icc, at + 4) % 4,
            0,
            "tag {} is not 4-byte aligned",
            String::from_utf8_lossy(&icc[at..at + 4])
        );
    }
}

#[test]
fn the_profile_has_no_trailing_bytes() {
    // The declared size equals the length (checked above), so this additionally
    // pins that the body was not padded past the size field.
    let icc = srgb_icc_profile();
    assert_eq!(be32(&icc, 0) as usize, icc.len());
    assert!(!icc.is_empty());
}

#[test]
fn the_profile_is_small() {
    // Not a correctness property, but a regression guard: if a future change
    // starts embedding a full profile by accident, the PDF gets ~6x larger for
    // no benefit and nothing else would notice.
    let icc = srgb_icc_profile();
    assert!(
        icc.len() < 1024,
        "profile grew to {} bytes; it should stay minimal",
        icc.len()
    );
}

#[test]
fn xmp_declares_part_one_and_conformance_b() {
    let xmp = build_xmp(Some("Report"), Some("Hikari Labs"), "Hikari");
    assert!(xmp.contains("<pdfaid:part>1</pdfaid:part>"));
    assert!(xmp.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
    // The namespace declarations are required; an RDF block without them is not
    // parseable and the whole packet is rejected.
    assert!(xmp.contains("http://www.aiim.org/pdfa/ns/id/"));
    assert!(xmp.contains("http://ns.adobe.com/xap/1.0/"));
    assert!(xmp.contains("http://ns.adobe.com/pdf/1.3/"));
}

#[test]
fn xmp_is_wrapped_in_an_xpacket() {
    // PDF/A requires the XMP packet to be delimited by xpacket PIs, not just to
    // be XML.
    assert!(xmp_packets_are_balanced(&build_xmp(
        None,
        Some("Hikari Labs"),
        "Hikari"
    )));
}

fn xmp_packets_are_balanced(xmp: &str) -> bool {
    xmp.starts_with("<?xpacket begin=") && xmp.trim_end().ends_with("<?xpacket end=\"w\"?>")
}

#[test]
fn xmp_carries_the_title_it_was_given() {
    let xmp = build_xmp(Some("Q3 Report"), Some("Hikari Labs"), "Hikari");
    assert!(xmp.contains("Q3 Report"));
    assert!(xmp.contains(r#"xml:lang="x-default""#));
}

#[test]
fn xmp_with_no_title_still_has_the_element() {
    // The document information dictionary has no `/Title` when the caller sets
    // none, and the XMP must agree. Writing an empty `<dc:title/>` and omitting
    // it entirely are both accepted, but writing a *different* value is not --
    // so this pins that "no title" produces an empty title rather than a
    // placeholder.
    let xmp = build_xmp(None, Some("Hikari Labs"), "Hikari");
    assert!(xmp.contains("dc:title"));
    assert!(
        !xmp.contains("untitled"),
        "a placeholder would disagree with the Info dict"
    );
}

#[test]
fn xmp_escapes_markup_in_the_title() {
    // The document information dictionary escapes for PDF, XMP escapes for XML,
    // and both get the same value. A title with an ampersand is the shortest way
    // to produce a malformed packet, and a malformed packet fails validation
    // before any other rule is reached.
    let xmp = build_xmp(Some("Tom & Jerry <Ltd> \"x\""), None, "Hikari 1.0");
    assert!(xmp.contains("Tom &amp; Jerry &lt;Ltd&gt;"));
    assert!(
        !xmp.contains("<Ltd>"),
        "angle brackets from the title leaked into the packet unescaped"
    );
    assert!(
        xmp.contains("Hikari 1.0"),
        "the producer is not escaped away"
    );
}

#[test]
fn xmp_is_deterministic() {
    // Two calls with the same input must produce identical bytes, or the
    // determinism guarantee this crate makes everywhere else does not hold for
    // the PDF path. Note the fixed creation date: a clock here would make every
    // render of the same tree differ.
    assert_eq!(
        build_xmp(Some("t"), None, "p"),
        build_xmp(Some("t"), None, "p")
    );
}

#[test]
fn the_document_id_is_sixteen_bytes_twice() {
    let id = document_id(b"hello");
    assert_eq!(id.len(), 2);
    assert_eq!(id[0].len(), 16);
    assert_eq!(id[1].len(), 16);
}

#[test]
fn the_two_id_halves_are_not_identical() {
    // The trailer `/ID` is `[original, current]`. Emitting the same value twice
    // is structurally valid and semantically wrong, and nothing at the byte level
    // would flag it.
    let id = document_id(b"hello");
    assert_ne!(id[0], id[1]);
}

#[test]
fn the_document_id_depends_on_the_content() {
    let a = document_id(b"document one");
    let b = document_id(b"document two");
    assert_ne!(a[0], b[0], "two different documents shared an identifier");
    assert_ne!(a[1], b[1]);
}

#[test]
fn the_document_id_is_stable_for_the_same_content() {
    // Determinism: the same document must always get the same id, or every PDF
    // this crate emits differs run to run.
    assert_eq!(document_id(b"same"), document_id(b"same"));
}

#[test]
fn the_document_id_handles_empty_and_single_byte_input() {
    // Edge cases that a hand-rolled hash is exactly where to break.
    assert_ne!(document_id(b""), document_id(b"\0"));
    assert_ne!(document_id(b"\0"), [[0u8; 16]; 2]);
    // A large input, to confirm the accumulation is wrapping rather than
    // overflowing-and-panicking in debug builds.
    let big = vec![0xABu8; 100_000];
    assert_eq!(document_id(&big).len(), 2);
}

#[test]
fn deflate_round_trips() {
    // Used for the XMP and ICC streams. A stream that cannot be decompressed is
    // a PDF that cannot be opened, and a broken deflate call is silent.
    let original = build_xmp(Some("t"), None, "p").into_bytes();
    let packed = deflate(&original);
    let mut out = Vec::new();
    // Through the read interface, which is what a PDF reader actually does.
    let mut decoder = zlib_reader(&packed);
    std::io::Read::read_to_end(&mut decoder, &mut out).expect("deflate round trip");
    assert_eq!(out, original);
}

#[test]
fn the_pdfa_claim_is_not_wider_than_it_is() {
    // A guard on the documentation itself. The module claims structural support
    // and explicitly not conformance; if the wording is ever changed to claim
    // validation, this test is the reminder that nothing here runs a checker.
    let src = include_str!("pdfa.rs");
    assert!(
        src.contains("has not been validated against a PDF/A conformance checker"),
        "the not-validated caveat is missing from the module docs"
    );
    assert!(
        count(src.as_bytes(), b"conformance checker") >= 1,
        "the conformance caveat should be stated where it matters"
    );
}

#[test]
fn every_tag_declares_its_type_in_its_data() {
    // The tag table says *which* tag this is; the first four bytes of the tag's
    // data say what *kind* of tag it is. A profile missing the second satisfies
    // every table-level check -- the fourcc is present, the offset is in range,
    // the size fits -- and leaves a reader unable to interpret any of them.
    //
    // The first version of the profile omitted these signatures entirely and
    // passed every test above. What caught it was a lint reporting that the
    // `kind` field was never read, which is a good argument for taking "this
    // field is set but nothing consumes it" seriously rather than deleting the
    // field.
    let icc = srgb_icc_profile();
    for (sig, kind) in [
        (b"desc", b"desc"),
        (b"cprt", b"text"),
        (b"wtpt", b"XYZ "),
        (b"rXYZ", b"XYZ "),
        (b"gXYZ", b"XYZ "),
        (b"bXYZ", b"XYZ "),
        (b"rTRC", b"curv"),
        (b"gTRC", b"curv"),
        (b"bTRC", b"curv"),
        (b"mntr", b"data"),
    ] {
        let sig: &[u8; 4] = sig;
        let (offset, size) = tag(&icc, sig).unwrap_or_else(|| {
            panic!("missing tag {}", String::from_utf8_lossy(sig));
        });
        assert!(
            size >= 4,
            "tag {sig:?} is too small to hold a type signature"
        );
        assert_eq!(
            &icc[offset..offset + 4],
            kind,
            "tag {:?} does not declare its type",
            String::from_utf8_lossy(sig)
        );
    }
}

#[test]
fn the_colourant_values_are_the_srgb_primaries() {
    // Reads past the type signature into the actual values. A profile whose
    // colorant tags are present but wrong passes every structural test above and
    // describes a colour nobody asked for.
    let icc = srgb_icc_profile();
    let (offset, _) = tag(&icc, b"rXYZ").expect("rXYZ");
    // Type signature (4), reserved (4), then X/Y/Z as s15Fixed16.
    assert_eq!(be32(&icc, offset + 8), 0x6F9D, "red X, expected 0.4360");
    let (offset, _) = tag(&icc, b"wtpt").expect("wtpt");
    assert_eq!(be32(&icc, offset + 12), 0x0001_0000, "white point Y");
}

#[test]
fn the_colourant_luminances_sum_to_the_white_point() {
    // The defining property of a set of colorants: their Y values are their
    // contributions to white, so they must sum to the white point's luminance.
    //
    // This is the check that catches a transposed green/blue row, which is
    // exactly the error in the first version of the table -- it summed to 0.65,
    // a profile describing a colour that was 35% too dark, and every other test
    // in this file passed.
    let icc = srgb_icc_profile();
    let mut total = 0f64;
    for sig in [b"rXYZ", b"gXYZ", b"bXYZ"] {
        let sig: &[u8; 4] = sig;
        let (offset, _) = tag(&icc, sig).expect("colorant tag");
        let y = f64::from(be32(&icc, offset + 12)) / 65536.0;
        total += y;
    }
    assert!(
        (total - 1.0).abs() < 0.001,
        "colorant luminances sum to {total}, expected 1.0"
    );
}
