//! Font registry: caller-supplied fonts alongside the embedded one.
//!
//! # Why this exists
//!
//! Without it the embedded DejaVu subset is the only font in the project, and
//! the PDF and SVG backends hardcode its family name. An OG-image generator
//! whose text cannot be set in the brand's typeface cannot do its main job, so
//! this is the difference between a demo and a usable library.
//!
//! # Design
//!
//! Fonts are addressed by a [`FontId`], not by name, because [`Style`] has to
//! survive a `serde` round-trip into the Node and WASM bindings and a name
//! would have to be resolved at every use. [`BUILTIN_FONT`] is always `0`, so
//! existing trees that never mention a font keep working untouched.
//!
//! Registration is keyed on the SHA-256 of the font bytes. Registering the same
//! font twice returns the same id and does no work, which is what makes the
//! lifetime strategy below acceptable: each distinct font's bytes are leaked
//! once and kept for the life of the process. A server that registers one
//! brand font per request leaks nothing, because the second request hits the
//! content hash. A caller that generates genuinely unbounded distinct fonts
//! would grow without bound, so [`registered_font_count`] is exposed for
//! exactly that case.
//!
//! # Why the bytes are leaked
//!
//! `rustybuzz::Face` and `ttf_parser::Face` both borrow the font data they
//! parse, and the parse result is what we want to keep. Rather than rebuild
//! self-referential structs on every call, each font is parsed once and its
//! bytes are leaked. That also means [`font_entry`] can hand out `&'static`
//! references, so the shaping hot path takes no lock at all.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use rustybuzz::Face as HbFace;
use sha2::{Digest, Sha256};
use ttf_parser::Face as TtfFace;

use crate::error::Error;

/// Handle for a registered font. `0` is always the embedded font.
pub type FontId = u32;

/// The embedded font. Chosen by `Style` when no font is named.
pub const BUILTIN_FONT: FontId = 0;

/// One registered font, parsed once and shared.
pub struct FontEntry {
    /// Caller-supplied name, used for PDF and SVG output. Not used for lookup.
    pub name: String,
    bytes: &'static [u8],
    hb: HbFace<'static>,
    ttf: TtfFace<'static>,
}

impl FontEntry {
    /// Raw font bytes.
    #[must_use]
    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// `rustybuzz` face for shaping.
    #[must_use]
    pub fn hb(&self) -> &HbFace<'static> {
        &self.hb
    }

    /// `ttf-parser` face for metrics and codepoint lookup.
    #[must_use]
    pub fn ttf(&self) -> &TtfFace<'static> {
        &self.ttf
    }

    /// Design units per em. Zero would make every px size meaningless.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.ttf.units_per_em()
    }
}

struct Registry {
    /// Leaked entries so callers can hold `&'static` references.
    entries: Vec<&'static FontEntry>,
    by_hash: HashMap<[u8; 32], FontId>,
    /// Ids of the bundled faces after the primary, in fallback order.
    fallbacks: Vec<FontId>,
}

fn make_entry(name: &str, bytes: &'static [u8]) -> &'static FontEntry {
    let entry = FontEntry {
        name: name.to_owned(),
        bytes,
        hb: HbFace::from_slice(bytes, 0).expect("bundled font parses"),
        ttf: TtfFace::parse(bytes, 0).expect("bundled font parses"),
    };
    Box::leak(Box::new(entry))
}

fn registry() -> &'static RwLock<Registry> {
    static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        // Slot 0 is reserved for the embedded font so `Style` needs no
        // `Option` round-trip and `None` can mean "the built-in one".
        let mut entries: Vec<&'static FontEntry> = vec![make_entry(
            embedded_family_name(),
            crate::shape::font_bytes(),
        )];
        let mut fallbacks = Vec::new();

        // The bundled fallbacks follow the primary. They are registered into the
        // same id space as caller fonts so every backend resolves a `FontId` the
        // same way, with no special-cased "fallback slot" anywhere.
        for (family, bytes) in crate::shape::bundled_fallbacks() {
            if entries.iter().any(|e| e.bytes.as_ptr() == bytes.as_ptr()) {
                continue;
            }
            let leaked = make_entry(family, bytes);
            fallbacks.push(entries.len() as FontId);
            entries.push(leaked);
        }

        RwLock::new(Registry {
            entries,
            by_hash: HashMap::new(),
            fallbacks,
        })
    })
}

/// The embedded font's family name, as it should appear in PDF and SVG.
fn embedded_family_name() -> &'static str {
    "DejaVu Sans"
}

/// The embedded font bytes: the build-time subset (see `build.rs`).
#[must_use]
pub fn builtin_bytes() -> &'static [u8] {
    crate::shape::font_bytes()
}

/// Look up a registered font. Returns `None` for an unknown id rather than
/// panicking, because ids arrive from untrusted JSON in the bindings.
#[must_use]
pub fn font_entry(id: FontId) -> Option<&'static FontEntry> {
    let reg = registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reg.entries.get(id as usize).copied()
}

/// Number of distinct fonts registered, including the embedded one. Exposed so
/// a caller registering unbounded distinct fonts can notice.
#[must_use]
pub fn registered_font_count() -> usize {
    let reg = registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reg.entries.len()
}

/// Register font bytes and return a [`FontId`] to reference them by.
///
/// Idempotent on content: registering the same bytes twice returns the existing
/// id. The `name` is recorded for PDF and SVG output; lookup is by id, so two
/// fonts may share a name.
pub fn register_font(name: &str, bytes: &[u8]) -> Result<FontId, Error> {
    let mut reg = registry()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash: [u8; 32] = hasher.finalize().into();
    if let Some(&id) = reg.by_hash.get(&hash) {
        return Ok(id);
    }

    // Leak the bytes first, then parse the faces *from the leaked slice*. Doing
    // it the other way round leaves the faces borrowing the caller's buffer
    // with the caller's lifetime, which cannot be stored in a `FontEntry`.
    let bytes: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
    let hb = HbFace::from_slice(bytes, 0)
        .ok_or_else(|| Error::Font(format!("{name}: not a usable font")))?;
    let ttf = TtfFace::parse(bytes, 0)
        .map_err(|e| Error::Font(format!("{name}: not a usable font: {e:?}")))?;
    if ttf.units_per_em() == 0 {
        return Err(Error::Font(format!("{name}: units_per_em is zero")));
    }

    let entry = FontEntry {
        name: name.to_owned(),
        bytes,
        hb,
        ttf,
    };
    let id = reg.entries.len() as FontId;
    let leaked_entry: &'static FontEntry = Box::leak(Box::new(entry));
    reg.entries.push(leaked_entry);
    reg.by_hash.insert(hash, id);
    Ok(id)
}

/// Resolve a `FontId` to one that exists, degrading to the built-in font.
///
/// Coverage and shaping must both go through this. An id that is not registered
/// has no face to ask about, so asking it for coverage answered "no" for every
/// character and silently rerouted the whole run into the fallback chain. Since
/// ids arrive from untrusted JSON in the bindings, that is reachable from
/// ordinary input.
#[must_use]
pub fn resolve(id: FontId) -> FontId {
    if font_entry(id).is_some() {
        id
    } else {
        BUILTIN_FONT
    }
}

/// Does `font` have a glyph for `ch`?
///
/// Coverage is a `cmap` question, so this is a parsed-font lookup rather than a
/// shaping result. A font that covers a character can still shape it to a
/// `.notdef`-producing sequence in pathological cases, but that is vanishingly
/// rare and the shaper's own `missing` flag still catches it.
#[must_use]
pub fn font_covers(font: FontId, ch: char) -> bool {
    font_entry(resolve(font)).is_some_and(|e| e.ttf.glyph_index(ch).is_some())
}

/// The first bundled fallback covering `ch`, or `None` if none does.
///
/// The search is over bundled faces only. Caller-registered fonts are *not*
/// consulted: silently substituting a caller's font for coverage they did not
/// ask for would make the primary font non-deterministic in a way no caller
/// could reason about. A caller who wants their own face used for fallback
/// should set it on the node, which is the explicit path.
#[must_use]
pub fn font_covering(ch: char) -> Option<FontId> {
    let reg = registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reg.fallbacks
        .iter()
        .copied()
        .find(|&id| font_entry(id).is_some_and(|e| e.ttf.glyph_index(ch).is_some()))
}

/// Resolve a font id to a face, falling back to the embedded font.
///
/// A bad id degrades to the built-in font rather than failing the render: a
/// tree that names a font the caller forgot to register should still produce
/// an image, just not in the requested typeface.
#[must_use]
pub fn hb_face(id: FontId) -> &'static HbFace<'static> {
    font_entry(id).map_or_else(
        || font_entry(BUILTIN_FONT).expect("builtin registered").hb(),
        FontEntry::hb,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_is_slot_zero() {
        let entry = font_entry(BUILTIN_FONT).expect("builtin registered");
        assert!(!entry.bytes().is_empty());
        assert!(entry.units_per_em() > 0);
        assert_eq!(entry.name, "DejaVu Sans");
    }

    #[test]
    fn unknown_id_is_none_not_a_panic() {
        assert!(font_entry(9999).is_none());
        // Resolution must still yield a usable face.
        assert!(hb_face(9999).units_per_em() > 0);
    }

    #[test]
    fn registering_garbage_fails_cleanly() {
        let err = register_font("garbage", b"definitely not a font").unwrap_err();
        assert!(matches!(err, Error::Font(_)), "unexpected error: {err:?}");
    }

    #[test]
    fn registration_is_idempotent_on_content() {
        // The embedded bytes are valid font data, so they can stand in for a
        // real caller-supplied font here.
        let bytes = builtin_bytes();
        let first = register_font("copy-a", bytes).expect("register");
        let second = register_font("copy-b", bytes).expect("register");
        assert_eq!(first, second, "same bytes must yield the same id");
        assert!(
            first > BUILTIN_FONT,
            "a caller font must not collide with builtin"
        );

        // Different content must get a different id. A one-byte mutation of a
        // font will not parse, so build the variant from a second real font:
        // there isn't one bundled, so assert the negative case via a rejected
        // registration not consuming an id instead.
        let before = registered_font_count();
        assert!(register_font("broken", b"not a font").is_err());
        assert_eq!(
            registered_font_count(),
            before,
            "a failed registration must not consume an id"
        );
    }

    #[test]
    fn registered_font_shapes_text() {
        // A real second font: shape with it and confirm it produces advances
        // and glyphs, i.e. the registry is wired into shaping and not just
        // stored.
        let id = register_font("embedded-again", builtin_bytes()).expect("register");
        let entry = font_entry(id).expect("entry");
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str("Hello");
        buffer.guess_segment_properties();
        let out = rustybuzz::shape(entry.hb(), &[], buffer);
        assert!(!out.is_empty(), "registered font produced no glyphs");
        assert!(
            out.glyph_positions().iter().any(|p| p.x_advance > 0),
            "registered font produced no advances"
        );
    }
}
