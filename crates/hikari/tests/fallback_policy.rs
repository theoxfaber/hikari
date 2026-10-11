//! The fallback policy, through the real renderer.
//!
//! The unit tests in `hikari-core` pin the policy's plumbing. These pin the
//! property the change exists to restore: **the same tree produces the same
//! bytes on a machine with a CJK font installed and on one without.**
//!
//! # Why this is one test and not several
//!
//! [`FallbackPolicy`] is process-global state, read deep in the render path
//! without a lock. Cargo runs tests in a file in parallel threads, so separate
//! `#[test]` functions mutating it race: the first version of this file had
//! eight, and `strict_fails_loudly_on_a_character_it_cannot_draw` failed because
//! another test had set `SystemFont` in the window between its `set` and its
//! render. The failure was a real one -- the policy genuinely was not Strict at
//! the moment it mattered -- and it was also a test bug, because the assertion
//! was about a global rather than about the code.
//!
//! Consolidating into one sequential function removes the race without weakening
//! any assertion. It also reflects how the API is meant to be used: set once at
//! startup, not toggled per test.

use hikari::{FallbackPolicy, Node, Style};

fn card(text: &str) -> Node {
    Node::container(
        Style::new()
            .with_size(320.0, 160.0)
            .with_background("#0f172a")
            .with_padding(16.0),
        vec![Node::text(text, Style::text(36.0, "#f8fafc"))],
    )
}

/// A string no bundled face covers.
const CJK: &str = "\u{6f22}\u{5b57}\u{30c6}\u{30b9}\u{30c8}";

fn render(node: &Node) -> Result<Vec<u8>, hikari::Error> {
    hikari::render_png(node, 320, 160)
}

#[test]
fn fallback_policy_behaviour() {
    // The policy the process starts with, restored at the end. A test that
    // mutated it and failed would otherwise leak the change into every other
    // test in this binary.
    let original = hikari::fallback_policy();

    // --- the default reads no host font ------------------------------------
    assert_eq!(
        original,
        FallbackPolicy::Bundled,
        "the default policy changed; callers may depend on the deterministic one"
    );
    // This machine does have a CJK font installed, so a `Some` here is evidence
    // rather than an environment accident.
    assert!(
        hikari::policy::fallback_font_bytes().is_none(),
        "the default policy returned host font bytes"
    );

    // --- Bundled: renders, reproducible, glyph simply not drawn -------------
    hikari::set_fallback_policy(FallbackPolicy::Bundled);
    let bundled_cjk = render(&card(CJK)).expect("Bundled must render");
    assert_eq!(
        hikari::hash_bytes(&bundled_cjk),
        hikari::hash_bytes(&render(&card(CJK)).expect("renders")),
        "Bundled output is not stable across calls"
    );
    // The card's background still paints. An uncovered glyph must not take the
    // rest of the node down with it.
    let img = image::load_from_memory(&bundled_cjk)
        .expect("decode")
        .to_rgba8();
    let bg = img.get_pixel(2, 2);
    assert_eq!(
        [bg[0], bg[1], bg[2]],
        [0x0f, 0x17, 0x2a],
        "the card background should still be painted"
    );
    // And *something* is drawn for the glyph. "Absent" here means drawn as
    // `.notdef` -- the empty box DejaVu substitutes -- not blank: the first
    // version of this test asserted near-zero bright pixels and failed at 480,
    // because it had forgotten that a missing glyph still gets a placeholder.
    //
    // A stronger check was tried and abandoned: rendering private-use
    // codepoints, which no font anywhere covers, and asserting the CJK card is
    // byte-identical to it. The two differ, because a private-use codepoint
    // and a CJK ideograph do not take the same path through shaping -- one is
    // dropped before an advance is produced, the other produces an advance
    // pointing at `.notdef`. Asserting on that confuses "no host font was
    // consulted" with "both inputs happen to lay out identically", and the
    // first claim is what matters.
    //
    // `fallback_font_bytes() == None` above is the direct proof, because that
    // function returns without touching the filesystem unless the policy is
    // `SystemFont`. Everything here is downstream of it.
    let ink = img.pixels().filter(|p| p[0] > 200).count();
    assert!(
        ink > 100,
        "expected .notdef placeholders for the CJK glyphs, got {ink} bright px"
    );

    // --- Strict: the same tree fails, loudly --------------------------------
    hikari::set_fallback_policy(FallbackPolicy::Strict);
    let err = match render(&card(CJK)) {
        Err(e) => e,
        Ok(_) => panic!(
            "Strict must reject a tree it cannot fully draw, but the render \
             succeeded. Either the policy was not in force, or a bundled face \
             now covers CJK -- in which case this test and the claim about \\
             bundled-cjk being opt-in are both out of date."
        ),
    };
    let msg = err.to_string();
    // The error must name the character and the escape hatch. An error that
    // only says "font error" leaves the caller with nothing to act on, and this
    // is the failure the policy exists to surface.
    assert!(
        msg.contains("Strict"),
        "message should mention Strict: {msg}"
    );
    assert!(
        msg.contains("bundled-cjk"),
        "message should point at the fix: {msg}"
    );
    assert!(
        hikari::policy::fallback_font_bytes().is_none(),
        "Strict consulted the host filesystem; it differs from Bundled only in \
         how a missing glyph is reported, never in where it looks"
    );

    // --- Strict does not fire on text every bundled face covers ------------
    for s in [
        "Release 1.2",
        "Hello, world",
        "naïve café — 100%",
        // Hebrew is bundled by default, so it must be unaffected by any of this.
        "\u{05e9}\u{05dc}\u{05d5}\u{05dd}",
    ] {
        if let Err(e) = render(&card(s)) {
            panic!("Strict should render {s:?} without a fallback: {e}");
        }
    }

    // --- Both deterministic policies agree on a tree needing no fallback -----
    let latin = card("Release 1.2");
    let under_strict = hikari::hash_bytes(&render(&latin).expect("renders"));
    hikari::set_fallback_policy(FallbackPolicy::Bundled);
    let under_bundled = hikari::hash_bytes(&render(&latin).expect("renders"));
    assert_eq!(
        under_bundled, under_strict,
        "the two deterministic policies disagree on a tree that needs no fallback"
    );

    // --- SystemFont is the only non-deterministic one -----------------------
    hikari::set_fallback_policy(FallbackPolicy::SystemFont);
    let under_system = render(&card(CJK)).expect("SystemFont must still render");
    assert!(!under_system.is_empty());
    let sys_digest = hikari::hash_bytes(&under_system);
    // Whether this diverges from `Bundled` depends on this machine's fonts, so
    // it is recorded rather than asserted. The point being made is that the
    // divergence is *possible* and only possible here.
    println!("SystemFont {sys_digest} vs Bundled {under_bundled}");

    // --- restore ------------------------------------------------------------
    hikari::set_fallback_policy(original);
    assert_eq!(hikari::fallback_policy(), original);
}
