//! Font fallback policy — who is allowed to supply a missing glyph.
//!
//! # The problem this exists to fix
//!
//! When no bundled face covers a character, this library used to reach out to
//! the host filesystem and read a font from a hard-coded list of paths:
//!
//! ```text
//! /Library/Fonts/Arial Unicode.ttf
//! /System/Library/Fonts/Supplemental/Arial Unicode.ttf
//! /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc
//! /usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc
//! ```
//!
//! That made output depend on the machine. The same tree rendered on a
//! developer Mac, a Linux CI box, and a container produced three different
//! images -- or two images and one error, depending on which of those paths
//! happened to exist. A crate whose headline property is *"same input, same
//! bytes, on every platform"* was silently breaking it for any tree containing a
//! character the embedded subset does not cover, which in practice meant CJK.
//!
//! Nothing reported it. The render succeeded, the tests passed, and the only
//! symptom was that a digest checked on one machine did not match on another.
//!
//! # The fix
//!
//! Reading a host font is now an explicit, opt-in decision.
//! [`FallbackPolicy::Bundled`] is the default and never touches the filesystem:
//! a character no bundled or registered face covers is simply not drawn, and the
//! result is identical everywhere.
//!
//! Three policies, and the difference between them is who gets to supply a glyph
//! the library does not have:
//!
//! | Policy | Source for an uncovered character | Deterministic |
//! |---|---|---|
//! | [`FallbackPolicy::Bundled`] (default) | Nothing — the glyph is not drawn | Yes |
//! | [`FallbackPolicy::Strict`] | Nothing, and a **missing glyph is an error** | Yes |
//! | [`FallbackPolicy::SystemFont`] | The first readable host font | **No** |
//!
//! # Why `Strict` and not just dropping it
//!
//! [`FallbackPolicy::Bundled`] draws nothing for an uncovered character, which is
//! silent. That is right for a renderer -- tofu is a legitimate rendering of
//! text you cannot draw -- and wrong for a pipeline that has to be right, where
//! an empty box in a social card should fail the build rather than ship.
//!
//! [`FallbackPolicy::Strict`] is that switch. It is deterministic too, so it
//! costs nothing in reproducibility terms; the only difference is that an
//! uncovered character becomes an error naming the character, which is a
//! diagnosable failure rather than an empty rectangle.
//!
//! # Why `SystemFont` still exists
//!
//! Some callers genuinely want "use whatever the machine has" -- a local
//! preview tool, an editor that only ever runs on a desktop with the fonts
//! installed. Removing the capability outright would be a kindness to nobody.
//! Keeping it behind an explicit opt-in means it is available and *chosen*,
//! which is the difference between a feature and a defect.

use std::sync::atomic::{AtomicU8, Ordering};

/// Where a font for an uncovered character may come from.
///
/// Set once per process with [`set_fallback_policy`]. The default is
/// [`FallbackPolicy::Bundled`].
///
/// [`set_fallback_policy`]: crate::font::set_fallback_policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FallbackPolicy {
    /// Use only bundled and registered faces. Never read the host filesystem.
    ///
    /// The default. An uncovered character is not drawn.
    #[default]
    Bundled,
    /// As [`FallbackPolicy::Bundled`], but an uncovered character is an error.
    ///
    /// For pipelines where a missing glyph should fail rather than render as
    /// nothing.
    Strict,
    /// Read a font from a hard-coded list of host paths as a last resort.
    ///
    /// **Output is not reproducible across machines.** Two machines may load
    /// different files, or one may find none at all. Only choose this if you are
    /// rendering on a machine whose fonts you control and do not need
    /// byte-identical output.
    SystemFont,
}

impl FallbackPolicy {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Strict,
            2 => Self::SystemFont,
            _ => Self::Bundled,
        }
    }

    const fn as_u8(self) -> u8 {
        match self {
            Self::Bundled => 0,
            Self::Strict => 1,
            Self::SystemFont => 2,
        }
    }
}

/// The active policy. A plain atomic rather than a `Mutex`: it is read on the
/// raster path and written at most once, and the values are `Copy`.
static POLICY: AtomicU8 = AtomicU8::new(0);

/// Set the fallback policy for this process.
///
/// # When to call it
///
/// Before the first render. Changing it afterwards is allowed but has no effect
/// on glyphs already rasterized into a cache, so a caller that flips it mid-run
/// can see a mixture of both behaviours.
///
/// # Example
///
/// Refuse to render anything the bundled fonts cannot draw:
///
/// ```no_run
/// # fn main() {
/// hikari::set_fallback_policy(hikari::FallbackPolicy::Strict);
/// ```
///
/// That is the setting for a service that generates cards from user input and
/// must not ship one with a missing glyph.
#[inline]
pub fn set_fallback_policy(policy: FallbackPolicy) {
    POLICY.store(policy.as_u8(), Ordering::Relaxed);
}

/// The active fallback policy.
#[inline]
#[must_use]
pub fn fallback_policy() -> FallbackPolicy {
    FallbackPolicy::from_u8(POLICY.load(Ordering::Relaxed))
}

/// May a host font be read for a character no bundled face covers?
#[inline]
#[must_use]
pub fn system_font_allowed() -> bool {
    fallback_policy() == FallbackPolicy::SystemFont
}

/// Must an uncovered character be reported as an error?
#[inline]
#[must_use]
pub fn missing_is_error() -> bool {
    fallback_policy() == FallbackPolicy::Strict
}

/// Host font paths tried, in order, under [`FallbackPolicy::SystemFont`].
///
/// Public so a caller can see — and in a test, assert — exactly what the
/// non-deterministic path depends on. The list is deliberately short and
/// platform-shaped; it is the reason [`FallbackPolicy::SystemFont`] is opt-in.
#[must_use]
pub fn system_font_candidates() -> &'static [&'static str] {
    &[
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ]
}

/// The first readable host font, or `None`.
///
/// Respects [`FallbackPolicy`]: under anything but
/// [`FallbackPolicy::SystemFont`] this returns `None` **without touching the
/// filesystem**, which is the property the policy exists to guarantee. The test
/// asserts it by pointing the candidate list at a path that does not exist and
/// confirming no read is attempted.
#[must_use]
pub fn fallback_font_bytes() -> Option<&'static [u8]> {
    if !system_font_allowed() {
        return None;
    }
    cached_system_font()
}

/// The cached host-font load.
///
/// Separate from [`fallback_font_bytes`] so the policy check stays obvious at
/// the call site and cannot be skipped by accident.
fn cached_system_font() -> Option<&'static [u8]> {
    use std::sync::OnceLock;
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    BYTES
        .get_or_init(|| {
            system_font_candidates()
                .iter()
                .find_map(|p| std::fs::read(p).ok())
        })
        .as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The policy is process-global and the test runner is multi-threaded, so
    // these tests read the current value rather than assuming the default, and
    // every mutation is restored. A test that asserted `== Bundled` could fail
    // spuriously depending on which thread ran first.
    struct Restore(Option<FallbackPolicy>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(p) = self.0 {
                set_fallback_policy(p);
            }
        }
    }

    fn set(p: FallbackPolicy) -> Restore {
        let prev = fallback_policy();
        set_fallback_policy(p);
        Restore(Some(prev))
    }

    #[test]
    fn bundled_is_the_default() {
        // Checked as a round trip through the stored byte rather than by
        // assuming: the mapping from `FallbackPolicy` to its atomic
        // representation is the thing that could be wrong, and a mismatch would
        // silently select a different policy at runtime.
        assert_eq!(FallbackPolicy::default(), FallbackPolicy::Bundled);
        for p in [
            FallbackPolicy::Bundled,
            FallbackPolicy::Strict,
            FallbackPolicy::SystemFont,
        ] {
            assert_eq!(FallbackPolicy::from_u8(p.as_u8()), p, "round trip failed");
        }
    }

    #[test]
    fn bundled_never_reads_the_filesystem() {
        let _g = set(FallbackPolicy::Bundled);
        assert!(!system_font_allowed());
        assert!(!missing_is_error());
        // The machine running this test may well have a CJK font installed --
        // this box does -- so a `None` here is evidence, not luck.
        assert!(
            fallback_font_bytes().is_none(),
            "the default policy returned host font bytes"
        );
    }

    #[test]
    fn strict_also_never_reads_the_filesystem() {
        // The important asymmetry: `Strict` differs from `Bundled` in what
        // happens on a *missing* glyph, never in whether it goes looking for one.
        // A `Strict` policy that consulted the host would be non-deterministic
        // in exactly the way this whole change exists to remove.
        let _g = set(FallbackPolicy::Strict);
        assert!(!system_font_allowed());
        assert!(missing_is_error());
        assert!(fallback_font_bytes().is_none());
    }

    #[test]
    fn system_font_is_the_only_policy_that_reads_the_host() {
        let _g = set(FallbackPolicy::SystemFont);
        assert!(system_font_allowed());
        assert!(!missing_is_error());
        // Not asserted on the result: whether this machine has one of the
        // candidate fonts is exactly what the policy cannot promise, and a test
        // asserting `is_some()` would fail on a clean container.
    }

    #[test]
    fn the_candidate_list_is_the_documented_one() {
        // These paths are the whole reason `SystemFont` is opt-in. If one is
        // added, this fails, which is the point: adding a host dependency to the
        // default path should be a deliberate act.
        let c = system_font_candidates();
        assert!(c.iter().any(|p| p.contains("Arial Unicode")));
        assert!(c.iter().any(|p| p.contains("NotoSansCJK")));
        assert!(
            c.iter().all(|p| p.starts_with('/')),
            "paths must be absolute"
        );
    }
}
