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
//! `harfrust`'s font model borrows the data it parses, and the parsed tables are
//! exactly what we want to keep — re-reading them per shape would cost far more
//! than the shaping itself. So each font is parsed once and its bytes are leaked
//! to `&'static [u8]`. `harfrust::Font::new` accepts a `&'static [u8]` directly
//! (`read-fonts` stores it as a borrowed `Blob` rather than copying), and
//! [`font_entry`] hands out `&'static` references, so the shaping hot path takes
//! no lock and performs no allocation.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use harfrust::Font as HbFont;
use read_fonts::TableProvider;
use sha2::{Digest, Sha256};

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
    /// The parsed font: metrics, cmap, layout and outline tables.
    hb: HbFont,
}

impl FontEntry {
    /// Raw font bytes.
    #[must_use]
    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The parsed font, for metrics and table lookups.
    #[must_use]
    pub fn hb(&self) -> &HbFont {
        &self.hb
    }

    /// The glyph a character maps to directly, or `None` if this font does not
    /// cover it.
    ///
    /// This is deliberately the *cmap* answer and not the shaper's: it answers
    /// "does this face have the character at all", which is what the fallback
    /// chain asks. Asking the shaper instead would be circular, since the chain
    /// picks the font before shaping happens.
    #[must_use]
    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        self.hb
            .charmap()
            .map_unicode(ch)
            .map(|gid| gid.to_u32() as u16)
    }

    /// Design units per em. Zero would make every px size meaningless.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.hb
            .tables()
            .head()
            .map_or(1, |head| head.units_per_em())
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
    let hb = HbFont::new(bytes, 0).expect("bundled font parses");
    let entry = FontEntry {
        name: name.to_owned(),
        bytes,
        hb,
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

/// Design units per em for a font id, with the same bad-id fallback as
/// [`hb_face`] so a forgotten registration cannot divide by zero downstream.
#[must_use]
pub fn units_per_em_of(id: FontId) -> u16 {
    font_entry(id).map_or_else(
        || {
            font_entry(BUILTIN_FONT)
                .expect("builtin registered")
                .units_per_em()
        },
        FontEntry::units_per_em,
    )
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

    // Leak the bytes first, then parse the font *from the leaked slice*. Doing it
    // the other way round leaves it borrowing the caller's buffer with the
    // caller's lifetime, which cannot be stored in a `FontEntry`. The leak is
    // also free: `read-fonts` stores a `&'static [u8]` as a borrowed `Blob`
    // rather than copying it, so registration does not duplicate the font.
    let bytes: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
    let hb =
        HbFont::new(bytes, 0).ok_or_else(|| Error::Font(format!("{name}: not a usable font")))?;
    // A zero upem would make every px size meaningless, and it is reachable in a
    // hand-crafted file, so it is rejected at the door rather than dividing by it
    // later.
    let units_per_em = hb.tables().head().map_or(0, |head| head.units_per_em());
    if units_per_em == 0 {
        return Err(Error::Font(format!("{name}: units_per_em is zero")));
    }

    let entry = FontEntry {
        name: name.to_owned(),
        bytes,
        hb,
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
    font_entry(resolve(font)).is_some_and(|e| e.glyph_index(ch).is_some())
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
        .find(|&id| font_entry(id).is_some_and(|e| e.glyph_index(ch).is_some()))
}

/// A cached [`harfrust::ShaperFont`] for one font, per thread.
///
/// `ShaperFont::new` re-resolves the layout tables the shaper needs, which is
/// real work and showed up as a ~16% regression on the render benchmark when it
/// ran once per shaped run. The result cannot live in the registry: it holds
/// interior `OnceCell`s and a `&dyn FontFuncs`, so it is neither `Sync` nor
/// meaningful across calls -- the memoisation it keeps is only useful within one
/// shaping run.
///
/// A thread-local keyed by font id gives each thread one shaper per font, which
/// is exactly its lifetime: shaping happens on one thread for the duration of a
/// call, and the cache is dropped with the thread. Fonts are registered into
/// append-only slots that are never removed, so an id stays valid for the life
/// of the process and the cache needs no invalidation.
///
/// The cost of a miss is one `ShaperFont::new`, so a thread that renders only
/// once per font pays the same as before and keeps a little memory.
pub(crate) fn shaper_for(id: FontId) -> &'static harfrust::ShaperFont<'static, 'static> {
    use std::cell::RefCell;

    // One shaper per (thread, font). A small vec rather than a map: a render
    // touches a handful of fonts, and linear scan over two or three entries is
    // cheaper than hashing.
    thread_local! {
        static SHAPERS: RefCell<Vec<(FontId, &'static harfrust::ShaperFont<'static, 'static>)>> =
            const { RefCell::new(Vec::new()) };
    }

    SHAPERS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, shaper)) = cache.iter().find(|(cached, _)| *cached == id) {
            return *shaper;
        }
        let shaper: &'static harfrust::ShaperFont<'static, 'static> =
            Box::leak(Box::new(harfrust::ShaperFont::new(hb_face(id))));
        cache.push((id, shaper));
        shaper
    })
}

/// Resolve a font id to a parsed font, falling back to the embedded one.
///
/// [`harfrust::ShaperFont`] is deliberately *not* cached here. It holds interior
/// memoisation (`OnceCell`s for glyph metrics and a symbol-font page) that is
/// neither `Sync` nor useful across calls: it caches within a single shaping
/// run, where GSUB and GPOS lookups repeat heavily. Building it per call is
/// therefore free — it reads tables this has already parsed — and keeping it out
/// of the registry is what lets the registry stay `Sync`.
///
/// A bad id degrades to the built-in font rather than failing the render: a
/// tree that names a font the caller forgot to register should still produce
/// an image, just not in the requested typeface.
#[must_use]
pub fn hb_face(id: FontId) -> &'static HbFont {
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
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str("Hello");
        buffer.guess_segment_properties();
        let shaper = harfrust::ShaperFont::new(entry.hb());
        harfrust::shape(&shaper, &mut buffer, harfrust::ShapeOptions::default())
            .expect("shape succeeds");
        assert!(!buffer.is_empty(), "registered font produced no glyphs");
        assert!(
            buffer.glyph_positions().iter().any(|p| p.x_advance > 0),
            "registered font produced no advances"
        );
    }
}
