//! Upstream text shaping: `rustybuzz` advances + `unicode-bidi` visual order.
//!
//! One embedded font (`assets/DejaVuSans.ttf`) is the single source of truth
//! for both measurement (here) and rasterization (`hikari-raster`), so layout
//! and paint agree. Complex-script joining is best-effort in v0.2: runs are
//! shaped per bidi run with correct direction; full itemization with per-font
//! fallback is Step 1b in `plans/hikari-beat-takumi.md`.

use std::sync::OnceLock;

use rustybuzz::{Direction, Face, UnicodeBuffer};
use unicode_bidi::BidiInfo;

/// Embedded font bytes: the build-time subset (see `build.rs`) — the
/// single source of truth for measurement, rasterization, and PDF.
#[must_use]
pub fn font_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/dejavu-subset.ttf"))
}

static FACE: OnceLock<Face<'static>> = OnceLock::new();
static TTF: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();

fn face() -> &'static Face<'static> {
    FACE.get_or_init(|| Face::from_slice(font_bytes(), 0).expect("embedded font parses"))
}

/// `ttf-parser` view of the embedded font (metrics, glyph ids).
fn ttf_face() -> &'static ttf_parser::Face<'static> {
    TTF.get_or_init(|| ttf_parser::Face::parse(font_bytes(), 0).expect("embedded font parses"))
}

/// System CJK-capable fallback bytes, loaded lazily. Never bundled
/// (proprietary on macOS); production deployments should ship a subsetted
/// OFL CJK font instead of relying on these paths.
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
}

/// Shape `text` at `px` size. Returns advances in visual order and total width.
/// Never fails: empty or unshaped input yields zero-width output.
pub fn shape_text(text: &str, px: f32) -> (Vec<PlacedAdvance>, f32) {
    if text.is_empty() || px <= 0.0 {
        return (Vec::new(), 0.0);
    }
    let upem = face().units_per_em() as f32;
    if upem <= 0.0 {
        return (Vec::new(), 0.0);
    }
    // First line only; callers split on '\n' for multiline.
    let line = text.split('\n').next().unwrap_or("");
    let mut out = Vec::new();
    let mut total = 0.0;
    for (run_text, rtl) in visual_runs(line) {
        let (mut adv, w) = shape_run(&run_text, rtl, px);
        out.append(&mut adv);
        total += w;
    }
    (out, total)
}

/// Measure `(width, height)` for possibly-multiline text at `px`.
#[must_use]
pub fn measure_text(text: &str, px: f32) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, line_height(px));
    }
    let mut w: f32 = 0.0;
    let mut lines = 0;
    for line in text.split('\n') {
        let (_, lw) = shape_text(line, px);
        w = w.max(lw);
        lines += 1;
    }
    (w, lines as f32 * line_height(px))
}

/// Greedy word wrap: reflow each `\n` paragraph to `max_w` px, joining with
/// `\n`. Words wider than `max_w` are hard-split by char. Pure function —
/// layout and paint call the same code so they agree exactly.
#[must_use]
pub fn wrap_text(text: &str, px: f32, max_w: f32) -> String {
    if max_w <= 0.0 {
        return text.to_owned();
    }
    let space_w = shape_text(" ", px).1.max(1.0);
    let mut out = Vec::new();
    for para in text.split('\n') {
        if para.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_w = 0.0;
        for word in para.split_whitespace() {
            let (_, ww) = shape_text(word, px);
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
                        let (_, cw) = shape_text(&ch.to_string(), px);
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
pub fn fit_font_size(text: &str, max_w: f32, max_h: f32, max_px: f32) -> f32 {
    if max_w <= 0.0 || max_h <= 0.0 {
        return 4.0;
    }
    let mut lo = 4.0_f32;
    let mut hi = max_px.max(lo);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        let laid = wrap_text(text, mid, max_w);
        let (w, h) = measure_text(&laid, mid);
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
pub fn balance_text(text: &str, px: f32, max_w: f32) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return text.to_owned();
    }
    // Width of words[i..j] joined by spaces.
    let width = |i: usize, j: usize| {
        let (_, w) = shape_text(&words[i..j].join(" "), px);
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

fn shape_run(run: &str, rtl: bool, px: f32) -> (Vec<PlacedAdvance>, f32) {
    if run.is_empty() {
        return (Vec::new(), 0.0);
    }
    let scale = px / face().units_per_em() as f32;
    // Byte-index -> char map (rustybuzz clusters are byte indices).
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
    let mut buf = UnicodeBuffer::new();
    buf.push_str(run);
    buf.set_direction(if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    let output = rustybuzz::shape(face(), &[], buf);
    let infos = output.glyph_infos();
    let positions = output.glyph_positions();
    let mut total = 0.0;
    let advances = infos
        .iter()
        .zip(positions.iter())
        .map(|(info, pos)| {
            let ch = char_at(info.cluster as usize);
            let gid = ttf_face()
                .glyph_index(ch)
                .map(|g| u32::from(g.0))
                .unwrap_or(0);
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

    #[test]
    fn advances_scale_linearly() {
        let (_, w36) = shape_text("Hello from Hikari", 36.0);
        let (_, w72) = shape_text("Hello from Hikari", 72.0);
        assert!(w36 > 50.0, "w36={w36}");
        assert!((w72 / w36 - 2.0).abs() < 0.05, "w36={w36} w72={w72}");
    }

    #[test]
    fn rtl_and_cjk_shape_without_panic() {
        let (adv_he, w_he) = shape_text("שלום", 48.0);
        assert!(!adv_he.is_empty() && w_he > 0.0);
        let (adv_ar, w_ar) = shape_text("مرحبا", 48.0);
        assert!(!adv_ar.is_empty() && w_ar > 0.0);
        let (adv_cjk, w_cjk) = shape_text("日本語", 48.0);
        assert!(!adv_cjk.is_empty() && w_cjk > 0.0);
    }

    #[test]
    fn balance_beats_greedy() {
        let text = "Hello beautiful wide world today";
        let px = 40.0;
        let max_w = 300.0;
        let balanced = balance_text(text, px, max_w);
        let greedy = wrap_text(text, px, max_w);
        let stats = |t: &str| {
            let ws: Vec<f32> = t.split('\n').map(|l| shape_text(l, px).1).collect();
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
            assert!(shape_text(line, px).1 <= max_w + 0.01, "{line}");
        }
    }

    #[test]
    fn fit_finds_largest_fitting_size() {
        let size = fit_font_size("Hello Hikari", 400.0, 100.0, 200.0);
        assert!(size > 4.0 && size <= 200.0);
        let laid = wrap_text("Hello Hikari", size, 400.0);
        let (w, h) = measure_text(&laid, size);
        assert!(w <= 400.0 && h <= 100.0);
        // One step bigger overflows some dimension (search converged).
        let laid2 = wrap_text("Hello Hikari", size + 2.0, 400.0);
        let (w2, h2) = measure_text(&laid2, size + 2.0);
        assert!(w2 > 400.0 || h2 > 100.0 || size + 2.0 > 200.0);
    }

    #[test]
    fn multiline_measure_stacks() {
        let (w1, h1) = measure_text("abc", 40.0);
        let (w2, h2) = measure_text("abc\nabc", 40.0);
        assert!((w2 - w1).abs() < 0.01);
        assert!((h2 - 2.0 * h1).abs() < 0.01);
    }
}
