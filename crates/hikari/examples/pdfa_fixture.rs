//! Emit a PDF/A-1b document for veraPDF to check.
//!
//! Written as an example rather than a test because veraPDF runs in CI as a
//! separate job against a real file, and the point of that job is to validate
//! bytes produced by the same path a user would take -- not a hand-built PDF
//! that happens to pass.
//!
//! The tree deliberately exercises the parts of the PDF writer that a
//! conformance checker looks at, so a regression in any of them is caught rather
//! than papered over:
//!
//!   * embedded fonts, which must be subset-embedded with their layout tables,
//!     a `CIDToGIDMap` and a `CIDSet`;
//!   * a title and an author, which must agree between the document information
//!     dictionary and the XMP packet.
//!
//! **No attachment.** The first version of this fixture embedded an invoice XML
//! to exercise that path, and veraPDF failed it on PDF/A-1 clauses 6.1.11/1 and
//! 6.1.11/2: a file specification shall not contain `/EF`, and the name
//! dictionary shall not contain `EmbeddedFiles`. PDF/A-1 forbids embedded files
//! outright, so a document that has one can never conform -- and a conformance
//! fixture that cannot conform is testing nothing. The attachment path is
//! exercised by the `invoice` example and by the PDF backend tests instead.
//!
//! Nothing here asserts the result is conformant. That is veraPDF's job, and
//! until it has run and agreed, this crate does not claim PDF/A conformance.

use hikari::{Node, PageSize, PdfOptions, Style};

fn main() {
    let page = Node::container(
        Style::new()
            .with_size(595.0, 842.0)
            .with_background("#ffffff")
            .with_padding(48.0),
        vec![
            Node::text("Archive fixture", Style::text(28.0, "#0f172a")),
            Node::text(
                "Deterministic text, an embedded face, and a link.",
                Style::text(13.0, "#334155"),
            ),
            Node::container(
                Style::new()
                    .with_size(499.0, 60.0)
                    .with_background("#f1f5f9")
                    .with_border(1.0, "#cbd5e1")
                    .with_padding(12.0),
                vec![
                    Node::text("PDF/A-1b structural metadata", Style::text(16.0, "#0f172a")),
                    Node::text(
                        "Rows, borders and a fill on one page.",
                        Style::text(11.0, "#475569"),
                    ),
                ],
            ),
        ],
    );

    let mut options = PdfOptions {
        title: Some("Hikari PDF/A fixture".into()),
        author: Some("Hikari Labs".into()),
        ..PdfOptions::new()
    };
    options.pdfa = true;

    let bytes = hikari::render_pdf_pages_with(&[page], PageSize::A4, &options)
        .expect("the PDF backend should render this tree");

    std::fs::write("pdfa-fixture.pdf", &bytes).expect("writing the fixture");
    println!("wrote pdfa-fixture.pdf ({} bytes)", bytes.len());
}
