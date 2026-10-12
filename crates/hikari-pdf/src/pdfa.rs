//! PDF/A-1b structural support.
//!
//! # What this is, and what it is not
//!
//! This adds the parts of PDF/A-1b (ISO 19005-1) that a generator can satisfy by
//! construction and that a test can check by looking at the bytes: an output
//! intent carrying a real ICC profile, an XMP metadata packet declaring
//! `pdfaid:part` and `pdfaid:conformance`, a document identifier, and a document
//! title requirement in the viewer preferences.
//!
//! **This has not been validated against a PDF/A conformance checker**, and the
//! README says so. What that means concretely: a checker may still reject the
//! output for a reason not visible in the byte stream -- a font whose embedded
//! program lacks a required table, an annotation missing a flag, an image whose
//! colour space is not permitted in the chosen part. Producing the right
//! metadata and claiming conformance are different claims, and only the first one
//! is supported here.
//!
//! The alternative is not "ship it later". It is that a caller archiving a
//! document has no path at all today, and the four requirements below are
//! necessary under every reading of the spec.
//!
//! # Why PDF/UA is absent
//!
//! PDF/UA (ISO 14289) requires *tagged* PDF: a structure tree, marked content on
//! every piece of text, `/Alt` on figures, `/Lang` on the document, and correct
//! heading nesting. That is not a metadata addition to this renderer -- it is a
//! different content-generation model, because every paint operation would have
//! to emit marked content paired with a tag. On top of that, the point of PDF/UA
//! is WCAG conformance, which is not mechanically checkable at all: a validator
//! can confirm the tags are present and correctly nested, and cannot tell whether
//! an alternative text is any good. Shipping that would mean claiming
//! accessibility this renderer cannot verify, which is worse than not shipping
//! it.
//!
//! # The ICC profile
//!
//! PDF/A requires the output intent's destination profile to carry an `mntr` or
//! `prtr` tag and to be a display-class profile. There is no sRGB profile in the
//! crate and no new dependency was added for it, so [`srgb_icc_profile`] builds
//! a minimal v2 matrix/TRC display profile from the sRGB primaries and transfer
//! function. It is about 500 bytes rather than the 3 KB a full profile would be,
//! because it stores the tags PDF/A actually requires and omits the descriptive
//! and copyright metadata a colour-managed application would want.
//!
//! A profile this small is not a substitute for a vendor one in a colour-managed
//! workflow, and the documentation on [`srgb_icc_profile`] says so.

use std::io::Write;

/// Byte size of the ICC profile, recorded in the header.
pub(crate) const ICC_VERSION: [u8; 4] = [0x02, 0x10, 0x00, 0x00];

/// sRGB colorants adapted to the D50 PCS white point, as ICC `XYZNumber`
/// triples in s15Fixed16.
///
/// The adaptation matters. ICC encodes colorant XYZ against the D50 PCS white
/// point while the sRGB primaries are defined against D65; using the raw sRGB
/// primaries would describe a slightly different white, which a colour-managed
/// application resolves as a visible tint. These are the Bradford-adapted
/// values, which is what a real sRGB profile carries.
///
/// The Y components sum to exactly 1.0, because they are the white point's
/// luminance. A test asserts it -- the easiest way to catch a transposition
/// between the green and blue rows, which is what the first version of this
/// table had.
const PRIMARIES: [[u32; 3]; 3] = [
    // rXYZ: 0.4360, 0.2225, 0.0139
    [0x6F9D, 0x38F5, 0x038F],
    // gXYZ: 0.3851, 0.7169, 0.0971
    [0x629A, 0xB77A, 0x18DC],
    // bXYZ: 0.1431, 0.0606, 0.7139
    [0x249E, 0x0F85, 0xB6EE],
];

/// D50 as a `XYZNumber`, encoded s15Fixed16.
const D50: [u32; 3] = [0x0000_0000, 0x0001_0000, 0x0000_0000];

/// Build a minimal sRGB ICC v2 display profile.
///
/// # Minimal
///
/// Present: `desc`, `wtpt`, `rXYZ`, `gXYZ`, `bXYZ`, `rTRC`, `gTRC`, `bTRC`,
/// `cprt`, `mntr`. Omitted: everything a colour-managed application might use
/// but a PDF/A checker does not require -- `chad` (the chromatic adaptation tag,
/// which is absent precisely because the primaries above are already adapted),
/// per-channel measurement tags, and the viewing-condition tags.
///
/// # Not a colour-managed profile
///
/// A caller rendering for a specific press should supply that press's profile
/// instead. This one exists because PDF/A requires *an* intent and refusing to
/// write one would make the option unavailable; it should not be mistaken for a
/// substitute for a vendor profile.
#[must_use]
pub fn srgb_icc_profile() -> Vec<u8> {
    // Tag layout. `sig` is the entry in the tag table; the tag *type* is the
    // first four bytes of the tag's data, which is where a reader looks for it.
    //
    // Omitting that signature is the bug this structure makes easy: the tag
    // table still lists every required fourcc, so every table-level check passes,
    // and a reader cannot tell an `XYZ ` tag from a `curv` one. The first version
    // here did exactly that, and only a lint noting an unused field exposed it.
    struct Tag {
        sig: &'static [u8; 4],
        kind: &'static [u8; 4],
        body: Vec<u8>,
    }

    impl Tag {
        /// Type signature followed by the tag body.
        fn data(&self) -> Vec<u8> {
            let mut out = Vec::with_capacity(4 + self.body.len());
            out.extend_from_slice(self.kind);
            out.extend_from_slice(&self.body);
            out
        }
    }

    // `desc` is a `textDescriptionType`: ASCII count, ASCII bytes (NUL-padded to
    // a 4-byte boundary), unicode language code, unicode count, unicode bytes,
    // scriptcode code, scriptcode count, scriptcode bytes.
    let name = b"sRGB IEC61966-2.1";
    let mut desc = Vec::new();
    desc.extend_from_slice(&(name.len() as u32).to_be_bytes());
    desc.extend_from_slice(name);
    while desc.len() % 4 != 0 {
        desc.push(0);
    }
    desc.extend_from_slice(&0u32.to_be_bytes()); // unicode language
    desc.extend_from_slice(&0u32.to_be_bytes()); // unicode count
    desc.extend_from_slice(&0u32.to_be_bytes()); // scriptcode code
    desc.extend_from_slice(&0u32.to_be_bytes()); // scriptcode count

    // The body of an `XYZType`: a reserved field then one `XYZNumber`. The
    // `XYZ ` type signature itself is prepended by `Tag::data`.
    let xyz = |v: [u32; 3]| {
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(&0u32.to_be_bytes());
        for c in v {
            out.extend_from_slice(&c.to_be_bytes());
        }
        out
    };

    // A `curveType` holding a single-entry gamma. ICC stores it as u8Fixed8Number,
    // so sRGB's 2.2 approximation is 0x0233 (2.19921875).
    let mut curv = Vec::new();
    curv.extend_from_slice(&0u32.to_be_bytes()); // reserved
    curv.extend_from_slice(&1u32.to_be_bytes()); // entry count
    curv.extend_from_slice(&0x0233u32.to_be_bytes()); // gamma 2.199

    // `mntr` is a `dataType`. PDF/A checks for this tag's presence specifically
    // rather than for the profile class in the header, so it has to exist as a
    // tag even though its contents are informational.
    let data = 0u32.to_be_bytes().to_vec();

    let tags = vec![
        Tag {
            sig: b"desc",
            kind: b"desc",
            body: desc,
        },
        Tag {
            sig: b"cprt",
            kind: b"text",
            body: text(b"Public Domain"),
        },
        Tag {
            sig: b"wtpt",
            kind: b"XYZ ",
            body: xyz(D50),
        },
        Tag {
            sig: b"rXYZ",
            kind: b"XYZ ",
            body: xyz(PRIMARIES[0]),
        },
        Tag {
            sig: b"gXYZ",
            kind: b"XYZ ",
            body: xyz(PRIMARIES[1]),
        },
        Tag {
            sig: b"bXYZ",
            kind: b"XYZ ",
            body: xyz(PRIMARIES[2]),
        },
        Tag {
            sig: b"rTRC",
            kind: b"curv",
            body: curv.clone(),
        },
        Tag {
            sig: b"gTRC",
            kind: b"curv",
            body: curv.clone(),
        },
        Tag {
            sig: b"bTRC",
            kind: b"curv",
            body: curv,
        },
        Tag {
            sig: b"mntr",
            kind: b"data",
            body: data,
        },
    ];

    // Layout: 128-byte header, 4-byte tag count, then 12 bytes per tag
    // (signature, offset, size), then the tag data, 4-byte aligned.
    let table_len = 4 + tags.len() * 12;
    let mut data_start = 128 + table_len;
    let mut entries = Vec::with_capacity(tags.len());
    let mut body = Vec::new();
    for tag in &tags {
        let bytes = tag.data();
        entries.push((tag.sig, data_start as u32, bytes.len() as u32));
        body.extend_from_slice(&bytes);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        data_start = 128 + table_len + body.len();
    }

    let total = 128 + table_len + body.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(total as u32).to_be_bytes()); // profile size
                                                          // The header field order below is fixed by ICC.1 and the widths are not
                                                          // all four bytes. `preferred CMM` is a *signature*, so it occupies
                                                          // four bytes -- writing it as a single zero shifted every field from
                                                          // offset 4 onward three bytes early, which put the data colour space
                                                          // at 13 instead of 16. veraPDF then read the PCS field where the colour
                                                          // space should be and reported an RGB profile as `XYZ`, failing
                                                          // clauses 6.2.3.2/2, 6.2.2/1 and 6.2.3.3/1 at once.
                                                          //
                                                          // The `debug_assert_eq!` on the total length still passed throughout,
                                                          // because the total was derived from the same wrong writes. A length
                                                          // check cannot catch a misaligned field layout.
    out.extend_from_slice(&[0; 4]); // preferred CMM: none
    out.extend_from_slice(&ICC_VERSION);
    out.extend_from_slice(b"mntr"); // device class: display
    out.extend_from_slice(b"RGB "); // data colour space
    out.extend_from_slice(b"XYZ "); // PCS
                                    // Date/time is six u16 fields: year, month, day, hours, minutes, seconds.
                                    // Written as zero rather than a wall clock because this crate promises
                                    // byte-identical output for the same input, and a timestamp would break that
                                    // on every render.
    out.extend_from_slice(&[0; 12]); // date/time
    out.extend_from_slice(b"acsp"); // file signature
    out.extend_from_slice(&0u32.to_be_bytes()); // primary platform: none
    out.extend_from_slice(&0u32.to_be_bytes()); // profile flags
    out.extend_from_slice(&0u32.to_be_bytes()); // device manufacturer
    out.extend_from_slice(&0u32.to_be_bytes()); // device model
    out.extend_from_slice(&0u64.to_be_bytes()); // device attributes
    out.extend_from_slice(&0u32.to_be_bytes()); // rendering intent: perceptual
    out.extend_from_slice(&xyz_pc_values()); // PCS illuminant, D50
    out.extend_from_slice(&0u32.to_be_bytes()); // profile creator
    out.extend_from_slice(&[0; 16]); // profile ID (unset)
                                     // Bytes 100..128 are reserved. The count is not round: it is whatever is
                                     // left after the last defined field, which is why the total is asserted
                                     // rather than derived.
    out.extend_from_slice(&[0; 28]); // reserved (100..128)
    debug_assert_eq!(out.len(), 128, "ICC header must be exactly 128 bytes");

    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for (sig, offset, size) in &entries {
        out.extend_from_slice(*sig);
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&size.to_be_bytes());
    }
    out.extend_from_slice(&body);
    debug_assert_eq!(out.len(), total, "ICC length must match the header field");
    out
}

/// The header's PCS illuminant: a single `XYZNumber` for D50.
fn xyz_pc_values() -> [u8; 12] {
    let mut v = [0u8; 12];
    for (i, c) in D50.iter().enumerate() {
        v[i * 4..i * 4 + 4].copy_from_slice(&c.to_be_bytes());
    }
    v
}

/// A `textType` ICC tag: NUL-terminated ASCII.
fn text(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s);
    v.push(0);
    v
}

/// Build the XMP packet PDF/A requires.
///
/// PDF/A checks `pdfaid:part` and `pdfaid:conformance` in an RDF description, and
/// requires the packet to declare the `xmp` and `pdf` namespaces. `dc:title`
/// carries the title so it lives in the XMP as well as the document information
/// dictionary -- PDF/A requires the two to agree, and having one without the
/// other is a common rejection.
///
/// [`build_xmp`] takes the title rather than hard-coding an empty one because a
/// PDF/A file whose `dc:title` disagrees with `/Title` fails validation, and the
/// only way to guarantee agreement is to write the same value to both.
#[must_use]
pub fn build_xmp(title: Option<&str>, author: Option<&str>, producer: &str) -> String {
    // PDF/A-1 clause 6.7.3 requires the document information dictionary and the
    // XMP packet to carry equivalent values for `/Title`, `/Author` and
    // `/Creator`, and veraPDF compares them rule by rule. Writing a title to one
    // and not the other was not cosmetic: it failed validation.
    //
    // The mapping is not mechanical. `/Creator` is the *tool*, which XMP calls
    // `xmp:CreatorTool`; `/Author` is a person or organisation, which XMP calls
    // `dc:creator`. Getting that pairing wrong satisfies neither rule.
    // The RDF *types* are not interchangeable, and veraPDF checks them
    // separately from the values (clause 6.7.9). `dc:title` is an
    // `rdf:Alt`; `dc:creator` and `xmp:CreatorTool` are `rdf:Seq` of an Agent.
    // Writing them all as alts failed two rules while looking correct in every
    // XML viewer, which is precisely the kind of error a conformance checker
    // exists to catch.
    //
    // Absent values are omitted rather than emitted empty: an empty `rdf:Alt` is
    // a *present* value with an empty string, which does not match an absent
    // `/Title` in the Info dictionary. That is clause 6.7.3.
    let title_xml = alt_list("dc:title", title.filter(|t| !t.is_empty()));
    let author_xml = agent_seq("dc:creator", author.filter(|a| !a.is_empty()));
    let creator_xml = seq_list("xmp:CreatorTool", Some("Hikari"));
    format!(
        concat!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>"#,
            "\n",
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
            "\n",
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
            "\n",
            r#"<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">"#,
            "\n",
            r#"<pdfaid:part>1</pdfaid:part>"#,
            "\n",
            r#"<pdfaid:conformance>B</pdfaid:conformance>"#,
            "\n",
            r#"</rdf:Description>"#,
            "\n",
            r#"<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/">"#,
            "\n",
            "{title}",
            "\n",
            "{author}",
            "\n",
            "{creator}",
            "\n",
            r#"</rdf:Description>"#,
            "\n",
            r#"<rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/">"#,
            "\n",
            r#"<pdf:Producer>{producer}</pdf:Producer>"#,
            "\n",
            r#"</rdf:Description>"#,
            "\n",
            r#"</rdf:RDF>"#,
            "\n",
            r#"</x:xmpmeta>"#,
            "\n",
            r#"<?xpacket end="w"?>"#,
            "\n"
        ),
        title = title_xml,
        author = author_xml,
        creator = creator_xml,
        producer = xml_escape(producer),
    )
}

/// An `rdf:Alt` language alternative. The right type for `dc:title`.
///
/// Omitted entirely when there is no value, because an empty element is a
/// present-but-empty value and does not match an absent `/Title`.
fn alt_list(tag: &str, value: Option<&str>) -> String {
    match value {
        Some(v) => format!(
            "<{tag}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{tag}>",
            xml_escape(v)
        ),
        None => String::new(),
    }
}

/// An `rdf:Seq` of one value.
///
/// Flat, not the three-level `rdf:parseType="Resource"` nesting that the XMP
/// specification's `Agent` type formally describes. The nested form is the one
/// that reads as correct, and it is also the one that fails: veraPDF's XMP
/// parser rejects the whole packet, after which `pdfaid:part` is no longer
/// found, `pdf:Producer` no longer matches, and every document-metadata rule
/// fails at once -- 8 failures with the flat form, 12 with the nested one.
/// Clause 6.7.9/3 remains unsatisfied either way and is reported as such
/// rather than papered over.
///
/// Omitted entirely when there is no value, for the same reason as `alt_list`:
/// an empty element is a present-but-empty value and does not match an absent
/// `/Title` or `/Author`.
fn seq_list(tag: &str, value: Option<&str>) -> String {
    match value {
        Some(v) => format!("<{tag}>{}</{tag}>", xml_escape(v)),
        None => String::new(),
    }
}

/// `dc:creator` as an XMP `Agent` list: an `rdf:Seq` whose item is a resource
/// carrying the name as an `rdf:Alt`.
///
/// Chosen by measurement, not by taste. Four shapes were tried against veraPDF
/// and each failed differently, which is why the shape is spelled out rather
/// than left to look obvious:
///
/// * flat `<rdf:Seq><rdf:li>Name</rdf:li></rdf:Seq>` -- read as `agentname`,
///   expected `agent` (6.7.9/3);
/// * `<rdf:li rdf:parseType="Resource"><rdf:Seq>...` -- the XMP-spec shape;
///   the whole packet stops parsing and every metadata rule fails at once;
/// * a bare `<dc:creator>Name</dc:creator>` -- read as `propername`;
/// * `rdf:Seq` nested inside `rdf:Description` -- again the whole packet stops
///   parsing (9 failures).
///
/// This one leaves `dc:creator` still failing clauses 6.7.9/3 and 6.7.3/3, and
/// that is recorded rather than hidden: veraPDF's XMP parser rejects every
/// fully-spec-conformant `Agent` encoding tried here. The document is *closer*
/// to conformant than it was, not conformant.
///
/// Omitted entirely when there is no value, so an absent `/Author` is matched by
/// an absent property rather than an empty one.
fn agent_seq(tag: &str, value: Option<&str>) -> String {
    let Some(v) = value else {
        return String::new();
    };
    let name = xml_escape(v);
    format!(
        "<{tag}><rdf:Seq><rdf:li><rdf:Description>\
         <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{name}</rdf:li></rdf:Alt></dc:title>\
         </rdf:Description></rdf:li></rdf:Seq></{tag}>",
    )
}

/// Escape the five XML predefined entities.
///
/// XMP is XML, and a title containing `&` or `<` would otherwise produce a
/// malformed packet -- which a PDF/A checker rejects before it looks at anything
/// else. The document information dictionary uses PDF string escaping, so the
/// two disagree, and both are applied to the same value.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// A deterministic document identifier for the trailer's `/ID` array.
///
/// PDF/A requires `/ID`, and a fixed identifier across every document from one
/// generator is a defect: two different files would share an identity, which is
/// exactly what the field exists to prevent.
///
/// So it is derived from the document's *content* rather than a timestamp. A
/// clock would have to be threaded in, and a caller generating two documents in
/// the same millisecond would get a collision anyway. Content-derived means the
/// same document always gets the same ID -- which is what the determinism
/// guarantee elsewhere in this crate already promises -- and two different
/// documents never collide.
///
/// FNV-1a, twice with different offsets so the two halves are not equal. This is
/// not a cryptographic hash and is not used as one; the trailer `/ID` is an
/// identity, not a security token.
#[must_use]
pub fn document_id(bytes: &[u8]) -> [[u8; 16]; 2] {
    // Two independent accumulators, each producing eight bytes, and each run
    // twice with different offsets so that a 16-byte half comes from four words
    // rather than a `u64` written into a 16-byte slot.
    fn fold(bytes: &[u8], mut seed: u64, prime: u64) -> [u8; 8] {
        for &byte in bytes {
            seed ^= u64::from(byte);
            seed = seed.wrapping_mul(prime);
            seed ^= seed >> 31;
        }
        seed.to_be_bytes()
    }

    let mut out = [[0u8; 16]; 2];
    for (index, half) in out.iter_mut().enumerate() {
        let offset = (index as u64) * 0x9e37_79b9_7f4a_7c15;
        let a = fold(bytes, 0xcbf2_9ce4_8422_2325 ^ offset, 0x100_0000_01b3);
        let b = fold(bytes, 0x9e37_79b9_7f4a_7c15 ^ offset, 0x0880_3552_34a5_d4b3);
        half[..8].copy_from_slice(&a);
        half[8..].copy_from_slice(&b);
    }
    out
}

/// Deflate, used for the XMP stream and the ICC profile.
///
/// A local copy rather than the crate's private `deflate` so this module can be
/// read on its own; both go through the same `flate2` backend.
/// Decompress bytes produced by [].
///
/// Exposed so a caller -- or a test -- can read back what was written without
/// adding  as a direct dependency of their crate.
#[must_use]
pub fn zlib_reader(bytes: &[u8]) -> flate2::read::ZlibDecoder<&[u8]> {
    flate2::read::ZlibDecoder::new(bytes)
}

/// Compress `bytes` with zlib, as PDF stream filters expect.
///
/// Used for the XMP metadata stream, which is large enough relative to the rest
/// of the file to be worth compressing.
#[must_use]
pub fn deflate(bytes: &[u8]) -> Vec<u8> {
    use flate2::{write::ZlibEncoder, Compression};
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to an in-memory buffer cannot fail.
    enc.write_all(bytes).expect("writing to a Vec cannot fail");
    enc.finish().expect("finishing a Vec encoder cannot fail")
}
