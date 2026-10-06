//! Upstream text shaping: `rustybuzz` advances + `unicode-bidi` visual order.
//!
//! One embedded font (`assets/DejaVuSans.ttf`) is the single source of truth
//! for both measurement (here) and rasterization (`hikari-raster`), so layout
//! and paint agree. Complex-script joining is best-effort in v0.2: runs are
//! shaped per bidi run with correct direction; full itemization with per-font
//! fallback is Step 1b in `plans/hikari-beat-takumi.md`.

use std::sync::OnceLock;

use harfrust::{Buffer, Direction, ShapeOptions};

use crate::font::{FontId, BUILTIN_FONT};
use unicode_bidi::BidiInfo;

/// Embedded font bytes: the build-time subset (see `build.rs`) — the single
/// source of truth for measurement, rasterization, and PDF. Also registered as
/// [`crate::BUILTIN_FONT`], which is what the shaping path actually uses.
#[must_use]
pub fn font_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/dejavu-subset.ttf"))
}

/// Bundled fallback faces, in fallback order: `(family, subset bytes)`.
///
/// These are build-time subsets of OFL fonts, so they are present on every
/// platform and their output is deterministic. That is the point: relying on a
/// system path meant a render could differ between a developer's Mac and a
/// Linux CI box.
#[must_use]
pub fn bundled_fallbacks() -> Vec<(&'static str, &'static [u8])> {
    // Each face sits behind its own `#[cfg]` function rather than a `cfg!` test
    // inside one body: `include_bytes!` resolves its path during compilation,
    // so a disabled feature's file must not merely be unreferenced, it must not
    // be named at all.
    let mut out: Vec<(&'static str, &'static [u8])> = Vec::new();
    if let Some(face) = hebrew_face() {
        out.push(face);
    }
    if let Some(face) = cjk_face() {
        out.push(face);
    }
    out
}

#[cfg(feature = "bundled-hebrew")]
fn hebrew_face() -> Option<(&'static str, &'static [u8])> {
    Some((
        "Noto Sans Hebrew",
        include_bytes!(concat!(env!("OUT_DIR"), "/noto-hebrew-subset.ttf")),
    ))
}

#[cfg(not(feature = "bundled-hebrew"))]
fn hebrew_face() -> Option<(&'static str, &'static [u8])> {
    None
}

#[cfg(feature = "bundled-cjk")]
fn cjk_face() -> Option<(&'static str, &'static [u8])> {
    Some((
        "Noto Sans SC",
        include_bytes!(concat!(env!("OUT_DIR"), "/noto-cjk-subset.ttf")),
    ))
}

#[cfg(not(feature = "bundled-cjk"))]
fn cjk_face() -> Option<(&'static str, &'static [u8])> {
    None
}

/// System CJK-capable fallback bytes, loaded lazily, as a last resort.
///
/// Only consulted when no bundled face covers a character — a rare case, since
/// the bundled faces cover the scripts this library claims to render. It is
/// deliberately *not* deterministic: two machines may load different files, or
/// none. Anything relying on byte-identical output must bundle the face it needs
/// rather than depend on this.
pub fn fallback_font_bytes() -> Option<&'static [u8]> {
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    const CANDIDATES: &[&str] = &[
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    BYTES
        .get_or_init(|| CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()))
        .as_deref()
}

/// Line height multiplier for stacked lines.
#[must_use]
pub const fn line_height(px: f32) -> f32 {
    px * 1.25
}

/// One shaped advance in visual (paint) order.
#[derive(Debug, Clone, Copy)]
pub struct PlacedAdvance {
    /// Representative char for rasterization and ToUnicode mapping.
    pub ch: char,
    /// Primary-font glyph id (`0` = missing there).
    pub gid: u32,
    /// Horizontal advance in px.
    pub advance: f32,
    /// Horizontal glyph offset in px (marks).
    pub x_offset: f32,
    /// True when the primary font lacks this glyph (fallback needed).
    pub missing: bool,
    /// Font this advance was shaped with, so paint uses the same face.
    pub font: FontId,
}

/// Shape `text` at `px` size. Returns advances in visual order and total width.
/// Never fails: empty or unshaped input yields zero-width output.
pub fn shape_text(text: &str, px: f32, font: FontId) -> (Vec<PlacedAdvance>, f32) {
    if text.is_empty() || px <= 0.0 {
        return (Vec::new(), 0.0);
    }
    // Resolve through `hb_face` so an unregistered id falls back to the
    // embedded font here exactly as it does in the rasterizer. Reading upem
    // from the raw registry entry instead would give 0.0 and silently shape
    // nothing at all, which is a different behaviour from every other backend.
    let upem = crate::font::hb_face(font).units_per_em() as f32;
    if upem <= 0.0 {
        return (Vec::new(), 0.0);
    }
    // Every advance is tagged with the id that was actually shaped, so the
    // painter cannot disagree with the shaper about which face produced a
    // glyph — including for an id that was not registered.
    let font = crate::font::resolve(font);
    // First line only; callers split on '\n' for multiline.
    let line = text.split('\n').next().unwrap_or("");
    let mut out = Vec::new();
    let mut total = 0.0;
    for (run_text, rtl) in visual_runs(line) {
        let (mut adv, w) = shape_run(&run_text, rtl, px, font);
        out.append(&mut adv);
        total += w;
    }
    (out, total)
}

/// Measure `(width, height)` for possibly-multiline text at `px`.
#[must_use]
pub fn measure_text(text: &str, px: f32, font: FontId) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, line_height(px));
    }
    let mut w: f32 = 0.0;
    let mut lines = 0;
    for line in text.split('\n') {
        let (_, lw) = shape_text(line, px, font);
        w = w.max(lw);
        lines += 1;
    }
    (w, lines as f32 * line_height(px))
}

/// Greedy word wrap: reflow each `\n` paragraph to `max_w` px, joining with
/// `\n`. Words wider than `max_w` are hard-split by char. Pure function —
/// layout and paint call the same code so they agree exactly.
#[must_use]
pub fn wrap_text(text: &str, px: f32, max_w: f32, font: FontId) -> String {
    if max_w <= 0.0 {
        return text.to_owned();
    }
    let space_w = shape_text(" ", px, font).1.max(1.0);
    let mut out = Vec::new();
    for para in text.split('\n') {
        if para.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_w = 0.0;
        for word in para.split_whitespace() {
            let (_, ww) = shape_text(word, px, font);
            let add = if line.is_empty() { ww } else { space_w + ww };
            if line_w + add <= max_w || line.is_empty() {
                if !line.is_empty() {
                    line.push(' ');
                    line_w += space_w;
                }
                // Hard-split an over-long single word.
                if line.is_empty() && ww > max_w {
                    let mut cur = String::new();
                    let mut cur_w = 0.0;
                    for ch in word.chars() {
                        let (_, cw) = shape_text(&ch.to_string(), px, font);
                        if cur_w + cw > max_w && !cur.is_empty() {
                            out.push(cur);
                            cur = String::new();
                            cur_w = 0.0;
                        }
                        cur.push(ch);
                        cur_w += cw;
                    }
                    line = cur;
                    line_w = cur_w;
                } else {
                    line.push_str(word);
                    line_w += ww;
                }
            } else {
                out.push(std::mem::take(&mut line));
                line.push_str(word);
                line_w = ww;
            }
        }
        out.push(line);
    }
    out.join("\n")
}

/// Largest font size in px such that `text` wrapped at `max_w` fits in
/// `max_w` x `max_h`, searching down from `max_px` (floor 4px).
/// Headline `text-fit` without a measure loop on the caller's side.
#[must_use]
pub fn fit_font_size(text: &str, max_w: f32, max_h: f32, max_px: f32, font: FontId) -> f32 {
    if max_w <= 0.0 || max_h <= 0.0 {
        return 4.0;
    }
    let mut lo = 4.0_f32;
    let mut hi = max_px.max(lo);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        let laid = wrap_text(text, mid, max_w, font);
        let (w, h) = measure_text(&laid, mid, font);
        if w <= max_w && h <= max_h {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Reflow `text` (single paragraph) to minimize the widest line, keeping
/// every line within `max_w`. Like CSS `text-wrap: balance` for headlines.
/// Returns the reflowed text with `\n` breaks; over-long single words are
/// left to overflow (callers can pre-split via [`wrap_text`]).
#[must_use]
pub fn balance_text(text: &str, px: f32, max_w: f32, font: FontId) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return text.to_owned();
    }
    // Width of words[i..j] joined by spaces.
    let width = |i: usize, j: usize| {
        let (_, w) = shape_text(&words[i..j].join(" "), px, font);
        w
    };
    // dp[i] = (best max width, fewest lines) for suffix starting at i.
    let n = words.len();
    let mut dp: Vec<(f32, usize)> = vec![(f32::INFINITY, usize::MAX); n + 1];
    let mut next: Vec<usize> = vec![n; n + 1];
    dp[n] = (0.0, 0);
    for i in (0..n).rev() {
        for j in i + 1..=n {
            let w = width(i, j);
            if w > max_w {
                break;
            }
            let cand = (dp[j].0.max(w), dp[j].1 + 1);
            if cand < dp[i] {
                dp[i] = cand;
                next[i] = j;
            }
        }
        // No break fits (over-long word): take it alone, overflowing.
        if dp[i].1 == usize::MAX {
            next[i] = i + 1;
            let w = width(i, i + 1);
            dp[i] = (dp[i + 1].0.max(w), dp[i + 1].1 + 1);
        }
    }
    let mut lines = Vec::new();
    let mut i = 0;
    while i < n {
        let j = next[i].max(i + 1);
        lines.push(words[i..j].join(" "));
        i = j;
    }
    lines.join("\n")
}

/// Split a line into `(text, is_rtl)` runs in visual (left-to-right) order.
fn visual_runs(line: &str) -> Vec<(String, bool)> {
    if line.is_empty() {
        return Vec::new();
    }
    let bidi = BidiInfo::new(line, None);
    if !bidi.has_rtl() {
        return vec![(line.to_owned(), false)];
    }
    let mut runs = Vec::new();
    for para in &bidi.paragraphs {
        let line_range = para.range.clone();
        let (_levels, level_runs) = bidi.visual_runs(para, line_range);
        for run in level_runs {
            let rtl = bidi.levels[run.start].is_rtl();
            runs.push((line[run.start..run.end].to_owned(), rtl));
        }
    }
    runs
}

/// The fallback chain: fonts tried, in order, when the primary font lacks a
/// glyph. Returns the id of the first bundled font covering `ch`, else
/// [`BUILTIN_FONT`] so the caller's "missing" path still triggers.
fn fallback_for_char(ch: char) -> FontId {
    crate::font::font_covering(ch).unwrap_or(BUILTIN_FONT)
}

/// Split `run` into maximal segments that the same font can shape.
///
/// This is what makes complex-script fallback actually work. Previously a
/// missing glyph fell back to *rasterizing the character* in a system font
/// (`GlyphRef::Char`), which never goes through the shaper — so Arabic, Hebrew
/// and Devanagari text drew as isolated letters no matter which font was
/// installed, because there is no joining to be had outside the shaper.
/// Shaping each segment with a font that covers it means the fallback gets the
/// same GSUB treatment as the primary and joins properly.
///
/// Segments break on coverage, never inside a grapheme cluster, so a joining
/// sequence whose letters share a font stays in one segment.
fn segment_by_coverage(run: &str, font: FontId) -> Vec<(&str, FontId)> {
    // Resolve first: an unregistered id must behave exactly like the built-in,
    // and asking an unknown id about coverage would answer "no" for every
    // character and divert the whole run into the fallback chain.
    let primary = crate::font::resolve(font);
    // Fast path: if the primary covers everything, there is one segment. This is
    // the overwhelmingly common case (Latin), so it stays allocation-free.
    if run.chars().all(|c| crate::font::font_covers(primary, c)) {
        return vec![(run, primary)];
    }

    let mut segments: Vec<(&str, FontId)> = Vec::new();
    let mut start = 0usize;
    let mut current = primary;
    for (idx, ch) in run.char_indices() {
        let want = if crate::font::font_covers(primary, ch) {
            primary
        } else {
            fallback_for_char(ch)
        };
        if want != current && idx > start {
            segments.push((&run[start..idx], current));
            start = idx;
        }
        current = want;
    }
    segments.push((&run[start..], current));
    segments
}

fn shape_run(run: &str, rtl: bool, px: f32, font: FontId) -> (Vec<PlacedAdvance>, f32) {
    if run.is_empty() {
        return (Vec::new(), 0.0);
    }
    let mut all: Vec<PlacedAdvance> = Vec::new();
    let mut total = 0.0;
    for (segment, seg_font) in segment_by_coverage(run, font) {
        let (mut adv, w) = shape_segment(segment, rtl, px, seg_font);
        all.append(&mut adv);
        total += w;
    }
    (all, total)
}

/// Shape a single segment, known to be covered by `font`.
fn shape_segment(run: &str, rtl: bool, px: f32, font: FontId) -> (Vec<PlacedAdvance>, f32) {
    if run.is_empty() {
        return (Vec::new(), 0.0);
    }
    let scale = px / crate::font::units_per_em_of(font) as f32;
    // Byte-index -> char map (shaper clusters are byte indices).
    let index: Vec<(usize, char)> = run.char_indices().collect();
    let char_at = |byte_idx: usize| -> char {
        let mut ch = '\u{FFFD}';
        for (b, c) in &index {
            if *b <= byte_idx {
                ch = *c;
            } else {
                break;
            }
        }
        ch
    };
    let mut buf = Buffer::new();
    buf.push_str(run);
    // Segment properties are guessed *first*, then only the direction is
    // overridden.
    //
    // The order matters and getting it wrong is invisible in Latin. Script
    // selection decides which GSUB features apply, and a shaper told only a
    // direction has to pick a script itself: `rustybuzz` 0.14 defaulted to the
    // Latin set, which gave correct ligatures and kerning but left Arabic
    // unjoined; `harfrust` defaults the other way, which joins Arabic and
    // silently drops `ffi` into three separate letters. Both looked fine in the
    // script they happened to be right about, which is why neither was caught.
    //
    // Guessing infers the script from the run's first strong character, and
    // `visual_runs` already splits on bidi, so a run is normally
    // script-homogeneous. It is not guaranteed -- a run may mix Latin into RTL
    // text -- so this is a known limit rather than a guarantee. Splitting runs by
    // script as well as direction is the fix, and needs script data this crate
    // does not currently carry.
    buf.guess_segment_properties();
    buf.set_direction(if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    // Cached per thread per font: `ShaperFont::new` re-resolves the layout
    // tables, which cost ~16% of the render benchmark when run per shaped run.
    let shaper = crate::font::shaper_for(font);
    harfrust::shape(shaper, &mut buf, ShapeOptions::default())
        .expect("shaping a registered font never fails");
    let infos = buf.glyph_infos();
    let positions = buf.glyph_positions();
    let mut total = 0.0;
    let advances = infos
        .iter()
        .zip(positions.iter())
        .map(|(info, pos)| {
            let ch = char_at(info.cluster as usize);
            // The glyph id must come from the shaper, not from a cmap lookup of
            // `ch`. After GSUB the shaped glyph is frequently *not* the glyph
            // the source character maps to: ligatures collapse several
            // characters into one glyph, and Arabic letters are rewritten into
            // contextual presentation forms. Re-deriving the id from `ch` threw
            // that away, so the painter drew a blank or the wrong letterform.
            let gid = info.glyph_id;
            let missing = info.glyph_id == 0 && ch != '\0';
            // Missing CJK ideographs are full-width in virtually every
            // fallback font; measure 1em so layout agrees with paint.
            let advance = if missing && is_wide(ch) {
                px
            } else {
                pos.x_advance as f32 * scale
            };
            total += advance;
            PlacedAdvance {
                ch,
                gid,
                advance: advance.max(0.0),
                x_offset: pos.x_offset as f32 * scale,
                missing,
                font,
            }
        })
        .collect();
    (advances, total.max(0.0))
}

/// True for scripts conventionally set full-width (CJK, Hangul, fullwidth).
fn is_wide(ch: char) -> bool {
    matches!(ch,
        '\u{2E80}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}'
        | '\u{FF00}'..='\u{FFEF}'
        | '\u{20000}'..='\u{2FFFF}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::BUILTIN_FONT;

    #[test]
    fn advances_scale_linearly() {
        let (_, w36) = shape_text("Hello from Hikari", 36.0, BUILTIN_FONT);
        let (_, w72) = shape_text("Hello from Hikari", 72.0, BUILTIN_FONT);
        assert!(w36 > 50.0, "w36={w36}");
        assert!((w72 / w36 - 2.0).abs() < 0.05, "w36={w36} w72={w72}");
    }

    #[test]
    fn rtl_and_cjk_shape_without_panic() {
        let (adv_he, w_he) = shape_text("שלום", 48.0, BUILTIN_FONT);
        assert!(!adv_he.is_empty() && w_he > 0.0);
        let (adv_ar, w_ar) = shape_text("مرحبا", 48.0, BUILTIN_FONT);
        assert!(!adv_ar.is_empty() && w_ar > 0.0);
        let (adv_cjk, w_cjk) = shape_text("日本語", 48.0, BUILTIN_FONT);
        assert!(!adv_cjk.is_empty() && w_cjk > 0.0);
    }

    #[test]
    fn balance_beats_greedy() {
        let text = "Hello beautiful wide world today";
        let px = 40.0;
        let max_w = 300.0;
        let balanced = balance_text(text, px, max_w, BUILTIN_FONT);
        let greedy = wrap_text(text, px, max_w, BUILTIN_FONT);
        let stats = |t: &str| {
            let ws: Vec<f32> = t
                .split('\n')
                .map(|l| shape_text(l, px, BUILTIN_FONT).1)
                .collect();
            let max = ws.iter().fold(0.0_f32, |a, b| a.max(*b));
            let min = ws.iter().fold(f32::INFINITY, |a, b| a.min(*b));
            (max, max - min, ws.len())
        };
        let (bmax, beven, _) = stats(&balanced);
        let (gmax, geven, _) = stats(&greedy);
        // Balanced minimizes the widest line first, evenness second.
        assert!(bmax <= gmax + 0.01, "{balanced}");
        assert!(beven <= geven + 0.01, "{balanced} vs {greedy}");
        // Every balanced line respects the width.
        for line in balanced.split('\n') {
            assert!(
                shape_text(line, px, BUILTIN_FONT).1 <= max_w + 0.01,
                "{line}"
            );
        }
    }

    #[test]
    fn fit_finds_largest_fitting_size() {
        let size = fit_font_size("Hello Hikari", 400.0, 100.0, 200.0, BUILTIN_FONT);
        assert!(size > 4.0 && size <= 200.0);
        let laid = wrap_text("Hello Hikari", size, 400.0, BUILTIN_FONT);
        let (w, h) = measure_text(&laid, size, BUILTIN_FONT);
        assert!(w <= 400.0 && h <= 100.0);
        // One step bigger overflows some dimension (search converged).
        let laid2 = wrap_text("Hello Hikari", size + 2.0, 400.0, BUILTIN_FONT);
        let (w2, h2) = measure_text(&laid2, size + 2.0, BUILTIN_FONT);
        assert!(w2 > 400.0 || h2 > 100.0 || size + 2.0 > 200.0);
    }

    #[test]
    fn multiline_measure_stacks() {
        let (w1, h1) = measure_text("abc", 40.0, BUILTIN_FONT);
        let (w2, h2) = measure_text("abc\nabc", 40.0, BUILTIN_FONT);
        assert!((w2 - w1).abs() < 0.01);
        assert!((h2 - 2.0 * h1).abs() < 0.01);
    }
}
