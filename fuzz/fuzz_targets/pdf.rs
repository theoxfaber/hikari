//! Fuzz the PDF writer.
//!
//! # Why this target exists
//!
//! `hikari_pdf` builds a PDF from scratch, by hand, including its own TrueType
//! subsetter and CID `ToUnicode` CMap generator. Both of those walk font tables
//! by offset and length, which is exactly the shape of code that reads
//! attacker-controlled data -- and the subsetter runs over whatever font a caller
//! registers.
//!
//! # Input shape
//!
//! The interesting half of the space is *font* bytes, so the target splits the
//! input: the first byte selects which of the two things to fuzz, and the rest is
//! the payload. Without that split the fuzzer would spend most of its time
//! producing text that never reaches the font code.
//!
//! # What counts as a failure
//!
//! Anything that is not a returned `Err` or `Ok`. Panics, hangs and unbounded
//! allocation are the targets. A rejected font is a normal outcome and must stay
//! one -- an error path that panics is the bug, not the rejection.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Selects the font surface. Anything else routes to the writer, on purpose:
/// the selector is an arbitrary fuzz byte, so discarding unrecognised values
/// would throw away most of the corpus' writer coverage for nothing.
const FONT: u8 = 0;

fuzz_target!(|data: &[u8]| {
    let Some((selector, payload)) = data.split_first() else {
        return;
    };

    match *selector {
        FONT => {
            // A real font path is worth far more than random bytes, so when the
            // environment names one, fuzz *that* font with mutated headers
            // rather than fuzzing nothing. Random bytes rarely form a valid
            // `sfnt`, and every invalid one is rejected before the subsetter's
            // table walk is reached -- which is the code most worth testing.
            if let Ok(path) = std::env::var("HIKARI_FUZZ_FONT") {
                if let Ok(real) = std::fs::read(&path) {
                    let mut bytes = real;
                    // Mutate a bounded prefix so the mutation stays in the
                    // header/table directory, where the offsets live.
                    let n = payload.len().min(bytes.len()).min(512);
                    for (i, &b) in payload[..n].iter().enumerate() {
                        bytes[i] = b;
                    }
                    let _ = hikari_pdf::subset_font_for_text(&bytes, "\u{6f22}");
                    return;
                }
            }
            let _ = hikari_pdf::subset_font_for_text(payload, "Hello");
        }
        _ => {
            // Valid UTF-8 text as a tree, exercising the writer's geometry,
            // escaping and pagination paths rather than the font code.
            let Ok(text) = std::str::from_utf8(payload) else {
                return;
            };
            let page = hikari_core::Node::container(
                hikari_core::Style::new()
                    .with_size(595.0, 842.0)
                    .with_padding(24.0)
                    .with_background("#ffffff"),
                vec![hikari_core::Node::text(
                    text,
                    hikari_core::Style::text(14.0, "#000000"),
                )],
            );
            let mut options = hikari_pdf::PdfOptions::new();
            options.title = Some(text.to_owned());
            options.pdfa = true;
            let _ = hikari_pdf::render_pdf_with(
                &[page],
                hikari_pdf::PageSize::A4,
                &options,
            );
        }
    }
});